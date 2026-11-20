//! A probe of an account ([`probe`]): account work that starts a process of
//! the account, asks it one question and closes it, such as Claude Code's
//! usage probe and Antigravity's new session.
//!
//! The process is held outside what gives the probe up, and closed, SIGTERM
//! first, however the probe ends: on its answer, its time limit or its
//! abandonment alike.

use std::time::Duration;

use futures_util::FutureExt as _;
use futures_util::future::LocalBoxFuture;
use futures_util::stream::LocalBoxStream;
use jush_device_interface::{Process, ProcessControl, ProcessOutput};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::{
    AccountMachine, AccountWork, Cli, CliSite, CliStarter, CliWork, HeldProcess, Placed,
    Placement, exit_message,
};

/// How long a probe may take, its start included: a process that never
/// answers, such as one held behind a lock a killed process left, must not
/// hold the account's probes or the machine's work for good.
pub const PROBE_LIMIT: Duration = Duration::from_secs(30);

/// A probe's process, as its question gets it.
pub struct Probed<'a> {
    pub control: &'a dyn ProcessControl,
    pub output: LocalBoxStream<'static, ProcessOutput>,
    /// Where the process runs.
    pub site: &'a CliSite,
}

/// Why a probe's question got no answer.
pub enum Unanswered {
    /// The process refused it or could not be asked, in words that stand as
    /// the probe's failure.
    Failed(String),
    /// The process's output ended first, with the tail of its standard error.
    Ended(String),
}

/// What a probe asks its process, answering `T`.
pub type Question<T> =
    Box<dyn for<'a> FnOnce(Probed<'a>) -> LocalBoxFuture<'a, Result<T, Unanswered>> + Send>;

/// Probes an account of `cli` for the entry `entry` on `machine`: starts the
/// process `start` says for `work`, asks it `question` and closes it, within
/// [`PROBE_LIMIT`].
///
/// It answers what `question` answered, or why there is no answer, in words
/// that stand as the probe's failure.
pub async fn probe<T: Send + 'static>(
    machine: &dyn AccountMachine,
    cli: &'static Cli,
    entry: String,
    work: CliWork,
    start: CliStarter,
    question: Question<T>,
) -> Result<T, String> {
    let (answer, answered) = oneshot::channel();
    let probe: AccountWork = Box::new(move |placement, abandoned| {
        Box::pin(async move {
            let asked = ask(&*placement, cli, work, start, question, abandoned).await;
            // A probe nobody waits for any more reports to nobody.
            let _ = answer.send(asked);
        })
    });
    machine.run(cli, entry, probe).await.map_err(|error| error.0)?;
    answered
        .await
        .unwrap_or_else(|_| Err(format!("{}'s probe ended without an answer", cli.name)))
}

/// Starts the probe's process through `placement` and asks it, giving up when
/// nobody waits for it any more (`abandoned`).
async fn ask<T>(
    placement: &dyn Placement,
    cli: &'static Cli,
    work: CliWork,
    start: CliStarter,
    question: Question<T>,
    abandoned: CancellationToken,
) -> Result<T, String> {
    let timed_out = || {
        format!(
            "{} did not answer its probe within {} seconds",
            cli.name,
            PROBE_LIMIT.as_secs()
        )
    };
    let given_up = || "The probe was given up".to_owned();
    let _work = placement.work();
    let deadline = tokio::time::Instant::now() + PROBE_LIMIT;
    // A start given up removes what it made.
    let started = tokio::select! {
        started = tokio::time::timeout_at(deadline, placement.start(work, start, &|_| {})) => started,
        () = abandoned.cancelled() => return Err(given_up()),
    };
    let Placed {
        process,
        config,
        site,
        ..
    } = started.map_err(|_| timed_out())?.map_err(|error| error.0)?;
    let Process {
        output,
        control,
        exit,
    } = process;
    let exit = exit.shared();
    let mut held = HeldProcess::of(control, exit.clone(), Some(config));
    let answered = {
        let asked = question(Probed {
            control: held.control(),
            output,
            site: &site,
        });
        tokio::select! {
            answered = tokio::time::timeout_at(deadline, asked) => answered.map_err(|_| timed_out()),
            () = abandoned.cancelled() => Err(given_up()),
        }
    };
    held.close().await;
    match answered? {
        Ok(answer) => Ok(answer),
        Err(Unanswered::Failed(message)) => Err(message),
        Err(Unanswered::Ended(stderr)) if !stderr.is_empty() => Err(stderr),
        Err(Unanswered::Ended(_)) => Err(exit_message(cli, &exit.await)),
    }
}
