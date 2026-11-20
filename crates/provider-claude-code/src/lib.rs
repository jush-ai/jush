//! The Claude Code provider: a `claude-code` subscription entry's account,
//! signed in by the CLI's own login on the Subscriptions machine, inferring
//! through Jush's own copy of the vendor's CLI.
//!
//! - A session's runtime keeps one CLI process and exchanges stream-json with
//!   it over the `shell` process a `Placement` starts
//!   (`crates/provider-subscription-cli/src/placement.rs`).
//! - A new process resumes a session written from the conversation's blocks,
//!   whose entries the CLI mirrors back onto them.
//! - Every process of an account runs in the account's one directory, where the
//!   CLI keeps and renews the sign-in itself.
//! - The model's tools reach Jush over the SDK MCP channel, an rmcp server
//!   inside the run, with the model's parallel tool batches kept whole.
//! - The catalog is models.dev's Claude models, and the account's quota comes
//!   from the CLI's own usage answer and from the rate limits its lines report.
//!
//! The CLI is Jush's own verified copy, never a `claude` found on `PATH`, and
//! is started with no tools of its own and no device configuration, so it is a
//! channel to the vendor and nothing else. It does not touch the project,
//! which is why where it runs is a choice and not a consequence of where the
//! conversation's files are. The model's tools run where the conversation's
//! tools run, the CLI handing each call back to the agent.
//!
//! ```text
//! backend (user's shard)       Subscriptions machine (nobody has a shell)
//! +--------------------+       +------------------------------------+
//! | ClaudeCodeRuntime  | start | runner                             |
//! |   Placement -------+------>|   CLI process, cwd ~/run           |
//! |   stream-json <----+------>|   config dir ~/accounts/<id>/      |
//! |   SDK MCP server   |       |   (the sign-in)  --HTTPS--> vendor |
//! +---------+----------+       +------------------------------------+
//!           | a tool call comes back as a batch of calls
//!           v
//!   the agent runs it on the conversation's own target
//! ```
//!
//! No sign-in reaches a user's Cloud or a paired device, and the backend holds
//! none; the CLI sends its inference traffic to the vendor directly, with no
//! inference proxy. The backend's transcript stays the only record: a new
//! process resumes a session written from it, so nothing depends on an earlier
//! process or on files the machine kept.
//!
//! The crate depends on no agent crate, plugin or device implementation: it runs
//! the CLI through the `device-interface` process the placement answers, and
//! which machine that is, is the backend's placement.

mod account;
mod cli;
mod failure;
mod files;
mod idle;
mod input;
mod live;
mod login;
mod mcp;
mod models;
mod output;
mod prepared;
mod mirror;
mod quota;
pub mod releases;
mod run;
mod session;
mod timing;

use std::rc::Rc;
use std::sync::Arc;

use jush_provider_common::credentials::{Accounts, CredentialPool, SubscriptionAccounts};
use jush_provider_common::models_dev::ModelsDevClient;
use jush_provider_common::quota::{ProbeAsk, ProviderQuota, QuotaError, QuotaSnapshotStore};
use jush_provider_common::{
    CatalogError, Provider, ProviderRuntime, RuntimeEnv, RuntimeError, Searches,
};
use jush_provider_subscription_cli::{AccountMachine, Cli, CliKit, Placement};
use jush_shared_types::{
    AuthState, Clock, ProviderErrorDiagnostics, ProviderFailureFacts, ProviderModelList,
    RuntimeState, Timestamp,
};
use futures_util::future::BoxFuture;

use crate::account::ClaudeAuth;
use crate::mcp::ClientProtocol;
use crate::login::ClaudeLogin;
use crate::quota::{ClaudeQuota, FlagRefreshes};
use crate::run::ClaudeCodeRuntime;

/// How messages name the family.
const FAMILY: &str = "Claude Code";

/// The Claude Code CLI as the Subscriptions machine runs it: each process's
/// own directory is under the account's `projects`, where the CLI keeps its
/// sessions (`CLAUDE_CODE_PROJECT_DIR_NAME`).
pub static CLI: Cli = Cli {
    name: "Claude Code",
    process_parent: "projects",
};

/// What the family keeps of the Subscriptions machine for every provider of
/// every account, since each session's runtime builds a provider of its own.
#[derive(Default)]
pub struct MachineState {
    /// When each account's feature flags were last fetched there
    /// (`crates/provider-claude-code/src/quota.rs`).
    pub(crate) flags: FlagRefreshes,
    /// The MCP protocol version the machine's CLI asks for
    /// (`crates/provider-claude-code/src/mcp.rs`).
    pub(crate) mcp_protocol: ClientProtocol,
}

/// The CLI's own tool that runs Anthropic's web search.
const WEB_SEARCH: &str = "WebSearch";

/// The owner of the entries the provider keeps, the CLI's own session lines
/// (`crates/provider-claude-code/src/session.rs`).
const RECORD_OWNER: &str = "claude-code:";

/// The configuration of a `claude-code` entry's provider for one account.
#[derive(Clone)]
pub struct ClaudeCodeConfig {
    /// The entry's id, which names the entry in the logs of its runs.
    pub id: String,
    /// The entry's label, which names the provider in its refusals.
    pub display_name: String,
    /// The account the provider stands for; `None` only for a provider built
    /// to add the entry's first account.
    pub account: Option<String>,
    /// The Subscriptions machine, which the account's usage probe runs on;
    /// none for a provider that cannot reach it, which cannot probe.
    pub machine: Option<Arc<dyn AccountMachine>>,
    /// What the family keeps of the machine, which every provider shares.
    pub state: Arc<MachineState>,
    /// The Anthropic search the entry turned on, which the CLI runs.
    pub searches: Searches,
}

impl ClaudeCodeConfig {
    /// The configuration of an entry's provider for `account`.
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        account: Option<String>,
        machine: Option<Arc<dyn AccountMachine>>,
        state: Arc<MachineState>,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            account,
            machine,
            state,
            searches: Searches::default(),
        }
    }
}

/// A `claude-code` entry's provider for one account, shared by every user and
/// request of the entry.
///
/// Its runtimes start their CLI through a placement
/// ([`ClaudeCodeProvider::process_runtime`]). The generic [`Provider::runtime`]
/// refuses, since only the backend can choose the machine
/// (`crates/provider-subscription-cli/src/placement.rs`).
pub struct ClaudeCodeProvider {
    shared: Arc<Shared>,
}

/// What the provider and all its runtimes share.
struct Shared {
    id: String,
    display_name: String,
    searches: Searches,
    pool: Arc<dyn CredentialPool>,
    clock: Arc<dyn Clock>,
    auth: Arc<ClaudeAuth>,
    quota: ProviderQuota,
    /// The accounts as inference and settings see them; signing in needs a
    /// machine ([`ClaudeCodeProvider::login_accounts`]).
    accounts: Accounts<CliKit>,
    models_dev: ModelsDevClient,
    /// The Subscriptions machine; none for a provider that cannot reach it,
    /// which cannot probe.
    machine: Option<Arc<dyn AccountMachine>>,
    /// What the family keeps of the machine.
    state: Arc<MachineState>,
}

impl Shared {
    /// The MCP protocol version the machine's CLI asks for, as far as the
    /// machine's processes told (`crates/provider-claude-code/src/mcp.rs`).
    fn mcp_protocol(&self) -> rmcp::model::ProtocolVersion {
        self.state.mcp_protocol.get()
    }

    /// Runs the usage probe of `account` in the background, as a process of
    /// the account starts, when the flags it refreshes are due
    /// (`crates/provider-claude-code/src/quota.rs`). Nothing waits for it.
    fn refresh_flags(self: &Arc<Self>, account: &str) {
        if self.machine.is_none() || !self.state.flags.due(account) {
            return;
        }
        let shared = self.clone();
        tokio::task::spawn_local(async move {
            match shared.quota.probe(ProbeAsk::Routine).await {
                Ok(_) => {}
                Err(QuotaError::Waiting { until }) => tracing::debug!(
                    event = "claude_code_flags.refresh_waits",
                    provider = %shared.id,
                    %until,
                    "the usage probe that refreshes Claude Code's feature flags did not run: the account's probes wait after an attempt; the next start after three hours runs another"
                ),
                Err(error) => tracing::warn!(
                    event = "claude_code_flags.refresh_failed",
                    provider = %shared.id,
                    %error,
                    "the usage probe that refreshes Claude Code's feature flags failed; the next start after three hours runs another"
                ),
            }
        });
    }
}

impl ClaudeCodeProvider {
    /// A provider over the entry's `pool`, keeping the account's quota in
    /// `snapshots`; `models_dev` is the backend's one models.dev copy.
    pub fn new(
        config: ClaudeCodeConfig,
        pool: Arc<dyn CredentialPool>,
        snapshots: Arc<dyn QuotaSnapshotStore>,
        models_dev: ModelsDevClient,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let auth = Arc::new(ClaudeAuth::new(pool.clone(), config.account));
        let source = ClaudeQuota {
            auth: auth.clone(),
            machine: config.machine.clone(),
            state: config.state.clone(),
            entry: config.id.clone(),
        };
        let kit = CliKit {
            login: Arc::new(ClaudeLogin {
                state: config.state.clone(),
            }),
            machine: None,
            entry: config.id.clone(),
            pool: pool.clone(),
        };
        Self {
            shared: Arc::new(Shared {
                id: config.id,
                display_name: config.display_name,
                searches: config.searches,
                pool: pool.clone(),
                clock: clock.clone(),
                auth,
                quota: ProviderQuota::new(Box::new(source), snapshots, clock.clone()),
                accounts: Accounts::new(pool, kit, clock),
                models_dev,
                machine: config.machine,
                state: config.state,
            }),
        }
    }

    /// A session's runtime, which starts its CLI processes through
    /// `placement` and keeps at most one of them between runs.
    pub fn process_runtime(&self, placement: Rc<dyn Placement>) -> Box<dyn ProviderRuntime> {
        Box::new(ClaudeCodeRuntime::new(self.shared.clone(), placement))
    }

    /// The entry's account operations with a sign-in that runs the CLI's own
    /// login on `machine` (`crates/provider-claude-code/src/account.rs`).
    pub fn login_accounts(&self, machine: Arc<dyn AccountMachine>) -> Box<dyn SubscriptionAccounts> {
        let kit = CliKit {
            login: Arc::new(ClaudeLogin {
                state: self.shared.state.clone(),
            }),
            machine: Some(machine),
            entry: self.shared.id.clone(),
            pool: self.shared.pool.clone(),
        };
        Box::new(Accounts::new(
            self.shared.pool.clone(),
            kit,
            self.shared.clock.clone(),
        ))
    }
}

impl Provider for ClaudeCodeProvider {
    /// The account as its record holds it: signed in, unless its vendor
    /// signed it out; reading it starts nothing.
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

    /// Whether the CLI runs is known only when a request starts it on the
    /// Subscriptions machine.
    fn runtime_state(&self) -> RuntimeState {
        RuntimeState::Unknown {
            message: Some("Claude Code runs on its machine when a request needs it".into()),
        }
    }

    fn list_models(&self) -> BoxFuture<'_, Result<ProviderModelList, CatalogError>> {
        Box::pin(models::list(&self.shared.models_dev))
    }

    /// A usage limit can succeed again at the reset its refused call names;
    /// any other failure is read by the standard HTTP rules.
    fn read_failure(
        &self,
        diagnostics: &ProviderErrorDiagnostics,
        received_at: Timestamp,
    ) -> ProviderFailureFacts {
        failure::read_failure(diagnostics, received_at)
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

/// Test support (feature `testing`): the session a new process resumes for
/// a request, for the compaction tests.
#[cfg(feature = "testing")]
pub mod testing {
    use jush_provider_common::InferenceRequest;

    /// The entries of the session a new CLI process resumes for `request`, one
    /// JSON line each, after the `environment` entry, written at `timestamp`.
    ///
    /// The `environment` entry describes the process's machine
    /// (`crates/provider-claude-code/src/session.rs`).
    pub fn session(request: &InferenceRequest, timestamp: &str) -> Vec<u8> {
        crate::session::SessionFile::new(
            &request.session_id,
            &request.items,
            &request.blocks,
            timestamp.to_owned(),
        )
        .history
    }
}
