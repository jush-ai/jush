//! The records of commands that run, in their conversation's database.
//!
//! The conversation's `running_commands` row of each is written when its job
//! starts, or as not yet sent while its start waits for its device, and deleted
//! in the transaction that writes its `command_outputs` row. The control
//! database keeps an index of their jobs by device (`running_jobs`).
//!
//! A backend that starts again reads them to take the commands up when their
//! runners connect (`crates/runner-client/src/device_jobs.rs`).

use std::collections::{BTreeMap, BTreeSet};

use jush_device_interface::{Seen, UnsentStart};
use jush_shared_types::{CommandEnd, CommandId, Labels, NodeId, Timestamp, TurnId};
use jush_web_api_protocol::ids::DeviceId;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::StorageError;
use super::columns::{decode, instant, json, labels, labels_column, to_json};
use super::command_outputs::{self, CommandOutput};

/// A command that runs, as its conversation records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningCommand {
    pub command: CommandId,
    /// The node that ran it.
    pub node: NodeId,
    pub device: DeviceId,
    /// Its job's id on that device.
    pub job: String,
    /// The `shell` call that started it.
    pub tool_use_id: String,
    /// The turn that call belongs to; none for a command recorded before
    /// records kept it.
    pub turn: Option<TurnId>,
    /// The labels that call's command is counted with
    /// (`crates/device-interface/src/environment.rs`, `JobCaller`).
    pub labels: Labels,
    /// The assignment that call's command is counted in, as `JobCaller` names it.
    pub assignment_id: Option<TurnId>,
    /// That call's title, which the command's reports name and the pages'
    /// view of it carries.
    pub title: String,
    /// That call's script, which the pages' view of the command
    /// carries.
    pub script: String,
    /// Whether its call started it as a service
    /// (`crates/agent-session/src/reports.rs`).
    pub service: bool,
    pub started: Timestamp,
    /// How far each node that looked at it has seen its output, which a
    /// look or a report moves.
    pub places: BTreeMap<NodeId, Seen>,
    /// What it starts once its device is back, while its start waits for it
    /// and was never sent (`crates/core-device-access/src/access.rs`).
    pub unsent: Option<UnsentStart>,
}

const COLUMNS: &str =
    "command_id, node_id, device_id, job_id, tool_use_id, started_at, places, title, service, turn_id, unsent, script, labels, assignment_id";

/// How far one node has seen a running command's output, as the `places`
/// column stores it (command places format 1): any count of bytes is one.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPlace {
    stdout: u64,
    stderr: u64,
}

/// The `places` column: each node's place, by the node.
#[derive(Serialize, Deserialize, garde::Validate)]
#[serde(transparent)]
struct StoredPlaces(#[garde(skip)] BTreeMap<NodeId, StoredPlace>);

impl StoredPlaces {
    fn of(places: &BTreeMap<NodeId, Seen>) -> Self {
        Self(
            places
                .iter()
                .map(|(node, seen)| {
                    let place = StoredPlace {
                        stdout: seen.stdout,
                        stderr: seen.stderr,
                    };
                    (node.clone(), place)
                })
                .collect(),
        )
    }

    fn places(self) -> BTreeMap<NodeId, Seen> {
        self.0
            .into_iter()
            .map(|(node, place)| {
                let seen = Seen {
                    stdout: place.stdout,
                    stderr: place.stderr,
                };
                (node, seen)
            })
            .collect()
    }
}

/// Records `running`. A command recorded already keeps its row, except
/// that one recorded as not yet sent becomes one recorded as `running`
/// says, sent once its job starts.
pub fn record(connection: &Connection, running: &RunningCommand) -> Result<(), StorageError> {
    connection
        .prepare_cached(&format!(
            "INSERT INTO running_commands ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (command_id) DO UPDATE SET unsent = excluded.unsent"
        ))?
        .execute(params![
            running.command.as_str(),
            running.node.as_str(),
            running.device.as_str(),
            running.job,
            running.tool_use_id,
            running.started.as_millisecond(),
            to_json(&StoredPlaces::of(&running.places)),
            running.title,
            running.service,
            running.turn.as_ref().map(TurnId::as_str),
            running.unsent.as_ref().map(to_json),
            running.script,
            labels_column(&running.labels),
            running.assignment_id.as_ref().map(TurnId::as_str),
        ])?;
    Ok(())
}

/// Records that `node` has seen `command`'s output as far as `seen`, in
/// one transaction; nothing for a command that does not run.
pub fn set_place(
    connection: &mut Connection,
    command: &CommandId,
    node: &NodeId,
    seen: Seen,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    let stored: Option<String> = transaction
        .prepare_cached("SELECT places FROM running_commands WHERE command_id = ?1")?
        .query_row([command.as_str()], |row| row.get(0))
        .optional()?;
    let Some(stored) = stored else {
        return Ok(());
    };
    let mut places = json::<StoredPlaces>("running_commands", "places", &stored)?.places();
    places.insert(node.clone(), seen);
    transaction
        .prepare_cached("UPDATE running_commands SET places = ?2 WHERE command_id = ?1")?
        .execute(params![command.as_str(), to_json(&StoredPlaces::of(&places))])?;
    transaction.commit()?;
    Ok(())
}

/// How far `node` has seen `command`'s output, as its record keeps it;
/// none when the node has not looked, or the command does not run.
pub fn place(connection: &Connection, command: &CommandId, node: &NodeId) -> Result<Option<Seen>, StorageError> {
    let stored: Option<String> = connection
        .prepare_cached("SELECT places FROM running_commands WHERE command_id = ?1")?
        .query_row([command.as_str()], |row| row.get(0))
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    Ok(json::<StoredPlaces>("running_commands", "places", &stored)?
        .places()
        .remove(node))
}

/// Records the command's end, `output`, in place of its record as running,
/// whose start it keeps, in one transaction; answers the job it ran as,
/// when it was recorded running.
pub fn end(connection: &mut Connection, mut output: CommandOutput) -> Result<Option<String>, StorageError> {
    let transaction = connection.transaction()?;
    let running: Option<(String, i64)> = transaction
        .prepare_cached("DELETE FROM running_commands WHERE command_id = ?1 RETURNING job_id, started_at")?
        .query_row([output.command.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    let job = match running {
        Some((job, started)) => {
            output.started = Some(decode("running_commands", "started_at", Timestamp::from_millisecond(started))?);
            Some(job)
        }
        None => None,
    };
    command_outputs::insert_in(&transaction, &[output])?;
    transaction.commit()?;
    Ok(job)
}

/// The record of `command` as running; none once it ended, and for one
/// never recorded.
pub fn read(connection: &Connection, command: &CommandId) -> Result<Option<RunningCommand>, StorageError> {
    connection
        .prepare_cached(&format!("SELECT {COLUMNS} FROM running_commands WHERE command_id = ?1"))?
        .query_row([command.as_str()], |row| Ok(decode_row(row)))
        .optional()?
        .transpose()
}

/// The commands `node` ran that run, oldest first.
pub fn of_node(connection: &Connection, node: &NodeId) -> Result<Vec<RunningCommand>, StorageError> {
    let mut statement = connection.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM running_commands WHERE node_id = ?1 ORDER BY started_at, command_id"
    ))?;
    let mut rows = statement.query([node.as_str()])?;
    let mut running = Vec::new();
    while let Some(row) = rows.next()? {
        running.push(decode_row(row)?);
    }
    Ok(running)
}

/// How a command stands now, as its conversation's records say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// It runs, since then.
    Running { started: Timestamp },
    /// It ended so, then; its start when its row keeps it, which a row
    /// written before rows kept it does not.
    Ended { end: CommandEnd, started: Option<Timestamp>, ended: Timestamp },
    /// The conversation holds no record of it.
    Unknown,
}

/// How each of `commands` stands now: by its `command_outputs` row once it
/// ended, else by its record as running.
pub fn standings(connection: &Connection, commands: &[CommandId]) -> Result<Vec<(CommandId, Standing)>, StorageError> {
    let mut ended = connection
        .prepare_cached("SELECT started_at, ended_at, ending FROM command_outputs WHERE command_id = ?1")?;
    let mut running = connection.prepare_cached("SELECT started_at FROM running_commands WHERE command_id = ?1")?;
    let mut standings = Vec::new();
    for command in commands {
        let row = ended
            .query_row([command.as_str()], |row| {
                Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?))
            })
            .optional()?;
        let standing = match row {
            Some((started, ended, ending)) => Standing::Ended {
                end: match ending {
                    Some(text) => json("command_outputs", "ending", &text)?,
                    None => CommandEnd::Unrecorded,
                },
                started: started
                    .map(|millis| decode("command_outputs", "started_at", Timestamp::from_millisecond(millis)))
                    .transpose()?,
                ended: decode("command_outputs", "ended_at", Timestamp::from_millisecond(ended))?,
            },
            None => match running.query_row([command.as_str()], |row| row.get::<_, i64>(0)).optional()? {
                Some(millis) => Standing::Running {
                    started: decode("running_commands", "started_at", Timestamp::from_millisecond(millis))?,
                },
                None => Standing::Unknown,
            },
        };
        standings.push((command.clone(), standing));
    }
    Ok(standings)
}

/// Every node that ran a command that runs.
pub fn nodes(connection: &Connection) -> Result<BTreeSet<NodeId>, StorageError> {
    let mut statement = connection.prepare_cached("SELECT DISTINCT node_id FROM running_commands")?;
    let mut rows = statement.query([])?;
    let mut nodes = BTreeSet::new();
    while let Some(row) = rows.next()? {
        nodes.insert(decode("running_commands", "node_id", NodeId::try_from(row.get::<_, String>(0)?))?);
    }
    Ok(nodes)
}

fn decode_row(row: &Row<'_>) -> Result<RunningCommand, StorageError> {
    let table = "running_commands";
    Ok(RunningCommand {
        command: decode(table, "command_id", CommandId::try_from(row.get::<_, String>(0)?))?,
        node: decode(table, "node_id", NodeId::try_from(row.get::<_, String>(1)?))?,
        device: decode(table, "device_id", DeviceId::try_from(row.get::<_, String>(2)?))?,
        job: row.get(3)?,
        tool_use_id: row.get(4)?,
        title: row.get(7)?,
        script: row.get(11)?,
        service: row.get(8)?,
        turn: row
            .get::<_, Option<String>>(9)?
            .map(|turn| decode(table, "turn_id", TurnId::try_from(turn)))
            .transpose()?,
        labels: labels(table, "labels", row.get::<_, Option<String>>(12)?.as_deref())?,
        assignment_id: row
            .get::<_, Option<String>>(13)?
            .map(|assignment| decode(table, "assignment_id", TurnId::try_from(assignment)))
            .transpose()?,
        started: instant(row, table, "started_at")?,
        places: json::<StoredPlaces>(table, "places", &row.get::<_, String>(6)?)?.places(),
        unsent: row
            .get::<_, Option<String>>(10)?
            .map(|unsent| json::<UnsentStart>(table, "unsent", &unsent))
            .transpose()?,
    })
}
