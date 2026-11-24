//! The Antigravity sign-in: `agy`'s own login, which every subscription's
//! CLI runs the same way (`crates/provider-subscription-cli/src/login.rs`).
//!
//! - `agy` ends on a code it refuses, saying why on its standard output, as
//!   `couldn't log in: …`.
//! - Jush reads the account it signed in, its email, which `sed` takes out
//!   of `agy`'s sign-in on the machine, and nothing else of it: the
//!   sign-in's tokens never leave the machine.
//! - A refresh token the user pastes reaches the machine on a process's
//!   input and is written there as `agy`'s sign-in, which `agy` renews
//!   (`start.rs`); Jush keeps no copy of it.

use futures_util::future::LocalBoxFuture;
use jush_provider_common::credentials::{AccountLabel, LoginError};
use jush_provider_subscription_cli::{Cli, CliLogin, CliSite, CliStart, ConfigDir};
use jush_shared_types::StreamKind;

use crate::account::{SIGN_IN, label};
use crate::{CLI, start};

/// The `sed` expression that prints the sign-in's `id_token_email`, which
/// `agy` writes in one line of JSON (measured).
const EMAIL: &str = r#"s/.*"id_token_email"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p"#;

/// `agy`'s login.
pub(crate) struct AgyLogin;

impl CliLogin for AgyLogin {
    fn cli(&self) -> &'static Cli {
        &CLI
    }

    fn request(&self, site: &CliSite) -> CliStart {
        start::login(site)
    }

    fn refresh_token_request(&self) -> Option<fn(&CliSite) -> CliStart> {
        Some(start::refresh_token_login)
    }

    fn says_failure_on(&self) -> StreamKind {
        StreamKind::Stdout
    }

    fn account<'a>(
        &'a self,
        config: &'a dyn ConfigDir,
    ) -> LocalBoxFuture<'a, Result<AccountLabel, LoginError>> {
        Box::pin(async move {
            let bytes = config.output("sed", &["-n", EMAIL, SIGN_IN]).await.map_err(|error| {
                LoginError::Failed(format!("Antigravity's sign-in could not be read: {error}"))
            })?;
            let email = String::from_utf8_lossy(&bytes).trim().to_owned();
            if email.is_empty() {
                return Err(LoginError::Failed(
                    "Antigravity's sign-in names no account email".into(),
                ));
            }
            Ok(label(&email))
        })
    }
}
