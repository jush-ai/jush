//! How a provider holds a CLI process it started: closed, SIGTERM first,
//! however it goes, with the tail of what it wrote kept to say why it ended.

use futures_util::future::{LocalBoxFuture, Shared};
use jush_device_interface::{ProcessControl, ProcessEnd};

use crate::{Cli, ConfigDir, close_process};

/// How much of a stream of a process is kept: a process may print without
/// end, and the tail is where it says why it failed.
const TAIL_BYTES: usize = 64 * 1024;

/// A process's control, its end and its own directory: closed, SIGTERM first,
/// however the process goes, its directory removed once it ended.
///
/// Every CLI process a provider holds is held this way, so none is dropped
/// raw, which would kill it outright.
pub struct HeldProcess(Option<Parts>);

struct Parts {
    control: Box<dyn ProcessControl>,
    exit: Shared<LocalBoxFuture<'static, ProcessEnd>>,
    config: Option<Box<dyn ConfigDir>>,
}

impl HeldProcess {
    /// Holds the process `control` drives, whose end is `exit`, with its
    /// own directory `config`, when it has one.
    pub fn of(
        control: Box<dyn ProcessControl>,
        exit: Shared<LocalBoxFuture<'static, ProcessEnd>>,
        config: Option<Box<dyn ConfigDir>>,
    ) -> Self {
        Self(Some(Parts {
            control,
            exit,
            config,
        }))
    }

    pub fn control(&self) -> &dyn ProcessControl {
        // The parts go only with the holder's end, after which nothing
        // writes to the process.
        &*self.0.as_ref().expect("the process is not closed").control
    }

    /// The process's own directory, which stays while it is held, after the
    /// process ended too; none for a process held without one.
    pub fn dir(&self) -> Option<&dyn ConfigDir> {
        self.0.as_ref()?.config.as_deref()
    }

    /// Closes the process and waits for its end.
    pub async fn close(&mut self) {
        if let Some(parts) = self.0.take() {
            close(parts).await;
        }
    }

    /// Waits for the process to end by itself, then removes its
    /// directory. Dropping the future while it waits leaves the process
    /// held, to be closed.
    pub async fn ended(&mut self) {
        let Some(parts) = &self.0 else {
            return;
        };
        parts.exit.clone().await;
        if let Some(Parts { config, .. }) = self.0.take()
            && let Some(config) = config
        {
            config.remove().await;
        }
    }
}

impl Drop for HeldProcess {
    /// A process nobody closed, as when a turn drops its run's stream, is
    /// closed by a task of its own, SIGTERM first.
    fn drop(&mut self) {
        if let Some(parts) = self.0.take() {
            tokio::task::spawn_local(close(parts));
        }
    }
}

/// Closes a process: SIGTERM asks it to end, and SIGKILL follows after five
/// seconds; its end is awaited, then its directory removed.
async fn close(parts: Parts) {
    let Parts {
        control,
        exit,
        config,
    } = parts;
    close_process(control, exit).await;
    if let Some(config) = config {
        config.remove().await;
    }
}

/// The last bytes of a stream.
#[derive(Default)]
pub struct Tail(Vec<u8>);

impl Tail {
    pub fn push(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
        let excess = self.0.len().saturating_sub(TAIL_BYTES);
        self.0.drain(..excess);
    }

    /// How many bytes the tail holds.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The tail as text, trimmed; a character the cut split is left out.
    pub fn text(&self) -> String {
        let start = self
            .0
            .iter()
            .position(|byte| byte & 0b1100_0000 != 0b1000_0000)
            .unwrap_or(self.0.len());
        String::from_utf8_lossy(&self.0[start..]).trim().to_owned()
    }
}

/// What a process of `cli` that ended as `end` did, in words that stand as a
/// failure's message.
pub fn exit_message(cli: &Cli, end: &ProcessEnd) -> String {
    let name = cli.name;
    match end {
        ProcessEnd::Exited(code) => format!("{name} exited with code {code}"),
        ProcessEnd::Signalled(signal) => format!("{name} was ended by {signal}"),
        ProcessEnd::NotStarted(error) => match &error.detail {
            Some(detail) => format!("{name} did not start ({}): {detail}", error.kind),
            None => format!("{name} did not start ({})", error.kind),
        },
        ProcessEnd::Lost(reason) => format!("The Subscriptions machine went away: {reason}"),
    }
}
