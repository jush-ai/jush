//! A conversation's schedules, as the web contract reaches them.
//!
//! - A program makes, reads, changes, runs and deletes a schedule of a
//!   conversation the caller owns, in the caller's core, with the types
//!   `jush schedule` uses; the page's switch and Delete are two of these
//!   changes. Each reaches the conversation's entry in the product state.
//! - A schedule's URL, `/hooks/<id>`, is authenticated by no session: its id
//!   and its secret are its only keys.

use std::time::Instant;

use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use jush_core::schedules::{CALLS_PER_MINUTE, HookAnswer, HookCall, ScheduleError};
use jush_shared_types::ScheduleId;
use jush_web_api_protocol::error::ErrorCode;
use jush_core_store::StorageError;
use jush_web_api_protocol::ids::{ConversationId, UserId};
use futures_util::future::BoxFuture;
use jush_web_api_protocol::schedules::{
    NewScheduleRun, ScheduleAnswer, ScheduleDto, SchedulePatch, ScheduleRunDto, ScheduleRuns, ScheduleRunsQuery,
};

use crate::api::Api;
use crate::body::{JsonBody, read};
use crate::error::ApiError;
use crate::caller::Principal;
use crate::page_client::client_address;
use crate::query::QueryParams;

/// The one answer an unknown address and a wrong secret share.
pub(crate) fn unauthorized() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        ErrorCode::Unauthenticated,
        "The call carries no valid secret for this address",
    )
}

fn too_many() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::RateLimited,
        format!("This address takes {CALLS_PER_MINUTE} calls a minute"),
    )
}

/// Which core of the process holds a schedule, which its URL's call
/// reaches: the server keeps a record of each schedule's owner, a program's
/// process looks among the cores it has open.
pub trait ScheduleOwners: Send + Sync {
    /// The principal whose core holds the schedule `id`; none for an id no
    /// open core holds.
    fn owner<'a>(&'a self, id: &'a ScheduleId) -> BoxFuture<'a, Result<Option<UserId>, StorageError>>;
}

/// `POST /hooks/:id`: a call of a schedule's URL. An unknown address gets
/// the answer a wrong secret gets.
pub async fn hook(
    State(state): State<Api>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let limits = &state.services.hook_limits;
    let (parts, body) = request.into_parts();
    let client = client_address(&parts);
    let (id, owner) = match admit(&state, id, |now| limits.admit_unknown(client, now)).await? {
        Admission::Known { id, owner } => (id, owner),
        Admission::Unknown => return Err(unauthorized()),
        Admission::TooMany => return Err(too_many()),
    };
    if parts.method != Method::POST {
        return Err(unauthorized());
    }
    let method = parts.method.to_string();
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned())))
        .collect();
    let body = read(Request::from_parts(parts, body), &state).await?;
    match take(&state, &owner, id, HookCall { method, headers, body }).await? {
        HookAnswer::Taken => Ok(StatusCode::OK.into_response()),
        HookAnswer::Unknown | HookAnswer::Secret => Err(unauthorized()),
    }
}

/// Where a call of a schedule's URL goes, once it is counted.
pub(crate) enum Admission {
    /// To the schedule `id`, in the core of `owner`.
    Known { id: ScheduleId, owner: UserId },
    /// Nowhere: no open core holds a schedule of the id.
    Unknown,
    /// Nowhere: the calls are over their limit.
    TooMany,
}

/// Finds the core that holds the schedule `id`, and counts the call against
/// the schedule's limit. `unknown` counts a call of an id no core holds:
/// the server limits those by the caller's address, which a program's
/// process does not see, so the program limits them at its own server.
pub(crate) async fn admit(
    state: &Api,
    id: String,
    unknown: impl FnOnce(Instant) -> bool,
) -> Result<Admission, ApiError> {
    let now = Instant::now();
    let id = ScheduleId::try_from(id).ok();
    let owner = match &id {
        Some(id) => state.schedule_owners.owner(id).await?,
        None => None,
    };
    let admitted = match (&id, &owner) {
        (Some(id), Some(_)) => state.services.hook_limits.admit_known(id, now),
        _ => unknown(now),
    };
    Ok(match (id, owner, admitted) {
        (_, _, false) => Admission::TooMany,
        (Some(id), Some(owner), true) => Admission::Known { id, owner },
        _ => Admission::Unknown,
    })
}

/// Hands `call` to the schedule `id` in the core of `owner`.
pub(crate) async fn take(state: &Api, owner: &UserId, id: ScheduleId, call: HookCall) -> Result<HookAnswer, ApiError> {
    Ok(state
        .shards
        .of(owner)
        .call(move |core, _| async move { core.hook_call(&id, call).await })
        .await??)
}

/// The conversation and the schedule a path names; an id that is not one is
/// not found.
fn ids(conversation: String, id: String) -> Result<(ConversationId, ScheduleId), ApiError> {
    let conversation =
        ConversationId::try_from(conversation).map_err(|_| refused(ScheduleError::ConversationNotFound))?;
    let id = ScheduleId::try_from(id).map_err(|_| refused(ScheduleError::NotFound))?;
    Ok((conversation, id))
}

/// `POST /conversations/:id/schedules`: a program makes a schedule of the
/// conversation from the defaults `jush schedule create` takes, with the
/// patch over them, on unless the patch pauses it; 201.
pub(crate) async fn create(
    State(state): State<Api>,
    Principal(user): Principal,
    Path(conversation): Path<String>,
    JsonBody(patch): JsonBody<SchedulePatch>,
) -> Result<(StatusCode, Json<ScheduleAnswer>), ApiError> {
    let conversation =
        ConversationId::try_from(conversation).map_err(|_| refused(ScheduleError::ConversationNotFound))?;
    let (schedule, secret) = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.create_schedule(&conversation, patch, None).await })
        .await?
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(ScheduleAnswer { schedule, secret })))
}

/// `GET /conversations/:id/schedules/:schedule`: one of the conversation's
/// schedules, as `jush schedule show` shows it.
pub(crate) async fn show(
    State(state): State<Api>,
    Principal(user): Principal,
    Path((conversation, id)): Path<(String, String)>,
) -> Result<Json<ScheduleDto>, ApiError> {
    let (conversation, id) = ids(conversation, id)?;
    let schedule = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.schedule(&conversation, &id).await })
        .await?
        .map_err(refused)?;
    Ok(Json(schedule))
}

/// `PATCH /conversations/:id/schedules/:schedule`: a program changes what the
/// patch names of one of the conversation's schedules, as the page's switch
/// turns it off and on.
pub(crate) async fn change(
    State(state): State<Api>,
    Principal(user): Principal,
    Path((conversation, id)): Path<(String, String)>,
    JsonBody(patch): JsonBody<SchedulePatch>,
) -> Result<Json<ScheduleAnswer>, ApiError> {
    let (conversation, id) = ids(conversation, id)?;
    let (schedule, secret) = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.change_schedule(&conversation, &id, patch, false, None).await })
        .await?
        .map_err(refused)?;
    Ok(Json(ScheduleAnswer { schedule, secret }))
}

/// `DELETE /conversations/:id/schedules/:schedule`: deletes one of the
/// conversation's schedules with its runs' records, 204.
pub(crate) async fn delete(
    State(state): State<Api>,
    Principal(user): Principal,
    Path((conversation, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let (conversation, id) = ids(conversation, id)?;
    state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.delete_schedule(&conversation, &id).await })
        .await?
        .map_err(refused)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /conversations/:id/schedules/:schedule/runs`: runs one of the
/// conversation's schedules now, as `jush schedule run` does; 201 with the
/// run as it started.
pub(crate) async fn run(
    State(state): State<Api>,
    Principal(user): Principal,
    Path((conversation, id)): Path<(String, String)>,
    JsonBody(run): JsonBody<NewScheduleRun>,
) -> Result<(StatusCode, Json<ScheduleRunDto>), ApiError> {
    let (conversation, id) = ids(conversation, id)?;
    let (_, run) = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.run_schedule_now(&conversation, &id, run.task).await })
        .await?
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(run)))
}

/// `GET /conversations/:id/schedules/:schedule/runs?before=`: a page of the
/// schedule's runs before the run `before`, newest first; the latest
/// without it.
pub(crate) async fn runs(
    State(state): State<Api>,
    Principal(user): Principal,
    Path((conversation, id)): Path<(String, String)>,
    QueryParams(query): QueryParams<ScheduleRunsQuery>,
) -> Result<Json<ScheduleRuns>, ApiError> {
    let (conversation, id) = ids(conversation, id)?;
    let runs = state
        .shards
        .of(&user)
        .call(move |core, _| async move { core.schedule_runs(&conversation, &id, query.before).await })
        .await?
        .map_err(refused)?;
    Ok(Json(ScheduleRuns { runs }))
}

fn refused(error: ScheduleError) -> ApiError {
    let message = error.to_string();
    let (status, code) = match error {
        ScheduleError::NotFound => (StatusCode::NOT_FOUND, ErrorCode::ScheduleNotFound),
        ScheduleError::ConversationNotFound => (StatusCode::NOT_FOUND, ErrorCode::ConversationNotFound),
        ScheduleError::NoRun(_) => (StatusCode::BAD_REQUEST, ErrorCode::InvalidQuery),
        ScheduleError::Storage(error) => return error.into(),
        ScheduleError::NotRecorded => return ApiError::internal_message(message),
        // The rest refuse a part of the patch, which the message names.
        _ => (StatusCode::BAD_REQUEST, ErrorCode::InvalidBody),
    };
    ApiError::new(status, code, message)
}
