//! A script the owner's work runs on a device to its end, which no
//! conversation's transcript holds: a schedule's change watch, and a
//! program's script (`POST /devices/:id/exec`).
//!
//! The script is one job on the device, driven as `jush device shell` drives
//! its far job (`device_commands.rs`): no record keeps it and no page follows
//! it. Its output is what the job's views carried when they hold all of it,
//! else the output the device keeps, read at its end; a read that fails
//! leaves the views' first and newest bytes, as the agent's shell does
//! (`jush_runner_client::received_output`).
//!
//! Its standard input is a pipe this process fills. A pipe cut before the
//! script's end ends the script as lost: it read only part of its input, and
//! a script left waiting for the rest would never end.
//!
//! A script that runs past its time, or whose stop comes, is stopped on its
//! device: `TERM`, then `KILL` once `STOP_GRACE` has passed
//! (`crates/runner-protocol/src/wire/mod.rs`). A stop while the device's
//! runner is away ends it as lost at once: only that runner could report the
//! stop, and it stops the job when it is back.

use std::collections::BTreeMap;
use std::time::Duration;

use bytes::Bytes;
use jush_command_protocol::CommandContext;
use jush_device_interface::{
    Device, DeviceError, JobCaller, OutputRecord, ProcessEnd, WholeOutput, signal_status,
};
use jush_runner_client::{
    JobEnd, JobStart, Pipe, RemoteDevice, RemoteJob, StreamViews, push_reason, received_output,
};
use jush_shared_types::StreamKind;
use tokio_util::sync::CancellationToken;

/// A script to run on a device.
pub struct Script {
    pub script: String,
    /// The directory it starts in; none for the device's own.
    pub cwd: Option<String>,
    /// The variables it starts with, above the device's own.
    pub env: BTreeMap<String, String>,
    /// What it reads on its standard input, which then ends; none for an
    /// input that is never written.
    pub stdin: Option<Bytes>,
    pub context: CommandContext,
    /// Who runs it, as the device's log names it.
    pub caller: JobCaller,
}

/// How a script ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEnd {
    /// It ended with this status, as the agent's shell reports one: its exit
    /// code, a signal's status, or 127 when bash could not run it.
    Exited(i32),
    /// It ran past its time. Its stop took, or its device's runner was away,
    /// which ended it as lost.
    TimedOut { stopped: bool },
    /// Its stop came first.
    Stopped,
    /// Its device lost it, or its input was cut, for this reason.
    Lost(String),
}

/// A script that ended, with its output.
pub struct Ran {
    pub end: ScriptEnd,
    pub output: WholeOutput,
}

/// Why this process cut a script short.
enum Cut {
    Time,
    Stop,
    Input(String),
}

/// Runs `script` on `device` until it ends, it runs past `time`, or `stop`
/// comes; answers how it ended, or why it never started.
pub async fn run(
    device: &RemoteDevice,
    script: Script,
    time: Option<Duration>,
    stop: &CancellationToken,
) -> Result<Ran, DeviceError> {
    if stop.is_cancelled() {
        return Ok(Ran {
            end: ScriptEnd::Stopped,
            output: WholeOutput::new(Vec::new(), None),
        });
    }
    let Script { script, cwd, mut env, stdin, context, caller } = script;
    let cwd = cwd.unwrap_or_else(|| device.default_cwd().to_owned());
    env.insert("PWD".into(), cwd.clone());
    // The pipe goes over the connection the start goes over: nothing waits
    // between the two.
    let input = match stdin {
        Some(bytes) => Some((device.write_pipe()?, bytes)),
        None => None,
    };
    let job_stop = CancellationToken::new();
    let started = device
        .start_job(JobStart {
            id: RemoteDevice::job_id(),
            script,
            cwd,
            env,
            context,
            viewable: None,
            caller: Some(caller),
            commands: None,
            stdin: input.as_ref().map(|(pipe, _)| pipe.wire_ref()),
            stdout: None,
            stop: job_stop.clone(),
        })
        .await;
    let job = match started {
        Ok(job) => job,
        Err(error) => {
            if let Some((pipe, _)) = &input {
                pipe.fail(&error.message);
            }
            return Err(error);
        }
    };
    let input = input.map(|(pipe, bytes)| {
        tokio::task::spawn_local(feed(pipe.clone(), bytes));
        pipe
    });
    let ending = async {
        let deadline = async {
            match time {
                Some(time) => tokio::time::sleep(time).await,
                None => std::future::pending().await,
            }
        };
        let input_failed = async {
            match &input {
                Some(pipe) => pipe.failed().await,
                None => std::future::pending().await,
            }
        };
        let cut = tokio::select! {
            end = job.end() => return (end, None),
            () = deadline => Cut::Time,
            () = stop.cancelled() => Cut::Stop,
            failure = input_failed => Cut::Input(format!("its standard input was cut: {}", failure.reason())),
        };
        job_stop.cancel();
        let reason = match &cut {
            Cut::Time => "the script ran past its time",
            Cut::Stop => "the script was stopped",
            Cut::Input(reason) => reason,
        };
        if let Some(pipe) = &input {
            pipe.fail(reason);
        }
        let end = job.stop_unless_away(reason).await;
        (end, Some(cut))
    };
    let ((end, cut), views) = tokio::join!(ending, views(&job));
    let output = match &end.status {
        ProcessEnd::NotStarted(error) => {
            let mut records = views.records;
            push_reason(&mut records, &error.to_string());
            WholeOutput::new(records, None)
        }
        ProcessEnd::Exited(_) | ProcessEnd::Signalled(_) => whole_output(&job, &end, views).await,
        // What the device held beyond the views went with it.
        ProcessEnd::Lost(_) => WholeOutput::new(views.records, None),
    };
    // The end and the output are read, stopped or not: the runner may remove
    // the job's directory.
    job.release().await;
    let cut_input = input.as_ref().and_then(Pipe::failure);
    if let Some(pipe) = &input {
        // An input the script did not read to its end is written no more.
        pipe.fail("the script ended");
    }
    let end = match (cut, cut_input, end.status) {
        (Some(Cut::Stop), ..) => ScriptEnd::Stopped,
        (Some(Cut::Input(reason)), ..) => ScriptEnd::Lost(reason),
        // It exited before its stop took: a stopped job reports the signal
        // that stopped it (`crates/runner-shell/src/job.rs`).
        (Some(Cut::Time), _, ProcessEnd::Exited(code)) => ScriptEnd::Exited(code),
        (Some(Cut::Time), _, ProcessEnd::Lost(_)) => ScriptEnd::TimedOut { stopped: false },
        (Some(Cut::Time), _, ProcessEnd::Signalled(_) | ProcessEnd::NotStarted(_)) => {
            ScriptEnd::TimedOut { stopped: true }
        }
        (None, Some(failure), _) => {
            ScriptEnd::Lost(format!("its standard input was cut: {}", failure.reason()))
        }
        (None, None, ProcessEnd::Exited(code)) => ScriptEnd::Exited(code),
        (None, None, ProcessEnd::Signalled(signal)) => ScriptEnd::Exited(signal_status(&signal)),
        (None, None, ProcessEnd::NotStarted(_)) => ScriptEnd::Exited(127),
        (None, None, ProcessEnd::Lost(reason)) => ScriptEnd::Lost(reason),
    };
    Ok(Ran { end, output })
}

/// Writes `bytes` into the script's input and ends it. A pipe that fails
/// first, as the script's runner reads it or as the script ends, says so
/// through [`Pipe::failed`].
async fn feed(pipe: Pipe, bytes: Bytes) {
    if let Ok(mut writer) = pipe.writer()
        && writer.write(bytes).await.is_ok()
    {
        writer.end();
    }
}

/// What the job's views of its output carried: the first bytes of its
/// streams in order, and each stream's views, stdout's first.
#[derive(Default)]
struct Views {
    records: Vec<OutputRecord>,
    streams: [StreamViews; 2],
}

/// The job's views of its output, to its end.
async fn views(job: &RemoteJob) -> Views {
    let mut views = Views::default();
    while let Some(output) = job.next_output().await {
        let stream = match output.stream {
            StreamKind::Stdout => &mut views.streams[0],
            StreamKind::Stderr => &mut views.streams[1],
        };
        stream.take(&output, &mut views.records);
    }
    views
}

/// The output of a job its runner saw end: the views when they hold all of
/// it, else the output the device keeps, else what the views hold of it.
async fn whole_output(job: &RemoteJob, end: &JobEnd, views: Views) -> WholeOutput {
    let [stdout, stderr] = &views.streams;
    let unreceived = end.output.as_ref().map_or(0, |lengths| {
        lengths.stdout_bytes.saturating_sub(stdout.received())
            + lengths.stderr_bytes.saturating_sub(stderr.received())
    });
    if unreceived == 0 {
        return WholeOutput::new(views.records, None);
    }
    match job.read_output().await {
        Ok(output) => output,
        Err(error) => {
            tracing::warn!(event = "script_output.read_failed", job_id = job.id(), error = %error.message, "a script's kept output could not be read");
            received_output(
                views.records,
                [stdout, stderr],
                unreceived,
                format!("not read from the device: {}", error.message),
            )
        }
    }
}
