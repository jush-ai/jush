//! The usage ledger's totals of the caller's core
//! (`crates/core-store/src/usage.rs`), and its report by period
//! (`crates/core-store/src/usage_report.rs`).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use jiff::tz::TimeZone;
use jush_core_store::usage_report::ReportAsk;
use jush_web_api_protocol::usage::{UsageDimension, UsageReport, UsageReportQuery, UsageTotals};

use crate::error::ApiError;
use crate::caller::Principal;
use crate::query::QueryParams;
use jush_core::services::CoreServices;

pub(crate) async fn totals(
    State(services): State<Arc<CoreServices>>,
    Principal(user): Principal,
) -> Result<Json<UsageTotals>, ApiError> {
    let totals = services.vault.stores().store_of(&user).await?.usage_totals().await?;
    Ok(Json(UsageTotals { totals }))
}

/// `GET /usage/report`: the calls of a period, by bucket and by what the
/// query groups them by.
pub(crate) async fn report(
    State(services): State<Arc<CoreServices>>,
    Principal(user): Principal,
    QueryParams(query): QueryParams<UsageReportQuery>,
) -> Result<Json<UsageReport>, ApiError> {
    if query.to <= query.from {
        return Err(ApiError::invalid_query("to: comes after from"));
    }
    let zone = match &query.time_zone {
        Some(name) => TimeZone::get(name)
            .map_err(|_| ApiError::invalid_query(format!("timeZone: {name} is no time zone this build knows")))?,
        None => TimeZone::UTC,
    };
    let mut group_by = Vec::new();
    // `groupBy=` is an empty list.
    for name in query.group_by.iter().filter(|names| !names.is_empty()).flat_map(|names| names.split(',')) {
        let dimension: UsageDimension = serde_plain::from_str(name)
            .map_err(|_| ApiError::invalid_query(format!("groupBy: {name} is not a dimension of the report")))?;
        if !group_by.contains(&dimension) {
            group_by.push(dimension);
        }
    }
    let ask = ReportAsk { from: query.from, to: query.to, bucket: query.bucket, zone, group_by };
    let report = services.vault.stores().store_of(&user).await?.usage_report(ask).await?;
    Ok(Json(report))
}
