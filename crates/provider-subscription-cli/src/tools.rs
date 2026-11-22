//! Jush's MCP server for one CLI process ([`ToolServer`]): it lists the
//! request's tools, and each `tools/call` waits for the agent's result, which
//! a later run delivers. Every CLI reaches Jush's tools through one, over its
//! own channel: Claude Code's control requests
//! (`crates/provider-claude-code/src/mcp.rs`), `agy`'s companion process
//! (`crates/provider-antigravity/src/tools.rs`).
//!
//! The server is built with rmcp, the official Rust MCP SDK, because the
//! protocol's types, the `initialize` handshake and version negotiation are the
//! library's job, and in its local mode because its state lives on the user's
//! shard, one thread. It is reached through an in-memory transport, whose
//! messages the provider carries to and from its CLI. rmcp handles each request
//! on its own task, so `ping` and `tools/list` are answered while a call waits.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use futures_channel::mpsc;
use futures_util::StreamExt as _;
use jush_provider_common::{ResultPart, ToolCall, ToolDefinition};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ClientJsonRpcMessage, ContentBlock,
    Implementation, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
    ServerJsonRpcMessage, Tool,
};
use rmcp::service::{RequestContext, RoleServer, serve_server_with_ct};
use rmcp::{ErrorData as McpError, ServerHandler};
use tokio::sync::oneshot;
use tokio_util::sync::{CancellationToken, DropGuard};
use tokio_util::task::AbortOnDropHandle;

/// A process's MCP server and the provider's end of it.
pub struct ToolServer {
    /// MCP messages from the CLI, into the server.
    to_server: mpsc::UnboundedSender<ClientJsonRpcMessage>,
    /// The server's messages, which the provider carries to the CLI.
    from_server: mpsc::UnboundedReceiver<ServerJsonRpcMessage>,
    /// Each call the server opens, as it opens.
    opened: mpsc::UnboundedReceiver<ToolCall>,
    board: Rc<RefCell<Board>>,
    _server: AbortOnDropHandle<()>,
    /// Stops the server, and with it every handler still waiting.
    _stop: DropGuard,
}

/// What the server shares with the provider: the tools it lists, and the
/// meeting of each call with its result.
struct Board {
    /// The current request's tools, which `tools/list` answers.
    tools: Arc<[ToolDefinition]>,
    /// Calls the CLI made whose result the agent has not given yet, by
    /// tool-use id.
    open: HashMap<String, oneshot::Sender<CallToolResult>>,
    /// Results the agent gave before the CLI asked for them, by tool-use id.
    ready: HashMap<String, CallToolResult>,
    /// Why the server ended its handshake, when it failed.
    failure: Option<String>,
}

/// Something the server did that the provider acts on.
pub enum ToolEvent {
    /// A message for the CLI, such as the reply to a request of its.
    Reply(Box<ServerJsonRpcMessage>),
    /// A call the server opened, which waits for its result.
    Opened(ToolCall),
}

impl ToolServer {
    /// Starts the server of one process, listing `tools`.
    ///
    /// A call is named by the id its request's `_meta` holds at `call_id`,
    /// when the CLI names the model's own; otherwise, by a new unique one.
    pub fn start(tools: Arc<[ToolDefinition]>, call_id: Option<&'static str>) -> Self {
        let (to_server, inbox) = mpsc::unbounded::<ClientJsonRpcMessage>();
        let (outbox, from_server) = mpsc::unbounded::<ServerJsonRpcMessage>();
        let (announce, opened) = mpsc::unbounded::<ToolCall>();
        let board = Rc::new(RefCell::new(Board {
            tools,
            open: HashMap::new(),
            ready: HashMap::new(),
            failure: None,
        }));
        let stop = CancellationToken::new();
        let server = Server {
            board: board.clone(),
            announce,
            call_id,
        };
        let task = {
            let board = board.clone();
            let stop = stop.clone();
            tokio::task::spawn_local(async move {
                match serve_server_with_ct(server, (outbox, inbox), stop).await {
                    // Serves until the process's end stops it.
                    Ok(running) => {
                        let _ = running.waiting().await;
                    }
                    Err(error) => board.borrow_mut().failure = Some(error.to_string()),
                }
            })
        };
        Self {
            to_server,
            from_server,
            opened,
            board,
            _server: AbortOnDropHandle::new(task),
            _stop: stop.drop_guard(),
        }
    }

    /// Passes the server `message`, one the CLI sent; why the server ended
    /// when it did.
    pub fn send(&self, message: ClientJsonRpcMessage) -> Result<(), String> {
        self.to_server
            .unbounded_send(message)
            .map_err(|_| self.ended())
    }

    /// The server's next message for the CLI, or the next call it opened.
    ///
    /// A call that is no longer open, such as one answered from a stored
    /// result, is skipped. A server that ended has nothing more; the CLI learns
    /// that from the answers to what it sends ([`ToolServer::send`]).
    pub async fn next(&mut self) -> ToolEvent {
        loop {
            tokio::select! {
                biased;
                Some(reply) = self.from_server.next() => return ToolEvent::Reply(Box::new(reply)),
                Some(call) = self.opened.next() => {
                    if self.board.borrow().open.contains_key(&call.tool_use_id) {
                        return ToolEvent::Opened(call);
                    }
                }
                else => std::future::pending::<()>().await,
            }
        }
    }

    /// Why the server ended.
    pub fn ended(&self) -> String {
        let reason = self.board.borrow().failure.clone();
        reason.unwrap_or_else(|| "Jush's MCP server ended".into())
    }

    /// Makes `tools` the ones `tools/list` answers.
    pub fn offer(&self, tools: Arc<[ToolDefinition]>) {
        self.board.borrow_mut().tools = tools;
    }

    /// Hands the agent's results to their calls: a call the CLI made gets its
    /// result now, and one it has not made yet finds it when it does.
    pub fn deliver(&self, results: Vec<(String, CallToolResult)>) {
        let mut board = self.board.borrow_mut();
        for (tool_use_id, result) in results {
            match board.open.remove(&tool_use_id) {
                // A handler that gave up, as when the CLI cancelled the call,
                // takes nothing.
                Some(call) => {
                    let _ = call.send(result);
                }
                None => {
                    board.ready.insert(tool_use_id, result);
                }
            }
        }
    }

    /// The results the CLI never asked for, by tool-use id.
    pub fn unasked(&self) -> Vec<String> {
        let mut unasked: Vec<String> = self.board.borrow().ready.keys().cloned().collect();
        unasked.sort();
        unasked
    }
}

/// Jush's MCP server for one process.
struct Server {
    board: Rc<RefCell<Board>>,
    /// Tells the provider of each call that opens.
    announce: mpsc::UnboundedSender<ToolCall>,
    /// Where a call's `_meta` names the model's own id for it.
    call_id: Option<&'static str>,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("jush", jush_shared_artifacts::BUILD_VERSION))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self.board.borrow().tools.clone();
        Ok(ListToolsResult::with_all_items(
            tools
                .iter()
                .map(|tool| {
                    Tool::new(
                        tool.name.clone(),
                        tool.description.clone(),
                        Arc::new(tool.input_schema.clone()),
                    )
                })
                .collect(),
        ))
    }

    /// A call of the model's, by the id the CLI names it by, else a new
    /// unique one. Its result comes from the agent: at once when it was given
    /// already, otherwise when a run delivers it.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tool_use_id = self
            .call_id
            .and_then(|key| context.meta.get(key))
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
            .map_or_else(|| format!("mcp-call-{}", uuid::Uuid::new_v4()), str::to_owned);
        let call = ToolCall {
            tool_use_id: tool_use_id.clone(),
            tool_name: request.name.to_string(),
            input: request
                .arguments
                .map_or_else(|| serde_json::json!({}), serde_json::Value::Object),
        };
        let receiver = {
            let mut board = self.board.borrow_mut();
            if let Some(result) = board.ready.remove(&tool_use_id) {
                return Ok(result.into());
            }
            let (sender, receiver) = oneshot::channel();
            board.open.insert(tool_use_id.clone(), sender);
            receiver
        };
        // The provider is gone only when the process is closing, which stops
        // this handler too.
        let _ = self.announce.unbounded_send(call);
        tokio::select! {
            result = receiver => result
                .map(CallToolResponse::from)
                .map_err(|_| McpError::internal_error("The tool call ended with its process", None)),
            () = context.ct.cancelled() => {
                self.board.borrow_mut().open.remove(&tool_use_id);
                Err(McpError::internal_error("The tool call was cancelled", None))
            }
        }
    }
}

/// A tool's result as MCP content.
///
/// - Text is text, and an image is base64 with its media type.
/// - A video or a document is text naming its media type, since MCP has no
///   video and a CLI may save a PDF resource to a file rather than give it to
///   the model (`crates/provider-common/src/contract.rs`).
/// - A failed tool sets the error flag.
pub fn tool_result(output: &[ResultPart], is_error: bool) -> CallToolResult {
    let content = output
        .iter()
        .map(|part| match part {
            ResultPart::Text(text) => ContentBlock::text(text.clone()),
            ResultPart::Image(bytes) => {
                ContentBlock::image(STANDARD.encode(&bytes.data), bytes.media_type.clone())
            }
            ResultPart::Video(bytes)
            | ResultPart::Audio(bytes)
            | ResultPart::Document { bytes, .. } => ContentBlock::text(bytes.unsent_text()),
        })
        .collect();
    if is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

