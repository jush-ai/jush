//! From 0.2.6320's core schema, which 0.2.6325 and 0.2.6332 shipped too,
//! whose conversations named the program's plugins their agents had, by id,
//! and had every command of Jush's:
//!
//! - A conversation's list becomes the commands its agents have: the top
//!   groups of every command Jush gave then, and the named plugins' ids,
//!   each taken for the name of its plugin's group. A plugin whose group is
//!   named otherwise is the program's to name again; a list of no plugins
//!   keeps Jush's commands of then, and none a later release adds.
//! - A pending fork's metadata, which keeps its source's list, turns the
//!   same way.
//! - The column's name and comment change in the table's text, as SQLite's
//!   procedure for a change of the text alone gives it
//!   (lang_altertable.html § 7), which keeps the rows; the integrity check
//!   proves the table reads as before.

use rusqlite::Transaction;

use super::super::corrupt_text;
use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Code(core_from_0_2_6320);

/// The top groups of every command Jush gave a conversation then, as a JSON
/// array.
const JUSH_COMMANDS: &str = r#"["agent","attachment","audio","browser","conversation","credentials","device","file","image","mobile","notify","pin","providers","schedule","shell","skills","video","web","widget"]"#;

/// The column's text, and what it becomes.
const COLUMN_TEXT: (&str, &str) = (
    "  -- The program's plugins its agents have, a JSON array of their ids; null
  -- for all of them.
  plugins TEXT,",
    "  -- The commands its agents have, a JSON array of their paths without
  -- `jush`; null for every command.
  commands TEXT,",
);

fn core_from_0_2_6320(transaction: &Transaction<'_>, _: &()) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE conversations SET plugins = (
           SELECT json_group_array(value) FROM (
             SELECT value FROM json_each(?1)
             UNION ALL
             SELECT value FROM json_each(conversations.plugins)
           )
         )
         WHERE plugins IS NOT NULL",
        [JUSH_COMMANDS],
    )?;
    transaction.execute(
        "UPDATE conversation_fork_operations SET metadata = json_remove(
           json_set(metadata, '$.commands', (
             SELECT json_group_array(value) FROM (
               SELECT value FROM json_each(?1)
               UNION ALL
               SELECT value FROM json_each(metadata, '$.plugins')
             )
           )),
           '$.plugins'
         )
         WHERE json_type(metadata, '$.plugins') = 'array'",
        [JUSH_COMMANDS],
    )?;
    let version: i64 = transaction.pragma_query_value(None, "schema_version", |row| row.get(0))?;
    transaction.pragma_update(None, "writable_schema", true)?;
    let (before, after) = COLUMN_TEXT;
    let changed = transaction.execute(
        "UPDATE sqlite_schema SET sql = replace(sql, ?1, ?2)
           WHERE type = 'table' AND name = 'conversations' AND instr(sql, ?1) > 0",
        [before, after],
    )?;
    transaction.pragma_update(None, "schema_version", version + 1)?;
    transaction.pragma_update(None, "writable_schema", false)?;
    if changed != 1 {
        return Err(corrupt_text("the conversations table holds no plugins column to rename"));
    }
    let problems: Vec<String> = transaction
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if problems != ["ok"] {
        return Err(corrupt_text(&format!("the conversations table fails its check: {problems:?}")));
    }
    Ok(())
}
