//! `GET /api/search?q=`: the caller's conversations that match;
//! `GET /api/conversations/{id}/search?q=`: the messages of one of them that
//! match; and `GET /api/history/search?q=`: the texts of every conversation's
//! agents that match, page by page; each read from the caller's search index.
//! A query outside its bounds answers 400 `invalid_query`.

use axum::Json;
use axum::extract::{Path, State};
use jush_core::conversation::search;
use jush_core_store::search::AllQuery;
use jush_web_api_protocol::search::{
    HISTORY_HITS_DEFAULT, HISTORY_HITS_MAX, HistoryHits, HistorySearchQuery, MessageResults, SearchKind, SearchQuery,
    SearchResults,
};

use crate::api::Api;
use crate::conversations::owned;
use crate::error::ApiError;
use crate::caller::Principal;
use crate::query::{QueryParams, comma_list};

pub(crate) async fn search(
    State(state): State<Api>,
    Principal(user): Principal,
    QueryParams(SearchQuery { q, workspace }): QueryParams<SearchQuery>,
) -> Result<Json<SearchResults>, ApiError> {
    let results = search::search(&state.store(&user).await?, &q, workspace.as_ref()).await?;
    Ok(Json(SearchResults { results }))
}

/// `GET /api/conversations/{id}/search?q=`; a conversation not the caller's
/// answers 404.
pub(crate) async fn search_in(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(id): Path<String>,
    QueryParams(SearchQuery { q, .. }): QueryParams<SearchQuery>,
) -> Result<Json<MessageResults>, ApiError> {
    let record = owned(&state.services, &user, &id).await?;
    Ok(Json(search::search_in(&state.store(&user).await?, &record.id, &q).await?))
}

/// `GET /api/history/search?q=&any=&kinds=&from=&to=&after=&limit=`: a page
/// of the texts of every conversation's agents, its subagents' and its
/// archived conversations' included, that hold the query's words, newest
/// first.
pub(crate) async fn history(
    State(state): State<Api>,
    Principal(user): Principal,
    QueryParams(query): QueryParams<HistorySearchQuery>,
) -> Result<Json<HistoryHits>, ApiError> {
    let kinds = comma_list(query.kinds.as_deref())
        .map(|name| {
            SearchKind::named(name.trim())
                .ok_or_else(|| ApiError::invalid_query(format!("kinds: {name:?} is none of message, answer and command")))
        })
        .collect::<Result<_, _>>()?;
    let limit = query.limit.unwrap_or(HISTORY_HITS_DEFAULT);
    if !(1..=HISTORY_HITS_MAX).contains(&limit) {
        return Err(ApiError::invalid_query(format!("limit: from 1 to {HISTORY_HITS_MAX}")));
    }
    let after = match query.after.as_deref().filter(|after| !after.is_empty()) {
        Some(after) => Some(
            search::place_of(after)
                .ok_or_else(|| ApiError::invalid_query(format!("after: {after} is no cursor a page of this search gave")))?,
        ),
        None => None,
    };
    let search = AllQuery {
        words: query.q.words(),
        any: query.any == Some(true),
        kinds,
        from: query.from,
        to: query.to,
        after,
        limit: usize::try_from(limit).unwrap_or(usize::MAX),
    };
    Ok(Json(search::search_history(&state.store(&user).await?, search).await?))
}
