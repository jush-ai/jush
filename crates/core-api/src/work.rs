//! The work's route: a request of changes, which the user's core applies
//! whole or not at all and answers with the work's revision; the
//! conversation's entry in the list carries the tabs.
//!
//! The core applies the changes of a conversation's work one at a time, the page's
//! and the plugin's alike. A plugin whose work on one conversation's tabs takes
//! several port operations, such as reading the browser's tabs and then adding
//! the missing ones, does that work one piece at a time per conversation, so a
//! change it makes is never based on what it read before its own earlier
//! change.
//!
//! The user creates a tab of a plugin's kind only through an intent the kind
//! opens; otherwise the plugin creates them.


use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use jush_plugin_host::WorkError;
use jush_web_api_protocol::error::ErrorCode;
use jush_web_api_protocol::work::{WORK_BYTES_MAX, WorkAnswer, WorkChanges};

use crate::api::Api;
use crate::body::JsonBody;
use crate::conversations::owned;
use crate::error::ApiError;
use crate::caller::CallerOf;

/// `POST /conversations/:id/work/changes { changes }`: applied in order,
/// all or none, as one change of the work.
pub(crate) async fn change(
    State(state): State<Api>,
    CallerOf(caller): CallerOf,
    Path(id): Path<String>,
    body: Result<JsonBody<WorkChanges>, ApiError>,
) -> Result<Json<WorkAnswer>, ApiError> {
    let user = caller.principal().clone();
    let record = owned(&state.services, &user, &id).await?;
    let JsonBody(WorkChanges { changes }) = body?;
    let scope = caller.scope().cloned();
    let (revision, changed) = state
        .shards
        .of(&user)
        .call(move |core, _| async move {
            // A page changes the tabs of the windows it may open alone.
            if let Some(scope) = scope {
                let work = core.conversation_summary(record.clone()).await?.work;
                crate::page::work(&scope, &work, &changes, core.services())?;
            }
            core.plugins().change_work(record.id, changes).await.map_err(refused)
        })
        .await??;
    Ok(Json(WorkAnswer { revision, changed }))
}

fn refused(error: WorkError) -> ApiError {
    let message = error.to_string();
    match error {
        WorkError::UnknownKind(_) => ApiError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::UnknownTabKind,
            message,
        ),
        WorkError::Full => ApiError::new(StatusCode::CONFLICT, ErrorCode::WorkFull, message),
        WorkError::TooLarge => {
            ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, ErrorCode::TooLarge, message)
                .with_param("limitBytes", WORK_BYTES_MAX)
        }
        WorkError::Missing => ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::ConversationNotFound,
            message,
        ),
        WorkError::Storage(error) => error.into(),
    }
}
