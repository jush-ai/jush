//! The vendor's Claude Code distribution: the newest version, which its
//! `latest` pointer names, and each platform's executable, which that
//! version's manifest lists, followed as every subscription's CLI is
//! (`crates/provider-subscription-cli/src/releases.rs`).

use std::collections::BTreeMap;

use futures_util::future::BoxFuture;
use jush_command_package_subscription_cli_protocol::{Artifact, ArtifactForm, ArtifactHash};
use jush_provider_subscription_cli::{Distribution, Fetch};
use serde::Deserialize;
use url::Url;

/// The vendor's official distribution.
pub const DEFAULT_RELEASES_URL: &str = "https://downloads.claude.ai/claude-code-releases";

/// A version's `manifest.json`: each platform's executable name, SHA-256
/// and byte size. Fields Jush does not read are ignored.
#[derive(Deserialize)]
struct Manifest {
    version: String,
    platforms: BTreeMap<String, ManifestPlatform>,
}

#[derive(Deserialize)]
struct ManifestPlatform {
    binary: String,
    checksum: String,
    size: u64,
}

/// The distribution at an address, whose platform names are Jush's.
pub struct ClaudeDistribution {
    /// The distribution's address, without a trailing slash.
    base: String,
}

impl ClaudeDistribution {
    pub fn new(base: &Url) -> Self {
        Self {
            base: base.as_str().trim_end_matches('/').to_owned(),
        }
    }
}

impl Distribution for ClaudeDistribution {
    fn newest<'a>(&'a self, fetch: &'a Fetch) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let pointer = fetch.text(format!("{}/latest", self.base)).await?;
            Ok(pointer.trim().to_owned())
        })
    }

    fn platforms<'a>(
        &'a self,
        fetch: &'a Fetch,
        version: &'a str,
    ) -> BoxFuture<'a, Result<BTreeMap<String, Artifact>, String>> {
        Box::pin(async move {
            let text = fetch
                .text(format!("{}/{version}/manifest.json", self.base))
                .await?;
            let manifest: Manifest =
                serde_json::from_str(&text).map_err(|error| error.to_string())?;
            if manifest.version != version {
                return Err(format!("its manifest names version {}", manifest.version));
            }
            Ok(manifest
                .platforms
                .into_iter()
                .map(|(platform, entry)| {
                    let artifact = Artifact {
                        url: format!("{}/{version}/{platform}/{}", self.base, entry.binary),
                        size: entry.size,
                        hash: ArtifactHash::Sha256(entry.checksum),
                        form: ArtifactForm::File,
                    };
                    (platform, artifact)
                })
                .collect())
        })
    }
}
