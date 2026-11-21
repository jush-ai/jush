//! A session's history, written into the database of a session `agy` made
//! (`live.rs`), so that a new process loads the conversation as Jush's
//! transcript holds it.
//!
//! `agy` keeps a session in an SQLite database of its own, one row of `steps`
//! per step, each a protobuf message. A database built by hand loads, but its
//! session runs `agy`'s default agent, so `agy` makes the session, with Jush's
//! agent, and the provider adds the steps:
//!
//! - a user message or a steer is a user step;
//! - the model's text, with the calls that follow it, is a response step;
//! - a call's result is an MCP tool step, which names the call again.
//!
//! Reasoning is left out: `agy` gives its text without Google's signature, and
//! a step holds only what `agy` itself would send back. A medium is named in
//! text, since the format of one in a step is unknown.
//!
//! The format is `agy`'s own and undocumented; these are the fields its
//! messages carry for each step (`agy` 1.5.13).

use std::collections::HashMap;

use bytes::Bytes;
use jush_provider_common::{InferenceItem, Medium, ResultPart, UserPart};
use jush_provider_subscription_cli::unsent_text;
use rusqlite::{Connection, MAIN_DB};
use serde_json::Value;

use crate::tools::SERVER;

/// The step types and the status of a finished step.
const USER_INPUT: u64 = 14;
const PLANNER_RESPONSE: u64 = 15;
const MCP_TOOL: u64 = 132;
const DONE: u64 = 3;

/// The bytes of an SQLite header that name its journal: 2 for WAL, which
/// `agy` uses, and 1 for a rollback journal, which an in-memory copy needs.
const JOURNAL_VERSIONS: std::ops::Range<usize> = 18..20;
const WAL: u8 = 2;
const ROLLBACK: u8 = 1;

/// `database`, the one `agy` made for a session, with the steps of `items`.
///
/// The database is the file alone: `agy` checkpoints its write-ahead log into
/// it when it ends (measured), and its process has ended before the file is
/// read.
pub(crate) fn fill(database: &[u8], items: &[InferenceItem]) -> Result<Bytes, String> {
    let steps = steps(items);
    let mut image = database.to_vec();
    let wal = image.get(JOURNAL_VERSIONS) == Some(&[WAL, WAL][..]);
    if wal {
        image[JOURNAL_VERSIONS].fill(ROLLBACK);
    }
    let unreadable = |error: rusqlite::Error| format!("Antigravity's session cannot be written: {error}");
    let mut connection = Connection::open_in_memory().map_err(unreadable)?;
    connection
        .deserialize_read_exact(MAIN_DB, image.as_slice(), image.len(), false)
        .map_err(unreadable)?;
    {
        let transaction = connection.transaction().map_err(unreadable)?;
        let first: i64 = transaction
            .query_row("SELECT COALESCE(MAX(idx) + 1, 0) FROM steps", [], |row| row.get(0))
            .map_err(unreadable)?;
        {
            let mut insert = transaction
                .prepare("INSERT INTO steps (idx, step_type, status, step_payload) VALUES (?1, ?2, ?3, ?4)")
                .map_err(unreadable)?;
            for (index, (kind, payload)) in (first..).zip(steps) {
                insert
                    .execute(rusqlite::params![index, kind as i64, DONE as i64, payload])
                    .map_err(unreadable)?;
            }
        }
        transaction.commit().map_err(unreadable)?;
    }
    let mut filled = connection.serialize(MAIN_DB).map_err(unreadable)?.to_vec();
    if wal && filled.len() >= JOURNAL_VERSIONS.end {
        filled[JOURNAL_VERSIONS].fill(WAL);
    }
    Ok(Bytes::from(filled))
}

/// The steps of `items`, each its type and its payload.
fn steps(items: &[InferenceItem]) -> Vec<(u64, Vec<u8>)> {
    let mut steps = Vec::new();
    // The response being written: its text and its calls.
    let mut response: Option<Response> = None;
    // Each call's message and input, by its id, which its result's step
    // names again.
    let mut calls: HashMap<&str, (Vec<u8>, &Value)> = HashMap::new();
    for item in items {
        match item {
            InferenceItem::UserMessage { content } | InferenceItem::UserSteer { content } => {
                end_response(&mut response, &mut steps);
                let text = user_text(content);
                let body = Message::default()
                    .text(2, &text)
                    .message(3, Message::default().text(1, &text));
                steps.push((USER_INPUT, step(USER_INPUT).message(19, body).0));
            }
            InferenceItem::AssistantText { text, .. } => {
                // Text after a call starts a response of its own.
                if response.as_ref().is_some_and(|response| !response.calls.is_empty()) {
                    end_response(&mut response, &mut steps);
                }
                response.get_or_insert_default().text.push_str(text);
            }
            InferenceItem::ToolUse {
                tool_use_id,
                tool_name,
                input,
                ..
            } => {
                let call = Message::default()
                    .text(1, tool_use_id)
                    .text(2, &format!("mcp_{SERVER}_{tool_name}"))
                    .text(3, &input.to_string())
                    .0;
                calls.insert(tool_use_id, (call.clone(), input));
                response.get_or_insert_default().calls.push(call);
            }
            InferenceItem::ToolResult {
                tool_use_id,
                output,
                ..
            } => {
                end_response(&mut response, &mut steps);
                // A result whose call is not in the history, as one a summary
                // cut off from it, has no call to name.
                let Some((call, input)) = calls.get(tool_use_id.as_str()) else {
                    continue;
                };
                let mut arguments = Message::default();
                for (name, value) in input.as_object().into_iter().flatten() {
                    let value = match value {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    arguments = arguments.message(1, Message::default().text(1, name).text(2, &value));
                }
                let result = arguments.message(2, Message::default().text(1, &result_text(output)));
                let payload = step(MCP_TOOL)
                    .message(5, Message::default().bytes(4, call))
                    .message(140, result)
                    .0;
                steps.push((MCP_TOOL, payload));
            }
            InferenceItem::AssistantThinking { .. } | InferenceItem::Opaque { .. } => {}
        }
    }
    end_response(&mut response, &mut steps);
    steps
}

/// A response step being written: the model's text and the calls after it.
#[derive(Default)]
struct Response {
    text: String,
    calls: Vec<Vec<u8>>,
}

/// Writes the response being written, if any, as a step.
fn end_response(response: &mut Option<Response>, steps: &mut Vec<(u64, Vec<u8>)>) {
    let Some(Response { text, calls }) = response.take() else {
        return;
    };
    let mut body = Message::default().text(1, &text);
    for call in &calls {
        body = body.bytes(7, call);
    }
    steps.push((PLANNER_RESPONSE, step(PLANNER_RESPONSE).message(20, body).0));
}

/// The common fields of a step of `kind`: its type and its status.
fn step(kind: u64) -> Message {
    Message::default().varint(1, kind).varint(4, DONE)
}

/// A user message's content as one text, a medium named in it.
fn user_text(content: &[UserPart]) -> String {
    content.iter().map(part_text).collect::<Vec<_>>().join("\n")
}

/// A part of a user message as text: its text, or a medium named as a tool's
/// result names one (`unsent_text`), or by its URL.
pub(crate) fn part_text(part: &UserPart) -> String {
    match part {
        UserPart::Text(text) => text.clone(),
        UserPart::Image(Medium::Bytes(bytes)) | UserPart::Video(Medium::Bytes(bytes)) => {
            unsent_text(bytes)
        }
        UserPart::Image(Medium::Url(url)) | UserPart::Video(Medium::Url(url)) => format!("[{url}]"),
        UserPart::Document { bytes, .. } => unsent_text(bytes),
    }
}

/// A tool's result as one text, a medium named in it.
fn result_text(output: &[ResultPart]) -> String {
    output
        .iter()
        .map(|part| match part {
            ResultPart::Text(text) => text.clone(),
            ResultPart::Image(bytes) | ResultPart::Video(bytes) | ResultPart::Document { bytes, .. } => {
                unsent_text(bytes)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A protobuf message being written, field by field.
#[derive(Default)]
struct Message(Vec<u8>);

impl Message {
    fn key(&mut self, field: u64, wire: u64) {
        write_varint(&mut self.0, (field << 3) | wire);
    }

    fn varint(mut self, field: u64, value: u64) -> Self {
        self.key(field, 0);
        write_varint(&mut self.0, value);
        self
    }

    fn bytes(mut self, field: u64, bytes: &[u8]) -> Self {
        self.key(field, 2);
        write_varint(&mut self.0, bytes.len() as u64);
        self.0.extend_from_slice(bytes);
        self
    }

    fn text(self, field: u64, text: &str) -> Self {
        self.bytes(field, text.as_bytes())
    }

    fn message(self, field: u64, message: Message) -> Self {
        self.bytes(field, &message.0)
    }
}

fn write_varint(into: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            into.push(byte);
            return;
        }
        into.push(byte | 0x80);
    }
}
