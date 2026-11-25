//! What the routes reach: the services every core of the process shares,
//! the pool its cores run in, and what the routes keep themselves.

use std::sync::Arc;

use axum::extract::FromRef;
use jush_core::pool::Shards;
use jush_core::services::CoreServices;
use jush_core_devices::claims::PendingClaims;
use jush_core_store::StorageError;
use jush_core_store::store::CoreStore;
use jush_web_api_protocol::ids::UserId;

use crate::connection::PageTuning;
use crate::debug::DebugLinks;
use crate::html_view::HtmlGrants;
use crate::page::PageUploads;
use crate::schedules::ScheduleOwners;

/// What the routes reach. Cloning it is cheap.
#[derive(Clone)]
pub struct Api {
    pub services: Arc<CoreServices>,
    pub shards: Shards,
    /// The tokens of the HTML files the File view shows.
    pub html: Arc<HtmlGrants>,
    /// What programs' pages uploaded.
    pub page_uploads: Arc<PageUploads>,
    /// The links of the pages the user chose to debug.
    pub debug: Arc<DebugLinks>,
    /// How a page's connection is timed.
    pub pages: PageTuning,
    /// Which core holds a schedule whose URL is called.
    pub schedule_owners: Arc<dyn ScheduleOwners>,
    /// The runners waiting to be paired.
    pub claims: Arc<PendingClaims>,
}

impl Api {
    /// The store of `user`'s core, through the lookup the vault uses.
    pub async fn store(&self, user: &UserId) -> Result<CoreStore, StorageError> {
        self.services.vault.stores().store_of(user).await
    }
}

impl FromRef<Api> for Arc<CoreServices> {
    fn from_ref(api: &Api) -> Self {
        api.services.clone()
    }
}

impl FromRef<Api> for Shards {
    fn from_ref(api: &Api) -> Self {
        api.shards.clone()
    }
}
