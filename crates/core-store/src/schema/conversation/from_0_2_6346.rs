//! From the conversation schema the SDK's release 0.2.6346 shipped, whose
//! work named the id its program gave its sender: work names its program's
//! labels in its place. Blocks are written in transcript block format 23
//! and checkpoints in checkpoint state format 14, whose messages, steers,
//! reports and agents' messages name labels. A record of format 22 still
//! reads as 23, its sender dropped, so none is rewritten; the step drops
//! the sender from each node's state. A running and an ended command keep
//! their labels in place of their sender, null for every command recorded
//! before: no program relied on the sender yet, so none is carried over.

use rusqlite::Transaction;
use serde_json::Value;

use super::Migration;
use super::super::{ReadBlob, corrupt_text};

pub(super) const MIGRATION: Migration = Migration::Code(conversation_from_0_2_6346);

fn conversation_from_0_2_6346(transaction: &Transaction<'_>, _: &dyn ReadBlob) -> rusqlite::Result<()> {
    let states: Vec<(String, String)> = transaction
        .prepare("SELECT id, state FROM nodes")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut update = transaction.prepare("UPDATE nodes SET state = ?1 WHERE id = ?2")?;
    for (id, state) in states {
        let mut value: Value = serde_json::from_str(&state)
            .map_err(|error| corrupt_text(&format!("a node's state is not JSON: {error}")))?;
        let mut changed = false;
        for list in ["queue", "reports", "nextTurnReports"] {
            if let Some(Value::Array(items)) = value.get_mut(list) {
                for item in items {
                    changed |= sender_dropped(item);
                }
            }
        }
        if let Some(Value::Array(inputs)) = value.get_mut("agentInputs") {
            for input in inputs {
                if let Some(message) = input.get_mut("message") {
                    changed |= sender_dropped(message);
                }
            }
        }
        if changed {
            update.execute(rusqlite::params![value.to_string(), id])?;
        }
    }
    transaction.execute_batch(
        "ALTER TABLE running_commands DROP COLUMN sender_id;
         ALTER TABLE running_commands ADD COLUMN labels TEXT;
         ALTER TABLE command_outputs DROP COLUMN sender_id;
         ALTER TABLE command_outputs ADD COLUMN labels TEXT;",
    )
}

/// `item` without the sender it named; false for one that named none.
fn sender_dropped(item: &mut Value) -> bool {
    item.as_object_mut().and_then(|fields| fields.remove("senderId")).is_some()
}
