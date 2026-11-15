//! From the control schema before a conversation heard when another one
//! finishes:
//!
//! - A message to another conversation can ask to wake its sender when the
//!   turn that takes it ends; each such wake is kept until then
//!   (`pending_wakes`), which starts empty.
//! - A schedule runs when another conversation finishes, a run's cause
//!   `done`. The runs' table is rebuilt, since its check names the causes.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_WAKES);

const CONTROL_FROM_0_1_65_DEV_WAKES: &str = "
CREATE TABLE schedule_runs_next (
  id          TEXT PRIMARY KEY,
  schedule_id TEXT NOT NULL REFERENCES schedules (id) ON DELETE CASCADE,
  cause       TEXT NOT NULL CHECK (cause IN ('time', 'change', 'url', 'done', 'now')),
  at          INTEGER NOT NULL,
  state       TEXT NOT NULL CHECK (state IN ('running', 'done', 'notified', 'skipped', 'failed')),
  note        TEXT,
  line        TEXT,
  turn_id     TEXT,
  notified    INTEGER NOT NULL CHECK (notified IN (0, 1)),
  task_id     TEXT
) STRICT;
INSERT INTO schedule_runs_next (id, schedule_id, cause, at, state, note, line, turn_id, notified, task_id)
  SELECT id, schedule_id, cause, at, state, note, line, turn_id, notified, task_id FROM schedule_runs;
DROP TABLE schedule_runs;
ALTER TABLE schedule_runs_next RENAME TO schedule_runs;
CREATE INDEX schedule_runs_by_schedule ON schedule_runs (schedule_id, at);
CREATE TABLE pending_wakes (
  recipient_id TEXT NOT NULL COLLATE NOCASE REFERENCES conversations (id) ON DELETE CASCADE,
  message_id   TEXT NOT NULL,
  sender_id    TEXT NOT NULL COLLATE NOCASE REFERENCES conversations (id) ON DELETE CASCADE,
  sender_node  TEXT NOT NULL,
  notice       TEXT,
  PRIMARY KEY (recipient_id, message_id)
) STRICT;
CREATE INDEX pending_wakes_by_sender ON pending_wakes (sender_id);
";
