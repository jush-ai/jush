//! From 0.2.6254's core schema, whose managed machine that runs the
//! subscriptions' CLIs ran Claude Code's alone and was named after it, whose
//! browser and simulator tabs were the primary device's alone, and whose
//! interrupted turns were each a root's:
//!
//! - The devices table's text names `subscriptions` in its CHECK where it
//!   named `claude_code`, with its comment: a change of the text alone, as
//!   SQLite's procedure for a change of constraints gives it
//!   (lang_altertable.html § 7), which keeps the rows. A machine's record
//!   says so, and the integrity check, which tests CHECK constraints, proves
//!   none is left.
//! - The browser and simulator tabs go, their ids not retired: the device a
//!   stored tab came from is not recorded, and each device lists its tabs
//!   again when its runner connects.
//! - An interrupted turn's mark says whether it was its root's; every mark
//!   stored before was.

use rusqlite::Transaction;

use super::super::corrupt_text;
use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Code(core_from_0_2_6254);

/// Each piece of the devices table's text that changes, and what it becomes.
const DEVICES_TEXT: [(&str, &str); 3] = [
    ("'claude_code'", "'subscriptions'"),
    ("-- Claude Code machine, which only", "-- Subscriptions machine, which only"),
    ("§ The Claude Code machine)", "§ The Subscriptions machine)"),
];

const TABS_AND_MARKS: &str = "
UPDATE conversation_work SET
  document = json_set(
    document,
    '$.tabs', (
      SELECT json_group_array(json(tab.value))
      FROM (
        SELECT value FROM json_each(document, '$.tabs')
        WHERE json_extract(value, '$.kind') NOT IN ('browser', 'mobile')
        ORDER BY key
      ) AS tab
    )
  ),
  revision = revision + 1
WHERE EXISTS (
  SELECT 1 FROM json_each(document, '$.tabs') AS tab
  WHERE json_extract(tab.value, '$.kind') IN ('browser', 'mobile')
);
ALTER TABLE interrupted_turns ADD COLUMN root INTEGER NOT NULL DEFAULT 1 CHECK (root IN (0, 1));
";

fn core_from_0_2_6254(transaction: &Transaction<'_>, _: &()) -> rusqlite::Result<()> {
    let version: i64 = transaction.pragma_query_value(None, "schema_version", |row| row.get(0))?;
    transaction.pragma_update(None, "writable_schema", true)?;
    for (before, after) in DEVICES_TEXT {
        transaction.execute(
            "UPDATE sqlite_schema SET sql = replace(sql, ?1, ?2)
               WHERE type = 'table' AND name = 'devices'",
            [before, after],
        )?;
    }
    transaction.pragma_update(None, "schema_version", version + 1)?;
    transaction.pragma_update(None, "writable_schema", false)?;
    transaction.execute(
        "UPDATE devices SET machine = 'subscriptions', name = 'Subscriptions' WHERE machine = 'claude_code'",
        [],
    )?;
    let problems: Vec<String> = transaction
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if problems != ["ok"] {
        return Err(corrupt_text(&format!("the devices' machines fail their check: {problems:?}")));
    }
    transaction.execute_batch(TABS_AND_MARKS)
}
