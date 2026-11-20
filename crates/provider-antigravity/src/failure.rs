//! The failures `agy` reports.
//!
//! `agy` reports a model call that failed as one piece of the session's text,
//! `⚠ <STATUS> (code <n>): <message>`: Google's status, its HTTP code and its
//! words, such as `⚠ RESOURCE_EXHAUSTED (code 429): …`. Its turn then ends as
//! any other, so the run reads the piece as the turn's failure, not as the
//! model's text.

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
        ProviderFailure {
            message: format!("Antigravity: {} ({}): {}", self.status, self.code, self.message),
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
