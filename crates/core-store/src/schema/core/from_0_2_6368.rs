//! From the core schema the SDK's release 0.2.6368 shipped, whose schedules
//! had no end: a schedule gains its end, null for every one before, and a
//! run's cause may be its end. The cause's check is widened in the table's
//! text, as SQLite's procedure for a change that every row already meets
//! gives it (lang_altertable.html § 7), which keeps the rows; the integrity
//! check proves the table reads as before.

use rusqlite::Transaction;

use super::super::corrupt_text;
use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Code(core_from_0_2_6368);

/// The runs' causes before, and after.
const CAUSES: (&str, &str) = ("'done', 'now'))", "'done', 'now', 'end'))");

fn core_from_0_2_6368(transaction: &Transaction<'_>, _: &()) -> rusqlite::Result<()> {
    let version: i64 = transaction.pragma_query_value(None, "schema_version", |row| row.get(0))?;
    transaction.pragma_update(None, "writable_schema", true)?;
    let changed = transaction.execute(
        "UPDATE sqlite_schema SET sql = replace(sql, ?1, ?2)
           WHERE type = 'table' AND name = 'schedule_runs' AND instr(sql, ?1) > 0",
        [CAUSES.0, CAUSES.1],
    )?;
    transaction.pragma_update(None, "schema_version", version + 1)?;
    transaction.pragma_update(None, "writable_schema", false)?;
    if changed != 1 {
        return Err(corrupt_text("the schedule_runs table holds no check of its causes to widen"));
    }
    transaction.execute_batch("ALTER TABLE schedules ADD COLUMN ends_at INTEGER;")?;
    let problems: Vec<String> = transaction
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if problems != ["ok"] {
        return Err(corrupt_text(&format!("the schedule_runs table fails its check: {problems:?}")));
    }
    Ok(())
}
