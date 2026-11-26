//! The usage ledger's totals (`crates/core-store/src/usage.rs`): one
//! group per provider entry and model, in the order each pair was first
//! used, with the number of requests and the sum of each token count; and
//! its report, by period and by what a program groups the calls by.

use jush_shared_types::{MAX_SAFE_INTEGER, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::rust::double_option;

use crate::ids::UserId;
use crate::text::EmailAddress;

/// `GET /usage`: the caller's totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UsageTotals {
    pub totals: Vec<UsageGroup>,
}

/// The requests of one provider entry and model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageGroup {
    pub provider_id: String,
    pub model_id: String,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub requests: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub input_tokens: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub output_tokens: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub cache_read_tokens: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub cache_write_tokens: u64,
}

/// `?from=&to=&bucket=&timeZone=&groupBy=&conversations=&agents=` of
/// `GET /usage/report`: the calls that ended from `from` until `to`, in
/// buckets of a minute, an hour or a day of `time_zone`, UTC unless named,
/// grouped by what `group_by` names; of the conversations `conversations`
/// names and the agents of the numbers `agents` names, every call when
/// absent. Each list has a comma between two. Queries are the backend's
/// alone and are not emitted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageReportQuery {
    pub from: Timestamp,
    pub to: Timestamp,
    pub bucket: UsageBucketSize,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub group_by: Option<String>,
    #[serde(default)]
    pub conversations: Option<String>,
    #[serde(default)]
    pub agents: Option<String>,
}

/// How long a report's bucket is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageBucketSize {
    Minute,
    Hour,
    Day,
}

/// What a report groups a bucket's calls by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageDimension {
    Conversation,
    /// The number the call's conversation knows its agent by, 0 for the
    /// main agent; none for a side question's call and one no agent made,
    /// such as a title's.
    Agent,
    /// Whom the call's agent worked for, by the id the program gave its
    /// sender: the sender of the latest message that handed it work, or of
    /// the work whose end opened its turn.
    Sender,
    Model,
    Provider,
    /// The provider entry's account; none for an API key.
    Account,
    Turn,
    /// Why the call was made: a turn, a compaction, a title and so on.
    Purpose,
}

/// `GET /usage/report`: each bucket of the period that holds a call, oldest
/// first; the first and the last start and end with the period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UsageReport {
    pub buckets: Vec<UsageBucket>,
}

/// One minute, hour or day of a report, and its calls' groups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UsageBucket {
    pub start: Timestamp,
    pub end: Timestamp,
    pub groups: Vec<UsageReportGroup>,
}

/// The calls of a bucket that share each of the report's dimensions, which
/// it names; a dimension the report does not group by is absent, and one
/// a call has no value of, such as the sender of a title's call, is null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageReportGroup {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub conversation: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<u64>")]
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub agent: Option<Option<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub sender: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub model: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub provider: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub account: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub turn: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub purpose: Option<Option<String>>,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub calls: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub input_tokens: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub output_tokens: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub cache_read_tokens: u64,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub cache_write_tokens: u64,
    /// What the vendors said the calls cost, by currency, never converted.
    pub cost: UsageCosts,
    /// What the calls' tokens would cost at their models' API prices, in
    /// dollars, as each row fixed it when it was written.
    pub api_price: f64,
    /// The calls whose tokens `api_price` leaves out, their model having no
    /// price for a kind they used. A call the vendor counted no tokens
    /// for, as a refused one, is none of them.
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub unpriced_calls: u64,
}

/// Amounts by currency; a currency no call cost in is absent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UsageCosts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cny: Option<f64>,
}

/// `GET /usage/instance`: on a shared instance, every account's totals, for
/// an administrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InstanceUsage {
    pub users: Vec<UserUsage>,
}

/// One account's totals, the accounts in the order they were created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserUsage {
    pub user_id: UserId,
    pub email: EmailAddress,
    pub totals: Vec<UsageGroup>,
}
