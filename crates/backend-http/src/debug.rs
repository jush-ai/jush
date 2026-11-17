//! Debug This Page (`crates/web-api-protocol/src/debug.rs`): the links, the
//! lane a page holds its link on, and the routes an agent runs code through.
//!
//! ```text
//!  agent --- GET | POST /api/debug/:token ---> DebugLinks --> the holder
//!    ^                                                          |
//!    |                                     run, describe        v
//!    `------------- the answer <------- ran, described ----- the page
//! ```
//!
//! - A link is made by a page's lane and lives in memory only: a restart
//!   loses it, and a page that resumes it then is refused 404, which tells
//!   it the link is over. Its token is the only key, as a share's is.
//! - One lane holds a link at a time. A lane that resumes it takes it from
//!   the one that held it, which closes `debug_moved`: the backend cannot
//!   tell a reload whose old connection it has not seen end from a second
//!   tab that copied the first's storage, and the newer page is the one the
//!   user looks at.
//! - While no lane holds a link, as while its page reloads, a run waits for
//!   one, up to [`PAGE_WAIT`]. A closed tab cannot be told from a lost
//!   connection, so its link stays until it expires, answering
//!   `page_not_connected`.
//! - A link ends as its page sends `stop`, closing `debug_stopped`, or at its
//!   expiry, closing `debug_expired`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use base64::Engine as _;
use jush_backend_user_shard::shard::lane::{Lane, LaneClose, LaneEnd, LaneMessage, LaneReader};
use jush_shared_types::Timestamp;
use jush_web_api_protocol::MAX_PAGE_MESSAGE_BYTES;
use jush_web_api_protocol::debug::{
    DEBUG_VALUE_DEPTH, DebugFromPage, DebugLinkAnswer, DebugRunAnswer, DebugToPage, PageFacts, RunOutcome, ThrownError,
};
use jush_web_api_protocol::error::ErrorCode;
use jush_web_api_protocol::ids::UserId;
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use super::AppState;
use super::connection::Opened;
use super::error::ApiError;
use super::query::QueryParams;

/// How long a link lasts after its page made it.
const LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a request waits for a page to hold its link: a reload, or a
/// connection lost and made again, takes a few seconds.
const PAGE_WAIT: Duration = Duration::from_secs(10);

/// How long a run may take when the request names no time.
const DEFAULT_TIMEOUT_SECONDS: u64 = 10;

/// The most time a request may give a run: the request is held open while it
/// runs, and a proxy in front of the backend, such as Cloudflare's at 100
/// seconds, cuts a longer one.
const MAX_TIMEOUT_SECONDS: u64 = 60;

/// How much of a run's code its log line keeps: enough to tell what it did,
/// while a script that inlines data stays one line.
const LOGGED_CODE_BYTES: usize = 4 * 1024;

/// How many of a token's characters the log names, to tell links apart
/// without making the line a key.
const LOGGED_TOKEN_CHARS: usize = 6;

/// What an agent given only the link reads to use it.
fn usage() -> String {
    let wait = PAGE_WAIT.as_secs();
    format!(
        "POST JavaScript to this link + /eval, such as with curl --data-binary @probe.js. \
         It runs in the page as the body of an async function, so await and return work, \
         and the answer is {{\"value\": what it returned, as JSON, \"ms\"}} \
         or {{\"error\": {{\"name\", \"message\", \"stack\"}}, \"ms\"}}. \
         An element is written as a short selector, a cycle as \"[Circular]\", a function as \"[Function name]\", \
         a list such as a Map, a Set or a NodeList as an array, undefined as null, \
         and what lies deeper than {DEBUG_VALUE_DEPTH} levels as \"[Deep]\". \
         ?timeout=<seconds> gives a run up to {MAX_TIMEOUT_SECONDS} seconds, {DEFAULT_TIMEOUT_SECONDS} unless asked. \
         While the page reloads, a run waits up to {wait} seconds for it."
    )
}

/// The backend's debug links, by token.
#[derive(Default)]
pub struct DebugLinks {
    links: Mutex<HashMap<String, Link>>,
}

struct Link {
    user: UserId,
    expires_at: Timestamp,
    /// The lane that holds the link now, if one does.
    holder: watch::Sender<Option<Holder>>,
    /// The number of the last lane that took it.
    taken: u64,
}

/// The lane that holds a link: what it is asked through, and what tells it
/// another lane took the link.
#[derive(Clone)]
struct Holder {
    /// Unbounded, as each ask is an agent's request held open for its answer.
    asks: mpsc::UnboundedSender<Ask>,
    /// The page load's ID, which the log lines name.
    page: Option<String>,
    released: CancellationToken,
}

enum Ask {
    Run { source: String, answer: oneshot::Sender<RunOutcome> },
    Describe { answer: oneshot::Sender<PageFacts> },
}

/// What a request found of a link.
struct Found {
    expires_at: Timestamp,
    holder: watch::Receiver<Option<Holder>>,
}

impl DebugLinks {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Link>> {
        self.links.lock().expect("the debug links are intact")
    }

    /// Forgets the links that expired by `now`, which no request may use.
    fn expire(links: &mut HashMap<String, Link>, now: Timestamp) {
        links.retain(|_, link| link.expires_at > now);
    }

    /// Makes a link of `user`'s, which lasts until the time it answers.
    fn make(&self, user: &UserId, now: Timestamp) -> (String, Timestamp) {
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let expires_at = Timestamp::truncate(now.to_jiff() + LIFETIME);
        let mut links = self.lock();
        Self::expire(&mut links, now);
        links.insert(
            token.clone(),
            Link {
                user: user.clone(),
                expires_at,
                holder: watch::Sender::new(None),
                taken: 0,
            },
        );
        (token, expires_at)
    }

    /// The expiry of `user`'s link `token`, while it lasts.
    fn resumable(&self, token: &str, user: &UserId, now: Timestamp) -> Option<Timestamp> {
        let mut links = self.lock();
        Self::expire(&mut links, now);
        links
            .get(token)
            .filter(|link| link.user == *user)
            .map(|link| link.expires_at)
    }

    /// Gives the link `token` to `holder`, releasing the one that held it,
    /// and answers the number of this take; none once the link ended.
    fn take(&self, token: &str, holder: Holder) -> Option<u64> {
        let mut links = self.lock();
        let link = links.get_mut(token)?;
        link.taken += 1;
        let before = link.holder.send_replace(Some(holder));
        if let Some(before) = before {
            before.released.cancel();
        }
        Some(link.taken)
    }

    /// Lets go of the link `token`, unless a later lane took it since `take`.
    fn release(&self, token: &str, take: u64) {
        if let Some(link) = self.lock().get(token).filter(|link| link.taken == take) {
            link.holder.send_replace(None);
        }
    }

    /// Ends the link `token`: a request waiting for a page hears it is over.
    fn end(&self, token: &str) {
        self.lock().remove(token);
    }

    fn find(&self, token: &str, now: Timestamp) -> Result<Found, ApiError> {
        let mut links = self.lock();
        Self::expire(&mut links, now);
        let link = links.get(token).ok_or_else(not_found)?;
        Ok(Found {
            expires_at: link.expires_at,
            holder: link.holder.subscribe(),
        })
    }
}

fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        ErrorCode::DebugLinkNotFound,
        "No debug link answers this token: it expired, was stopped, or the backend restarted",
    )
}

fn not_connected() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        ErrorCode::PageNotConnected,
        "No page holds this link: its tab closed, or it did not come back from a reload in time",
    )
}

/// The first `chars` characters of `text`.
fn prefix(text: &str, chars: usize) -> &str {
    text.char_indices().nth(chars).map_or(text, |(at, _)| &text[..at])
}

/// At most the first `bytes` bytes of `text`, cut at a character's start.
fn head(text: &str, bytes: usize) -> &str {
    if text.len() <= bytes {
        return text;
    }
    let mut at = bytes;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    &text[..at]
}

/// The debug lane of a page of `user`'s: one with a new link, or the one
/// that resumes `resume`, which is refused 404 once the link is over or
/// another user's.
pub(super) fn open(
    state: &AppState,
    user: &UserId,
    page: Option<String>,
    resume: Option<String>,
) -> Result<Opened, ApiError> {
    let now = state.services.clock.now();
    let (token, expires_at, resumed) = match resume {
        Some(token) => {
            let expires_at = state.debug.resumable(&token, user, now).ok_or_else(not_found)?;
            (token, expires_at, true)
        }
        None => {
            let (token, expires_at) = state.debug.make(user, now);
            (token, expires_at, false)
        }
    };
    let links = state.debug.clone();
    // A link already past its expiry was refused above, so this is its rest.
    let lasts = Duration::try_from(expires_at.to_jiff().duration_since(now.to_jiff())).unwrap_or(Duration::ZERO);
    tracing::info!(
        event = "debug.started",
        token = prefix(&token, LOGGED_TOKEN_CHARS),
        resumed,
        expires_at = %expires_at,
        "a page holds a debug link"
    );
    Ok(Opened::new(move |lane, reader| async move {
        let held = Held {
            links,
            token,
            expires_at,
            lasts,
            page,
        };
        held.serve(lane, reader).await;
    }))
}

/// A page's lane that holds the link `token`.
struct Held {
    links: std::sync::Arc<DebugLinks>,
    token: String,
    expires_at: Timestamp,
    /// How long the link has left as the lane opened.
    lasts: Duration,
    page: Option<String>,
}

/// How a debug lane ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// The page stopped debugging: the link is over.
    Stopped,
    /// The link expired.
    Expired,
    /// Another lane took the link.
    Moved,
    /// The page sent a message it should not have.
    Refused,
    /// The page closed the lane, or its connection ended.
    PageClosed,
}

impl End {
    fn lane_end(self) -> LaneEnd {
        let close = |code, reason| LaneEnd::Close(LaneClose { code, reason });
        match self {
            Self::Stopped => close(1000, "debug_stopped"),
            Self::Expired => close(4000, "debug_expired"),
            Self::Moved => close(4000, "debug_moved"),
            Self::Refused => close(1003, "invalid_message"),
            Self::PageClosed => LaneEnd::Gone,
        }
    }
}

/// What the lane waits to hear from the page, by the id it asked with.
enum Pending {
    Run(oneshot::Sender<RunOutcome>),
    Describe(oneshot::Sender<PageFacts>),
}

impl Pending {
    /// The request that asked stopped waiting, as one that timed out.
    fn abandoned(&self) -> bool {
        match self {
            Self::Run(answer) => answer.is_closed(),
            Self::Describe(answer) => answer.is_closed(),
        }
    }
}

impl Held {
    async fn serve(self, mut page: Lane, mut from_page: LaneReader) {
        let (asks_to, mut asks) = mpsc::unbounded_channel();
        let released = CancellationToken::new();
        let holder = Holder {
            asks: asks_to,
            page: self.page.clone(),
            released: released.clone(),
        };
        let Some(take) = self.links.take(&self.token, holder) else {
            // Stopped or expired while the lane opened.
            page.end(from_page, End::Expired.lane_end());
            return;
        };
        let started = DebugToPage::Started {
            token: self.token.clone(),
            expires_at: self.expires_at,
        };
        let expiry = tokio::time::sleep(self.lasts);
        tokio::pin!(expiry);
        let mut pending: HashMap<u32, Pending> = HashMap::new();
        let mut last_id: u32 = 0;
        let end = 'serve: {
            if page.send(text(&started)).await.is_err() {
                break 'serve End::PageClosed;
            }
            loop {
                tokio::select! {
                    biased;
                    () = released.cancelled() => break 'serve End::Moved,
                    () = &mut expiry => break 'serve End::Expired,
                    Some(ask) = asks.recv() => {
                        pending.retain(|_, waiting| !waiting.abandoned());
                        last_id = last_id.wrapping_add(1);
                        let id = last_id;
                        let message = match ask {
                            Ask::Run { source, answer } => {
                                pending.insert(id, Pending::Run(answer));
                                DebugToPage::Run { id, source }
                            }
                            Ask::Describe { answer } => {
                                pending.insert(id, Pending::Describe(answer));
                                DebugToPage::Describe { id }
                            }
                        };
                        if page.send(text(&message)).await.is_err() {
                            break 'serve End::PageClosed;
                        }
                    }
                    message = from_page.next() => {
                        let Some(LaneMessage::Text(message)) = message else {
                            break 'serve if message.is_none() { End::PageClosed } else { End::Refused };
                        };
                        match serde_json::from_str::<DebugFromPage>(&message) {
                            Ok(DebugFromPage::Ran { id, outcome }) => {
                                if let Some(Pending::Run(answer)) = pending.remove(&id) {
                                    // A request that stopped waiting takes no answer.
                                    let _ = answer.send(outcome);
                                }
                            }
                            Ok(DebugFromPage::Described { id, page }) => {
                                if let Some(Pending::Describe(answer)) = pending.remove(&id) {
                                    let _ = answer.send(page);
                                }
                            }
                            Ok(DebugFromPage::Stop) => break 'serve End::Stopped,
                            Err(_) => break 'serve End::Refused,
                        }
                    }
                }
            }
        };
        match end {
            End::Stopped | End::Expired => {
                self.links.end(&self.token);
                tracing::info!(
                    event = "debug.ended",
                    token = prefix(&self.token, LOGGED_TOKEN_CHARS),
                    reason = if end == End::Stopped { "stopped" } else { "expired" },
                    "a debug link ended"
                );
            }
            End::Refused | End::PageClosed => self.links.release(&self.token, take),
            End::Moved => {}
        }
        page.end(from_page, end.lane_end());
    }
}

fn text(message: &DebugToPage) -> String {
    serde_json::to_string(message).expect("a debug message serializes")
}

/// The lane that holds the link `found`, waiting up to [`PAGE_WAIT`] for one.
async fn holder(found: &mut Found) -> Result<Holder, ApiError> {
    let waited = tokio::time::timeout(PAGE_WAIT, found.holder.wait_for(Option::is_some)).await;
    match waited {
        Ok(Ok(holder)) => Ok(holder.clone().expect("the wait was for a holder")),
        // The link ended while the request waited.
        Ok(Err(_)) => Err(not_found()),
        Err(_) => Err(not_connected()),
    }
}

/// `GET /api/debug/:token`: the page that holds the link now, and how to
/// use it. A page that does not answer within [`PAGE_WAIT`] is told as none.
pub(super) async fn describe(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<DebugLinkAnswer>, ApiError> {
    let found = state.debug.find(&token, state.services.clock.now())?;
    let holder = found.holder.borrow().clone();
    let page = match holder {
        Some(holder) => {
            let (answer, answered) = oneshot::channel();
            let asked = holder.asks.send(Ask::Describe { answer }).is_ok();
            match tokio::time::timeout(PAGE_WAIT, answered).await {
                Ok(Ok(page)) if asked => Some(page),
                _ => None,
            }
        }
        None => None,
    };
    Ok(Json(DebugLinkAnswer {
        page,
        expires_at: found.expires_at,
        usage: usage(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EvalQuery {
    /// How many seconds the run may take.
    timeout: Option<u64>,
}

/// `POST /api/debug/:token/eval?timeout=<seconds>`: runs the body, as
/// JavaScript, in the page that holds the link.
pub(super) async fn eval(
    State(state): State<AppState>,
    Path(token): Path<String>,
    QueryParams(query): QueryParams<EvalQuery>,
    request: Request,
) -> Result<Json<DebugRunAnswer>, ApiError> {
    let seconds = query.timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    if !(1..=MAX_TIMEOUT_SECONDS).contains(&seconds) {
        return Err(ApiError::invalid_query(format!(
            "timeout is a number of seconds from 1 to {MAX_TIMEOUT_SECONDS}"
        )));
    }
    let mut found = state.debug.find(&token, state.services.clock.now())?;
    let body = super::body::read(request, &state).await?;
    let source = String::from_utf8(body.to_vec())
        .map_err(|_| ApiError::invalid_body("The code is not UTF-8 text"))?;
    // The run's message carries the code as a JSON string, within what a page
    // takes as one message.
    let message_bytes = serde_json::to_string(&source).map_or(usize::MAX, |quoted| quoted.len()) + 64;
    if message_bytes > MAX_PAGE_MESSAGE_BYTES {
        return Err(too_large("The code is more than a page takes in one message"));
    }
    let holder = holder(&mut found).await?;
    // The run goes on, and its line is written, also once the agent stops
    // waiting: the code runs in the page either way.
    let run = tokio::spawn(run(holder, token, source, seconds).in_current_span());
    run.await
        .map_err(|_| ApiError::internal_message("A debug run's task ended without its answer"))?
        .map(Json)
}

/// Runs `source` in the page `holder` names, for up to `seconds`, and writes
/// its line.
async fn run(holder: Holder, token: String, source: String, seconds: u64) -> Result<DebugRunAnswer, ApiError> {
    let began = Instant::now();
    let (answer, answered) = oneshot::channel();
    let code = head(&source, LOGGED_CODE_BYTES).to_owned();
    let ran = if holder.asks.send(Ask::Run { source, answer }).is_err() {
        Err((not_connected(), "page_not_connected"))
    } else {
        match tokio::time::timeout(Duration::from_secs(seconds), answered).await {
            Ok(Ok(outcome)) => Ok(outcome),
            // The lane ended before the page answered, as a reload the code
            // made ends it.
            Ok(Err(_)) => Err((
                ApiError::new(
                    StatusCode::CONFLICT,
                    ErrorCode::PageNotConnected,
                    "The page left before the code settled, as a reload or a lost connection makes it",
                ),
                "page_left",
            )),
            Err(_) => Err((
                ApiError::new(
                    StatusCode::REQUEST_TIMEOUT,
                    ErrorCode::RunTimedOut,
                    format!("The code did not settle within {seconds} seconds; the page's own work goes on"),
                )
                .with_param("timeoutSeconds", seconds),
                "timed_out",
            )),
        }
    };
    let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (answer, outcome) = match ran {
        Ok(RunOutcome::Returned { value }) => (
            Ok(DebugRunAnswer {
                value: Some(value),
                error: None,
                ms,
            }),
            "returned",
        ),
        Ok(RunOutcome::Threw { name, message, stack }) => (
            Ok(DebugRunAnswer {
                value: None,
                error: Some(ThrownError { name, message, stack }),
                ms,
            }),
            "threw",
        ),
        Ok(RunOutcome::TooLarge { bytes }) => (
            Err(too_large(&format!(
                "What the code returned is at least {bytes} bytes as JSON, more than a page sends in one message: \
                 return less"
            ))),
            "too_large",
        ),
        Err((error, outcome)) => (Err(error), outcome),
    };
    let bytes = answer
        .as_ref()
        .map_or(0, |answer| serde_json::to_string(answer).map_or(0, |json| json.len()));
    tracing::info!(
        event = "debug.ran",
        page = holder.page.as_deref(),
        token = prefix(&token, LOGGED_TOKEN_CHARS),
        code = %code,
        ms,
        outcome,
        bytes,
        "a debug link ran code in its page"
    );
    answer
}

fn too_large(message: &str) -> ApiError {
    ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, ErrorCode::TooLarge, message)
        .with_param("limitBytes", MAX_PAGE_MESSAGE_BYTES)
}
