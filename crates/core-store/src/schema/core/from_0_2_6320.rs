//! From 0.2.6320's core schema, which 0.2.6325 and 0.2.6332 shipped too,
//! whose conversations named the program's plugins their agents had, by id,
//! and had every command of Jush's:
//!
//! - A conversation's list becomes the commands its agents have: the top
//!   groups of every command Jush gave then, and the named plugins' ids,
//!   each taken for the name of its plugin's group. A plugin whose group is
//!   named otherwise is the program's to name again; a list of no plugins
//!   keeps Jush's commands of then, and none a later release adds.
//! - A conversation's identity, the sentences that opened its system prompt
//!   before Jush's guide, becomes its whole system prompt: the identity, and
//!   the guide it read then after it, so its agents read what they read.
//! - A pending fork's metadata, which keeps its source's list and identity,
//!   turns the same way.
//! - The usage ledger's rows name whom their agent worked for, null for
//!   every row before: no message named one. A kept wake names whom its
//!   message's work is for, null for every wake kept before.
//! - A Fork's attempt names where its history ends, through a text or
//!   before a turn: its table is made again, as SQLite's procedure for a
//!   column that may be null now gives it (lang_altertable.html § 7), and
//!   every attempt kept is through its text.
//! - The two columns' names and comments change in the table's text, as
//!   SQLite's procedure for a change of the text alone gives it
//!   (lang_altertable.html § 7), which keeps the rows; the integrity check
//!   proves the table reads as before.

use rusqlite::Transaction;

use super::super::corrupt_text;
use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Code(core_from_0_2_6320);

/// The top groups of every command Jush gave a conversation then, as a JSON
/// array.
const JUSH_COMMANDS: &str = r#"["agent","attachment","audio","browser","conversation","credentials","device","file","image","mobile","notify","pin","providers","schedule","shell","skills","video","web","widget"]"#;

/// The guide a conversation with an identity read after it then, the two
/// a blank line apart in its prompt.
const GUIDE: &str = "\n\nGuidelines:\n- Commands run on the conversation's primary device, in its working directory; a context block names both. The directory stays between turns; to work in another directory, move there with `jush device move` first.\n- To wait for a command or a subagent, end your turn: its end or its message wakes you, and you continue then. Never wait with sleep or a loop.\n- The user reads only your replies, rendered as Markdown. Be concise.\n- Deliver a file as an attachment: `jush attachment upload`, then `<attachment id=\"a3\" />` on its own line; a medium a tool result showed you has its `attachment_id` already.\n- Link what the user can open: a device file `[runner.md](docs/runner.md#L438)`, or `device:<id>/abs/path` on an attached device, a subagent `[File icons](agent:4)`, a commit `[Fix the cart total](commit:3f2a9c1)`, or `commit:src/v8@86229da` in another repository, a conversation `[Login test](conversation:<id>)`.";

/// The Fork attempts' table, made again with the two ends.
const FORK_ENDS: &str = "
CREATE TABLE conversation_fork_operations_new (
  id          TEXT PRIMARY KEY COLLATE NOCASE,
  source_id   TEXT NOT NULL,
  -- Where the history ends, one of the two: through the completed text
  -- `block_id`, or before the turn `before_turn`, by the id of the message
  -- that started it.
  block_id    TEXT,
  before_turn TEXT,
  metadata    TEXT NOT NULL,
  CHECK ((block_id IS NULL) != (before_turn IS NULL))
) STRICT;
INSERT INTO conversation_fork_operations_new (id, source_id, block_id, metadata)
  SELECT id, source_id, block_id, metadata FROM conversation_fork_operations;
DROP TABLE conversation_fork_operations;
ALTER TABLE conversation_fork_operations_new RENAME TO conversation_fork_operations;
";

/// Each column's text, and what it becomes.
const COLUMN_TEXT: [(&str, &str); 2] = [
    (
        "  -- The sentences that open its agents' system prompts in place of Jush's;
  -- null for Jush's.
  identity TEXT,",
        "  -- The system prompt its agents read in place of Jush's; null for Jush's.
  system_prompt TEXT,",
    ),
    (
        "  -- The program's plugins its agents have, a JSON array of their ids; null
  -- for all of them.
  plugins TEXT,",
        "  -- The commands its agents have, a JSON array of their paths without
  -- `jush`; null for every command.
  commands TEXT,",
    ),
];

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
    transaction.execute(
        "UPDATE conversations SET identity = identity || ?1 WHERE identity IS NOT NULL",
        [GUIDE],
    )?;
    transaction.execute(
        "UPDATE conversation_fork_operations SET metadata = json_remove(
           json_set(metadata, '$.systemPrompt', json_extract(metadata, '$.identity') || ?1),
           '$.identity'
         )
         WHERE json_type(metadata, '$.identity') = 'text'",
        [GUIDE],
    )?;
    let version: i64 = transaction.pragma_query_value(None, "schema_version", |row| row.get(0))?;
    transaction.pragma_update(None, "writable_schema", true)?;
    let mut changed = 0;
    for (before, after) in COLUMN_TEXT {
        changed += transaction.execute(
            "UPDATE sqlite_schema SET sql = replace(sql, ?1, ?2)
               WHERE type = 'table' AND name = 'conversations' AND instr(sql, ?1) > 0",
            [before, after],
        )?;
    }
    transaction.pragma_update(None, "schema_version", version + 1)?;
    transaction.pragma_update(None, "writable_schema", false)?;
    if changed != COLUMN_TEXT.len() {
        return Err(corrupt_text("the conversations table holds no identity and plugins columns to rename"));
    }
    transaction.execute_batch(
        "ALTER TABLE usage_ledger ADD COLUMN sender_id TEXT;
         ALTER TABLE pending_wakes ADD COLUMN message_sender_id TEXT;",
    )?;
    transaction.execute_batch(FORK_ENDS)?;
    let problems: Vec<String> = transaction
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if problems != ["ok"] {
        return Err(corrupt_text(&format!("the conversations table fails its check: {problems:?}")));
    }
    Ok(())
}
