//! An Antigravity account: its record holds only its public metadata, since
//! `agy` keeps the sign-in in the account's directory on the Subscriptions
//! machine and renews it there, through the link each process of the account
//! has to it (`crates/provider-antigravity/src/start.rs`).
//!
//! Jush reads the account's email, which a command takes out of the sign-in
//! on the machine, to name the account, and nothing else of the sign-in
//! (`crates/provider-antigravity/src/login.rs`).

use std::sync::Arc;

use jush_provider_common::credentials::{
    AccountLabel, AccountMeta, AuthFailure, AuthReason, CredentialPool, sign_out_account,
};
use jush_provider_subscription_cli::CliWork;

use crate::CLI;
use crate::acp::Refusal;

/// Where `agy` keeps the sign-in, in its `HOME`.
pub(crate) const SIGN_IN: &str = ".gemini/antigravity-cli/antigravity-oauth-token";

/// How the account of `email` names itself, and what tells it apart from
/// an entry's others.
pub(crate) fn label(email: &str) -> AccountLabel {
    AccountLabel {
        label: email.to_owned(),
        detail: None,
        identity_key: Some(format!("google:{email}")),
    }
}

/// The account a provider stands for.
pub(crate) struct AgyAuth {
    pool: Arc<dyn CredentialPool>,
    account: Option<String>,
}

impl AgyAuth {
    pub(crate) fn new(pool: Arc<dyn CredentialPool>, account: Option<String>) -> Self {
        Self { pool, account }
    }

    pub(crate) fn account(&self) -> Result<&str, AuthFailure> {
        self.account
            .as_deref()
            .ok_or(AuthFailure::new(CLI.name, AuthReason::Missing))
    }

    /// The account as its record holds it: a usable one, else why it is
    /// not, such as a sign-in Google refused.
    pub(crate) async fn status(&self) -> Result<AccountMeta, AuthFailure> {
        let account = self.account()?;
        let meta = self
            .pool
            .meta(account)
            .await
            .map_err(|error| AuthFailure::new(CLI.name, AuthReason::Store(error.to_string())))?
            .ok_or(AuthFailure::new(CLI.name, AuthReason::Missing))?;
        if meta.signed_out {
            return Err(AuthFailure::new(CLI.name, AuthReason::SignedOut));
        }
        Ok(meta)
    }

    /// What a process of the account starts with: the account's directory.
    /// A signed-out account starts nothing.
    pub(crate) async fn work(&self) -> Result<CliWork, AuthFailure> {
        self.status().await?;
        Ok(CliWork::Account {
            account: self.account()?.to_owned(),
            handover: None,
        })
    }

    /// Why `agy` refused a request, when it refused it for the account's
    /// sign-in, which Google no longer takes: then the account is signed out.
    pub(crate) async fn refused(&self, refusal: &Refusal) -> Option<AuthFailure> {
        if !refusal.asks_sign_in() {
            return None;
        }
        self.signed_out().await;
        Some(AuthFailure::new(CLI.name, AuthReason::SignedOut))
    }

    /// Marks the account signed out: Google no longer takes its sign-in.
    pub(crate) async fn signed_out(&self) {
        let Ok(account) = self.account() else {
            return;
        };
        sign_out_account(&*self.pool, CLI.name, account).await;
    }
}
