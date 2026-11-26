//! The wakes a message to another conversation asked for (`jush conversation
//! send --wake-when-done`), kept from the message's delivery until its sender
//! is woken, so a restart of the backend loses none
//! (`crates/core/src/conversation/wakes.rs`).
//!
//! A wake is first open: the work the message started runs. Once it ends,
//! the wake holds the notice to deliver, which a failed delivery keeps, so
//! the next try sends the same message, which the sender's agent takes once.
//! Either conversation's deletion drops it.

use jush_shared_types::{AgentMessage, BlockId, NodeId, TurnId};
use jush_web_api_protocol::ids::ConversationId;
use rusqlite::{Row, params};

use super::StorageError;
use super::columns::{decode, json, to_json};
use super::store::CoreStore;

const TABLE: &str = "pending_wakes";

const COLUMNS: &str = "recipient_id, message_id, sender_id, sender_node, notice, message_sender_id, message_assignment_id";

/// A wake a message asked for, until its sender is woken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWake {
    /// The conversation the message went to, and its block there.
    pub recipient: ConversationId,
    pub message: BlockId,
    /// The conversation that sent it, and its agent that the end wakes.
    pub sender: ConversationId,
    pub sender_node: NodeId,
    /// The notice to deliver, once the work the message started ended.
    pub notice: Option<AgentMessage>,
    /// Whom the message's work is for, by the id its program gave its
    /// sender, whom the notice's turn is then counted for.
    pub message_sender_id: Option<String>,
    /// The assignment the message's work is part of, which the notice's
    /// turn then continues.
    pub message_assignment_id: Option<TurnId>,
}

impl CoreStore {
    /// Keeps `wake`, open; a repeated one changes nothing.
    pub async fn keep_wake(&self, wake: PendingWake) -> Result<(), StorageError> {
        self.call(move |connection, _| {
            connection.execute(
                &format!("INSERT OR IGNORE INTO pending_wakes ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"),
                params![
                    wake.recipient.as_str(),
                    wake.message.as_str(),
                    wake.sender.as_str(),
                    wake.sender_node.as_str(),
                    wake.notice.as_ref().map(to_json),
                    wake.message_sender_id,
                    wake.message_assignment_id.as_ref().map(TurnId::as_str),
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// The wakes the messages `recipient` took asked for, in the order they
    /// were kept.
    pub async fn pending_wakes(&self, recipient: ConversationId) -> Result<Vec<PendingWake>, StorageError> {
        self.call(move |connection, _| {
            let mut statement = connection
                .prepare_cached(&format!("SELECT {COLUMNS} FROM pending_wakes WHERE recipient_id = ?1 ORDER BY rowid"))?;
            let mut rows = statement.query(params![recipient.as_str()])?;
            let mut wakes = Vec::new();
            while let Some(row) = rows.next()? {
                wakes.push(wake(row)?);
            }
            Ok(wakes)
        })
        .await
    }

    /// The wakes whose notice waits for delivery.
    pub async fn undelivered_wakes(&self) -> Result<Vec<PendingWake>, StorageError> {
        self.call(move |connection, _| {
            let mut statement = connection.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM pending_wakes WHERE notice IS NOT NULL ORDER BY rowid"
            ))?;
            let mut rows = statement.query([])?;
            let mut wakes = Vec::new();
            while let Some(row) = rows.next()? {
                wakes.push(wake(row)?);
            }
            Ok(wakes)
        })
        .await
    }

    /// Gives the wake of `recipient`'s message `message` the notice to
    /// deliver.
    pub async fn settle_wake(
        &self,
        recipient: ConversationId,
        message: BlockId,
        notice: AgentMessage,
    ) -> Result<(), StorageError> {
        self.call(move |connection, _| {
            connection.execute(
                "UPDATE pending_wakes SET notice = ?3 WHERE recipient_id = ?1 AND message_id = ?2",
                params![recipient.as_str(), message.as_str(), to_json(&notice)],
            )?;
            Ok(())
        })
        .await
    }

    /// Drops the wake of `recipient`'s message `message`, once its sender
    /// is woken or gone.
    pub async fn forget_wake(&self, recipient: ConversationId, message: BlockId) -> Result<(), StorageError> {
        self.call(move |connection, _| {
            connection.execute(
                "DELETE FROM pending_wakes WHERE recipient_id = ?1 AND message_id = ?2",
                params![recipient.as_str(), message.as_str()],
            )?;
            Ok(())
        })
        .await
    }
}

fn wake(row: &Row<'_>) -> Result<PendingWake, StorageError> {
    Ok(PendingWake {
        recipient: decode(TABLE, "recipient_id", ConversationId::try_from(row.get::<_, String>("recipient_id")?))?,
        message: decode(TABLE, "message_id", BlockId::try_from(row.get::<_, String>("message_id")?))?,
        sender: decode(TABLE, "sender_id", ConversationId::try_from(row.get::<_, String>("sender_id")?))?,
        sender_node: decode(TABLE, "sender_node", NodeId::try_from(row.get::<_, String>("sender_node")?))?,
        notice: row
            .get::<_, Option<String>>("notice")?
            .map(|text| json(TABLE, "notice", &text))
            .transpose()?,
        message_sender_id: row.get("message_sender_id")?,
        message_assignment_id: row
            .get::<_, Option<String>>("message_assignment_id")?
            .map(|assignment| decode(TABLE, "message_assignment_id", TurnId::try_from(assignment)))
            .transpose()?,
    })
}
