//! From the core schema the SDK's release 0.2.6342 shipped: a call names its
//! assignment, the core keeps a history of its agents' commands, and a
//! conversation names versions of the program's plugins. The usage ledger's
//! rows name the assignment their agent worked in, a kept wake its
//! message's, and a schedule an agent made whom and which assignment the
//! turn that made it worked for, which its runs' messages continue; null
//! for every row before. The command history starts empty, and every
//! conversation names no version.

use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(
    "ALTER TABLE usage_ledger ADD COLUMN assignment_id TEXT;
     ALTER TABLE pending_wakes ADD COLUMN message_assignment_id TEXT;
     ALTER TABLE schedules ADD COLUMN created_sender_id TEXT;
     ALTER TABLE schedules ADD COLUMN created_assignment_id TEXT;
     ALTER TABLE conversations ADD COLUMN plugin_versions TEXT;
     CREATE TABLE command_history (
       id              INTEGER PRIMARY KEY AUTOINCREMENT,
       conversation_id TEXT NOT NULL,
       agent           INTEGER CHECK (agent >= 0),
       command_id      TEXT,
       device_id       TEXT,
       turn_id         TEXT,
       sender_id       TEXT,
       assignment_id   TEXT,
       title           TEXT NOT NULL,
       script          TEXT NOT NULL,
       started_at      INTEGER,
       ended_at        INTEGER NOT NULL,
       ending          TEXT NOT NULL CHECK (ending IN ('exited', 'stopped', 'lost', 'not_started')),
       exit_code       INTEGER,
       reason          TEXT,
       CHECK ((ending = 'exited') = (exit_code IS NOT NULL)),
       CHECK ((ending IN ('lost', 'not_started')) = (reason IS NOT NULL)),
       CHECK ((ending = 'not_started') = (command_id IS NULL)),
       CHECK ((command_id IS NULL) = (device_id IS NULL))
     ) STRICT;
     CREATE UNIQUE INDEX command_history_command ON command_history (conversation_id, command_id);
     CREATE INDEX command_history_time ON command_history (ended_at);",
);
