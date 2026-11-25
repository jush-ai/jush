//! A program's page of one conversation: the conversation a program bound
//! it to and what the program lets it do beyond reading it
//! (`crates/core-api/src/page.rs`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::ConversationId;

/// What a user does on a program's page of a conversation, each a control
/// of its views and the requests behind it. A page reads its conversation
/// with none of them.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum PageOperation {
    /// Sends a message, queues one and takes it back, keeps a draft and
    /// saved messages.
    Send,
    /// Attaches files to a message.
    Attach,
    /// Steers a running turn.
    Steer,
    /// Edits a message and sends it again, regenerates a reply, switches
    /// its version.
    Edit,
    /// Stops the turn, its subagents or a command.
    Stop,
    /// Retries a failed turn, resumes a stopped one.
    Retry,
    /// Compacts the context.
    Compact,
    /// Clears the context.
    Clear,
    /// Asks side questions.
    Ask,
    /// Messages a subagent.
    MessageAgents,
    /// Answers a permission request.
    Decide,
    /// Forks the conversation.
    Fork,
    /// Deletes the conversation.
    Delete,
    /// Changes the title.
    Rename,
    /// Chooses the model, its reasoning effort and its service tier.
    ChooseModel,
    /// Chooses where it runs, detaches a device.
    ChooseDevice,
    /// Chooses what its subagents spawn with.
    ChooseWorkflow,
    /// Unpins a pinned document.
    Pins,
    /// Creates, changes, runs and deletes its schedules.
    Schedules,
    /// The Files window: browses the conversation's devices, searches,
    /// previews.
    Files,
    /// Uploads, makes folders and removes in the Files window.
    WriteFiles,
    /// The Changes window.
    Changes,
    /// The Browser window: its live view, its tabs and their pages.
    Browser,
    /// The Simulators window, as the Browser window.
    Simulators,
    /// The other plugins' controls in the conversation, and reloading its
    /// plugins.
    Plugins,
}

/// `GET /page`: the conversation a program's page shows, and what it may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageAnswer {
    pub conversation: ConversationId,
    pub allow: Vec<PageOperation>,
}
