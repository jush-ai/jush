//! The Agent Client Protocol as Jush speaks it with `agy acp`
//! (<https://agentclientprotocol.com>): JSON-RPC 2.0, one message per line,
//! Jush's requests on `agy`'s standard input, its answers and notifications
//! on its standard output.

use bytes::Bytes;
use futures_util::StreamExt as _;
use futures_util::stream::LocalBoxStream;
use jush_device_interface::{ProcessControl, ProcessOutput};
use jush_provider_subscription_cli::{Tail, Unanswered};
use jush_shared_types::StreamKind;
use serde::Deserialize;
use serde_json::{Value, json};

/// The protocol version Jush speaks.
const PROTOCOL_VERSION: u32 = 1;

/// The error code of a request that needs the client to authenticate first.
const AUTH_REQUIRED: i64 = -32000;

/// The line of request `id`, `method` with `params`.
pub(crate) fn request(id: u64, method: &str, params: Value) -> Bytes {
    line(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
}

/// The `initialize` request: Jush offers `agy` no file system and no
/// terminal of its own.
fn initialize(id: u64) -> Bytes {
    request(
        id,
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false },
                "terminal": false,
            },
            "clientInfo": { "name": "jush", "version": env!("CARGO_PKG_VERSION") },
        }),
    )
}

/// What `session/new` answers, as far as Jush reads it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewSession {
    /// The models the account's plan offers.
    #[serde(default)]
    pub(crate) models: Option<SessionModels>,
    #[serde(default)]
    pub(crate) config_options: Vec<ConfigOption>,
    #[serde(default, rename = "_meta")]
    pub(crate) meta: Option<SessionMeta>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionModels {
    pub(crate) available_models: Vec<ModelInfo>,
    #[serde(default)]
    pub(crate) current_model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelInfo {
    /// Such as `gemini-4-neon-high`.
    pub(crate) model_id: String,
    /// Such as `Gemini 4 Neon (High)`.
    pub(crate) name: String,
}

/// A setting of the session, such as its thinking level, with the values it
/// takes.
#[derive(Debug, Deserialize)]
pub(crate) struct ConfigOption {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) options: Vec<ConfigValue>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConfigValue {
    pub(crate) value: String,
}

/// What `agy` adds to a new session's answer.
#[derive(Debug, Deserialize)]
pub(crate) struct SessionMeta {
    #[serde(default)]
    pub(crate) antigravity: Option<AntigravityMeta>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AntigravityMeta {
    /// The account's plan.
    #[serde(default)]
    pub(crate) tier: Option<Tier>,
}

/// A plan, such as `g1-pro-tier`, "Google AI Pro".
#[derive(Debug, Deserialize)]
pub(crate) struct Tier {
    pub(crate) id: String,
    pub(crate) name: String,
}

/// An error `agy` answered a request with.
#[derive(Deserialize)]
pub(crate) struct Refusal {
    code: i64,
    /// Why, in `agy`'s words.
    pub(crate) message: String,
}

impl Refusal {
    /// Whether `agy` has no sign-in Google takes, as when the account's was
    /// removed or revoked.
    pub(crate) fn asks_sign_in(&self) -> bool {
        self.code == AUTH_REQUIRED
    }
}

/// A line of `agy`'s output, as far as Jush reads it.
#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<Refusal>,
}

/// A line of `agy`'s output.
pub(crate) enum Line {
    /// The answer to Jush's request `id`.
    Answer {
        id: Value,
        answer: Result<Value, Refusal>,
    },
    /// A notification, such as a session's `session/update`, with its
    /// parameters.
    Notification { method: String, params: Value },
    /// A request of `agy`'s, which Jush answers.
    Request { id: Value, method: String, params: Value },
}

/// The `session/new` request, in the workspace `cwd`: the session's MCP
/// servers are the agent's.
fn new_session(id: u64, cwd: &str) -> Bytes {
    request(id, "session/new", json!({ "cwd": cwd, "mcpServers": [] }))
}

/// The `session/load` request of the session `session`, in `cwd`.
fn load_session(id: u64, session: &str, cwd: &str) -> Bytes {
    request(
        id,
        "session/load",
        json!({ "sessionId": session, "cwd": cwd, "mcpServers": [] }),
    )
}

/// The `session/set_model` request: the session's model from now on, at its
/// level, such as `gemini-4-neon-high`.
pub(crate) fn set_model(id: u64, session: &str, model: &str) -> Bytes {
    request(
        id,
        "session/set_model",
        json!({ "sessionId": session, "modelId": model }),
    )
}

/// The `session/prompt` request: a turn of the session, answering `prompt`,
/// ACP content blocks.
pub(crate) fn prompt(id: u64, session: &str, prompt: Vec<Value>) -> Bytes {
    request(
        id,
        "session/prompt",
        json!({ "sessionId": session, "prompt": prompt }),
    )
}

/// Writes request `line`, of id `id`, to the process `control` drives, and
/// reads its `output` until the answer: what `agy` answered, or refused.
///
/// A request of `agy`'s meanwhile is refused, and every other line passed
/// over, such as the history a load replays as updates, which the transcript
/// holds already.
pub(crate) async fn ask(
    control: &dyn ProcessControl,
    output: &mut Output,
    id: u64,
    line: Bytes,
) -> Result<Result<Value, Refusal>, Unanswered> {
    let written = |error| Unanswered::Failed(format!("Antigravity's input could not be written: {error}"));
    control.write_stdin(line).await.map_err(written)?;
    let id = Value::from(id);
    loop {
        match output.line().await {
            Some(Line::Answer { id: answered, answer }) if answered == id => return Ok(answer),
            Some(Line::Request { id, .. }) => control.write_stdin(refuse(id)).await.map_err(written)?,
            Some(_) => {}
            None => return Err(Unanswered::Ended(output.stderr())),
        }
    }
}

/// The request ids of opening a session, in their order; a process's later
/// requests count on from [`FIRST_FREE`].
const INITIALIZE: u64 = 1;
const SESSION: u64 = 2;
pub(crate) const FIRST_FREE: u64 = 3;

/// Opens a session whose workspace is `cwd` in a new process: `initialize`,
/// then `session/load` of `load`, else `session/new`. What the session's
/// request answered, or how `agy` refused either.
pub(crate) async fn open(
    control: &dyn ProcessControl,
    output: &mut Output,
    cwd: &str,
    load: Option<&str>,
) -> Result<Result<Value, Refusal>, Unanswered> {
    if let Err(refusal) = ask(control, output, INITIALIZE, initialize(INITIALIZE)).await? {
        return Ok(Err(refusal));
    }
    let session = match load {
        Some(session) => load_session(SESSION, session, cwd),
        None => new_session(SESSION, cwd),
    };
    ask(control, output, SESSION, session).await
}

/// The answer that refuses `agy`'s request `id`: Jush answers only a
/// permission request (`permission`), since it gave `agy` no file system and
/// no terminal.
pub(crate) fn refuse(id: Value) -> Bytes {
    line(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32601, "message": "Method not found" },
    }))
}

/// The answer to `session/request_permission` `id`, whose parameters are
/// `params`, and whether it allows the call: the option of the kind that
/// allows the call once, or denies it once. A request that offers no such
/// option is refused, which `agy` takes as a denial.
pub(crate) fn permission(id: Value, params: &Value, allow: bool) -> (Bytes, bool) {
    let kind = if allow { "allow_once" } else { "reject_once" };
    let option = params["options"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| option["kind"] == kind)
        .and_then(|option| option["optionId"].as_str());
    match option {
        Some(option) => {
            let line = line(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "outcome": { "outcome": "selected", "optionId": option } },
            }));
            (line, allow)
        }
        None => (refuse(id), false),
    }
}

fn line(message: &Value) -> Bytes {
    let mut line = message.to_string().into_bytes();
    line.push(b'\n');
    line.into()
}

/// Bytes a process wrote, taken a whole line at a time.
#[derive(Default)]
pub(crate) struct Lines(Vec<u8>);

impl Lines {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// The next whole line, its newline included.
    pub(crate) fn next(&mut self) -> Option<Vec<u8>> {
        let end = self.0.iter().position(|byte| *byte == b'\n')?;
        Some(self.0.drain(..=end).collect())
    }
}

/// `agy`'s output, read line by line.
pub(crate) struct Output {
    stream: LocalBoxStream<'static, ProcessOutput>,
    stdout: Lines,
    stderr: Tail,
}

impl Output {
    pub(crate) fn new(stream: LocalBoxStream<'static, ProcessOutput>) -> Self {
        Self {
            stream,
            stdout: Lines::default(),
            stderr: Tail::default(),
        }
    }

    /// The next line `agy` wrote; none once its output ended. A line that is
    /// no JSON-RPC message is passed over. Dropping the future loses nothing.
    pub(crate) async fn line(&mut self) -> Option<Line> {
        loop {
            while let Some(line) = self.stdout.next() {
                let Ok(message) = serde_json::from_slice::<Message>(&line) else {
                    continue;
                };
                let line = match (message.id, message.method) {
                    (Some(id), Some(method)) => Line::Request {
                        id,
                        method,
                        params: message.params.unwrap_or(Value::Null),
                    },
                    (None, Some(method)) => Line::Notification {
                        method,
                        params: message.params.unwrap_or(Value::Null),
                    },
                    (Some(id), None) => Line::Answer {
                        id,
                        answer: match message.error {
                            Some(refusal) => Err(refusal),
                            None => Ok(message.result.unwrap_or(Value::Null)),
                        },
                    },
                    (None, None) => continue,
                };
                return Some(line);
            }
            let chunk = self.stream.next().await?;
            match chunk.stream {
                StreamKind::Stdout => self.stdout.push(&chunk.bytes),
                StreamKind::Stderr => self.stderr.push(&chunk.bytes),
            }
        }
    }

    /// The tail of what `agy` wrote to its standard error.
    pub(crate) fn stderr(&self) -> String {
        self.stderr.text()
    }
}
