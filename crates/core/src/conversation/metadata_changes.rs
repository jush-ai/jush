//! What a conversation's lane sends of a change of its metadata to a page
//! that holds the metadata before it: the top-level fields that changed, and
//! a list's change as one splice, so a change of one field never sends the
//! whole metadata, whose list of ended commands is most of it.
//!
//! The lane keeps a digest of each field of the metadata the page holds,
//! and of each item of a list, never the values themselves: a lane per page
//! and conversation keeps a few kilobytes, not the metadata.

use std::collections::BTreeMap;

use jush_web_api_protocol::conversations::FieldChange;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// A field of the metadata as the lane keeps it.
#[derive(Debug, PartialEq, Eq)]
enum Field {
    /// The digest of a value that is no list.
    Whole(u64),
    /// The digest of each item of a list.
    List(Vec<u64>),
}

/// The digests of the metadata a page holds, by field.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Digests(BTreeMap<String, Field>);

impl Digests {
    /// The digests of `metadata`, the metadata's JSON object.
    pub(super) fn of(metadata: &Map<String, Value>) -> Self {
        Self(
            metadata
                .iter()
                .map(|(name, value)| {
                    let field = match value {
                        Value::Array(items) => Field::List(items.iter().map(digest).collect()),
                        value => Field::Whole(digest(value)),
                    };
                    (name.clone(), field)
                })
                .collect(),
        )
    }

    /// The changes that make the metadata these digests describe into
    /// `metadata`, whose digests are `now`; none when a field came or went,
    /// which the whole metadata tells.
    pub(super) fn changes(&self, now: &Digests, metadata: &Map<String, Value>) -> Option<BTreeMap<String, FieldChange>> {
        if self.0.len() != now.0.len() || self.0.keys().any(|name| !now.0.contains_key(name)) {
            return None;
        }
        let mut changes = BTreeMap::new();
        for (name, after) in &now.0 {
            let before = &self.0[name];
            if before == after {
                continue;
            }
            let value = &metadata[name];
            let change = match (before, after, value) {
                (Field::List(before), Field::List(after), Value::Array(items)) => splice(before, after, items),
                _ => None,
            };
            changes.insert(name.clone(), change.unwrap_or_else(|| FieldChange::Set { value: value.clone() }));
        }
        Some(changes)
    }
}

/// The splice that makes the list of item digests `before` into `after`,
/// whose items are `items`: what lies between the items both start and end
/// with. None when it would carry every item, which the list whole does as
/// well.
fn splice(before: &[u64], after: &[u64], items: &[Value]) -> Option<FieldChange> {
    let start = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let end = before[start..]
        .iter()
        .rev()
        .zip(after[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let insert = &items[start..items.len() - end];
    if insert.len() == items.len() {
        return None;
    }
    Some(FieldChange::Splice {
        at: u32::try_from(start).ok()?,
        remove: u32::try_from(before.len() - start - end).ok()?,
        insert: insert.to_vec(),
    })
}

/// The first eight bytes of the SHA-256 of `value`'s JSON: a field's value
/// and the next one share them by chance once in 2^64 changes.
fn digest(value: &Value) -> u64 {
    // A JSON value serializes, which serde_json never refuses.
    let json = serde_json::to_vec(value).expect("a JSON value serializes");
    let hash = Sha256::digest(json);
    u64::from_le_bytes(hash[..8].try_into().expect("a SHA-256 is longer than eight bytes"))
}
