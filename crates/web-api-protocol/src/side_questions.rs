//! A conversation's side questions: asking one, a follow-up, a Retry and
//! folding its card. Each is an agent of the conversation, whose entry the
//! conversation's agents list (`crates/conversation-lane-protocol/src/agents.rs`)
//! and whose answers are its transcript, which a page reads and follows as any
//! agent's.

use jush_conversation_lane_protocol::{ClientContent, SideQuestion};
use jush_shared_types::{BlockId, CommentPart, Labels, MAX_SAFE_INTEGER, MediaSource, NodeId, TurnId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `POST /conversations/:id/side-questions`: a new side question, under the
/// id the page chose, about the words `quote` of the completed assistant
/// text `blockId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, garde::Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskRequest {
    #[garde(length(chars, min = 1))]
    pub id: String,
    #[garde(skip)]
    pub block_id: BlockId,
    #[garde(range(max = MAX_SAFE_INTEGER))]
    pub start: u64,
    #[garde(range(max = MAX_SAFE_INTEGER), custom(after_start(&self.start)))]
    pub end: u64,
    #[garde(length(min = 1))]
    pub quote: String,
    /// The picture asked of, for a side question about a picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(dive)]
    pub picture: Option<MediaSource>,
    /// What is asked, a message's content, as a frame names it.
    #[garde(dive, custom(says_something))]
    pub question: Vec<ClientContent>,
    /// The labels its program gave it, as a message's
    /// (`crates/conversation-lane-protocol/src/client.rs`).
    #[serde(default, skip_serializing_if = "Labels::is_empty")]
    #[garde(custom(jush_shared_types::check_labels))]
    pub labels: Labels,
    /// The assignment its question continues, as a message's; one of its
    /// own id when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub assignment_id: Option<TurnId>,
}

/// `POST /conversations/:id/side-questions/:sideId/messages`: a follow-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, garde::Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideFollowUp {
    /// Its id, which names the assignment it opens unless it names another,
    /// and which a retry repeats so the side question takes it once; one is
    /// made when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub id: Option<TurnId>,
    /// A message's content, as a frame names it.
    #[garde(dive, custom(says_something))]
    pub content: Vec<ClientContent>,
    /// The labels its program gave it, as a message's.
    #[serde(default, skip_serializing_if = "Labels::is_empty")]
    #[garde(custom(jush_shared_types::check_labels))]
    pub labels: Labels,
    /// The assignment it continues, as a message's; one of its own id when
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub assignment_id: Option<TurnId>,
}

/// `PATCH /conversations/:id/side-questions/:sideId`: folds its card to
/// its header, or opens it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, garde::Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideQuestionPatch {
    #[garde(skip)]
    pub folded: bool,
}

/// The answer of a side question's creation and of its patch: the side
/// question as its agent entry shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, garde::Validate)]
#[serde(rename_all = "camelCase")]
pub struct SideQuestionAnswer {
    #[garde(skip)]
    pub id: NodeId,
    #[garde(dive)]
    pub side_question: SideQuestion,
}

fn after_start(start: &u64) -> impl FnOnce(&u64, &()) -> garde::Result + '_ {
    move |end, _| {
        if end > start {
            Ok(())
        } else {
            Err(garde::Error::new("ends after it starts"))
        }
    }
}

fn says_something(content: &[ClientContent], _: &()) -> garde::Result {
    if content.iter().any(CommentPart::says_something) {
        Ok(())
    } else {
        Err(garde::Error::new("holds no words and no file"))
    }
}
