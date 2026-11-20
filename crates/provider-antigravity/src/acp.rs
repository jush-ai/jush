//! The Agent Client Protocol as Jush speaks it with `agy acp`
//! (<https://agentclientprotocol.com>): JSON-RPC 2.0, one message per line,
//! Jush's requests on `agy`'s standard input, its answers and notifications
//! on its standard output.

use bytes::Bytes;
use futures_util::StreamExt as _;
use futures_util::stream::LocalBoxStream;
use jush_device_interface::ProcessOutput;
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
    let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let mut line = message.to_string().into_bytes();
    line.push(b'\n');
    line.into()
}

/// The `initialize` request: Jush offers `agy` no file system and no
/// terminal of its own.
pub(crate) fn initialize(id: u64) -> Bytes {
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

/// A line of `agy`'s output, as far as an answer is read from it.
#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<Refusal>,
}

/// `agy`'s output, read line by line.
pub(crate) struct Output {
    stream: LocalBoxStream<'static, ProcessOutput>,
    stdout: Vec<u8>,
    stderr: Tail,
}

impl Output {
    pub(crate) fn new(stream: LocalBoxStream<'static, ProcessOutput>) -> Self {
        Self {
            stream,
            stdout: Vec::new(),
            stderr: Tail::default(),
        }
    }

    /// Reads until the answer to request `id`, passing over every other
    /// line, notifications among them.
    pub(crate) async fn answer(&mut self, id: u64) -> Result<Result<Value, Refusal>, Unanswered> {
        let id = Value::from(id);
        loop {
            while let Some(end) = self.stdout.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.stdout.drain(..=end).collect();
                let Ok(message) = serde_json::from_slice::<Message>(&line) else {
                    continue;
                };
                if message.method.is_some() || message.id.as_ref() != Some(&id) {
                    continue;
                }
                if let Some(refusal) = message.error {
                    return Ok(Err(refusal));
                }
                return Ok(Ok(message.result.unwrap_or(Value::Null)));
            }
            let Some(chunk) = self.stream.next().await else {
                return Err(Unanswered::Ended(self.stderr.text()));
            };
            match chunk.stream {
                StreamKind::Stdout => self.stdout.extend_from_slice(&chunk.bytes),
                StreamKind::Stderr => self.stderr.push(&chunk.bytes),
            }
        }
    }
}
