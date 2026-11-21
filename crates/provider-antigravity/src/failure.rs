//! The failures `agy` reports.
//!
//! `agy` reports a model call that failed as one piece of the session's text,
//! `⚠ <STATUS> (code <n>): <message>`: Google's status, its HTTP code and its
//! words, such as `⚠ RESOURCE_EXHAUSTED (code 429): …`. Its turn then ends as
//! any other, so the run reads the piece as the turn's failure, not as the
//! model's text.
//!
//! Google answers `RESOURCE_EXHAUSTED` for a used-up account, and also,
//! whatever quota is left, for a request whose system prompt it refuses, as
//! it does for other agents' prompts, and for a short throttle. The run tells
//! a used-up account from the rest by a usage read made then (`live.rs`).

use jush_provider_common::{ErrorCode, ProviderFailure};
use jush_shared_types::{FailureSource, ProviderErrorDiagnostics};

/// The status of a used-up account: the plan's quota for the model, which
/// waiting a few seconds does not lift.
const RESOURCE_EXHAUSTED: &str = "RESOURCE_EXHAUSTED";

/// The status of a request Google does not serve where `agy` runs, such as
/// "User location is not supported".
const FAILED_PRECONDITION: &str = "FAILED_PRECONDITION";

/// The HTTP code of a call Google refused for its sign-in.
const UNAUTHORIZED: u16 = 401;

/// A failure `agy` reported.
pub(crate) struct Reported {
    status: String,
    code: u16,
    message: String,
    /// The piece of text as `agy` wrote it.
    text: String,
}

/// The failure `text`, a piece of the session's text, reports; none for the
/// model's own text.
pub(crate) fn reported(text: &str) -> Option<Reported> {
    let rest = text.trim_start().strip_prefix("⚠ ")?;
    let (status, rest) = rest.split_once(" (code ")?;
    let (code, message) = rest.split_once("): ")?;
    if status.is_empty() || !status.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'_') {
        return None;
    }
    Some(Reported {
        status: status.to_owned(),
        code: code.parse().ok()?,
        message: message.trim().to_owned(),
        text: text.to_owned(),
    })
}

impl Reported {
    /// Whether Google refused the account's sign-in, which signs it out.
    pub(crate) fn signs_out(&self) -> bool {
        self.code == UNAUTHORIZED
    }

    /// Whether Google answered that the account's quota is used up.
    pub(crate) fn exhausted(&self) -> bool {
        self.status == RESOURCE_EXHAUSTED
    }

    /// The run's failure when Google answered that the quota is used up while
    /// a usage read shows quota left for the model: no code, so nothing sends
    /// the request again, which a refused prompt would meet again, and no
    /// account is rested or rotated away from.
    pub(crate) fn refused_with_quota_left(&self) -> ProviderFailure {
        let message = format!(
            "Antigravity: Google answered {} ({}): {}, but the account's usage shows quota left for this model. Google gives that answer to a request whose system prompt it refuses, and to a short throttle.",
            self.status, self.code, self.message
        );
        self.with(None, message)
    }

    /// The run's failure:
    ///
    /// - a used-up account, `usage_limit`, which the entry's rotation answers
    ///   with another account;
    /// - a request refused where `agy` runs, `permission_denied`, which is
    ///   refused again wherever it is sent from that machine;
    /// - any other by its HTTP code.
    pub(crate) fn failure(&self) -> ProviderFailure {
        let code = match self.status.as_str() {
            RESOURCE_EXHAUSTED => Some(ErrorCode::UsageLimit),
            FAILED_PRECONDITION => Some(ErrorCode::PermissionDenied),
            _ => ErrorCode::from_http(self.code, &self.message),
        };
        self.with(code, format!("Antigravity: {} ({}): {}", self.status, self.code, self.message))
    }

    /// The failure of `code` and `message`, with what `agy` reported.
    fn with(&self, code: Option<ErrorCode>, message: String) -> ProviderFailure {
        ProviderFailure {
            message,
            code,
            diagnostics: Some(Box::new(ProviderErrorDiagnostics {
                source: FailureSource::Stream,
                client_request_id: None,
                provider_request_id: None,
                provider_response_id: None,
                provider_code: Some(self.status.clone()),
                http_status: Some(self.code),
                upstream: Some(self.text.clone()),
            })),
            retry_after: None,
        }
    }
}
