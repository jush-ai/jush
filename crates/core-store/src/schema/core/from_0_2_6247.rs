//! From 0.2.6247's core schema, which a server's split into one core per user
//! writes too: a schedule names the agent's turn that made it and the one that
//! last changed what it does, none for the schedules there.

use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(
    "ALTER TABLE schedules ADD COLUMN created_node TEXT;
     ALTER TABLE schedules ADD COLUMN created_turn TEXT;
     ALTER TABLE schedules ADD COLUMN edited_node TEXT;
     ALTER TABLE schedules ADD COLUMN edited_turn TEXT;",
);
