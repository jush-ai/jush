//! The usage ledger's report: its calls of a period in buckets of a minute,
//! an hour or a day of a time zone, of the conversations and the agents it
//! names, each bucket's calls grouped by what the report names, the agent,
//! the sender and the assignment among them, which each row keeps.
//!
//! The rows of the period are read and summed here a bucket at a time, each
//! in a turn of the database's thread of its own: a report of a long period
//! keeps every other read and write of the core waiting for one bucket's
//! rows at most, never for the whole period's. They are found through the
//! ledger's time index, or, for a report of named conversations, through
//! its conversation index, one conversation at a time, so a conversation's
//! next row is found without reading the other conversations' between.
//! A call's row is written as the call ends, at that time, and the buckets
//! are read oldest first, so a row written while a report reads falls in the
//! bucket it reads last: the report is the ledger as that read found it.
//! A row's bucket is the minute, the hour or the day of the zone it ended
//! in, and a bucket starts no earlier and ends no later than the period, so
//! two reports of adjoining periods share no bucket. A bucket without a row
//! of the report is never read: the next is the one of the next row.
//!
//! A minute or an hour is the clock's in the offset the zone has then, cut
//! short where that offset changes, as it does by half an hour on
//! `Australia/Lord_Howe` or at a quarter past on `Pacific/Chatham`; a day
//! runs from one start of the zone's day to the next, 23 or 25 hours across
//! a change.

use std::collections::BTreeMap;
use std::sync::Arc;

use jiff::tz::{TimeZone, TimeZoneTransition};
use jush_shared_types::{Currency, Timestamp};
use jush_web_api_protocol::usage::{
    UsageBucket, UsageBucketSize, UsageCosts, UsageDimension, UsageReport, UsageReportGroup,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::StorageError;
use super::columns::decode;
use super::store::CoreStore;
use super::usage::{row_cost, row_prices, row_usage};

/// What a report asks of the ledger.
pub struct ReportAsk {
    pub from: Timestamp,
    pub to: Timestamp,
    pub bucket: UsageBucketSize,
    pub zone: TimeZone,
    pub group_by: Vec<UsageDimension>,
    /// The calls it keeps: every call, or those of the conversations, and
    /// of the agents there, it names.
    pub kept: Kept,
}

/// The calls a report keeps.
pub enum Kept {
    Every,
    /// The calls of `conversations`, of the agents there of the numbers
    /// `agents` names, or all of them for none. An empty list keeps no call.
    Of { conversations: Vec<String>, agents: Option<Vec<u64>> },
}

const COLUMNS: &str = "ended_at, conversation_id, agent, sender_id, model_id, provider_id, account_id,
       turn_id, assignment_id, purpose, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
       cost, cost_currency, api_price_input, api_price_cache_write, api_price_cache_read,
       api_price_output";

/// The first row's time from `?1` until `?2`, of every conversation.
const EVERY_FIRST: &str = "SELECT min(ended_at) FROM usage_ledger WHERE ended_at >= ?1 AND ended_at < ?2";

/// The first row's time from `?1` until `?2` of the conversations of the
/// JSON array `?3`, and of the agents of `?4` there when that is no null:
/// each conversation's first, which its index finds without reading the
/// others' rows, the earliest of them.
const OF_FIRST: &str = "SELECT min(first) FROM (
       SELECT (SELECT ended_at FROM usage_ledger
               WHERE conversation_id = conversation.value AND ended_at >= ?1 AND ended_at < ?2
                 AND (?4 IS NULL OR agent IN (SELECT value FROM json_each(?4)))
               ORDER BY ended_at LIMIT 1) AS first
       FROM json_each(?3) AS conversation)";

const MINUTE_MS: i64 = 60 * 1000;

const HOUR_MS: i64 = 60 * MINUTE_MS;

impl CoreStore {
    /// The report `ask` asks for.
    pub async fn usage_report(&self, ask: ReportAsk) -> Result<UsageReport, StorageError> {
        if matches!(&ask.kept, Kept::Of { conversations, agents } if conversations.is_empty() || agents.as_ref().is_some_and(Vec::is_empty)) {
            return Ok(UsageReport { buckets: Vec::new() });
        }
        let ask = Arc::new(ask);
        let mut buckets = Vec::new();
        let mut from = ask.from.as_millisecond();
        loop {
            let asked = ask.clone();
            let Some(bucket) = self.call(move |connection, _| next_bucket(connection, &asked, from)).await? else {
                return Ok(UsageReport { buckets });
            };
            from = bucket.end.as_millisecond();
            buckets.push(bucket);
        }
    }
}

/// The first bucket of `ask` that holds a row of it that ended at `from` or
/// after, none when no row of the period did.
fn next_bucket(connection: &Connection, ask: &ReportAsk, from: i64) -> Result<Option<UsageBucket>, StorageError> {
    let to = ask.to.as_millisecond();
    let (conversations, agents) = match &ask.kept {
        Kept::Every => (None, None),
        Kept::Of { conversations, agents } => (
            Some(serde_json::to_string(conversations).expect("ids serialize")),
            agents.as_ref().map(|agents| serde_json::to_string(agents).expect("numbers serialize")),
        ),
    };
    let first: Option<i64> = match &conversations {
        None => connection.prepare_cached(EVERY_FIRST)?.query_row(params![from, to], |row| row.get(0)),
        Some(_) => connection
            .prepare_cached(OF_FIRST)?
            .query_row(params![from, to, conversations, agents], |row| row.get(0)),
    }
    .optional()?
    .flatten();
    let Some(first) = first else {
        return Ok(None);
    };
    let instant = decode("usage_ledger", "ended_at", jiff::Timestamp::from_millisecond(first))?;
    let (start, end) = bucket_of(instant, ask.bucket, &ask.zone);
    let (start, end) = (start.max(ask.from.as_millisecond()), end.min(to));
    let mut groups: BTreeMap<Vec<Value>, Sums> = BTreeMap::new();
    let mut statement;
    let mut rows = match &conversations {
        None => {
            statement = connection
                .prepare_cached(&format!("SELECT {COLUMNS} FROM usage_ledger WHERE ended_at >= ?1 AND ended_at < ?2"))?;
            statement.query(params![first, end])?
        }
        Some(_) => {
            statement = connection.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM usage_ledger
                 WHERE conversation_id IN (SELECT value FROM json_each(?3)) AND ended_at >= ?1 AND ended_at < ?2
                   AND (?4 IS NULL OR agent IN (SELECT value FROM json_each(?4)))"
            ))?;
            statement.query(params![first, end, conversations, agents])?
        }
    };
    while let Some(row) = rows.next()? {
        let key = ask
            .group_by
            .iter()
            .map(|dimension| value(row, *dimension))
            .collect::<Result<Vec<Value>, _>>()?;
        groups.entry(key).or_default().add(row)?;
    }
    Ok(Some(UsageBucket {
        start: decode("usage_ledger", "ended_at", Timestamp::from_millisecond(start))?,
        end: decode("usage_ledger", "ended_at", Timestamp::from_millisecond(end))?,
        groups: groups.into_iter().map(|(key, sums)| sums.group(&ask.group_by, key)).collect(),
    }))
}

/// The minute, the hour or the day of `zone` the instant `instant` is in, as
/// its start and end in milliseconds.
fn bucket_of(instant: jiff::Timestamp, size: UsageBucketSize, zone: &TimeZone) -> (i64, i64) {
    let at = instant.as_millisecond();
    match size {
        UsageBucketSize::Minute | UsageBucketSize::Hour => {
            let span = if size == UsageBucketSize::Minute { MINUTE_MS } else { HOUR_MS };
            let offset = i64::from(zone.to_offset(instant).seconds()) * 1000;
            let start = at - (at + offset).rem_euclid(span);
            let end = start + span;
            let changes = |transition: &TimeZoneTransition<'_>| {
                let at = transition.timestamp().as_millisecond();
                let before = jiff::Timestamp::from_millisecond(at - 1).expect("a transition follows the earliest time");
                (at, zone.to_offset(before) != transition.offset())
            };
            let after = jiff::Timestamp::from_millisecond(at + 1).expect("a ledger time is a timestamp");
            let start = zone
                .preceding(after)
                .map(|transition| changes(&transition))
                .take_while(|(change, _)| *change > start)
                .find_map(|(change, changed)| changed.then_some(change))
                .unwrap_or(start);
            let end = zone
                .following(instant)
                .map(|transition| changes(&transition))
                .take_while(|(change, _)| *change < end)
                .find_map(|(change, changed)| changed.then_some(change))
                .unwrap_or(end);
            (start, end)
        }
        UsageBucketSize::Day => {
            let day = instant.to_zoned(zone.clone()).date();
            let start_of = |date: jiff::civil::Date| {
                date.to_zoned(zone.clone())
                    .map(|start| start.timestamp().as_millisecond())
                    .expect("a ledger time's day starts in its zone")
            };
            let next = day.tomorrow().expect("a ledger time's day has a next");
            (start_of(day), start_of(next))
        }
    }
}

/// A row's value of a dimension: an agent's number or another column's text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Value {
    Number(Option<u64>),
    Text(Option<String>),
}

/// The value of `dimension` in the ledger's row `row`.
fn value(row: &Row<'_>, dimension: UsageDimension) -> Result<Value, StorageError> {
    let text = |column: &str| row.get::<_, Option<String>>(column).map(Value::Text);
    Ok(match dimension {
        UsageDimension::Agent => Value::Number(
            row.get::<_, Option<i64>>("agent")?
                .map(|number| decode("usage_ledger", "agent", u64::try_from(number)))
                .transpose()?,
        ),
        UsageDimension::Conversation => text("conversation_id")?,
        UsageDimension::Sender => text("sender_id")?,
        UsageDimension::Model => text("model_id")?,
        UsageDimension::Provider => text("provider_id")?,
        UsageDimension::Account => text("account_id")?,
        UsageDimension::Turn => text("turn_id")?,
        UsageDimension::Assignment => text("assignment_id")?,
        UsageDimension::Purpose => text("purpose")?,
    })
}

/// A group's sums.
#[derive(Default)]
struct Sums {
    calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    cost: UsageCosts,
    api_price: f64,
    unpriced_calls: u64,
}

impl Sums {
    /// Adds the ledger's row `row`.
    fn add(&mut self, row: &Row<'_>) -> Result<(), StorageError> {
        let usage = row_usage(row)?;
        let (parts, _) = row_prices(row)?;
        self.calls += 1;
        if let Some(usage) = usage {
            self.input_tokens += usage.input_tokens;
            self.output_tokens += usage.output_tokens;
            self.cache_read_tokens += usage.cache_read_tokens;
            self.cache_write_tokens += usage.cache_write_tokens;
        }
        if let Some(cost) = row_cost(row)? {
            let sum = match cost.currency {
                Currency::Usd => &mut self.cost.usd,
                Currency::Cny => &mut self.cost.cny,
            };
            *sum.get_or_insert(0.0) += cost.amount;
        }
        match (parts, usage) {
            (Some(parts), _) => self.api_price += parts.total(),
            (None, Some(_)) => self.unpriced_calls += 1,
            // The vendor counted no tokens, so there is nothing to price.
            (None, None) => {}
        }
        Ok(())
    }

    /// The group of `key`, the values of `group_by` in its order.
    fn group(self, group_by: &[UsageDimension], key: Vec<Value>) -> UsageReportGroup {
        let mut group = UsageReportGroup {
            conversation: None,
            agent: None,
            sender: None,
            model: None,
            provider: None,
            account: None,
            turn: None,
            assignment: None,
            purpose: None,
            calls: self.calls,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
            cost: self.cost,
            api_price: self.api_price,
            unpriced_calls: self.unpriced_calls,
        };
        for (dimension, value) in group_by.iter().zip(key) {
            let text = match value {
                Value::Number(number) => {
                    group.agent = Some(number);
                    continue;
                }
                Value::Text(text) => text,
            };
            let field = match dimension {
                UsageDimension::Agent => unreachable!("an agent's value is a number"),
                UsageDimension::Conversation => &mut group.conversation,
                UsageDimension::Sender => &mut group.sender,
                UsageDimension::Model => &mut group.model,
                UsageDimension::Provider => &mut group.provider,
                UsageDimension::Account => &mut group.account,
                UsageDimension::Turn => &mut group.turn,
                UsageDimension::Assignment => &mut group.assignment,
                UsageDimension::Purpose => &mut group.purpose,
            };
            *field = Some(text);
        }
        group
    }
}
