//! Finding a conversation, `GET /search?q=`, finding in one,
//! `GET /conversations/{id}/search?q=`, and finding in every conversation's
//! history, `GET /history/search?q=`, and what each answers.

use jush_shared_types::{BlockId, MAX_SAFE_INTEGER, NodeId, Nullable, Timestamp, trim};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::ConversationId;

/// The most conversations, or messages of one conversation, one search
/// answers.
///
/// Bounded because a search answers whole, and the conversations, as the
/// messages of one, have no end.
pub const RESULTS_MAX: usize = 50;

/// The most characters (Unicode scalar values) of a match's line.
///
/// Bounded because a line of a result goes to the page whole, and a line has no
/// length of its own.
pub const LINE_MAX: usize = 160;

/// `?q=`: what the user typed; with `&workspace=`, of that workspace's
/// conversations alone.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchQuery {
    pub q: SearchText,
    #[serde(default)]
    pub workspace: Option<crate::ids::WorkspaceId>,
}

/// A query as the backend reads it: trimmed, with at least one
/// character. Its words are the pieces between white space.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct SearchText(String);

/// Why a text is not a query.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a query has a character besides the white space around it")]
pub struct NotQuery;

impl TryFrom<String> for SearchText {
    type Error = NotQuery;

    fn try_from(text: String) -> Result<Self, NotQuery> {
        let trimmed = trim(&text);
        if trimmed.is_empty() {
            return Err(NotQuery);
        }
        Ok(Self(trimmed.to_owned()))
    }
}

impl SearchText {
    /// The words, each once, in the order typed.
    pub fn words(&self) -> Vec<String> {
        let mut words: Vec<String> = Vec::new();
        for word in self.0.split_whitespace() {
            if !words.iter().any(|seen| seen == word) {
                words.push(word.to_owned());
            }
        }
        words
    }
}

/// What `GET /search` answers: the caller's conversations that match, title
/// matches first, then the others, each group most recently active first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
}

/// A conversation the query found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub conversation_id: ConversationId,
    /// Its title; null while it is the placeholder, as the conversation's
    /// summary has it.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<String>")]
    pub title: Option<String>,
    /// The places of the query's words in the title, as `SearchMatch`'s
    /// ranges give them; empty when the title does not match.
    pub title_ranges: Vec<[u32; 2]>,
    pub archived: bool,
    /// When the conversation's latest message was written.
    pub last_active_at: Timestamp,
    /// The message that matched; null when only the title does.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(with = "Nullable<SearchMatch>")]
    pub r#match: Option<SearchMatch>,
}

/// The newest message that holds every word of the query: its block, the
/// line of its text around the first match, and where the words are in that
/// line, as `[start, end]` offsets in UTF-16 code units, end exclusive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub block_id: BlockId,
    pub text: String,
    pub ranges: Vec<[u32; 2]>,
}

/// What `GET /conversations/{id}/search` answers: the messages of the
/// conversation's main agent, the user's and the agent's answers, that hold
/// every word of the query, newest first, at most [`RESULTS_MAX`] of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MessageResults {
    pub messages: Vec<MessageResult>,
    /// More messages hold the words than the answer lists.
    pub limited: bool,
}

/// A message of the conversation the query found: its block, who wrote it
/// and when, and its line around the first match with where the words are,
/// as `SearchMatch` gives them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageResult {
    pub block_id: BlockId,
    pub author: MessageAuthor,
    pub time: Timestamp,
    pub text: String,
    pub ranges: Vec<[u32; 2]>,
}

/// The hits one page of a history search holds by default.
pub const HISTORY_HITS_DEFAULT: u32 = 50;

/// The most hits one page of a history search holds.
///
/// Bounded because a page is answered whole, and the texts of a core's
/// conversations have no end.
pub const HISTORY_HITS_MAX: u32 = 200;

/// `?q=&any=&kinds=&from=&to=&after=&limit=` of `GET /history/search`: the
/// texts of every conversation's agents that hold every word of `q`, or any
/// with `any`, of the kinds `kinds` names with a comma between two, every
/// kind when absent, written at or after `from` and before `to`, newest
/// first, after the cursor `after`, `limit` of them. Queries are the
/// backend's alone and are not emitted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistorySearchQuery {
    pub q: SearchText,
    #[serde(default)]
    pub any: Option<bool>,
    #[serde(default)]
    pub kinds: Option<String>,
    #[serde(default)]
    pub from: Option<Timestamp>,
    #[serde(default)]
    pub to: Option<Timestamp>,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// A kind of a block's searchable text. The product's search finds a
/// root's messages and answers; a history search finds all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    /// A message or a steer of the user or of the agent's parent.
    Message,
    /// A text of the agent.
    Answer,
    /// The title of a command a `shell` call of the agent ran.
    Command,
}

impl SearchKind {
    /// Its name, as `?kinds=` lists it and the search index keeps it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Answer => "answer",
            Self::Command => "command",
        }
    }

    /// The kind `name` names.
    pub fn named(name: &str) -> Option<Self> {
        [Self::Message, Self::Answer, Self::Command].into_iter().find(|kind| kind.as_str() == name)
    }
}

/// What `GET /history/search` answers: a page of at most `limit` hits,
/// newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryHits {
    pub hits: Vec<HistoryHit>,
    /// Where the page ends, which `after` takes to read on with the same
    /// query: the place of the last text the page read, a hit or one left
    /// out; for a page that read none, the `after` given, empty without
    /// one.
    pub next: String,
    /// Whether hits wait after the page.
    pub more: bool,
}

/// A text a history search found: its conversation, its agent by the
/// number its conversation knows it by, 0 for the main agent, and by the
/// node a transcript is read by, its block, its kind and time, and its line
/// around the first match with where the words are, as `SearchMatch` gives
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryHit {
    pub conversation: ConversationId,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub agent: u64,
    pub node: NodeId,
    pub block_id: BlockId,
    pub kind: SearchKind,
    pub time: Timestamp,
    pub text: String,
    pub ranges: Vec<[u32; 2]>,
}

/// Who wrote a message: the user, or the agent, whose answer it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MessageAuthor {
    User,
    Agent,
}
