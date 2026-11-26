//! A program's labels: its own key-value pairs, which a message carries with
//! its work and which Jush counts by without reading them. Every call and
//! command of the work keeps them, as a usage ledger's row and a command's
//! row do, so a program sums its own dimensions, such as who asked or what
//! kind of work it was.

use std::collections::BTreeMap;

/// A program's labels, each value by its key; empty for none.
pub type Labels = BTreeMap<String, String>;

/// The most labels a message carries: each row of usage keeps them all.
pub const LABELS_LIMIT: usize = 16;

/// The most characters of a label's key, which a report's `label:<key>`
/// names.
pub const LABEL_KEY_LIMIT: usize = 64;

/// The most characters of a label's value: each row of usage keeps it.
pub const LABEL_VALUE_LIMIT: usize = 512;

/// No labels, which what names none carries.
pub static NO_LABELS: Labels = BTreeMap::new();

/// Checks that `key` names a label: 1 to [`LABEL_KEY_LIMIT`] letters,
/// digits, `.`, `_` and `-`, the first a letter or a digit. A key such as
/// `__proto__` would name no label to a JavaScript program, whose objects
/// read it as their prototype.
pub fn check_label_key(key: &str) -> Result<(), String> {
    let named = (1..=LABEL_KEY_LIMIT).contains(&key.len())
        && key.as_bytes()[0].is_ascii_alphanumeric()
        && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if named {
        Ok(())
    } else {
        Err(format!(
            "{key:?} is no label's key: 1 to {LABEL_KEY_LIMIT} letters, digits, `.`, `_` and `-`, the first a letter or a digit"
        ))
    }
}

/// Checks `labels` against the limits above.
pub fn check_labels(labels: &Labels, _: &()) -> garde::Result {
    if labels.len() > LABELS_LIMIT {
        return Err(garde::Error::new(format!("at most {LABELS_LIMIT} labels")));
    }
    for (key, value) in labels {
        check_label_key(key).map_err(garde::Error::new)?;
        if value.is_empty() || value.chars().count() > LABEL_VALUE_LIMIT {
            return Err(garde::Error::new(format!("{key:?}: a value is 1 to {LABEL_VALUE_LIMIT} characters")));
        }
    }
    Ok(())
}
