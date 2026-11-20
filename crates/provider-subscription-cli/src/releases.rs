//! Following a vendor's newest release of its CLI ([`FollowedReleases`]),
//! read from the vendor's distribution as its [`Distribution`] says, and
//! believed for six hours.
//!
//! - Concurrent readers share one read, and a version's release is read once,
//!   since a published release never changes.
//! - A redirect is refused, and every read runs on the edge's runtime, whoever
//!   asks.
//! - A start that has a version to run never waits for one, and reads in the
//!   background instead.
//!
//! Jush always follows the vendor's newest release. Provider entries store no
//! CLI version and expose no version-selection or update-disable setting. Jush
//! keeps no list of versions it was tested with, because such a list would be
//! out of date with each release and a version it does not name would still
//! run. Each CLI's scenario suite checks whichever CLI it is given against
//! what the product relies on.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures_util::future::BoxFuture;
use jiff::SignedDuration;
use jush_command_package_subscription_cli_protocol::{Artifact, Release, is_version};
use jush_provider_common::kept::KeptRead;
use jush_shared_types::Clock;
use tokio::runtime::Handle;

use crate::{Cli, CliReleases, ReleaseError};

/// How long the newest version is believed before it is read again.
const LATEST_TTL: SignedDuration = SignedDuration::from_hours(6);

/// How long one read of a distribution may take.
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// How a vendor publishes its CLI.
pub trait Distribution: Send + Sync + 'static {
    /// The newest version the vendor names.
    fn newest<'a>(&'a self, fetch: &'a Fetch) -> BoxFuture<'a, Result<String, String>>;

    /// Each platform's artifact of version `version`, by Jush's name of the
    /// platform, such as `linux-x64`.
    fn platforms<'a>(
        &'a self,
        fetch: &'a Fetch,
        version: &'a str,
    ) -> BoxFuture<'a, Result<BTreeMap<String, Artifact>, String>>;
}

/// What a distribution is read with.
pub struct Fetch {
    /// A client that follows no redirect and gives up after `READ_TIMEOUT`.
    http: reqwest::Client,
    edge: Handle,
}

impl Fetch {
    /// The text at `url`, or why it was not read.
    pub async fn text(&self, url: String) -> Result<String, String> {
        let http = self.http.clone();
        self.on_edge(async move {
            let response = answered(http.get(&url).send().await)?;
            response.text().await.map_err(unanswered)
        })
        .await
    }

    /// The byte size of the file at `url`, as its `Content-Length` says.
    pub async fn size(&self, url: String) -> Result<u64, String> {
        let http = self.http.clone();
        self.on_edge(async move {
            let response = answered(http.head(&url).send().await)?;
            // The header itself: the length of a HEAD response's empty body
            // is no answer.
            response
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|length| length.to_str().ok()?.parse().ok())
                .ok_or_else(|| format!("the distribution named no size for {url}"))
        })
        .await
    }

    async fn on_edge<T: Send + 'static>(
        &self,
        read: impl Future<Output = Result<T, String>> + Send + 'static,
    ) -> Result<T, String> {
        self.edge
            .spawn(read)
            .await
            .unwrap_or_else(|error| Err(format!("the distribution was not read ({error})")))
    }
}

/// A response with success, or why there is none.
fn answered(sent: Result<reqwest::Response, reqwest::Error>) -> Result<reqwest::Response, String> {
    let response = sent.map_err(unanswered)?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("the distribution answered {status}"));
    }
    Ok(response)
}

fn unanswered(error: reqwest::Error) -> String {
    format!("the distribution did not answer ({error})")
}

/// The vendor's releases of a CLI, as the backend follows them.
pub struct FollowedReleases(Arc<Followed>);

struct Followed {
    cli: &'static Cli,
    distribution: Box<dyn Distribution>,
    fetch: Fetch,
    /// Each version's release. A `std` mutex: no section awaits, and
    /// `newest` reads one at a time.
    releases: Mutex<HashMap<String, Release>>,
    /// The newest release, which a start reads without waiting for a read
    /// under way.
    newest: KeptRead<Release>,
}

impl FollowedReleases {
    /// The releases of `cli` that `distribution` reads, on `edge`, believed
    /// for six hours by `clock`.
    pub fn new(
        cli: &'static Cli,
        distribution: impl Distribution,
        edge: Handle,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(READ_TIMEOUT)
            .build()?;
        Ok(Self(Arc::new(Followed {
            cli,
            distribution: Box::new(distribution),
            fetch: Fetch { http, edge },
            releases: Mutex::default(),
            newest: KeptRead::new(LATEST_TTL, clock),
        })))
    }
}

impl CliReleases for FollowedReleases {
    /// The vendor's newest release: the one read within six hours, or a new
    /// read. `refresh` reads the newest version again unless a read finished
    /// after the call began.
    fn latest(&self, refresh: bool) -> BoxFuture<'_, Result<Release, ReleaseError>> {
        Box::pin(self.0.latest(refresh))
    }

    /// The newest release read within six hours, without waiting. Without
    /// one, as after the backend starts, it answers none and reads the
    /// newest version in the background, unless a read is under way, for a
    /// later call to answer with.
    fn believed(&self) -> Option<Release> {
        let followed = self.0.clone();
        // The read ends within `READ_TIMEOUT`; the edge's runtime ending at
        // shutdown drops it before. A failed read is read again by a later
        // start, which still finds no release believed.
        let read_again = async move {
            if let Err(error) = followed.latest(false).await {
                tracing::warn!(
                    event = "cli_release.read_failed",
                    cli = followed.cli.name,
                    %error,
                    "the newest release of a subscription's CLI was not read"
                );
            }
        };
        let kept = self.0.newest.kept(&self.0.fetch.edge, read_again)?;
        kept.fresh.then_some(kept.value)
    }
}

impl Followed {
    async fn latest(&self, refresh: bool) -> Result<Release, ReleaseError> {
        self.newest.read(refresh, || self.read_latest()).await
    }

    /// The release of the newest version, read once.
    async fn read_latest(&self) -> Result<Release, ReleaseError> {
        let version = self
            .distribution
            .newest(&self.fetch)
            .await
            .map_err(ReleaseError)?;
        if !is_version(&version) {
            return Err(ReleaseError("the distribution named no version".into()));
        }
        if let Some(release) = self.releases().get(&version) {
            return Ok(release.clone());
        }
        let release = self.release(&version).await.map_err(|reason| {
            ReleaseError(format!(
                "the release of version {version} could not be read ({reason})"
            ))
        })?;
        self.releases().insert(version, release.clone());
        Ok(release)
    }

    fn releases(&self) -> std::sync::MutexGuard<'_, HashMap<String, Release>> {
        // A section only looks a release up or adds one.
        self.releases.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Version `version`'s release record.
    async fn release(&self, version: &str) -> Result<Release, String> {
        let platforms = self.distribution.platforms(&self.fetch, version).await?;
        let release = Release {
            cli: self.cli.name.to_owned(),
            version: version.to_owned(),
            platforms,
        };
        garde::Validate::validate(&release)
            .map_err(|report| report.to_string().trim_end().to_owned())?;
        Ok(release)
    }
}
