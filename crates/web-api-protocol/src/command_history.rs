//! The core's command history (`crates/core-store/src/command_history.rs`) as
//! a program reads it: each command its agents' `shell` calls started, as
//! it ended, and each such call that started none, in the order its rows
//! were written.

use jush_shared_types::{CommandEnd, CommandId, DeviceId, Labels, MAX_SAFE_INTEGER, Timestamp, TurnId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::ConversationId;

/// `?after=&from=&limit=` of `GET /commands/history`: the rows recorded
/// after the cursor `after`, or from the first that ended at or after
/// `from`, or from the first; `limit` of them at most. Queries are the
/// backend's alone and are not emitted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandHistoryQuery {
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub from: Option<Timestamp>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// A page of the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandHistory {
    pub commands: Vec<CommandHistoryEntry>,
    /// Where the page ends, which `after` takes to read on: its last row's
    /// place; for an empty page, the `after` given, the newest row's when
    /// nothing ended at `from` or after, or the place before the first row.
    pub next: String,
    /// Whether rows were written after the page.
    pub more: bool,
}

/// A command an agent's `shell` call started, as it ended, or a call that
/// started none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandHistoryEntry {
    pub conversation: ConversationId,
    /// The number its conversation knows the call's agent by, 0 for the
    /// main agent; none for a side question's.
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub agent: Option<u64>,
    /// None for a call that started no command.
    pub command: Option<CommandId>,
    /// The device it ran on; none for a call that started no command.
    pub device: Option<DeviceId>,
    /// The turn of the call.
    pub turn: Option<TurnId>,
    /// The program's labels of the call's work.
    pub labels: Labels,
    /// The assignment the call's work was part of.
    pub assignment: Option<TurnId>,
    pub title: String,
    pub script: String,
    /// When its call started it, a wait for its device to come online
    /// included; none for a call that started no command, and for a
    /// command that ended before its start was recorded.
    pub started_at: Option<Timestamp>,
    pub ended_at: Timestamp,
    pub end: CommandHistoryEnd,
}

/// How a command ended, or why a call started none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum CommandHistoryEnd {
    Exited { exit_code: i32 },
    /// It was stopped: by a page, `jush shell stop`, a Stop of its action,
    /// or the end of its node's shells.
    Stopped,
    /// Its device lost it.
    Lost { reason: String },
    /// The call returned this error before its command started.
    NotStarted { reason: String },
}

impl CommandHistoryEnd {
    /// How a command ended as its conversation records it; none for a
    /// record that kept no end, which no command that ends now makes.
    pub fn of(end: &CommandEnd) -> Option<Self> {
        match end {
            CommandEnd::Exited { exit_code } => Some(Self::Exited { exit_code: *exit_code }),
            CommandEnd::Stopped => Some(Self::Stopped),
            CommandEnd::Lost { reason } => Some(Self::Lost { reason: reason.clone() }),
            CommandEnd::Unrecorded => None,
        }
    }
}
