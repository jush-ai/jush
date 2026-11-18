//! What a page reads of a conversation's live tree: a node's latest page, with
//! the output of its request and its cache chart.
//!
//! A live node's latest page is read from its session's window, whole in
//! memory, so it holds what no checkpoint has stored yet and is at the version
//! of the patches that follow it. The store adds what the window does not
//! keep: the entries of the newest instructions block and the summary size of
//! each compaction marker.

use jush_agent_session::LiveTurn;
use jush_core_store::StorageError;
use jush_core_store::{tree::{self, RootTurn, Terminal, Transcript}};
use jush_conversation_lane_protocol::{CacheChart, ShellStatus, index_u32};
use jush_shared_types::{NodeId, TurnId};
use jush_web_api_protocol::conversations::{TranscriptPage, TurnState};
use jush_web_api_protocol::ids::ConversationId;

use super::summary::outcome;

use crate::Core;
use jush_core_device_access::root_of;

impl Core {
    /// Where the root's turn `turn` of the conversation `id` stands, by the
    /// id of the message that started it: queued or running as the root's
    /// session tells while a tree runs, and otherwise as the store holds it,
    /// the turn's end and answer included; none when the conversation holds
    /// no turn of it.
    pub(crate) async fn root_turn(&self, id: &ConversationId, turn: &TurnId) -> Result<Option<RootTurn>, StorageError> {
        let tree = self.agent().tree(&root_of(id));
        let live = tree.is_some();
        match tree.and_then(|tree| tree.root().session().live_turn(turn)) {
            Some(LiveTurn::Queued) => return Ok(Some(RootTurn::Queued)),
            Some(LiveTurn::Running) => return Ok(Some(RootTurn::Running)),
            None => {}
        }
        let turn = turn.clone();
        Ok(self.store().databases()
            .read(id, move |connection| tree::turn(connection, &turn, live))
            .await?
            .flatten())
    }

    /// [`Core::root_turn`] as `GET /conversations/{id}/turns/{turnId}`
    /// answers it.
    pub async fn turn_state(&self, id: &ConversationId, turn: &TurnId) -> Result<Option<TurnState>, StorageError> {
        Ok(self.root_turn(id, turn).await?.map(|found| match found {
            RootTurn::Queued => TurnState::Queued,
            RootTurn::Running => TurnState::Running,
            RootTurn::Ended { end, .. } => TurnState::Ended {
                outcome: outcome(Terminal::of(&end)),
                end: end.id().clone(),
            },
        }))
    }

    /// The latest `count` blocks, a page at most, of the live node `node` of
    /// the conversation `id`, read from its window now, with the transcript's version and the view of each
    /// live command of the node and its cache chart, without the failure facts
    /// and attachments its route adds; none when the node is not live.
    pub async fn live_transcript(
        &self,
        id: &ConversationId,
        node: &NodeId,
        count: usize,
    ) -> Result<Option<TranscriptPage>, StorageError> {
        let Some(tree) = self.agent().tree(&root_of(id)) else {
            return Ok(None);
        };
        // Read at once, nothing awaited, so the patches that follow begin at
        // the version it names.
        let Some((page, commands, request_output)) = tree.latest_page(node, count) else {
            return Ok(None);
        };
        let cache_chart = tree.cache_chart(node);
        let transcript = Transcript::Agent(node.clone());
        let blocks = page.blocks.clone();
        let (instructions, summaries) = self.store().databases()
            .read(id, move |connection| tree::read_context(connection, &transcript, &blocks))
            .await?
            .unwrap_or_default();
        Ok(Some(TranscriptPage {
            start: index_u32(page.start),
            length: index_u32(page.length),
            blocks: page.blocks,
            failures: None,
            instructions,
            summaries,
            versions: page.versions,
            attachments: Vec::new(),
            reply_blocks: Vec::new(),
            version: Some(page.version),
            commands,
            request_output,
            cache_chart,
        }))
    }

    /// The view of each command a closed subagent's shells hold while they
    /// stop, for the page of a node that is not live.
    pub fn stopping_commands(&self, id: &ConversationId, node: &NodeId) -> Vec<ShellStatus> {
        self.agent()
            .tree(&root_of(id))
            .map(|tree| tree.stopping_commands(node))
            .unwrap_or_default()
    }

    /// The cache chart of the node `node` of the conversation `id`; none for a
    /// node that has none.
    pub async fn cache_chart(&self, id: &ConversationId, node: &NodeId) -> Result<Option<CacheChart>, jush_agent_store::StoreError> {
        self.agent().cache_chart(&root_of(id), node).await
    }

}
