//! The conversations whose work a graceful shutdown began to interrupt: a
//! root's running turn, or only its subagents' while the root rested.
//!
//! The shutdown marks each such conversation before it aborts the work, so a
//! shutdown cut short still leaves a mark for every one it began to interrupt.
//! The next start finds them here, without reading any conversation's database,
//! and resumes each once.
//!
//! A crash writes no mark. A mark goes with its conversation.

use jush_web_api_protocol::ids::ConversationId;

use super::StorageError;
use super::columns::decode;
use super::store::CoreStore;

/// A conversation a shutdown marked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedTurn {
    pub conversation: ConversationId,
    /// Its root's turn ran; false when only its subagents worked.
    pub root: bool,
}

impl CoreStore {
    /// Marks the conversation as one whose work the shutdown is about to
    /// interrupt: its root's running turn when `root`, else only its
    /// subagents'. A conversation that is gone is not marked.
    pub async fn mark_interrupted_turn(&self, conversation: ConversationId, root: bool) -> Result<(), StorageError> {
        self.call(move |connection, _| {
            connection.execute(
                "INSERT INTO interrupted_turns (conversation_id, root)
                   SELECT id, ?2 FROM conversations WHERE id = ?1
                   ON CONFLICT (conversation_id) DO NOTHING",
                rusqlite::params![conversation.as_str(), root],
            )?;
            Ok(())
        })
        .await
    }

    /// The marked conversations.
    pub async fn interrupted_turns(&self) -> Result<Vec<InterruptedTurn>, StorageError> {
        self.call(move |connection, _| {
            let mut statement = connection.prepare(
                "SELECT interrupted_turns.conversation_id, interrupted_turns.root FROM interrupted_turns
                   JOIN conversations ON conversations.id = interrupted_turns.conversation_id
                   ORDER BY interrupted_turns.conversation_id",
            )?;
            let mut rows = statement.query([])?;
            let mut marked = Vec::new();
            while let Some(row) = rows.next()? {
                marked.push(InterruptedTurn {
                    conversation: decode(
                        "interrupted_turns",
                        "conversation_id",
                        ConversationId::try_from(row.get::<_, String>(0)?),
                    )?,
                    root: row.get(1)?,
                });
            }
            Ok(marked)
        })
        .await
    }

    /// Removes the conversation's mark, as its start takes it up; false
    /// when it is no longer there.
    pub async fn take_interrupted_turn(&self, conversation: ConversationId) -> Result<bool, StorageError> {
        self.call(move |connection, _| {
            let removed = connection.execute(
                "DELETE FROM interrupted_turns WHERE conversation_id = ?1",
                [conversation.as_str()],
            )?;
            Ok(removed == 1)
        })
        .await
    }
}
