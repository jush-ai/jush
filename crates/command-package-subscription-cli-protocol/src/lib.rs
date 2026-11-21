//! The `jush.subscription-cli` package's contract
//! (`crates/command-package-subscription-cli/src/lib.rs`):
//!
//! - `subscription-cli.ensure` installs the version of a subscription's CLI,
//!   such as Claude Code, that a release record names.
//! - `subscription-cli.status` lists a CLI's installations.
//!
//! A CLI is named by its name, such as `Claude Code`, which names its line in
//! the runner's cache and the package's messages. The package knows no CLI:
//! the backend reads each vendor's releases into the record.
//!
//! Each writes one JSON document to stdout, a failure included, because a
//! resident stream carries only stdout.
//!
//! A failed invocation's nonzero exit does not discard its document: the
//! backend validates it and reports its code, message and requested version. An
//! output with no document is a resident that failed.

use std::{collections::BTreeMap, path::PathBuf};

pub use jush_command_protocol::{ArtifactForm, ArtifactHash};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::Value;

/// The package's id in the native catalog.
pub const PACKAGE: &str = "jush.subscription-cli";

/// The package's operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// `subscription-cli.ensure`: installs the version a release record names.
    Ensure,
    /// `subscription-cli.status`: lists the installed versions.
    Status,
}

impl Operation {
    /// Every operation, in the order the descriptor lists them.
    pub const ALL: [Operation; 2] = [Self::Ensure, Self::Status];

    /// The operation's name, such as `subscription-cli.ensure`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ensure => "subscription-cli.ensure",
            Self::Status => "subscription-cli.status",
        }
    }

    /// The operation named `name`, if the package has it.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|operation| operation.name() == name)
    }
}

/// Why a record was refused.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error(transparent)]
    Shape(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

impl From<garde::Report> for DecodeError {
    fn from(report: garde::Report) -> Self {
        Self::Invalid(report.to_string().trim_end().to_owned())
    }
}

/// `version` as a SemVer version when it is one without build metadata, such
/// as `2.1.3` or `2.1.3-beta.1`. A version names its installation directory,
/// so `+build` is refused.
pub fn parse_version(version: &str) -> Option<semver::Version> {
    semver::Version::parse(version)
        .ok()
        .filter(|version| version.build.is_empty())
}

/// Whether [`parse_version`] accepts `version`.
pub fn is_version(version: &str) -> bool {
    parse_version(version).is_some()
}

fn cli_version(value: &str, _: &()) -> garde::Result {
    if !is_version(value) {
        return Err(garde::Error::new(
            "is not a SemVer version without build metadata",
        ));
    }
    Ok(())
}

/// What `subscription-cli.ensure` reads from stdin: the CLI, one version of
/// it and its vendor's download for each platform key. Every entry is
/// checked, not only this machine's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct Release {
    /// The CLI's name, such as `Claude Code`; at most 100 bytes, as the
    /// runner takes a line's name.
    #[garde(length(min = 1, max = 100))]
    pub cli: String,
    #[garde(custom(cli_version))]
    pub version: String,
    #[garde(dive)]
    pub platforms: BTreeMap<String, Artifact>,
}

impl Release {
    /// Decodes a release record.
    pub fn parse(bytes: &[u8]) -> Result<Self, DecodeError> {
        let release: Self = serde_json::from_slice(bytes)?;
        garde::Validate::validate(&release)?;
        Ok(release)
    }
}

/// Where one platform's executable is, its byte size, the hash its vendor
/// gives, and its form: the executable itself, or an archive and the
/// executable inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    #[garde(custom(jush_command_protocol::download_url))]
    pub url: String,
    #[garde(range(min = 1))]
    pub size: u64,
    #[serde(flatten)]
    #[garde(dive)]
    pub hash: ArtifactHash,
    #[garde(dive)]
    pub form: ArtifactForm,
}

/// One usable executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installed {
    pub version: String,
    pub path: PathBuf,
}

/// What `subscription-cli.status` reads from stdin: the CLI whose
/// installations it lists, by its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct StatusRequest {
    #[garde(length(min = 1, max = 100))]
    pub cli: String,
}

impl StatusRequest {
    /// Decodes a status request.
    pub fn parse(bytes: &[u8]) -> Result<Self, DecodeError> {
        let request: Self = serde_json::from_slice(bytes)?;
        garde::Validate::validate(&request)?;
        Ok(request)
    }
}

/// What `subscription-cli.status` answers: this machine's platform key and the
/// versions of the CLI the runner has, the newest install first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub platform: String,
    pub installed: Vec<Installed>,
}

/// Why an operation failed; a cancelled invocation writes no document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRelease,
    InvalidRequest,
    UnsupportedPlatform,
    InstallFailed,
}

serde_plain::derive_display_from_serialize!(ErrorCode);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
}

/// The document an operation writes: `{"ok": true, ...answer}`, or
/// `{"ok": false, "code", "message"}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply<T> {
    Done(T),
    Failed(Failure),
}

impl<T: Serialize> Serialize for Reply<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Done<'a, T> {
            ok: bool,
            #[serde(flatten)]
            answer: &'a T,
        }
        #[derive(Serialize)]
        struct Failed<'a> {
            ok: bool,
            #[serde(flatten)]
            failure: &'a Failure,
        }
        match self {
            Self::Done(answer) => Done { ok: true, answer }.serialize(serializer),
            Self::Failed(failure) => Failed { ok: false, failure }.serialize(serializer),
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Reply<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut document = serde_json::Map::<String, Value>::deserialize(deserializer)?;
        let rest = |document| Value::Object(document);
        match document.remove("ok") {
            Some(Value::Bool(true)) => T::deserialize(rest(document))
                .map(Self::Done)
                .map_err(D::Error::custom),
            Some(Value::Bool(false)) => Failure::deserialize(rest(document))
                .map(Self::Failed)
                .map_err(D::Error::custom),
            Some(_) => Err(D::Error::custom("`ok` is not a boolean")),
            None => Err(D::Error::missing_field("ok")),
        }
    }
}
