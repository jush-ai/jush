//! The usage ledger's report: its calls of a period in buckets of an hour or
//! a day of a time zone, each bucket's calls grouped by what the report
//! names, the sender among them, which each row keeps.
//!
//! The rows of the period are read by the ledger's time index and summed
//! here, a bucket at a time, each in a turn of the database's thread of its
//! own: a report of a long period keeps every other read and write of the
//! core waiting for one bucket's rows at most, never for the whole period's.
//! A call's row is written as the call ends, at that time, and the buckets
//! are read oldest first, so a row written while a report reads falls in the
//! bucket it reads last: the report is the ledger as that read found it.
//! A row's bucket is the hour or the day of the zone it ended in, and a
//! bucket starts no earlier and ends no later than the period, so two
//! reports of adjoining periods share no bucket.
//!
//! An hour is the clock's hour in the offset the zone has then, cut short
//! where that offset changes, as it does by half an hour on
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
}

/// When the first row from `?1` until `?2` ended.
const FIRST: &str = "SELECT min(ended_at) FROM usage_ledger WHERE ended_at >= ?1 AND ended_at < ?2";

const ROWS: &str = "SELECT ended_at, conversation_id, sender_id, model_id, provider_id, account_id,
       turn_id, purpose, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
       cost, cost_currency, api_price_input, api_price_cache_write, api_price_cache_read,
       api_price_output
     FROM usage_ledger
     WHERE ended_at >= ?1 AND ended_at < ?2";

const HOUR_MS: i64 = 60 * 60 * 1000;

impl CoreStore {
    /// The report `ask` asks for.
    pub async fn usage_report(&self, ask: ReportAsk) -> Result<UsageReport, StorageError> {
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

/// The first bucket of `ask` that holds a row that ended at `from` or after,
/// none when no row of the period did.
fn next_bucket(connection: &Connection, ask: &ReportAsk, from: i64) -> Result<Option<UsageBucket>, StorageError> {
    let to = ask.to.as_millisecond();
    let first: Option<i64> = connection
        .prepare_cached(FIRST)?
        .query_row(params![from, to], |row| row.get(0))
        .optional()?
        .flatten();
    let Some(first) = first else {
        return Ok(None);
    };
    let instant = decode("usage_ledger", "ended_at", jiff::Timestamp::from_millisecond(first))?;
    let (start, end) = bucket_of(instant, ask.bucket, &ask.zone);
    let (start, end) = (start.max(ask.from.as_millisecond()), end.min(to));
    let mut groups: BTreeMap<Vec<Option<String>>, Sums> = BTreeMap::new();
    let mut statement = connection.prepare_cached(ROWS)?;
    let mut rows = statement.query(params![first, end])?;
    while let Some(row) = rows.next()? {
        let key = ask
            .group_by
            .iter()
            .map(|dimension| row.get(column(*dimension)))
            .collect::<Result<Vec<Option<String>>, _>>()?;
        groups.entry(key).or_default().add(row)?;
    }
    Ok(Some(UsageBucket {
        start: decode("usage_ledger", "ended_at", Timestamp::from_millisecond(start))?,
        end: decode("usage_ledger", "ended_at", Timestamp::from_millisecond(end))?,
        groups: groups.into_iter().map(|(key, sums)| sums.group(&ask.group_by, key)).collect(),
    }))
}

/// The hour or the day of `zone` the instant `instant` is in, as its start
/// and end in milliseconds.
fn bucket_of(instant: jiff::Timestamp, size: UsageBucketSize, zone: &TimeZone) -> (i64, i64) {
    let at = instant.as_millisecond();
    match size {
        UsageBucketSize::Hour => {
            let offset = i64::from(zone.to_offset(instant).seconds()) * 1000;
            let start = at - (at + offset).rem_euclid(HOUR_MS);
            let end = start + HOUR_MS;
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

/// The ledger's column `dimension` groups by.
fn column(dimension: UsageDimension) -> &'static str {
    match dimension {
        UsageDimension::Conversation => "conversation_id",
        UsageDimension::Sender => "sender_id",
        UsageDimension::Model => "model_id",
        UsageDimension::Provider => "provider_id",
        UsageDimension::Account => "account_id",
        UsageDimension::Turn => "turn_id",
        UsageDimension::Purpose => "purpose",
    }
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
    fn group(self, group_by: &[UsageDimension], key: Vec<Option<String>>) -> UsageReportGroup {
        let mut group = UsageReportGroup {
            conversation: None,
            sender: None,
            model: None,
            provider: None,
            account: None,
            turn: None,
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
            let field = match dimension {
                UsageDimension::Conversation => &mut group.conversation,
                UsageDimension::Sender => &mut group.sender,
                UsageDimension::Model => &mut group.model,
                UsageDimension::Provider => &mut group.provider,
                UsageDimension::Account => &mut group.account,
                UsageDimension::Turn => &mut group.turn,
                UsageDimension::Purpose => &mut group.purpose,
            };
            *field = Some(value);
        }
        group
    }
}
