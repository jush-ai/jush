//! The failures `agy` reports.
//!
//! A model call that fails ends `agy`'s turn as any other. `agy` records the
//! failure as the session's last step, an error step, and sends one piece of
//! the session's text for people, which starts with `⚠`: Google's own words
//! for a used-up account, `⚠ Individual quota reached. … Resets in 1h5m15s.`,
//! or the error's, `⚠ agent executor error: … Error 400, …`. The run holds
//! such a piece and reads the session's last step when the turn ends
//! (`live.rs`): a turn that ends with no error step gives the piece as the
//! model's text.
//!
//! The error step (`agy` 1.5.14, measured) holds, in its payload's field 24.3:
//!
//! - 7, the HTTP code;
//! - 9, the error's text, which names Google's status as
//!   `RESOURCE_EXHAUSTED (code 429)` or as `Status: INVALID_ARGUMENT`;
//! - 10, each of Google's error details as JSON: `google.rpc.ErrorInfo`, whose
//!   `reason` `QUOTA_EXHAUSTED` marks a used-up account and whose
//!   `metadata.quotaResetTimeStamp` names its reset, and
//!   `google.rpc.RetryInfo`, whose `retryDelay` names the wait.
//!
//! Google answers `RESOURCE_EXHAUSTED` for a used-up account, and also,
//! whatever quota is left, for a request whose system prompt it refuses, as
//! it does for other agents' prompts, and for a short throttle. The run tells
//! a used-up account from the rest by a usage read made then (`live.rs`).

use jush_provider_common::{ErrorCode, ProviderFailure, read_http_failure};
use jush_shared_types::{FailureSource, ProviderErrorDiagnostics, ProviderFailureFacts, Timestamp};
use serde_json::Value;

use crate::history::{self, Field};

/// The status of a used-up account, and its HTTP code: the plan's quota for
/// the model, which waiting a few seconds does not lift.
const RESOURCE_EXHAUSTED: &str = "RESOURCE_EXHAUSTED";
const TOO_MANY_REQUESTS: u16 = 429;

/// The `ErrorInfo` reason of a used-up account.
const QUOTA_EXHAUSTED: &str = "QUOTA_EXHAUSTED";

/// `agy`'s own sign of a used-up account in an error's words, lowercased, as
/// its `IsHardQuotaExhaustedMessage` reads them, for a failure whose step
/// cannot be read.
const QUOTA_REACHED: &str = "individual quota reached";

/// The status of a request Google does not serve where `agy` runs, such as
/// "User location is not supported".
const FAILED_PRECONDITION: &str = "FAILED_PRECONDITION";

/// The HTTP code of a call Google refused for its sign-in.
const UNAUTHORIZED: u16 = 401;

/// Whether `text`, a piece of the session's text, is how `agy` words a
/// failure for people, which the run holds until the turn's last step says.
pub(crate) fn warns(text: &str) -> bool {
    text.trim_start().starts_with('⚠')
}

/// A failure `agy` reported.
pub(crate) struct Reported {
    /// Google's status, when the error's text names it.
    status: Option<String>,
    code: u16,
    /// The `ErrorInfo` reason, when Google gave one.
    reason: Option<String>,
    /// Google's error details, each a JSON object.
    details: Vec<Value>,
    /// The error's own text, which names Google's status and words.
    error: String,
    /// The words `agy` showed, without its sign.
    words: String,
}

/// The failure the session's last step reports, `kind` and `payload`, with
/// `warning` the piece of text `agy` showed for it; none when the step is no
/// error. An error step whose fields are not where they were measured still
/// fails the turn, by `warning` alone.
pub(crate) fn reported(kind: u64, payload: &[u8], warning: &str) -> Option<Reported> {
    if kind != history::ERROR {
        return None;
    }
    let Some(error) = bytes(payload, 24).and_then(|error| bytes(error, 3)) else {
        return Some(Reported::unread(warning));
    };
    let code = history::fields(error, 7).into_iter().find_map(|field| match field {
        Field::Varint(code) => u16::try_from(code).ok(),
        Field::Bytes(_) => None,
    });
    let text = bytes(error, 9).map(String::from_utf8_lossy).unwrap_or_default();
    let details: Vec<Value> = history::fields(error, 10)
        .into_iter()
        .filter_map(|field| match field {
            Field::Bytes(bytes) => serde_json::from_slice(bytes).ok(),
            Field::Varint(_) => None,
        })
        .collect();
    let reason = details
        .iter()
        .find_map(|detail| detail.get("reason").and_then(Value::as_str))
        .map(str::to_owned);
    let words = words(warning);
    Some(Reported {
        status: status(&text),
        code: code.unwrap_or_default(),
        reason,
        details,
        error: text.trim().to_owned(),
        words: if words.is_empty() { text.trim().to_owned() } else { words.to_owned() },
    })
}

/// The words of `warning`, without `agy`'s sign.
fn words(warning: &str) -> &str {
    warning.trim().trim_start_matches('⚠').trim()
}

/// The first length-delimited field `number` of `message`.
fn bytes(message: &[u8], number: u64) -> Option<&[u8]> {
    history::fields(message, number).into_iter().find_map(|field| match field {
        Field::Bytes(bytes) => Some(bytes),
        Field::Varint(_) => None,
    })
}

/// Google's status as the error's text names it: `<STATUS> (code <n>)`, as
/// for a call with the account's sign-in, or `Status: <STATUS>`, as for a
/// call with an API key.
fn status(text: &str) -> Option<String> {
    let is_status = |word: &str| word.len() > 2 && word.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'_');
    let named = text.match_indices(" (code ").find_map(|(at, _)| {
        let word = text[..at].rsplit(|c: char| !(c.is_ascii_uppercase() || c == '_')).next()?;
        is_status(word).then(|| word.to_owned())
    });
    named.or_else(|| {
        text.match_indices("Status: ").find_map(|(at, marker)| {
            let word: String = text[at + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                .collect();
            is_status(&word).then_some(word)
        })
    })
}

impl Reported {
    /// The failure `warning` words when the session's last step cannot be
    /// read: `agy` shows such a piece for a failure.
    pub(crate) fn unread(warning: &str) -> Self {
        Self {
            status: None,
            code: 0,
            reason: None,
            details: Vec::new(),
            error: String::new(),
            words: words(warning).to_owned(),
        }
    }

    /// Whether Google refused the account's sign-in, which signs it out.
    pub(crate) fn signs_out(&self) -> bool {
        self.code == UNAUTHORIZED
    }

    /// Whether Google answered that the account's quota is used up.
    pub(crate) fn exhausted(&self) -> bool {
        self.status.as_deref() == Some(RESOURCE_EXHAUSTED)
            || self.reason.as_deref() == Some(QUOTA_EXHAUSTED)
            || self.code == TOO_MANY_REQUESTS
            || self.words.to_lowercase().contains(QUOTA_REACHED)
    }

    /// The run's failure when Google answered that the quota is used up while
    /// a usage read shows quota left for the model: no code and no time to
    /// retry at, so nothing sends the request again, which a refused prompt
    /// would meet again, and no account is rested or rotated away from.
    pub(crate) fn refused_with_quota_left(&self) -> ProviderFailure {
        let message = format!(
            "Antigravity: {}, but the account's usage shows quota left for this model. Google gives that answer to a request whose system prompt it refuses, and to a short throttle.",
            self.words.trim_end_matches('.')
        );
        self.with(None, message, Value::String(self.error.clone()))
    }

    /// The run's failure:
    ///
    /// - a used-up account, `usage_limit`, which the entry's rotation answers
    ///   with another account and which resumes at its reset
    ///   ([`read_failure`]);
    /// - a request refused where `agy` runs, `permission_denied`, which is
    ///   refused again wherever it is sent from that machine;
    /// - any other by its HTTP code.
    pub(crate) fn failure(&self) -> ProviderFailure {
        let code = if self.exhausted() {
            Some(ErrorCode::UsageLimit)
        } else if self.status.as_deref() == Some(FAILED_PRECONDITION) {
            Some(ErrorCode::PermissionDenied)
        } else {
            // Google's own words tell a request too long, which `agy`'s may
            // shorten.
            ErrorCode::from_http(self.code, &format!("{}\n{}", self.error, self.words))
        };
        self.with(code, format!("Antigravity: {}", self.words), Value::Array(self.details.clone()))
    }

    /// The failure of `code` and `message`, with `upstream` as the record
    /// [`read_failure`] reads: Google's details, or the error's text for a
    /// failure that names no time to retry at.
    fn with(&self, code: Option<ErrorCode>, message: String, upstream: Value) -> ProviderFailure {
        ProviderFailure {
            message,
            code,
            diagnostics: Some(Box::new(ProviderErrorDiagnostics {
                source: FailureSource::Stream,
                client_request_id: None,
                provider_request_id: None,
                provider_response_id: None,
                provider_code: self.status.clone().or_else(|| self.reason.clone()),
                http_status: (self.code != 0).then_some(self.code),
                upstream: Some(match upstream {
                    Value::String(text) => text,
                    details => details.to_string(),
                }),
            })),
            retry_after: None,
        }
    }
}

/// The facts of a failure this provider produced, received at `received_at`:
/// a used-up account's reset, `quotaResetTimeStamp`, else the wait Google
/// asked for, `retryDelay`, counted from `received_at`. Any other failure is
/// read by the HTTP rules.
pub(crate) fn read_failure(diagnostics: &ProviderErrorDiagnostics, received_at: Timestamp) -> ProviderFailureFacts {
    let mut facts = read_http_failure(diagnostics, received_at);
    let details = (diagnostics.source == FailureSource::Stream)
        .then(|| diagnostics.upstream.as_deref())
        .flatten()
        .and_then(|upstream| serde_json::from_str::<Vec<Value>>(upstream).ok())
        .unwrap_or_default();
    let reset = details.iter().find_map(|detail| {
        detail
            .get("metadata")?
            .get("quotaResetTimeStamp")?
            .as_str()?
            .parse::<Timestamp>()
            .ok()
    });
    let wait = || {
        details.iter().find_map(|detail| {
            let seconds: f64 = detail.get("retryDelay")?.as_str()?.strip_suffix('s')?.parse().ok()?;
            if !(seconds.is_finite() && seconds >= 0.0) {
                return None;
            }
            let moment = received_at.as_millisecond().checked_add((seconds * 1000.0) as i64)?;
            Timestamp::from_millisecond(moment).ok()
        })
    };
    if let Some(moment) = reset.or_else(wait) {
        facts.retry_at = Some(moment);
    }
    facts
}
