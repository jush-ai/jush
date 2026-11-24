//! From the conversation schema the SDK's releases 0.2.6247 to 0.2.6332
//! shipped: a message names its sender. Blocks are written in transcript
//! block format 21 and checkpoints in checkpoint state format 12, whose
//! `user` and `steer` blocks and queued messages may name who sent them; a
//! record of format 20 still reads as 21 and a state of format 11 as 12, each
//! naming no one, so neither is rewritten. A running command keeps whom its
//! call's command is counted for, and an ended one whom its work was for,
//! null for every command recorded before.

use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(
    "ALTER TABLE running_commands ADD COLUMN sender_id TEXT;
     ALTER TABLE command_outputs ADD COLUMN sender_id TEXT;",
);
