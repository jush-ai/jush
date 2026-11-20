//! Google's `agy` distribution, followed as every subscription's CLI is
//! (`crates/provider-subscription-cli/src/releases.rs`): one manifest per
//! platform, each naming the newest version, its archive's URL and the
//! archive's SHA-512.
//!
//! - The `linux_amd64` manifest names the newest version, and every
//!   platform's must name the same one.
//! - A manifest leaves out its archive's size, which the archive's
//!   `Content-Length` gives.
//! - Each archive is a `tar.gz` that holds the executable `antigravity`, but
//!   Windows's, which is the executable itself.
//! - Android has no Jush runner, and its manifest is not read.

use std::collections::BTreeMap;

use futures_util::future::{BoxFuture, try_join_all};
use jush_command_package_subscription_cli_protocol::{Artifact, ArtifactForm, ArtifactHash};
use jush_provider_subscription_cli::{Distribution, Fetch};
use serde::Deserialize;
use url::Url;

/// Google's distribution, which `agy`'s own updater reads.
pub const DEFAULT_RELEASES_URL: &str =
    "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests";

/// Google's name of each platform Jush runs on, and Jush's.
const PLATFORMS: [(&str, &str); 8] = [
    ("darwin_arm64", "darwin-arm64"),
    ("darwin_amd64", "darwin-x64"),
    ("linux_amd64", "linux-x64"),
    ("linux_arm64", "linux-arm64"),
    ("linux_amd64_musl", "linux-x64-musl"),
    ("linux_arm64_musl", "linux-arm64-musl"),
    ("windows_amd64", "win32-x64"),
    ("windows_arm64", "win32-arm64"),
];

/// The platform whose manifest names the newest version.
const NEWEST: &str = "linux_amd64";

/// The executable inside an archive.
const ENTRY: &str = "antigravity";

/// A platform's manifest. Fields Jush does not read are ignored.
#[derive(Deserialize)]
struct Manifest {
    version: String,
    url: String,
    sha512: String,
}

/// The distribution at an address.
pub struct AgyDistribution {
    /// The address of the manifests, without a trailing slash.
    base: String,
}

impl AgyDistribution {
    pub fn new(base: &Url) -> Self {
        Self {
            base: base.as_str().trim_end_matches('/').to_owned(),
        }
    }

    /// The manifest of Google's platform `platform`.
    async fn manifest(&self, fetch: &Fetch, platform: &str) -> Result<Manifest, String> {
        let text = fetch.text(format!("{}/{platform}.json", self.base)).await?;
        serde_json::from_str(&text).map_err(|error| format!("the {platform} manifest: {error}"))
    }

    /// The artifact of Google's platform `platform` at `version`.
    async fn artifact(&self, fetch: &Fetch, platform: &str, version: &str) -> Result<Artifact, String> {
        let manifest = self.manifest(fetch, platform).await?;
        if manifest.version != version {
            return Err(format!(
                "the {platform} manifest names version {}",
                manifest.version
            ));
        }
        let form = if manifest.url.ends_with(".tar.gz") {
            ArtifactForm::Archive {
                entry: ENTRY.to_owned(),
            }
        } else if manifest.url.ends_with(".exe") {
            ArtifactForm::File
        } else {
            return Err(format!("the {platform} manifest names a file of no known form"));
        };
        Ok(Artifact {
            size: fetch.size(manifest.url.clone()).await?,
            url: manifest.url,
            hash: ArtifactHash::Sha512(manifest.sha512),
            form,
        })
    }
}

impl Distribution for AgyDistribution {
    fn newest<'a>(&'a self, fetch: &'a Fetch) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(self.manifest(fetch, NEWEST).await?.version) })
    }

    fn platforms<'a>(
        &'a self,
        fetch: &'a Fetch,
        version: &'a str,
    ) -> BoxFuture<'a, Result<BTreeMap<String, Artifact>, String>> {
        Box::pin(async move {
            let artifacts = try_join_all(PLATFORMS.map(|(google, jush)| async move {
                Ok::<_, String>((jush.to_owned(), self.artifact(fetch, google, version).await?))
            }))
            .await?;
            Ok(artifacts.into_iter().collect())
        })
    }
}
