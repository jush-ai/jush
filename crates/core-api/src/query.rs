//! Query parameters decoded into their types: a value outside its type answers
//! 400 `invalid_query`.

use axum::extract::{FromRequestParts, Query};
use axum::http::StatusCode;
use axum::http::request::Parts;
use jush_web_api_protocol::error::ErrorCode;
use serde::de::DeserializeOwned;

use crate::error::ApiError;

/// The request's query as `T`.
pub struct QueryParams<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for QueryParams<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let Query(value) =
            Query::<T>::from_request_parts(parts, state)
                .await
                .map_err(|rejection| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        ErrorCode::InvalidQuery,
                        rejection.body_text(),
                    )
                })?;
        Ok(Self(value))
    }
}

/// The values of a comma list a query names; `name=` is an empty one.
pub(crate) fn comma_list(names: Option<&str>) -> impl Iterator<Item = &str> {
    names.into_iter().filter(|names| !names.is_empty()).flat_map(|names| names.split(','))
}
