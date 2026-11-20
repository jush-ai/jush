//! The Antigravity provider: an `antigravity` subscription entry's Google
//! account, signed in by Google's `agy` on the Subscriptions machine, whose
//! every request is `agy`'s own.
//!
//! - Jush's own copy of `agy` runs there as `agy acp`, an Agent Client
//!   Protocol server (`acp.rs`), only ever with flags its help lists, and
//!   follows Google's newest release (`releases.rs`).
//! - The account's sign-in stays in the account's directory, linked into the
//!   directory of each process of the account, which is that process's
//!   `HOME` (`start.rs`). Jush reads the account's email, which a command
//!   takes out of it on the machine, and nothing of its tokens (`login.rs`).
//! - A probe, a process of the account that opens a session and is closed,
//!   gives the catalog and the plan (`probe.rs`, `models.rs`, `quota.rs`).
//! - A session's runtime keeps one process between turns (`run.rs`,
//!   `live.rs`), which runs Jush's agent with the request's system prompt and
//!   loads the conversation's history written into its database
//!   (`history.rs`). The model's tool calls reach the agent over an MCP
//!   channel through a companion process (`tools.rs`), and `agy`'s reported
//!   failures are read from its text (`failure.rs`).
//!
//! ```text
//! backend (user's shard)          Subscriptions machine
//! +--------------------+          +----------------------------------+
//! | AntigravityRuntime | start    | agy acp --agent jush             |
//! |   ACP lines <------+--------->|   HOME = processes/<id>/         |
//! |   MCP server <-----+--------->|   companion sh <-pipes-> agy's   |
//! +--------------------+          |   sign-in -> the account's       |
//!                                 |   --HTTPS--> Google              |
//!                                 +----------------------------------+
//! ```
//!
//! Jush never calls Google with the account's tokens: `agy` alone does. The
//! crate depends on no agent crate, plugin or device implementation, as
//! Claude Code's does not (`crates/provider-claude-code/src/lib.rs`).

mod account;
mod acp;
mod failure;
mod history;
mod live;
mod login;
mod models;
mod probe;
mod quota;
pub mod releases;
mod run;
mod start;
mod tools;

use std::rc::Rc;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use jush_provider_common::credentials::{Accounts, CredentialPool, SubscriptionAccounts};
use jush_provider_common::models_dev::ModelsDevClient;
use jush_provider_common::quota::{ProviderQuota, QuotaSnapshotStore};
use jush_provider_common::{
    CatalogError, Provider, ProviderRuntime, RuntimeEnv, RuntimeError, read_http_failure,
};
use jush_provider_subscription_cli::{AccountMachine, Cli, CliKit, Placement};
use jush_shared_types::{
    AuthState, Clock, ProviderErrorDiagnostics, ProviderFailureFacts, ProviderModelList,
    RuntimeState, Timestamp,
};

use crate::account::AgyAuth;
use crate::login::AgyLogin;
use crate::probe::Prober;
use crate::quota::AgyQuota;
use crate::run::AgyRuntime;

/// `agy` as the Subscriptions machine runs it: each process's own directory
/// is under the account's `processes`.
pub static CLI: Cli = Cli {
    name: "Antigravity",
    process_parent: "processes",
};

/// The configuration of an `antigravity` entry's provider for one account.
#[derive(Clone)]
pub struct AntigravityConfig {
    /// The entry's id, which its account work is work of.
    pub id: String,
    /// The entry's label, which names the provider in its refusals.
    pub display_name: String,
    /// The account the provider stands for; `None` only for a provider built
    /// to add the entry's first account.
    pub account: Option<String>,
    /// The Subscriptions machine, which the account's probes run on; none
    /// for a provider that cannot reach it, which cannot probe.
    pub machine: Option<Arc<dyn AccountMachine>>,
}

/// An `antigravity` entry's provider for one account, shared by every user
/// and request of the entry.
///
/// Its runtimes start their processes through a placement
/// ([`AntigravityProvider::process_runtime`]). The generic
/// [`Provider::runtime`] refuses, since only the backend can choose the
/// machine (`crates/provider-subscription-cli/src/placement.rs`).
pub struct AntigravityProvider {
    shared: Arc<Shared>,
}

struct Shared {
    display_name: String,
    pool: Arc<dyn CredentialPool>,
    clock: Arc<dyn Clock>,
    entry: String,
    auth: Arc<AgyAuth>,
    quota: ProviderQuota,
    /// The accounts as settings see them; signing in needs a machine
    /// ([`AntigravityProvider::login_accounts`]).
    accounts: Accounts<CliKit>,
    models_dev: ModelsDevClient,
    prober: Prober,
}

impl AntigravityProvider {
    /// A provider over the entry's `pool`, keeping the account's quota in
    /// `snapshots`; `models_dev` is the backend's one models.dev copy.
    pub fn new(
        config: AntigravityConfig,
        pool: Arc<dyn CredentialPool>,
        snapshots: Arc<dyn QuotaSnapshotStore>,
        models_dev: ModelsDevClient,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let auth = Arc::new(AgyAuth::new(pool.clone(), config.account));
        let prober = || Prober {
            auth: auth.clone(),
            machine: config.machine.clone(),
            entry: config.id.clone(),
        };
        let source = AgyQuota(prober());
        let kit = CliKit {
            login: Arc::new(AgyLogin),
            machine: None,
            entry: config.id.clone(),
            pool: pool.clone(),
        };
        Self {
            shared: Arc::new(Shared {
                display_name: config.display_name,
                pool: pool.clone(),
                clock: clock.clone(),
                entry: config.id.clone(),
                prober: prober(),
                auth,
                quota: ProviderQuota::new(Box::new(source), snapshots, clock.clone()),
                accounts: Accounts::new(pool, kit, clock),
                models_dev,
            }),
        }
    }

    /// A session's runtime, which starts its processes through `placement`
    /// and keeps at most one of them between runs.
    pub fn process_runtime(&self, placement: Rc<dyn Placement>) -> Box<dyn ProviderRuntime> {
        Box::new(AgyRuntime::new(self.shared.clone(), placement))
    }

    /// The entry's account operations with a sign-in that runs `agy`'s own
    /// login on `machine`.
    pub fn login_accounts(&self, machine: Arc<dyn AccountMachine>) -> Box<dyn SubscriptionAccounts> {
        let kit = CliKit {
            login: Arc::new(AgyLogin),
            machine: Some(machine),
            entry: self.shared.entry.clone(),
            pool: self.shared.pool.clone(),
        };
        Box::new(Accounts::new(
            self.shared.pool.clone(),
            kit,
            self.shared.clock.clone(),
        ))
    }
}

impl Provider for AntigravityProvider {
    /// The account as its record holds it: signed in, unless Google signed it
    /// out; reading it starts nothing.
    fn auth_status(&self) -> BoxFuture<'_, AuthState> {
        Box::pin(async {
            match self.shared.auth.status().await {
                Ok(meta) => AuthState::Authenticated {
                    account_label: Some(meta.label),
                },
                Err(failure) => failure.state(),
            }
        })
    }

    /// Whether `agy` runs is known only when a request starts it on the
    /// Subscriptions machine.
    fn runtime_state(&self) -> RuntimeState {
        RuntimeState::Unknown {
            message: Some("Antigravity runs on its machine when a request needs it".into()),
        }
    }

    /// The models a new session of the account offers, read by a probe, with
    /// models.dev's facts asked for again.
    fn list_models(&self) -> BoxFuture<'_, Result<ProviderModelList, CatalogError>> {
        Box::pin(async {
            let session = self
                .shared
                .prober
                .session()
                .await
                .map_err(|failure| failure.catalog_error())?;
            let snapshot = self
                .shared
                .models_dev
                .refreshed()
                .await
                .map_err(|error| CatalogError::Unavailable(error.to_string()))?;
            models::catalog(&session, &snapshot)
        })
    }

    fn read_failure(
        &self,
        diagnostics: &ProviderErrorDiagnostics,
        received_at: Timestamp,
    ) -> ProviderFailureFacts {
        read_http_failure(diagnostics, received_at)
    }

    fn quota(&self) -> Option<&ProviderQuota> {
        Some(&self.shared.quota)
    }

    fn accounts(&self) -> Option<&dyn SubscriptionAccounts> {
        Some(&self.shared.accounts)
    }

    fn runtime(&self, _env: RuntimeEnv) -> Result<Box<dyn ProviderRuntime>, RuntimeError> {
        Err(RuntimeError::PlacementRequired {
            provider: self.shared.display_name.clone(),
        })
    }
}
