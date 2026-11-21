//! The SDK MCP channel: Jush's MCP server for one CLI process
//! (`crates/provider-subscription-cli/src/tools.rs`), reached through the
//! CLI's control requests.
//!
//! Before the CLI starts, the provider makes the handshake the CLI's client
//! would make, `initialize` and the first `tools/list`, and hands the CLI the
//! answers as the server's manifest, so that its first turn finds the tools
//! connected (`crates/provider-claude-code/src/live.rs`). After that, the run
//! passes the server each MCP message the CLI sends in a control request and
//! writes its replies back; a handshake the CLI makes again, as when it
//! declined the manifest, is answered as any message. A `tools/call` waits for
//! the agent's result, which a later run delivers. Calls and results are
//! matched by the original tool-use ID in `_meta` (`claudecode/toolUseId`).
//!
//! # Parallel tool batches
//!
//! The CLI's SDK MCP channel exposes only the first `tools/call` of an
//! assistant message with several `tool_use` blocks: a later call may stay
//! blocked until the earlier has its result. Handled as each arrived, the agent
//! would see one call per run and mistake the delayed calls for separate model
//! turns.
//!
//! So the provider yields the whole batch, collected until `message_stop`,
//! before answering any call, and keeps the process. The next run delivers the
//! results, every call of the batch must have one, and each later `tools/call`
//! is answered at once from its stored result.
//!
//! A `tool_use` that names a tool the request does not offer is no part of the
//! batch. The CLI answers it itself with an error result and the model corrects
//! itself within the turn, where failing the run would end it for a slip the
//! model recovers from.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use jush_provider_common::ToolDefinition;
use jush_provider_subscription_cli::{ToolEvent, ToolServer};
use rmcp::model::{
    CallToolResult, ClientJsonRpcMessage, JsonRpcMessage, ProtocolVersion, RequestId,
    ServerJsonRpcMessage, ServerResult,
};

use crate::input::Manifest;

/// The MCP protocol version the CLI's client asks for, which a manifest's
/// `initialize` result must name: rmcp's newest, until a CLI asks for another.
///
/// A CLI declines a manifest of another version and makes its handshake over
/// the control channel instead, whose `initialize` names the version it wants,
/// so only the first process of a CLI that asks for another races its tools.
pub(crate) struct ClientProtocol(Mutex<ProtocolVersion>);

impl Default for ClientProtocol {
    fn default() -> Self {
        Self(Mutex::new(ProtocolVersion::LATEST))
    }
}

impl ClientProtocol {
    fn lock(&self) -> std::sync::MutexGuard<'_, ProtocolVersion> {
        // The version stays whole whatever panicked while it was held.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get(&self) -> ProtocolVersion {
        self.lock().clone()
    }

    /// Keeps `asked`, the version of an `initialize` the CLI sent.
    pub(crate) fn asked(&self, asked: &ProtocolVersion) {
        let mut kept = self.lock();
        if *kept != *asked {
            tracing::info!(
                event = "claude_code_mcp.protocol_learned",
                was = %*kept,
                now = %asked,
                "Claude Code asked for another MCP protocol version, which its next manifests name"
            );
            *kept = asked.clone();
        }
    }
}

/// Where a `tools/call` names the model's own tool-use id.
const TOOL_USE_ID: &str = "claudecode/toolUseId";

/// A process's MCP server and the run's end of it.
pub(crate) struct Mcp {
    server: ToolServer,
    /// The control request each MCP request came in, by the request's id,
    /// oldest first, so that a reply goes back in the right one.
    requests: HashMap<RequestId, VecDeque<String>>,
}

/// Something the server did that the run acts on.
pub(crate) enum McpEvent {
    /// A reply, and the control request it answers.
    Reply(String, Box<ServerJsonRpcMessage>),
    /// A call the server opened.
    Opened(jush_provider_common::ToolCall),
}

impl Mcp {
    /// Starts the server of one process, listing `tools`.
    pub(crate) fn start(tools: Arc<[ToolDefinition]>) -> Self {
        Self {
            server: ToolServer::start(tools, Some(TOOL_USE_ID)),
            requests: HashMap::new(),
        }
    }

    /// Makes the handshake the CLI's client would make with the server, asking
    /// for `protocol`: its `initialize`, the `initialized` notification and the
    /// first `tools/list`, whose answers the CLI takes as the server's manifest.
    ///
    /// It comes before any message of the CLI's, which it would otherwise
    /// read in place of their replies.
    pub(crate) async fn manifest(&mut self, protocol: &ProtocolVersion) -> Result<Manifest, String> {
        let initialize_result = self
            .ask(serde_json::json!({
                "jsonrpc": "2.0",
                "id": "jush-initialize",
                "method": "initialize",
                "params": {
                    "protocolVersion": protocol,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "jush",
                        "version": jush_shared_artifacts::BUILD_VERSION,
                    },
                },
            }))
            .await?;
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        }))?;
        let tools_list_result = self
            .ask(serde_json::json!({
                "jsonrpc": "2.0",
                "id": "jush-tools-list",
                "method": "tools/list",
            }))
            .await?;
        Ok(Manifest {
            initialize_result,
            tools_list_result,
        })
    }

    /// Sends the server `request` and reads until its result.
    async fn ask(&mut self, request: serde_json::Value) -> Result<serde_json::Value, String> {
        let id = serde_json::from_value::<RequestId>(request["id"].clone())
            .map_err(|error| format!("The MCP request has no id: {error}"))?;
        self.send(request)?;
        loop {
            // Nothing else is asked yet, so nothing else is answered, and no
            // call opens.
            let ToolEvent::Reply(reply) = self.server.next().await else {
                continue;
            };
            match *reply {
                JsonRpcMessage::Response(response) if response.id == id => {
                    return serde_json::to_value(&response.result).map_err(|error| error.to_string());
                }
                JsonRpcMessage::Error(error) if error.id.as_ref() == Some(&id) => {
                    return Err(format!("The SDK MCP server refused its handshake: {}", error.error.message));
                }
                _ => {}
            }
        }
    }

    /// Sends the server `message`, which Jush wrote.
    fn send(&self, message: serde_json::Value) -> Result<(), String> {
        let message = serde_json::from_value::<ClientJsonRpcMessage>(message)
            .map_err(|error| format!("The MCP message cannot be read: {error}"))?;
        self.server.send(message)
    }

    /// Makes `tools` the ones `tools/list` answers.
    pub(crate) fn offer(&self, tools: Arc<[ToolDefinition]>) {
        self.server.offer(tools);
    }

    /// Passes one MCP message the CLI sent in control request `request_id` to
    /// the server.
    ///
    /// A request is answered by the server's reply, later. Any other message is
    /// acknowledged at once, and the acknowledgment is returned to be written.
    pub(crate) fn pass(
        &mut self,
        request_id: &str,
        message: ClientJsonRpcMessage,
    ) -> Result<Option<ServerJsonRpcMessage>, String> {
        let request = match &message {
            JsonRpcMessage::Request(request) => Some(request.id.clone()),
            _ => None,
        };
        self.server.send(message)?;
        match request {
            Some(id) => {
                self.requests
                    .entry(id)
                    .or_default()
                    .push_back(request_id.to_owned());
                Ok(None)
            }
            None => Ok(Some(ServerJsonRpcMessage::response(
                ServerResult::empty(()),
                RequestId::Number(0),
            ))),
        }
    }

    /// The server's next reply or the next call it opened.
    pub(crate) async fn next(&mut self) -> McpEvent {
        loop {
            match self.server.next().await {
                ToolEvent::Reply(reply) => {
                    if let Some(event) = self.request_of(*reply) {
                        return event;
                    }
                }
                ToolEvent::Opened(call) => return McpEvent::Opened(call),
            }
        }
    }

    /// `reply` with the control request it answers; none for a message that
    /// answers no request of the CLI's, which the CLI would not expect.
    fn request_of(&mut self, reply: ServerJsonRpcMessage) -> Option<McpEvent> {
        let id = match &reply {
            JsonRpcMessage::Response(response) => Some(&response.id),
            JsonRpcMessage::Error(error) => error.id.as_ref(),
            JsonRpcMessage::Request(_) | JsonRpcMessage::Notification(_) => None,
        }?;
        let waiting = self.requests.get_mut(id)?;
        let request_id = waiting.pop_front()?;
        if waiting.is_empty() {
            self.requests.remove(id);
        }
        Some(McpEvent::Reply(request_id, Box::new(reply)))
    }

    /// Hands the agent's results to their calls.
    pub(crate) fn deliver(&self, results: Vec<(String, CallToolResult)>) {
        self.server.deliver(results);
    }

    /// The results the CLI never asked for, by tool-use id.
    pub(crate) fn unasked(&self) -> Vec<String> {
        self.server.unasked()
    }
}
