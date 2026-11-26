//! A program's addresses: where its own server takes the calls from outside
//! that reach its cores, and the routes it hands each call on to.
//!
//! ```text
//! another system --> program's server --> POST /schedules/calls?id=
//!   calls a schedule's   (its route,        the call as message/http:
//!   URL                   its answer)       taken, or refused and why
//! a vendor sends the --> program's server --> POST /sign-ins
//!   browser back                            {core, plugin, query}:
//!                                           signed in, or refused and why
//! ```
//!
//! The program declares the addresses as URI templates (RFC 6570, its
//! first level: `{name}` stands for a value), which a core fills each time
//! it writes one: an agent's `jush schedule create --url` prints a
//! schedule's, and a plugin's sign-in sends its return's to the vendor.
//! The core writes them, not the program, which may be away from a process
//! on its own as an agent works.
//!
//! The program's route decides what it takes, its methods among them, and
//! its answer. A call that ends with no answer of the core's, as one whose
//! body stopped coming, fails as its request does.

use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use jush_core::cores::CoreUnavailable;
use jush_core::schedules::{HookAnswer, HookCall};
use jush_plugin_host::{Callback, CallbackError};
use jush_shared_types::ScheduleId;
use jush_web_api_protocol::cores::{
    CompleteSignIn, ProgramAddresses, ScheduleCallAnswer, ScheduleCallRefusal, SignInAnswer, SignInRefusal,
};
use jush_web_api_protocol::error::ErrorCode;
use jush_web_api_protocol::ids::UserId;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use url::Url;

use super::{Authorized, Process};
use crate::body::{JSON_BODY_LIMIT, JsonBody, read_within};
use crate::error::ApiError;
use crate::query::QueryParams;
use crate::schedules::{self, Admission};

/// What a template's value keeps as it is: RFC 3986's unreserved
/// characters.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// The most a call's head, its request line and headers, takes: room for
/// what the servers in front of a program take, 16 KiB in Node's and Bun's
/// and 32 KiB in nginx's. Its body takes what the server's `/hooks/{id}`
/// takes.
pub(super) const HEAD_LIMIT: usize = 64 * 1024;

/// The most a call takes, its head and its body.
pub(super) const CALL_LIMIT: usize = HEAD_LIMIT + JSON_BODY_LIMIT;

/// A URI template of RFC 6570's first level.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Template(Vec<Part>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    /// A `{name}`.
    Value(String),
}

impl Template {
    /// Reads `text`, whose `{name}`s are among `names`, and which holds
    /// each of `needed`; filled, it is an `http` or `https` URL.
    fn read(text: &str, names: &[&str], needed: &[&str]) -> Result<Self, String> {
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let (before, after) = rest.split_at(open);
            let close = after.find('}').ok_or("a { is not closed")?;
            let name = &after[1..close];
            if !names.contains(&name) {
                return Err(format!("{{{name}}} is no value it takes, which are {}", braced(names)));
            }
            if before.contains('}') {
                return Err("a } closes no {".to_owned());
            }
            if !before.is_empty() {
                parts.push(Part::Text(before.to_owned()));
            }
            parts.push(Part::Value(name.to_owned()));
            rest = &after[close + 1..];
        }
        if rest.contains('}') {
            return Err("a } closes no {".to_owned());
        }
        if !rest.is_empty() {
            parts.push(Part::Text(rest.to_owned()));
        }
        if let Some(missing) = needed.iter().find(|name| !parts.contains(&Part::Value((**name).to_owned()))) {
            return Err(format!("it holds no {{{missing}}}"));
        }
        let template = Self(parts);
        let example: Vec<(&str, &str)> = names.iter().map(|name| (*name, "x")).collect();
        match template.fill(&example) {
            // A fragment never leaves the browser or the caller, and an OAuth
            // redirect URI may hold none (RFC 6749 § 3.1.2).
            Some(url) if url.fragment().is_some() => Err("it holds a fragment, which no call carries".to_owned()),
            Some(url) if matches!(url.scheme(), "http" | "https") => Ok(template),
            _ => Err("filled, it is no http or https URL".to_owned()),
        }
    }

    /// The URL with `values` in place of their names, each percent-encoded.
    fn fill(&self, values: &[(&str, &str)]) -> Option<Url> {
        let mut text = String::new();
        for part in &self.0 {
            match part {
                Part::Text(literal) => text.push_str(literal),
                Part::Value(name) => {
                    let (_, value) = values.iter().find(|(each, _)| each == name)?;
                    text.extend(utf8_percent_encode(value, UNRESERVED));
                }
            }
        }
        Url::parse(&text).ok()
    }
}

fn braced(names: &[&str]) -> String {
    names.iter().map(|name| format!("{{{name}}}")).collect::<Vec<_>>().join(" and ")
}

/// The addresses the programs declared, the last declaration standing.
#[derive(Default)]
pub(super) struct Addresses(RwLock<Declared>);

#[derive(Default)]
struct Declared {
    schedule: Option<Template>,
    sign_in_return: Option<Template>,
}

impl Declared {
    fn read(addresses: &ProgramAddresses) -> Result<Self, String> {
        let read = |field: &str, text: &Option<String>, names: &[&str], needed: &[&str]| {
            text.as_deref()
                .map(|text| Template::read(text, names, needed))
                .transpose()
                .map_err(|refusal| format!("{field}: {refusal}"))
        };
        Ok(Self {
            schedule: read("schedule", &addresses.schedule, &["id"], &["id"])?,
            sign_in_return: read("signInReturn", &addresses.sign_in_return, &["core", "plugin"], &[])?,
        })
    }
}

impl Addresses {
    fn read(&self) -> RwLockReadGuard<'_, Declared> {
        // No section panics while it holds the lock.
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// The URL of the schedule `id`.
    pub(super) fn schedule_url(&self, id: &ScheduleId) -> Option<Url> {
        self.read().schedule.as_ref()?.fill(&[("id", id.as_str())])
    }

    /// Where the vendor of the plugin `plugin` sends the browser back to
    /// after a sign-in the core `core` started.
    pub(super) fn sign_in_return(&self, core: &UserId, plugin: &str) -> Option<Url> {
        self.read()
            .sign_in_return
            .as_ref()?
            .fill(&[("core", core.as_str()), ("plugin", plugin)])
    }
}

/// `PUT /addresses`: declares where the program serves the calls from
/// outside, in place of the addresses declared before, 204; a template
/// that breaks a rule is refused, 400.
pub(super) async fn declare(
    State(process): State<Arc<Process>>,
    _: Authorized,
    JsonBody(addresses): JsonBody<ProgramAddresses>,
) -> Result<StatusCode, ApiError> {
    let declared = Declared::read(&addresses).map_err(ApiError::invalid_body)?;
    *process.addresses.0.write().unwrap_or_else(PoisonError::into_inner) = declared;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /schedules/calls?id=`: a call of the URL of the schedule `id`,
/// which the program received at its own address and hands on whole, as an
/// HTTP message (`message/http`, RFC 9112 § 10.1) whose bytes are the
/// call's: a signature over its body verifies. The id is in the query,
/// where no `.` or `..` is a path's step.
pub(super) async fn schedule_call(
    State(process): State<Arc<Process>>,
    _: Authorized,
    QueryParams(CallQuery { id }): QueryParams<CallQuery>,
    request: Request,
) -> Result<Json<ScheduleCallAnswer>, ApiError> {
    let refused = |reason| Ok(Json(ScheduleCallAnswer::Refused { reason }));
    let api = &process.api;
    // The program's server limits the calls of unknown addresses.
    let (id, owner) = match schedules::admit(api, id, |_| true).await? {
        Admission::Known { id, owner } => (id, owner),
        Admission::Unknown => return refused(ScheduleCallRefusal::Unknown),
        Admission::TooMany => return refused(ScheduleCallRefusal::TooMany),
    };
    let message = match read_within(request, &(), CALL_LIMIT, too_large).await {
        Ok(message) => message,
        Err(error) if error.code() == ErrorCode::TooLarge => return refused(ScheduleCallRefusal::TooLarge),
        Err(error) => return Err(error),
    };
    let call = match call_of(message)? {
        Ok(call) => call,
        Err(reason) => return refused(reason),
    };
    let reason = match schedules::take(api, &owner, id, call).await {
        Ok(HookAnswer::Taken) => return Ok(Json(ScheduleCallAnswer::Taken)),
        Ok(HookAnswer::Unknown) => ScheduleCallRefusal::Unknown,
        Ok(HookAnswer::Secret) => ScheduleCallRefusal::Secret,
        // A core that closed as the call came is open no longer.
        Err(error) if error.code() == ErrorCode::BackendClosing => ScheduleCallRefusal::Unknown,
        Err(error) => return Err(error),
    };
    refused(reason)
}

#[derive(serde::Deserialize)]
pub(super) struct CallQuery {
    id: String,
}

fn too_large() -> ApiError {
    ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::TooLarge,
        format!("The call is over its {CALL_LIMIT}-byte limit"),
    )
}

/// The call `message` carries, or why it is refused: a head or a body
/// over its limit. A message that is no HTTP request is the program's
/// mistake, 400.
fn call_of(message: Bytes) -> Result<Result<HookCall, ScheduleCallRefusal>, ApiError> {
    let no_message = || ApiError::invalid_body("The call is no HTTP request");
    let searched = &message[..message.len().min(HEAD_LIMIT)];
    let Some(end) = searched.windows(4).position(|four| four == b"\r\n\r\n").map(|at| at + 4) else {
        return if message.len() >= HEAD_LIMIT {
            Ok(Err(ScheduleCallRefusal::TooLarge))
        } else {
            Err(no_message())
        };
    };
    if message.len() - end > JSON_BODY_LIMIT {
        return Ok(Err(ScheduleCallRefusal::TooLarge));
    }
    let head = &message[..end];
    let lines = head.windows(2).filter(|two| *two == b"\r\n").count();
    let mut headers = vec![httparse::EMPTY_HEADER; lines];
    let mut request = httparse::Request::new(&mut headers);
    if !matches!(request.parse(head), Ok(httparse::Status::Complete(_))) {
        return Err(no_message());
    }
    let method = request.method.unwrap_or_default().to_owned();
    // A value that is no visible ASCII is left out, as the server's
    // `/hooks/{id}` leaves it out.
    let headers = request
        .headers
        .iter()
        .filter_map(|header| {
            let value = HeaderValue::from_bytes(header.value).ok()?;
            Some((header.name.to_ascii_lowercase(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    Ok(Ok(HookCall { method, headers, body: message.slice(end..) }))
}

/// `POST /sign-ins`: the return of a plugin's sign-in, which the program
/// received at its own address and hands on: how the sign-in ended.
pub(super) async fn complete_sign_in(
    State(process): State<Arc<Process>>,
    _: Authorized,
    JsonBody(sign_in): JsonBody<CompleteSignIn>,
) -> Result<Json<SignInAnswer>, ApiError> {
    let refused = |reason| Ok(Json(SignInAnswer::Refused { reason }));
    // A sign-in lasts while its core is open.
    let core = match UserId::try_from(sign_in.core) {
        Ok(core) if process.cores.serving(&core).is_some() => core,
        _ => return refused(SignInRefusal::Expired),
    };
    // A query that does not read brings no sign-in's state.
    let callback: Callback = serde_urlencoded::from_str(sign_in.query.trim_start_matches('?')).unwrap_or_default();
    let plugin = sign_in.plugin;
    let completed = match process
        .api
        .shards
        .of(&core)
        .call(move |core, cancel| async move { core.plugins().oauth_callback(&plugin, callback, cancel).await })
        .await
    {
        Ok(completed) => completed,
        // A core that closed as the browser came back took its sign-in along.
        Err(CoreUnavailable::Closing) => return refused(SignInRefusal::Expired),
        Err(failed) => return Err(failed.into()),
    };
    let reason = match completed {
        Ok(vendor) => return Ok(Json(SignInAnswer::SignedIn { vendor })),
        Err(CallbackError::UnknownPlugin(_)) => SignInRefusal::UnknownPlugin,
        Err(CallbackError::UnknownState) => SignInRefusal::Expired,
        Err(CallbackError::Denied { .. }) => SignInRefusal::Declined,
        Err(CallbackError::Exchange { vendor, failure }) => {
            tracing::warn!(event = "plugin_sign_in.failed", vendor, error = %failure, "a plugin's sign-in did not complete");
            SignInRefusal::VendorFailed
        }
        Err(CallbackError::Storage(error)) => return Err(error.into()),
    };
    refused(reason)
}
