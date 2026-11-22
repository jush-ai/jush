//! A probe of an account: a process of the account answers one question and
//! is closed (`crates/provider-subscription-cli/src/probe.rs`).
//!
//! - A new session: what `session/new` answers says which models the
//!   account's plan offers (`models.rs`) and the plan (`quota.rs`). A sign-in
//!   Google no longer takes signs the account out.
//! - A usage read: what `agy --print /usage` prints (`quota.rs`).
//!
//! Neither makes a model request. What `agy` keeps of either goes with the
//! process's directory.

use std::sync::Arc;

use futures_util::StreamExt as _;
use jush_provider_common::CatalogError;
use jush_provider_common::credentials::AuthFailure;
use jush_provider_common::quota::QuotaError;
use jush_provider_subscription_cli::{
    AccountMachine, CliStarter, Probed, Question, Tail, Unanswered, probe,
};
use jush_shared_types::StreamKind;
use serde::Deserialize as _;
use serde_json::Value;

use crate::account::AgyAuth;
use crate::acp::{self, NewSession, Output, Refusal};
use crate::{CLI, start};

/// Why a probe read nothing.
pub(crate) enum ProbeFailure {
    Auth(AuthFailure),
    Unavailable(String),
    Invalid(String),
}

impl ProbeFailure {
    pub(crate) fn catalog_error(self) -> CatalogError {
        match self {
            Self::Auth(failure) => failure.catalog_error(),
            Self::Unavailable(message) => CatalogError::Unavailable(message),
            Self::Invalid(message) => CatalogError::Invalid(message),
        }
    }

    pub(crate) fn quota_error(self) -> QuotaError {
        match self {
            Self::Auth(failure) => failure.quota_error(),
            Self::Unavailable(message) => QuotaError::Unavailable(message),
            Self::Invalid(message) => QuotaError::Invalid(message),
        }
    }
}

/// What probes an account: its record, the machine its processes run on
/// and the entry they are work of.
pub(crate) struct Prober {
    pub(crate) auth: Arc<AgyAuth>,
    /// None for a provider built without the machine, which cannot probe.
    pub(crate) machine: Option<Arc<dyn AccountMachine>>,
    pub(crate) entry: String,
}

impl Prober {
    /// What a new session of the account answers.
    pub(crate) async fn session(&self) -> Result<NewSession, ProbeFailure> {
        let answered = self
            .ask(Arc::new(start::probe), Box::new(|probed| Box::pin(new_session(probed))))
            .await?;
        match answered {
            Ok(session) => NewSession::deserialize(session).map_err(|error| {
                ProbeFailure::Invalid(format!("Antigravity's new session cannot be read: {error}"))
            }),
            Err(refusal) => match self.auth.refused(&refusal).await {
                Some(failure) => Err(ProbeFailure::Auth(failure)),
                None => Err(ProbeFailure::Unavailable(format!(
                    "Antigravity did not open a session: {}",
                    refusal.message
                ))),
            },
        }
    }

    /// What `agy --print /usage` printed for the account, as its JSON.
    pub(crate) async fn usage(&self) -> Result<Value, ProbeFailure> {
        let printed = self.ask(Arc::new(start::usage), Box::new(|probed| Box::pin(printed(probed)))).await?;
        serde_json::from_slice(&printed).map_err(|error| {
            ProbeFailure::Invalid(format!("Antigravity's usage cannot be read: {error}"))
        })
    }

    /// What the process `start` says answers `question`, on the account's
    /// machine.
    async fn ask<T: Send + 'static>(&self, start: CliStarter, question: Question<T>) -> Result<T, ProbeFailure> {
        let machine = self.machine.clone().ok_or_else(|| {
            ProbeFailure::Unavailable("The Subscriptions machine is not available".into())
        })?;
        let work = self.auth.work().await.map_err(ProbeFailure::Auth)?;
        probe(&*machine, &CLI, self.entry.clone(), work, start, question)
            .await
            .map_err(ProbeFailure::Unavailable)
    }
}

/// Everything the probe's process printed, once it ended; the tail of its
/// standard error when it printed nothing.
async fn printed(probed: Probed<'_>) -> Result<Vec<u8>, Unanswered> {
    let mut output = probed.output;
    let mut stdout = Vec::new();
    let mut stderr = Tail::default();
    while let Some(chunk) = output.next().await {
        match chunk.stream {
            StreamKind::Stdout => stdout.extend_from_slice(&chunk.bytes),
            StreamKind::Stderr => stderr.push(&chunk.bytes),
        }
    }
    if stdout.iter().all(u8::is_ascii_whitespace) {
        return Err(Unanswered::Ended(stderr.text()));
    }
    Ok(stdout)
}

/// Opens a session in the probe's process: what `session/new` answered, or
/// how `agy` refused it or `initialize`.
async fn new_session(probed: Probed<'_>) -> Result<Result<Value, Refusal>, Unanswered> {
    let mut output = Output::new(probed.output);
    acp::open(probed.control, &mut output, &start::workspace(probed.site), None).await
}
