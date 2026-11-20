//! A probe of an account: a process of the account opens a session and is
//! closed (`crates/provider-subscription-cli/src/probe.rs`), and what
//! `session/new` answers says which models the account's plan offers
//! (`models.rs`) and the plan (`quota.rs`).
//!
//! `session/new` makes no model request. The session's database goes with
//! the process's directory. A sign-in Google no longer takes signs the
//! account out.

use std::sync::Arc;

use jush_provider_common::CatalogError;
use jush_provider_common::credentials::AuthFailure;
use jush_provider_common::quota::QuotaError;
use jush_provider_subscription_cli::{AccountMachine, Probed, Unanswered, probe};
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
        let machine = self.machine.clone().ok_or_else(|| {
            ProbeFailure::Unavailable("The Subscriptions machine is not available".into())
        })?;
        let work = self.auth.work().await.map_err(ProbeFailure::Auth)?;
        let answered = probe(
            &*machine,
            &CLI,
            self.entry.clone(),
            work,
            Arc::new(start::probe),
            Box::new(|probed| Box::pin(new_session(probed))),
        )
        .await
        .map_err(ProbeFailure::Unavailable)?;
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
}

/// Opens a session in the probe's process: what `session/new` answered, or
/// how `agy` refused it or `initialize`.
async fn new_session(probed: Probed<'_>) -> Result<Result<Value, Refusal>, Unanswered> {
    let mut output = Output::new(probed.output);
    acp::open(probed.control, &mut output, &start::home(probed.site), None).await
}
