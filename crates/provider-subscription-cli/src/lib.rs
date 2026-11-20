//! What every subscription's CLI provider shares: a provider whose account
//! signs in with the vendor's own CLI, and whose requests run that CLI as a
//! process on the Subscriptions machine, such as Claude Code's
//! (`crates/provider-claude-code/src/lib.rs`).
//!
//! - [`Cli`] says what differs between CLIs as the machine runs them, and
//!   [`CliReleases`] reads a vendor's releases into the release record the
//!   `jush.subscription-cli` package installs
//!   (`crates/command-package-subscription-cli/src/lib.rs`).
//! - [`Placement`] starts a CLI's processes on the machine the backend
//!   chooses, and [`AccountMachine`] reaches it for account work
//!   (`placement.rs`).
//! - [`IdleProcesses`] holds the idle processes of the machine, of every CLI
//!   (`idle.rs`).
//! - [`CliKit`] signs an account in through its CLI's own login, the same
//!   for every CLI but in what [`CliLogin`] says (`login.rs`), and
//!   [`HeldProcess`] holds a process a provider started (`process.rs`).
//!
//! The crate knows no CLI and no vendor. The backend implements the
//! placement, and each CLI's provider builds what it starts; the crate runs
//! only a sign-in's process, which a [`CliLogin`] describes.

mod idle;
mod login;
mod placement;
mod process;

use futures_util::future::{BoxFuture, LocalBoxFuture, Shared};
use jush_command_package_subscription_cli_protocol::Release;
use jush_device_interface::{ProcessControl, ProcessEnd, Signal};
use std::time::Duration;

pub use idle::{Closing, IDLE_FOR, Idle, IdleProcesses, Seat};
pub use login::{CliKit, CliLogin};
pub use process::{HeldProcess, Tail, exit_message};
pub use placement::{
    AccountMachine, AccountWork, CliSite, CliStart, CliStarter, CliSystem, CliWork, ConfigDir,
    Handover, Placed, Placement, StartError, StartStage, StartSteps, WorkLease,
};

/// A subscription's CLI, as the Subscriptions machine runs it.
#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    /// Its name, such as `Claude Code`, which names it in messages and names
    /// its installs on the machine.
    pub name: &'static str,
    /// The directory of an account's directory that holds each process's own
    /// directory, such as `projects`.
    pub process_parent: &'static str,
}

/// A vendor's releases of its CLI, as the backend follows them: always the
/// newest.
pub trait CliReleases: Send + Sync {
    /// The newest release. `refresh` reads the vendor again, unless a read
    /// finished after the call began.
    fn latest(&self, refresh: bool) -> BoxFuture<'_, Result<Release, ReleaseError>>;

    /// The newest release read lately, without waiting; none when there is
    /// none, as after the backend starts, when it reads in the background for
    /// a later call.
    fn believed(&self) -> Option<Release>;
}

/// Why the newest release could not be read, in words that complete
/// "Claude Code could not be installed: …" for its CLI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ReleaseError(pub String);

/// How long a closed process has to end after SIGTERM before SIGKILL.
const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// Closes the process `control` drives, whose end is `exit`, SIGTERM first and
/// SIGKILL five seconds later, and waits for its end.
///
/// It is how anything that holds a CLI process closes it: a runtime, a sign-in
/// or the backend's relay of a process to another user's shard.
pub async fn close_process(
    control: Box<dyn ProcessControl>,
    exit: Shared<LocalBoxFuture<'static, ProcessEnd>>,
) {
    // A process that already ended, or whose machine went away, takes no
    // signal; its end is known either way.
    let _ = control.kill(Signal::Terminate).await;
    if tokio::time::timeout(CLOSE_GRACE, exit.clone()).await.is_err() {
        let _ = control.kill(Signal::Kill).await;
        exit.await;
    }
}
