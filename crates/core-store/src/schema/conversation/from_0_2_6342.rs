//! From the conversation schema the SDK's release 0.2.6342 shipped: a call
//! names its assignment. Blocks are written in transcript block format 22
//! and checkpoints in checkpoint state format 13, whose messages, steers,
//! reports and agents' messages may name it; a record of format 21 still
//! reads as 22 and a state of format 12 as 13, each naming none, so neither
//! is rewritten. A running and an ended command keep the assignment their
//! call's work was part of, null for every command recorded before.

use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(
    "ALTER TABLE running_commands ADD COLUMN assignment_id TEXT;
     ALTER TABLE command_outputs ADD COLUMN assignment_id TEXT;",
);
