//! The Claude sign-in (`crates/provider-claude-code/src/account.rs`): the
//! CLI's own login, which every subscription's CLI runs the same way
//! (`crates/provider-subscription-cli/src/login.rs`).
//!
//! - The CLI writes a code it refuses to its standard error, as `Invalid
//!   code…`, and waits for another; it says why a sign-in failed there too.
//! - Jush reads the account it signed in, its ID and email, from the CLI's
//!   `.claude.json`, and nothing of its tokens.

use std::sync::Arc;

use futures_util::future::LocalBoxFuture;
use jush_provider_common::credentials::{AccountLabel, LoginError, decode_secret};
use jush_provider_subscription_cli::{Cli, CliLogin, CliSite, CliStart, ConfigDir};
use jush_shared_types::StreamKind;

use crate::account::label;
use crate::files::{CONFIG, ConfigFile};
use crate::{CLI, MachineState, cli};

/// Claude Code's login, with what the family keeps of the machine, whose
/// flags a sign-in fetches.
pub(crate) struct ClaudeLogin {
    pub(crate) state: Arc<MachineState>,
}

impl CliLogin for ClaudeLogin {
    fn cli(&self) -> &'static Cli {
        &CLI
    }

    fn request(&self, site: &CliSite) -> CliStart {
        cli::login_request(site)
    }

    fn refuses(&self, line: &str) -> bool {
        line.starts_with("Invalid code")
    }

    fn says_failure_on(&self) -> StreamKind {
        StreamKind::Stderr
    }

    fn account<'a>(
        &'a self,
        config: &'a dyn ConfigDir,
    ) -> LocalBoxFuture<'a, Result<AccountLabel, LoginError>> {
        Box::pin(async move {
            let bytes = config.read(CONFIG).await.map_err(|error| {
                LoginError::Failed(format!("Claude Code wrote no {CONFIG}: {error}"))
            })?;
            let text = String::from_utf8(bytes.to_vec())
                .map_err(|_| LoginError::Failed(format!("Claude Code's {CONFIG} is not text")))?;
            let file = decode_secret::<ConfigFile>(&text).map_err(|error| {
                LoginError::Failed(format!("Claude Code's {CONFIG} cannot be read: {error}"))
            })?;
            let account = file.oauth_account;
            Ok(label(&account.account_uuid, account.email_address.as_deref()))
        })
    }

    /// The CLI fetched the account's feature flags as it signed in.
    fn signed_in(&self, id: &str) {
        self.state.flags.refreshed(id);
    }
}
