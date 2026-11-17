//! Debug This Page: the link through which an agent runs code in the one
//! page the user chose, with `curl`.
//!
//! The page holds the link on a lane of its connection
//! ([`crate::connection::LaneTarget::Debug`]), which carries
//! [`DebugToPage`] and [`DebugFromPage`]. The agent reaches it over plain
//! HTTP, the link's token its only key:
//!
//! - `GET /api/debug/:token` answers [`DebugLinkAnswer`];
//! - `POST /api/debug/:token/eval?timeout=<seconds>` takes JavaScript as the
//!   body and answers [`DebugRunAnswer`].

use jush_shared_types::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The path of the links under `/api`.
pub const DEBUG_ROUTE: &str = "/debug";

/// How many levels deep the page writes what a run returned, and what lies
/// deeper as `"[Deep]"`: the backend's JSON reader stops at 128 levels, and
/// the page's message wraps the value in two more.
pub const DEBUG_VALUE_DEPTH: usize = 100;

/// What the backend sends on a debug lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DebugToPage {
    /// The lane's link, as it opened: a new one, or the one the page resumed.
    Started {
        token: String,
        #[serde(rename = "expiresAt")]
        expires_at: Timestamp,
    },
    /// Code to run, the body of an async function, which the page answers
    /// with `ran` and the same `id`.
    Run { id: u32, source: String },
    /// What the page is now, which the page answers with `described` and the
    /// same `id`.
    Describe { id: u32 },
}

/// What the page sends on a debug lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DebugFromPage {
    /// What the run `id` came to.
    Ran { id: u32, outcome: RunOutcome },
    /// What the page is, for the describe `id`.
    Described { id: u32, page: PageFacts },
    /// The user stopped debugging: the link ends.
    Stop,
}

/// What a run came to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunOutcome {
    /// What the code returned, as the page wrote it as JSON.
    Returned { value: serde_json::Value },
    /// What the code threw.
    Threw {
        name: String,
        message: String,
        stack: Option<String>,
    },
    /// What the code returned took `bytes` as a message, more than a page
    /// sends ([`crate::MAX_PAGE_MESSAGE_BYTES`]).
    TooLarge { bytes: u32 },
}

/// What a debugged page is, as it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageFacts {
    /// The page load's ID, which a reload makes anew.
    pub id: String,
    /// Its path, query and fragment.
    pub url: String,
    pub title: String,
    /// The tab is the one its window shows, and the window is not minimized.
    pub visible: bool,
    /// The build of the web app the page loaded.
    pub build: String,
}

/// `GET /api/debug/:token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugLinkAnswer {
    /// The page that holds the link now; null while none does, as while it
    /// reloads.
    pub page: Option<PageFacts>,
    pub expires_at: Timestamp,
    /// How to run code with the link, for an agent given only the link.
    pub usage: String,
}

/// `POST /api/debug/:token/eval`: what the code returned or threw, and how
/// long the page took.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugRunAnswer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ThrownError>,
    pub ms: u64,
}

/// An error the code threw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ThrownError {
    pub name: String,
    pub message: String,
    pub stack: Option<String>,
}
