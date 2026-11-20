//! A subscription's CLI on the machine that runs it
//! (`crates/command-package-subscription-cli/src/lib.rs`).
//!
//! It covers the release's executable for this machine, which the runner
//! installs as the package's artifact named after the CLI, such as `Claude
//! Code`, and the versions of it the runner has. Nothing here downloads or
//! looks for the CLI.

use jush_command_package_subscription_cli_protocol::{
    ErrorCode, Installed, Release, Status, StatusRequest,
};
use jush_command_protocol::{ArtifactForm, ArtifactInstall};
use jush_command_sdk::{Artifacts, ResidentError};

use crate::{platform, release};

/// Why an operation failed. `code` is what the caller branches on; a
/// cancellation ends the invocation without an answer.
#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    #[error("invalid release record: {0}")]
    InvalidRelease(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("{0}")]
    UnsupportedPlatform(String),
    #[error("installation failed: {0}")]
    InstallFailed(String),
    #[error("cancelled")]
    Cancelled,
}

impl OperationError {
    /// The code the caller branches on; a cancellation has none, since it
    /// ends the invocation without a document.
    pub fn code(&self) -> Option<ErrorCode> {
        Some(match self {
            Self::InvalidRelease(_) => ErrorCode::InvalidRelease,
            Self::InvalidRequest(_) => ErrorCode::InvalidRequest,
            Self::UnsupportedPlatform(_) => ErrorCode::UnsupportedPlatform,
            Self::InstallFailed(_) => ErrorCode::InstallFailed,
            Self::Cancelled => return None,
        })
    }
}

impl From<ResidentError> for OperationError {
    fn from(error: ResidentError) -> Self {
        match error {
            ResidentError::Cancelled => Self::Cancelled,
            ResidentError::Artifacts(reason) => Self::InstallFailed(reason),
            error => Self::InstallFailed(error.to_string()),
        }
    }
}

/// Parses and validates the input of `subscription-cli.ensure`.
pub fn release(input: &[u8]) -> Result<Release, OperationError> {
    release::parse(input)
}

/// Parses and validates the input of `subscription-cli.status`.
pub fn status_request(input: &[u8]) -> Result<StatusRequest, OperationError> {
    StatusRequest::parse(input).map_err(|error| OperationError::InvalidRequest(error.to_string()))
}

/// The release's executable for this machine, which the runner installs for
/// `invocation` when it has none.
pub async fn ensure(
    artifacts: &Artifacts,
    invocation: &str,
    release: &Release,
) -> Result<Installed, OperationError> {
    let platform = current_platform(&release.cli)?;
    let artifact = release.platforms.get(platform).ok_or_else(|| {
        OperationError::UnsupportedPlatform(format!(
            "{} {} has no build for {platform}",
            release.cli, release.version
        ))
    })?;
    let install = ArtifactInstall {
        invocation: invocation.to_owned(),
        name: release.cli.clone(),
        version: release.version.clone(),
        sha256: artifact.sha256.clone(),
        size: artifact.size,
        form: ArtifactForm::File,
        url: None,
    };
    let path = artifacts.install(install, None).await?;
    Ok(Installed {
        version: release.version.clone(),
        path,
    })
}

/// This machine's platform key, and the versions of the CLI `cli` the runner
/// has, the newest install first.
pub async fn status(artifacts: &Artifacts, cli: &str) -> Result<Status, OperationError> {
    let platform = current_platform(cli)?;
    let installed = artifacts
        .installed(cli)
        .await?
        .into_iter()
        .map(|installed| Installed {
            version: installed.version,
            path: installed.path.into(),
        })
        .collect();
    Ok(Status {
        platform: platform.into(),
        installed,
    })
}

fn current_platform(cli: &str) -> Result<&'static str, OperationError> {
    platform::current().ok_or_else(|| {
        OperationError::UnsupportedPlatform(format!(
            "{cli} has no build for this machine ({} {})",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })
}
