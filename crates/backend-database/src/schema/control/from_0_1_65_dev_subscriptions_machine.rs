//! From the control schema where the managed machine that runs the
//! subscriptions' CLIs ran Claude Code's alone and was named after it: the
//! machine is `subscriptions`, and its device record is named so.
//!
//! - The devices table's text names `subscriptions` in its CHECK where it
//!   named `claude_code`, with its comment: a change of the text alone, as
//!   SQLite's procedure for a change of constraints gives it
//!   (lang_altertable.html § 7), which keeps the rows and every foreign key
//!   to them.
//! - The machine's record, the master's, says so. The integrity check, which
//!   tests CHECK constraints, proves none is left.

use rusqlite::Transaction;

use super::super::{Migration, ReadBlob, corrupt_text};

pub(super) const MIGRATION: Migration = Migration::Code(control_from_0_1_65_dev_subscriptions_machine);

/// Each piece of the devices table's text that changes, and what it becomes.
const TEXT: [(&str, &str); 3] = [
    ("'claude_code'", "'subscriptions'"),
    ("-- Claude Code machine, which only", "-- Subscriptions machine, which only"),
    ("§ The Claude Code machine)", "§ The Subscriptions machine)"),
];

fn control_from_0_1_65_dev_subscriptions_machine(
    transaction: &Transaction<'_>,
    _: &dyn ReadBlob,
) -> rusqlite::Result<()> {
    let version: i64 = transaction.pragma_query_value(None, "schema_version", |row| row.get(0))?;
    transaction.pragma_update(None, "writable_schema", true)?;
    for (before, after) in TEXT {
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
    Ok(())
}
