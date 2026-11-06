//! `DELETE /conversations/:id/pins/:pin`: the user unpins a document the
//! conversation's main agent pinned
//! (`crates/core/src/conversation/pins.rs`), which the agent
//! learns before its next request. A document no longer pinned answers as
//! one just unpinned.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use jush_core::services::CoreServices;
use jush_core_store::pins::Unpinner;
use jush_core_sync::Part;

use crate::caller::Principal;
use crate::conversations::owned;
use crate::error::ApiError;

pub(crate) async fn unpin(
    State(services): State<Arc<CoreServices>>,
    Principal(user): Principal,
    Path((id, pin)): Path<(String, u64)>,
) -> Result<StatusCode, ApiError> {
    let record = owned(&services, &user, &id).await?;
    if services.vault.stores().store_of(&user).await?.unpin(record.id.clone(), pin, Unpinner::User).await? {
        services.sync.mark(&user, Part::Metadata(record.id));
    }
    Ok(StatusCode::NO_CONTENT)
}
