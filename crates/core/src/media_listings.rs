//! The media models the product state shows each entry making: what each
//! entry listed of each medium last, held in memory, so that no page waits
//! for a vendor to list its models.
//!
//! - The state answers the held list at once. A list never listed, listed
//!   more than 15 minutes before, or asked again by Settings' Refresh, is
//!   listed again in the background, one listing per entry and medium at a
//!   time. A listing that ends with other models than were held marks every
//!   page's models, which they read again.
//! - A listing that fails holds none, as the state showed none for a vendor
//!   that failed before, and the next is tried after a minute.
//! - The background listing, `jush image models`' and a call's write it
//!   (`crate::media`); those two still list from the vendor at the moment,
//!   since a lent entry's maker learns from its listing what it may ask.
//! - A list is held with the vendor its maker named after listing, which a
//!   lent entry's maker knows only then.
//! - A list belongs to the entry's catalog key, its family, configuration
//!   and active account, and to the media catalog of the time. Under another
//!   key or catalog it shows on until the listing it starts ends, so a
//!   change to the entry never empties the page's lists meanwhile.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use jush_core_sync::Part;
use jush_provider_common::media::{MediaModelInfo, MediaVendor, REQUEST_TIMEOUT};
use jush_provider_registry::catalog::catalog_key;
use jush_shared_types::{MediaKind, Timestamp};
use jush_web_api_protocol::ids::ProviderId;

use crate::media::Route;
use crate::services::CoreServices;

/// How long a list is shown before it is listed again: as long as an
/// entry's text catalog stays fresh.
const FRESH_FOR: Duration = Duration::from_secs(15 * 60);

/// How long after a failed listing the next is tried: as long as a text
/// catalog's failed refresh holds off the next.
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// The longest a background listing may take, a vendor's one request's: a
/// lent entry's channel has no limit of its own, and a listing that never
/// ended would hold off every later one.
const LISTING_LIMIT: Duration = REQUEST_TIMEOUT;

/// Each entry's models of each medium, as it listed them last.
#[derive(Clone, Default)]
pub struct MediaListings {
    /// A `std` mutex: a section only reads or replaces a list, never across
    /// an await.
    lists: Arc<Mutex<HashMap<(ProviderId, MediaKind), Held>>>,
}

/// What a page shows of one entry's medium.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Shown {
    /// The models, ordered by ID so that one vendor's two orders are one list.
    pub(crate) models: Vec<MediaModelInfo>,
    /// The vendor the entry's maker named after the listing.
    pub(crate) vendor: Option<MediaVendor>,
}

/// One entry's list of one medium.
struct Held {
    /// The entry's catalog key and the media catalog's date the list was
    /// listed under.
    key: String,
    catalog: Timestamp,
    shown: Shown,
    /// When a listing under this key and catalog last answered.
    answered: Option<Instant>,
    /// When the last listing failed, if it did.
    failed: Option<Instant>,
    /// Whether a background listing runs.
    listing: bool,
}

impl Held {
    /// Whether a background listing is due; `force` for Settings' Refresh.
    fn due(&self, force: bool) -> bool {
        !self.listing
            && (force
                || (self.answered.is_none_or(|at| at.elapsed() >= FRESH_FOR)
                    && self.failed.is_none_or(|at| at.elapsed() >= RETRY_AFTER)))
    }
}

impl MediaListings {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(ProviderId, MediaKind), Held>> {
        // A section only reads or replaces fields, so a poisoned map is whole.
        self.lists.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The list of `route`'s entry under `key` and `catalog`: a list held
    /// under another shows on, to be listed again.
    fn held<'a>(
        lists: &'a mut HashMap<(ProviderId, MediaKind), Held>,
        route: &Route,
        key: String,
        catalog: Timestamp,
    ) -> &'a mut Held {
        let held = lists.entry((route.entry.id.clone(), route.kind)).or_insert_with(|| Held {
            key: key.clone(),
            catalog,
            shown: Shown::default(),
            answered: None,
            failed: None,
            listing: false,
        });
        if held.key != key || held.catalog != catalog {
            // A listing under the old ones ends unheard (`settle`).
            *held = Held { key, catalog, shown: std::mem::take(&mut held.shown), answered: None, failed: None, listing: false };
        }
        held
    }

    /// What `route`'s entry listed last, at once, and a listing in the
    /// background when none was, it is old, or `force` asks for one.
    pub(crate) fn shown(&self, services: &CoreServices, route: &Route, force: bool) -> Shown {
        let catalog = services.media.current();
        let key = catalog_key(&route.entry);
        let (shown, due) = {
            let mut lists = self.lock();
            let held = Self::held(&mut lists, route, key.clone(), catalog.updated);
            let due = held.due(force);
            held.listing |= due;
            (held.shown.clone(), due)
        };
        if due {
            let lists = self.clone();
            let sync = services.sync.clone();
            let http = route.http(services, services.provider_registry.http());
            let maker = route.maker.clone();
            let id = route.entry.id.clone();
            let kind = route.kind;
            tokio::spawn(async move {
                let answer = match tokio::time::timeout(LISTING_LIMIT, maker.media().models(&http, &catalog)).await {
                    Ok(Ok(models)) => Some(models),
                    Ok(Err(failure)) => {
                        tracing::info!(event = "media_listing.failed", provider = %id, ?kind, ?failure, "an entry's media models could not be listed");
                        None
                    }
                    Err(_) => {
                        tracing::info!(event = "media_listing.failed", provider = %id, ?kind, failure = "timed out", "an entry's media models could not be listed");
                        None
                    }
                };
                if lists.settle(&id, kind, &key, catalog.updated, maker.media().vendor(), answer, true) {
                    sync.mark_everyone(&Part::Models);
                }
            });
        }
        shown
    }

    /// Holds what `route`'s entry listed now under the media catalog of date
    /// `catalog`, as `jush image models` or a call listed it.
    pub(crate) fn listed(&self, services: &CoreServices, route: &Route, catalog: Timestamp, models: &[MediaModelInfo]) {
        let key = catalog_key(&route.entry);
        Self::held(&mut self.lock(), route, key.clone(), catalog);
        let vendor = route.media().vendor();
        let changed = self.settle(&route.entry.id, route.kind, &key, catalog, vendor, Some(models.to_vec()), false);
        if changed {
            services.sync.mark_everyone(&Part::Models);
        }
    }

    /// Drops the lists of an entry that was deleted.
    pub fn forget(&self, provider: &ProviderId) {
        self.lock().retain(|(id, _), _| id != provider);
    }

    /// Holds what a listing of `id`'s `kind` under `key` and `catalog`
    /// answered, with `vendor`, its maker's after it: the models, or none
    /// when it failed; whether the page's list changed. `background` ends
    /// the background listing. A list held under another key or catalog
    /// meanwhile takes nothing.
    fn settle(
        &self,
        id: &ProviderId,
        kind: MediaKind,
        key: &str,
        catalog: Timestamp,
        vendor: Option<MediaVendor>,
        answer: Option<Vec<MediaModelInfo>>,
        background: bool,
    ) -> bool {
        let mut lists = self.lock();
        let Some(held) = lists.get_mut(&(id.clone(), kind)) else {
            return false;
        };
        if held.key != key || held.catalog != catalog {
            return false;
        }
        if background {
            held.listing = false;
        }
        let now = Instant::now();
        let mut models = match answer {
            Some(models) => {
                held.answered = Some(now);
                held.failed = None;
                models
            }
            None => {
                held.failed = Some(now);
                Vec::new()
            }
        };
        models.sort_by(|a, b| a.id.cmp(&b.id));
        let shown = Shown { models, vendor };
        let changed = held.shown != shown;
        held.shown = shown;
        changed
    }
}
