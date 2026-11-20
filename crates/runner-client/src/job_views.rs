//! What the backend receives of a job's output over its views
//! (`crates/runner-jobs/src/tasks.rs` sends them): each stream's first
//! `JOB_VIEW_BYTES` once and without overlap, and beyond them the stream's
//! newest bytes, again with each message, or a message without bytes that
//! says only that the stream grew.
//!
//! The agent's shell (`shell_environment.rs`) and the owner's scripts
//! (`crates/core-device-access/src/scripts.rs`) settle a job's output from
//! it alone when it holds the whole output, and fall back on it when the
//! device's kept output cannot be read.

use bytes::Bytes;
use jush_device_interface::{Missing, OutputRecord, WholeOutput};
use jush_runner_protocol::wire::JOB_VIEW_BYTES;
use jush_shared_types::StreamKind;

use crate::device_jobs::Newest;
use crate::link::JobOutput;

/// What one message of a stream's views was.
pub enum View {
    /// One of the stream's first bytes. The device's jobs deliver them
    /// without overlap (`JobEntry::deliver`), so one that does not start
    /// where the received output ends lies past a gap and stays out of it.
    First,
    /// Beyond them, with the stream's newest bytes when it holds any.
    Beyond,
}

/// What the backend received of one stream of a job.
#[derive(Default)]
pub struct StreamViews {
    /// Where the stream's part of the received output ends: its first bytes,
    /// within `JOB_VIEW_BYTES`, up to the first it did not receive, such as
    /// bytes the runner printed while no connection served that no read
    /// delivered again. Bytes after such a gap stay out of it, so the
    /// output never joins bytes across it, and the end reads the kept
    /// output.
    head: u64,
    /// The stream's length on the device, as its messages told it.
    device: u64,
    /// The stream's newest bytes beyond its first `JOB_VIEW_BYTES`.
    newest: Newest,
}

impl StreamViews {
    /// Takes one message of the stream; first bytes that continue the
    /// received output go into `received`.
    pub fn take(&mut self, chunk: &JobOutput, received: &mut Vec<OutputRecord>) -> View {
        let end = chunk.offset + chunk.bytes.len() as u64;
        self.device = self.device.max(end);
        if chunk.offset < JOB_VIEW_BYTES as u64 {
            if chunk.offset == self.head {
                self.head = end;
                if !chunk.bytes.is_empty() {
                    received.push(OutputRecord::Output(chunk.stream, chunk.bytes.clone()));
                }
            }
            return View::First;
        }
        if !chunk.bytes.is_empty() {
            self.newest.keep(chunk.offset, &chunk.bytes);
        }
        View::Beyond
    }

    /// The bytes of the stream the received output holds.
    pub fn received(&self) -> u64 {
        self.head
    }

    /// The bytes of the stream the device held beyond those, as far as its
    /// messages told.
    pub fn unreceived(&self) -> u64 {
        self.device.saturating_sub(self.head)
    }

    /// The stream's newest bytes beyond its first, and where they start in
    /// the stream; none before any came.
    pub fn newest(&self) -> Option<(u64, &[u8])> {
        (!self.newest.is_empty()).then(|| (self.newest.offset(), self.newest.bytes()))
    }
}

/// Adds why a command ended without running to its end to its output: a
/// line on stderr, after the output's last line. Returns the text it added.
pub fn push_reason(output: &mut Vec<OutputRecord>, reason: &str) -> String {
    let last = output.iter().rev().find_map(|record| match record {
        OutputRecord::Output(_, bytes) => bytes.last().copied(),
        OutputRecord::LeftOut(_) => None,
    });
    let separator = if last.is_none_or(|byte| byte == b'\n') {
        ""
    } else {
        "\n"
    };
    let text = format!("{separator}{reason}\n");
    output.push(OutputRecord::Output(
        StreamKind::Stderr,
        Bytes::from(text.clone()),
    ));
    text
}

/// The output of a job whose kept output could not be read, for `reason`:
/// the `received` first bytes, then each stream's newest bytes, past a line
/// that counts what lies between them and the first; the `unreceived` bytes
/// that neither holds are missing.
pub fn received_output(
    mut received: Vec<OutputRecord>,
    streams: [&StreamViews; 2],
    unreceived: u64,
    reason: String,
) -> WholeOutput {
    let newest: Vec<(u64, StreamKind, Bytes)> = [StreamKind::Stdout, StreamKind::Stderr]
        .into_iter()
        .zip(streams)
        .filter_map(|(kind, stream)| {
            let (offset, bytes) = stream.newest()?;
            Some((offset.saturating_sub(stream.head), kind, Bytes::copy_from_slice(bytes)))
        })
        .collect();
    let kept: u64 = newest
        .iter()
        .map(|(left_out, _, bytes)| left_out + bytes.len() as u64)
        .sum();
    if !newest.is_empty() {
        let left_out = newest.iter().map(|(left_out, ..)| left_out).sum();
        received.push(OutputRecord::LeftOut(left_out));
        received.extend(
            newest
                .into_iter()
                .map(|(_, stream, bytes)| OutputRecord::Output(stream, bytes)),
        );
    }
    let missing = Some(Missing {
        bytes: unreceived.saturating_sub(kept),
        reason,
    })
    .filter(|missing| missing.bytes > 0);
    WholeOutput::new(received, missing)
}
