//! The quota of an Antigravity account: its plan, from a probe's new session,
//! and its windows, from a usage read (`probe.rs`).
//!
//! `agy --print /usage` reports the account's limits by group of models, each
//! a share left and its reset (agy 1.5.14):
//!
//! ```text
//! {"status": "SUCCESS", "command": {"name": "usage", "data": {"groups": [
//!   {"name": "Gemini Models",
//!    "description": "Models within this group: Gemini 4 Neon, Gemini 3.8 Flash",
//!    "buckets": [{"id": "gemini-5h", "window": "5h",
//!    "remaining_fraction": 0.9, "reset_time": "…"}]}]}}}
//! ```
//!
//! Each bucket is a window scoped to its group, whose scope names the group's
//! models by their catalog ids: the models its description lists by the
//! names the new session gives them, or, for a group that lists none, the
//! model of the group's own name. `agy` reports no usage of a live response,
//! so nothing is observed.

use futures_util::future::BoxFuture;
use jush_provider_common::quota::{
    ObservedWindows, Observation, ProbeCost, ProbeReading, QuotaError, QuotaSource, clamp_used_percent,
    rfc3339, severity,
};
use jush_shared_types::{QuotaKind, QuotaPlan, QuotaScope, QuotaUnit, QuotaWindow};
use serde::Deserialize;
use serde_json::Value;

use crate::models::{self, Offered};
use crate::probe::Prober;

/// The quota source of one Antigravity account.
pub(crate) struct AgyQuota(pub(crate) Prober);

impl QuotaSource for AgyQuota {
    fn probe_cost(&self) -> Option<ProbeCost> {
        Some(ProbeCost::Free)
    }

    /// The plan a new session of the account names, and the windows its
    /// usage read reports. A usage read that fails fails the probe, so the
    /// page shows why and the next probe waits as after any failed read.
    fn probe(&self) -> BoxFuture<'_, Result<ProbeReading, QuotaError>> {
        Box::pin(async move {
            let session = self.0.session().await.map_err(|failure| failure.quota_error())?;
            let printed = self.0.usage().await.map_err(|failure| failure.quota_error())?;
            let windows = windows(printed, &models::offered(&session)).map_err(QuotaError::Invalid)?;
            let tier = session
                .meta
                .and_then(|meta| meta.antigravity)
                .and_then(|antigravity| antigravity.tier);
            Ok(ProbeReading {
                plan: tier.map(|tier| QuotaPlan {
                    id: tier.id,
                    label: tier.name,
                }),
                account_label: None,
                windows,
                absent: Vec::new(),
            })
        })
    }

    fn observe(&self, _: Observation<'_>) -> Option<ObservedWindows> {
        None
    }
}

/// What `agy --print /usage` prints.
#[derive(Deserialize)]
struct Printed {
    status: String,
    #[serde(default)]
    response: String,
    command: Option<Command>,
}

#[derive(Deserialize)]
struct Command {
    data: Usage,
}

#[derive(Deserialize)]
struct Usage {
    groups: Vec<Group>,
}

/// A group of models that share their limits.
#[derive(Deserialize)]
struct Group {
    name: String,
    /// Such as "Models within this group: Gemini 4 Neon, Gemini 3.8 Flash";
    /// a group of one model describes it in prose instead.
    #[serde(default)]
    description: Option<String>,
    buckets: Vec<Bucket>,
}

/// What a group's description says before the names of its models.
const MEMBERS: &str = "Models within this group:";

/// One limit of a group.
#[derive(Deserialize)]
struct Bucket {
    id: String,
    window: Option<String>,
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
}

/// The windows a usage read printed, their groups' models among `offered`,
/// or why it printed none.
fn windows(printed: Value, offered: &[Offered]) -> Result<Vec<QuotaWindow>, String> {
    let printed = Printed::deserialize(printed).map_err(|error| error.to_string())?;
    let Some(command) = printed.command.filter(|_| printed.status == "SUCCESS") else {
        return Err(format!("{}: {}", printed.status, printed.response.trim()));
    };
    Ok(command
        .data
        .groups
        .into_iter()
        .flat_map(|group| {
            let scope = scope(&group, offered);
            group.buckets.into_iter().map(move |bucket| window(bucket, scope.clone()))
        })
        .collect())
}

/// A bucket as a window of its group's `scope`.
fn window(bucket: Bucket, scope: QuotaScope) -> QuotaWindow {
    let used_percent = bucket
        .remaining_fraction
        .and_then(|left| clamp_used_percent((1.0 - left) * 100.0));
    QuotaWindow {
        id: bucket.id,
        kind: QuotaKind::Limit,
        used_percent,
        used: None,
        limit: None,
        unit: Some(QuotaUnit::Percent),
        resets_at: bucket.reset_time.as_deref().and_then(rfc3339),
        period_seconds: bucket.window.as_deref().and_then(period_seconds),
        requested_at: None,
        severity: severity(used_percent),
        scope: Some(scope),
    }
}

/// How long a bucket's window lasts, by the name `agy` gives it.
fn period_seconds(window: &str) -> Option<u64> {
    match window {
        "5h" => Some(5 * 60 * 60),
        "weekly" => Some(7 * 24 * 60 * 60),
        _ => None,
    }
}

/// The scope of `group`'s windows: its models among `offered`, labeled as
/// the group is named without a trailing "models", since the page puts the
/// label in "Weekly … limit".
fn scope(group: &Group, offered: &[Offered]) -> QuotaScope {
    let name = group.name.trim();
    let label = name
        .strip_suffix(" Models")
        .or_else(|| name.strip_suffix(" models"))
        .unwrap_or(name);
    let members: Vec<&str> = match group.description.as_deref().and_then(|text| text.trim().strip_prefix(MEMBERS)) {
        Some(listed) => listed.split(',').map(str::trim).collect(),
        None => vec![name],
    };
    let model_ids = offered
        .iter()
        .filter(|model| members.iter().any(|member| member.eq_ignore_ascii_case(&model.name)))
        .map(|model| model.id.clone())
        .collect();
    QuotaScope {
        kind: "model".to_owned(),
        label: Some(label.to_owned()),
        model_ids,
    }
}
