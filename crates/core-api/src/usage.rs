//! The usage ledger's totals of the caller's core
//! (`crates/core-store/src/usage.rs`), and its report by period
//! (`crates/core-store/src/usage_report.rs`).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use jiff::tz::TimeZone;
use jush_core_store::usage_report::{Grouping, Kept, ReportAsk};
use jush_shared_types::check_label_key;
use jush_web_api_protocol::ids::ConversationId;
use jush_web_api_protocol::usage::{UsageDimension, UsageReport, UsageReportQuery, UsageTotals};

use crate::error::ApiError;
use crate::caller::Principal;
use crate::query::{QueryParams, comma_list};
use jush_core::services::CoreServices;

pub(crate) async fn totals(
    State(services): State<Arc<CoreServices>>,
    Principal(user): Principal,
) -> Result<Json<UsageTotals>, ApiError> {
    let totals = services.vault.stores().store_of(&user).await?.usage_totals().await?;
    Ok(Json(UsageTotals { totals }))
}

/// `GET /usage/report`: the calls of a period, of the conversations and
/// agents the query names, by bucket and by what the query groups them by.
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
    for name in comma_list(query.group_by.as_deref()) {
        let grouping = match name.strip_prefix("label:") {
            Some(key) => {
                check_label_key(key).map_err(|reason| ApiError::invalid_query(format!("groupBy: {reason}")))?;
                Grouping::Label(key.to_owned())
            }
            None => serde_plain::from_str::<UsageDimension>(name).map(Grouping::Dimension).map_err(|_| {
                ApiError::invalid_query(format!("groupBy: {name} is not a dimension of the report, nor label:<key>"))
            })?,
        };
        if !group_by.contains(&grouping) {
            group_by.push(grouping);
        }
    }
    // A list that is there and empty keeps no call.
    let named = |names: &str| names.split(',').filter(|name| !name.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let agents = query
        .agents
        .as_deref()
        .map(|numbers| {
            named(numbers)
                .into_iter()
                .map(|number| {
                    number.parse::<u64>().map_err(|_| ApiError::invalid_query(format!("agents: {number} is no agent's number")))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let kept = match (query.conversations.as_deref(), agents) {
        (None, None) => Kept::Every,
        // A number names an agent of its conversation alone.
        (None, Some(_)) => return Err(ApiError::invalid_query("agents: names agents of the conversations `conversations` names")),
        (Some(ids), agents) => {
            let conversations = named(ids)
                .into_iter()
                .map(|id| {
                    ConversationId::try_from(id.clone())
                        .map(|_| id.clone())
                        .map_err(|_| ApiError::invalid_query(format!("conversations: {id} is no conversation's id")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Kept::Of { conversations, agents }
        }
    };
    let ask = ReportAsk { from: query.from, to: query.to, bucket: query.bucket, zone, group_by, kept };
    let report = services.vault.stores().store_of(&user).await?.usage_report(ask).await?;
    Ok(Json(report))
}
