//! How the model's tool calls reach Jush: `agy` starts the MCP server its
//! configuration names, `jush`, whose command is `sh` and `cat` joined to two
//! named pipes in the process's directory, and the companion process the
//! placement starts beside `agy` joins the pipes to its own standard streams,
//! which [`Channel`] carries to and from Jush's MCP server
//! (`crates/provider-subscription-cli/src/tools.rs`):
//!
//! ```text
//! agy --stdio-- sh of mcp_config.json      companion sh --stdio-- Channel
//!                 cat > mcp-in  --------->  cat mcp-in             ToolServer
//!                 cat mcp-out  <---------  cat > mcp-out
//! ```
//!
//! Whichever side opens a pipe first waits for the other, so they may start
//! in either order. The channel serves one MCP session: once either side ends
//! it, the pipes are done, and so is the process.
//!
//! The server's one tool, `shell`, is eager, so the model sees it with its
//! schema as `mcp_jush_shell`; a lazy one is called through `agy`'s own tool
//! after the model reads its schema from a file. A `tools/call` names no call
//! of the model's, so the server gives each an id of its own.

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::StreamExt as _;
use futures_util::stream::LocalBoxStream;
use jush_device_interface::{Process, ProcessControl, ProcessOutput, SpawnEnv, SpawnRequest};
use jush_provider_common::{ToolCall, ToolDefinition};
use jush_provider_subscription_cli::{ToolEvent, ToolServer};
use jush_shared_types::StreamKind;
use rmcp::model::{CallToolResult, ClientJsonRpcMessage};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::acp::Lines;

/// The MCP server's name in the agent and the configuration.
pub(crate) const SERVER: &str = "jush";

/// Where `agy` reads its MCP servers in its `HOME`.
pub(crate) const CONFIG: &str = ".gemini/config/mcp_config.json";

/// The shell of both ends, which every machine `agy` runs on has.
const SH: &str = "/bin/sh";

/// The MCP server's command, run in the process's directory, its first
/// argument: it makes the pipes unless the companion made them, and joins its
/// input to `mcp-in` and `mcp-out` to its output.
///
/// The input is moved to descriptor 3 first: a job `sh` runs in the
/// background reads `/dev/null` otherwise.
const SERVER_SCRIPT: &str =
    "cd \"$0\" || exit; mkfifo mcp-in mcp-out 2>/dev/null; exec 3<&0; cat <&3 > mcp-in & exec cat mcp-out";

/// How long `agy` waits for a call, in seconds: the most its setting holds.
/// Jush returns every call itself, by the shell's window, and a call that
/// waits for the user has no end, so `agy` never gives up on one first; by
/// default it gives up after three minutes and the model calls again.
const CALL_TIMEOUT_SECONDS: i32 = i32::MAX;

/// The companion's command, the other end.
const COMPANION_SCRIPT: &str =
    "cd \"$0\" || exit; mkfifo mcp-in mcp-out 2>/dev/null; cat mcp-in & exec cat > mcp-out";

/// The MCP configuration of a process whose directory is `home`.
pub(crate) fn config(home: &str) -> Bytes {
    let config = json!({
        "mcpServers": {
            SERVER: {
                "command": SH,
                "args": ["-c", SERVER_SCRIPT, home],
                "timeoutSeconds": CALL_TIMEOUT_SECONDS,
                "tools": { "shell": { "eager": true } },
            },
        },
    });
    Bytes::from(config.to_string())
}

/// The companion of a process whose directory is `home`. It leads a process
/// group of its own, so that closing it ends its `cat`s too.
pub(crate) fn companion(home: &str) -> SpawnRequest {
    SpawnRequest {
        command: SH.to_owned(),
        args: vec!["-c".to_owned(), COMPANION_SCRIPT.to_owned(), home.to_owned()],
        cwd: Some(home.to_owned()),
        env: SpawnEnv::Overlay(BTreeMap::new()),
        descriptors: Vec::new(),
        retained: true,
        process_group: true,
    }
}

/// The MCP channel of one process: Jush's MCP server, and the companion's
/// streams that reach `agy`'s end.
pub(crate) struct Channel {
    server: ToolServer,
    output: LocalBoxStream<'static, ProcessOutput>,
    /// What the companion wrote that holds no whole line yet.
    pending: Lines,
    /// Lines for `agy`, which a task writes to the companion in order, so
    /// that reading the channel never waits on a write.
    writes: mpsc::UnboundedSender<Bytes>,
    /// Why the channel ended, once it did.
    ended: Option<String>,
}

/// What the channel did that the run acts on.
pub(crate) enum ChannelEvent {
    /// The model called a tool, which waits for its result.
    Opened(ToolCall),
    /// The channel ended, and why.
    Ended(String),
}

impl Channel {
    /// The channel over the process's companion, offering `tools`.
    pub(crate) fn start(companion: Process, tools: Arc<[ToolDefinition]>) -> Self {
        let Process {
            output, control, ..
        } = companion;
        let (writes, mut written) = mpsc::unbounded_channel::<Bytes>();
        tokio::task::spawn_local(async move {
            let control: Box<dyn ProcessControl> = control;
            while let Some(line) = written.recv().await {
                if control.write_stdin(line).await.is_err() {
                    // The companion ended, which its output says as well.
                    break;
                }
            }
            // The control goes with the task, which closes the companion.
        });
        Self {
            server: ToolServer::start(tools, None),
            output,
            pending: Lines::default(),
            writes,
            ended: None,
        }
    }

    /// Makes `tools` the ones the server lists.
    pub(crate) fn offer(&self, tools: Arc<[ToolDefinition]>) {
        self.server.offer(tools);
    }

    /// Hands the agent's results to their calls.
    pub(crate) fn deliver(&self, results: Vec<(String, CallToolResult)>) {
        self.server.deliver(results);
    }

    /// The results `agy` never asked for, by tool-use id.
    pub(crate) fn unasked(&self) -> Vec<String> {
        self.server.unasked()
    }

    /// Carries messages both ways until the model calls a tool or the channel
    /// ends. Dropping the future loses nothing: what it read is kept, and what
    /// it writes is queued.
    pub(crate) async fn next(&mut self) -> ChannelEvent {
        loop {
            if let Some(reason) = &self.ended {
                return ChannelEvent::Ended(reason.clone());
            }
            tokio::select! {
                biased;
                event = self.server.next() => match event {
                    ToolEvent::Reply(reply) => self.write(&*reply),
                    ToolEvent::Opened(call) => return ChannelEvent::Opened(call),
                },
                chunk = self.output.next() => match chunk {
                    Some(chunk) if chunk.stream == StreamKind::Stdout => {
                        self.pending.push(&chunk.bytes);
                        self.pass_lines();
                    }
                    // The companion's own errors, such as a pipe that could
                    // not be made, end its output too.
                    Some(_) => {}
                    None => self.ended = Some("Antigravity's MCP channel ended".into()),
                },
            }
        }
    }

    /// Passes each whole line the companion wrote to the server.
    fn pass_lines(&mut self) {
        while let Some(line) = self.pending.next() {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<ClientJsonRpcMessage>(&line) {
                Ok(message) => {
                    if let Err(reason) = self.server.send(message) {
                        self.ended = Some(reason);
                    }
                }
                // A request the server's protocol does not know is refused
                // as a method it has not; anything else unreadable is
                // dropped, as JSON-RPC drops a notification it cannot read.
                Err(_) => {
                    if let Some(id) = serde_json::from_slice::<Value>(&line)
                        .ok()
                        .and_then(|message| message.get("id").cloned())
                    {
                        self.write(&json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32601, "message": "Method not found" },
                        }));
                    }
                }
            }
        }
    }

    /// Queues `message` for `agy`, one line.
    fn write(&self, message: &impl serde::Serialize) {
        let Ok(mut line) = serde_json::to_vec(message) else {
            return;
        };
        line.push(b'\n');
        // A writer that ended left with the companion, which the output says.
        let _ = self.writes.send(Bytes::from(line));
    }
}
