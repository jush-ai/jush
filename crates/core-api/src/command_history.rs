//! The core's command history (`crates/core-store/src/command_history.rs`),
//! which a program reads in the order its rows were written.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use jush_core::services::CoreServices;
use jush_core_store::command_history::HistoryStart;
use jush_web_api_protocol::command_history::{CommandHistory, CommandHistoryQuery};

use crate::caller::Principal;
use crate::error::ApiError;
use crate::query::QueryParams;

/// The most rows a page holds: the core reads a page whole while it holds
/// its database, and sends it whole, so a page holds a bounded number of
/// scripts, each what one call of a model wrote; a reader reads on with
/// `next`.
const MOST: u32 = 1_000;

/// A page's rows when the query names no limit.
const PAGE: u32 = 100;

/// `GET /commands/history`: the history's rows after the cursor `after`, or
/// from the first that ended at `from` or after, or from the first.
pub(crate) async fn read(
    State(services): State<Arc<CoreServices>>,
    Principal(user): Principal,
    QueryParams(query): QueryParams<CommandHistoryQuery>,
) -> Result<Json<CommandHistory>, ApiError> {
    let unknown = |after: &str| ApiError::invalid_query(format!("after: {after} is no cursor a page of this core gave"));
    let start = match (query.after.as_deref(), query.from) {
        (Some(_), Some(_)) => return Err(ApiError::invalid_query("after and from: give one of them")),
        (Some(after), None) => HistoryStart::After(after.parse().map_err(|()| unknown(after))?),
        (None, Some(from)) => HistoryStart::From(from),
        (None, None) => HistoryStart::First,
    };
    let limit = query.limit.unwrap_or(PAGE);
    if !(1..=MOST).contains(&limit) {
        return Err(ApiError::invalid_query(format!("limit: from 1 to {MOST}")));
    }
    let history = services.vault.stores().store_of(&user).await?.command_history(start, limit).await?;
    history.map(Json).ok_or_else(|| unknown(query.after.as_deref().unwrap_or_default()))
}
