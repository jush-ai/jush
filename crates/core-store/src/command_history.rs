//! The command history: a row for each command an agent's `shell` call
//! started, written as the command ends, and for each such call that started
//! none, as the call returns its error.
//!
//! A program reads it in the order the rows were written, which is the order
//! their commands ended, by the cursor of the last row it read. A row's id
//! is its cursor, and ids never repeat, so a reader that follows its cursor
//! reads each row once, also rows written while it reads.
//!
//! The history keeps each script again, as the usage ledger keeps tokens the
//! transcripts hold, so that a reader opens no conversation's database. A
//! command's row is written before its conversation records its end
//! (`crates/core-device-access/src/shells.rs`): a backend that stops in
//! between ends the command again, and the row the history holds already
//! stays, one per command. A row is written only while its conversation
//! exists, and a conversation's deletion removes its rows
//! (`deletions.rs`).

use jush_shared_types::{CommandId, DeviceId, Timestamp, TurnId};
use jush_web_api_protocol::command_history::{CommandHistory, CommandHistoryEnd, CommandHistoryEntry};
use jush_web_api_protocol::ids::ConversationId;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::StorageError;
use super::columns::{decode, instant};
use super::store::CoreStore;

const TABLE: &str = "command_history";

const COLUMNS: &str = "id, conversation_id, agent, command_id, device_id, turn_id, sender_id, assignment_id, title,
     script, started_at, ended_at, ending, exit_code, reason";

/// Where a read of the history starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryStart {
    /// After the row of this cursor.
    After(Cursor),
    /// At the first row that ended at or after this moment.
    From(Timestamp),
    /// At the first row.
    First,
}

/// A row's place in the history, which a reader passes back to read on:
/// opaque to it, the row's id within; 0 before the first row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor(i64);

impl std::str::FromStr for Cursor {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        text.parse::<i64>().ok().filter(|id| *id >= 0).map(Self).ok_or(())
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl CoreStore {
    /// Writes `entry`, unless its conversation is gone or the history holds
    /// its command's row already.
    pub async fn record_command(&self, entry: CommandHistoryEntry) -> Result<(), StorageError> {
        self.call(move |connection, _| {
            let (ending, exit_code, reason) = match &entry.end {
                CommandHistoryEnd::Exited { exit_code } => ("exited", Some(*exit_code), None),
                CommandHistoryEnd::Stopped => ("stopped", None, None),
                CommandHistoryEnd::Lost { reason } => ("lost", None, Some(reason.as_str())),
                CommandHistoryEnd::NotStarted { reason } => ("not_started", None, Some(reason.as_str())),
            };
            // The row names the conversation as its record spells it, which
            // its deletion names.
            connection
                .prepare_cached(
                    "INSERT INTO command_history (conversation_id, agent, command_id, device_id, turn_id, sender_id,
                       assignment_id, title, script, started_at, ended_at, ending, exit_code, reason)
                     SELECT id, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14 FROM conversations WHERE id = ?1
                     ON CONFLICT (conversation_id, command_id) DO NOTHING",
                )?
                .execute(params![
                    entry.conversation.as_str(),
                    entry.agent.map(|agent| i64::try_from(agent).unwrap_or(i64::MAX)),
                    entry.command.as_ref().map(CommandId::as_str),
                    entry.device.as_ref().map(DeviceId::as_str),
                    entry.turn.as_ref().map(TurnId::as_str),
                    entry.sender,
                    entry.assignment.as_ref().map(TurnId::as_str),
                    entry.title,
                    entry.script,
                    entry.started_at.map(Timestamp::as_millisecond),
                    entry.ended_at.as_millisecond(),
                    ending,
                    exit_code,
                    reason,
                ])?;
            Ok(())
        })
        .await
    }

    /// The rows from `start` in the order written, `limit` of them at most;
    /// none for a cursor past every row the history ever held, which no page
    /// of it gave.
    pub async fn command_history(&self, start: HistoryStart, limit: u32) -> Result<Option<CommandHistory>, StorageError> {
        self.call(move |connection, _| history(connection, start, limit)).await
    }
}

/// Removes the rows of the conversation `id`, as its record spells it.
pub(crate) fn forget(connection: &Connection, id: &ConversationId) -> Result<(), StorageError> {
    connection.execute("DELETE FROM command_history WHERE conversation_id = ?1", [id.as_str()])?;
    Ok(())
}

fn history(connection: &Connection, start: HistoryStart, limit: u32) -> Result<Option<CommandHistory>, StorageError> {
    // The newest id the history ever gave, whose row a deletion may have
    // removed since; 0 before its first row.
    let newest: i64 = connection
        .prepare_cached("SELECT seq FROM sqlite_sequence WHERE name = 'command_history'")?
        .query_row([], |row| row.get(0))
        .optional()?
        .unwrap_or(0);
    // The id after which the page starts.
    let after = match start {
        HistoryStart::After(Cursor(id)) if id > newest => return Ok(None),
        HistoryStart::After(Cursor(id)) => id,
        // The first row written of those that ended at `from` or after,
        // which a row that ended earlier may follow; past the newest when
        // none did, where a reader goes on.
        HistoryStart::From(from) => connection
            .prepare_cached("SELECT MIN(id) FROM command_history WHERE ended_at >= ?1")?
            .query_row([from.as_millisecond()], |row| row.get::<_, Option<i64>>(0))?
            .map_or(newest, |first| first - 1),
        HistoryStart::First => 0,
    };
    let mut statement = connection.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM command_history WHERE id > ?1 ORDER BY id LIMIT ?2"
    ))?;
    let mut rows = statement.query(params![after, i64::from(limit) + 1])?;
    let mut commands = Vec::new();
    let mut last = after;
    let mut more = false;
    while let Some(row) = rows.next()? {
        if commands.len() == limit as usize {
            more = true;
            break;
        }
        last = row.get("id")?;
        commands.push(entry(row)?);
    }
    Ok(Some(CommandHistory { commands, next: Cursor(last).to_string(), more }))
}

fn entry(row: &Row<'_>) -> Result<CommandHistoryEntry, StorageError> {
    let text = |column: &'static str| row.get::<_, Option<String>>(column);
    let reason = || -> Result<String, StorageError> { decode(TABLE, "reason", text("reason")?.ok_or("its end names a reason")) };
    let end = match row.get::<_, String>("ending")?.as_str() {
        "exited" => CommandHistoryEnd::Exited {
            exit_code: decode(TABLE, "exit_code", row.get::<_, Option<i32>>("exit_code")?.ok_or("an exit names its code"))?,
        },
        "stopped" => CommandHistoryEnd::Stopped,
        "lost" => CommandHistoryEnd::Lost { reason: reason()? },
        "not_started" => CommandHistoryEnd::NotStarted { reason: reason()? },
        other => return Err(StorageError::Corrupt { table: TABLE, column: "ending", reason: format!("{other} is no end") }),
    };
    Ok(CommandHistoryEntry {
        conversation: decode(TABLE, "conversation_id", ConversationId::try_from(row.get::<_, String>("conversation_id")?))?,
        agent: row
            .get::<_, Option<i64>>("agent")?
            .map(|agent| decode(TABLE, "agent", u64::try_from(agent)))
            .transpose()?,
        command: text("command_id")?.map(|id| decode(TABLE, "command_id", CommandId::try_from(id))).transpose()?,
        device: text("device_id")?.map(|id| decode(TABLE, "device_id", DeviceId::try_from(id))).transpose()?,
        turn: text("turn_id")?.map(|id| decode(TABLE, "turn_id", TurnId::try_from(id))).transpose()?,
        sender: text("sender_id")?,
        assignment: text("assignment_id")?.map(|id| decode(TABLE, "assignment_id", TurnId::try_from(id))).transpose()?,
        title: row.get("title")?,
        script: row.get("script")?,
        started_at: row
            .get::<_, Option<i64>>("started_at")?
            .map(|at| decode(TABLE, "started_at", Timestamp::from_millisecond(at)))
            .transpose()?,
        ended_at: instant(row, TABLE, "ended_at")?,
        end,
    })
}
