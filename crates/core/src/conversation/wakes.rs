//! Waking the sender of a message when the work it started in another
//! conversation ends (`jush conversation send --wake-when-done`).
//!
//! `send` and `new` keep a wake for such a message in the core's database
//! (`crates/core-store/src/wakes.rs`) once the message is admitted.
//! The work ends when the recipient rests after it, its subagents and
//! commands done, as a `--when-done` schedule fires: each open wake whose
//! message comes before the last turn's end settles there, with a notice of
//! how that turn ended and its last answer. A rest that came before the wake
//! was kept is found by the check that follows keeping it.
//!
//! ```text
//!   admit ─► keep ─► check ──┐
//!                            ├─► settle: notice kept ─► admit to the
//!   rest after the message ──┘   sender, or the nearest agent above
//!                                it ─► forget
//! ```
//!
//! The notice is kept before it goes, its id naming the end and the agent it
//! wakes and its time the end's, so a delivery that failed is tried again
//! as the same message at the next rest of any of the user's conversations
//! and at the core's start, and the sender's agent takes it once.
//!
//! - A wake whose message an edit removed from the recipient's path wakes its
//!   sender to say so.
//! - The recipient's deletion wakes every sender still waiting, to say it
//!   was deleted, once the deletion is done.
//! - A sender archived or deleted wakes for nothing: its wake goes.

use std::collections::HashMap;

use jush_agent_session::{AgentSession, EndedTurn, TurnOutcome};
use jush_core_store::wakes::PendingWake;
use jush_core_device_access::root_of;
use jush_shared_types::{
    AgentMessage, AgentMessageEvent, Attribution, BlockId, CompletionOutcome, NodeId, Sender, SenderConversation, Timestamp,
    done_id,
};
use jush_web_api_protocol::ids::ConversationId;

use crate::schedules::message::{Finished, turn_end};
use crate::Core;

/// What a wake's notice tells.
enum Told {
    /// The work the message started ended, its last turn so.
    Ended(Finished),
    /// The recipient's path no longer holds the message.
    Removed,
    /// The recipient was deleted.
    Deleted,
}

impl Core {
    /// `conversation` rests after work, its last turn ended as `end`, a
    /// failed one with `error`: each wake whose message came before settles,
    /// and each notice not delivered yet is tried again.
    pub(crate) fn settle_wakes_at_rest(&self, conversation: &ConversationId, end: EndedTurn, error: Option<String>) {
        let core = self.this();
        let conversation = conversation.clone();
        self.tasks().spawn_local(async move {
            core.settle_wakes(&conversation, &end, error).await;
            core.deliver_undelivered_wakes().await;
        });
    }

    /// The wake of `wake`'s message was just kept: settles it when the
    /// recipient already rests after the message.
    pub(crate) async fn check_kept_wake(&self, wake: &PendingWake) {
        let Some(tree) = self.agent().tree(&root_of(&wake.recipient)) else {
            return;
        };
        if tree.is_running() {
            return;
        }
        let session = tree.root().session();
        let Some(end) = session.ended_turn_result() else {
            return;
        };
        let error = failure(&session, &end);
        self.settle_wakes(&wake.recipient, &end, error).await;
    }

    /// Delivers each notice of the user's wakes that is not delivered yet.
    pub(crate) async fn deliver_undelivered_wakes(&self) {
        let wakes = match self.store().undelivered_wakes().await {
            Ok(wakes) => wakes,
            Err(error) => {
                tracing::warn!(event = "wake.read_failed", user = %self.user(), %error, "the wakes waiting for delivery were not read");
                return;
            }
        };
        for wake in wakes {
            if let Some(notice) = wake.notice.clone() {
                self.deliver_wake(&wake, notice).await;
            }
        }
    }

    /// Settles each open wake of `conversation` whose message comes before
    /// `end`, the end of its last turn.
    async fn settle_wakes(&self, conversation: &ConversationId, end: &EndedTurn, error: Option<String>) {
        let control = &self.store();
        let wakes = match control.pending_wakes(conversation.clone()).await {
            Ok(wakes) => wakes,
            Err(error) => {
                tracing::warn!(event = "wake.read_failed", %conversation, %error, "the wakes a conversation's messages asked for were not read");
                return;
            }
        };
        let mut named = Named::of(&wakes);
        let open: Vec<PendingWake> = wakes.into_iter().filter(|wake| wake.notice.is_none()).collect();
        if open.is_empty() {
            return;
        }
        // A tree that closed since its rest settles at its next one.
        let Some(tree) = self.agent().tree(&root_of(conversation)) else {
            return;
        };
        let session = tree.root().session();
        let round = tree.root().record().round;
        let title = control.conversation(conversation.clone()).await.ok().flatten().and_then(|record| record.title);
        let finished = Finished { conversation: conversation.clone(), title, end: end.clone(), error };
        let now = control.now();
        for wake in open {
            let told = match told_at_rest(&session, &wake, &finished).await {
                Ok(Some(told)) => told,
                Ok(None) => continue,
                Err(error) => {
                    // The wake stays; the next rest reads it again.
                    tracing::warn!(event = "wake.turn_read_failed", %conversation, message = %wake.message, %error, "a woken message's place was not read");
                    continue;
                }
            };
            let notice = named.name(notice(&wake, finished.title.as_deref(), round, &told, now));
            if let Err(error) = control.settle_wake(wake.recipient.clone(), wake.message.clone(), notice.clone()).await {
                tracing::warn!(event = "wake.notice_unsaved", %conversation, message = %wake.message, %error, "a wake's notice was not kept, so a failed delivery is not tried again");
            }
            self.deliver_wake(&wake, notice).await;
        }
    }

    /// The notices to deliver once `conversation` is deleted, one for each
    /// wake its messages asked for that waits: of the end of its work when
    /// that ended, of the deletion otherwise.
    pub(crate) async fn wakes_of_deleted(&self, conversation: &ConversationId) -> Vec<(PendingWake, AgentMessage)> {
        let control = &self.store();
        let wakes = match control.pending_wakes(conversation.clone()).await {
            Ok(wakes) => wakes,
            Err(error) => {
                tracing::warn!(event = "wake.read_failed", %conversation, %error, "the wakes a deleted conversation's messages asked for were not read");
                return Vec::new();
            }
        };
        if wakes.is_empty() {
            return Vec::new();
        }
        let tree = self.agent().tree(&root_of(conversation));
        let round = tree.as_ref().map_or(1, |tree| tree.root().record().round);
        let title = control.conversation(conversation.clone()).await.ok().flatten().and_then(|record| record.title);
        // The work ended when the tree rests after the message.
        let resting = tree.as_ref().filter(|tree| !tree.is_running()).map(|tree| tree.root().session());
        let rest = resting.as_ref().and_then(|session| {
            let end = session.ended_turn_result()?;
            let error = failure(session, &end);
            Some((session, Finished { conversation: conversation.clone(), title: title.clone(), end, error }))
        });
        let now = control.now();
        let mut named = Named::of(&wakes);
        let mut notices = Vec::new();
        for wake in wakes {
            if let Some(notice) = wake.notice.clone() {
                notices.push((wake, notice));
                continue;
            }
            let told = match &rest {
                Some((session, finished)) => match told_at_rest(session, &wake, finished).await {
                    Ok(Some(Told::Ended(finished))) => Told::Ended(finished),
                    _ => Told::Deleted,
                },
                None => Told::Deleted,
            };
            let notice = named.name(notice(&wake, title.as_deref(), round, &told, now));
            notices.push((wake, notice));
        }
        notices
    }

    /// Wakes `wake`'s sender with `notice`, and forgets the wake once it is
    /// delivered or its sender is gone.
    pub(crate) async fn deliver_wake(&self, wake: &PendingWake, notice: AgentMessage) {
        let control = &self.store();
        let sender = match control.conversation(wake.sender.clone()).await {
            Ok(sender) => sender,
            Err(error) => {
                // The wake stays; the next try reads the sender again.
                tracing::warn!(event = "wake.sender_read_failed", conversation = %wake.recipient, to_conversation = %wake.sender, %error, "a wake's sender was not read");
                return;
            }
        };
        let waiting = sender.is_some_and(|record| !record.archived());
        if waiting {
            let message = |recipient: &NodeId| AgentMessage { recipient_id: recipient.clone(), ..notice.clone() };
            if let Err(error) = self.permission_core().admit(&wake.sender, &wake.sender_node, &message).await {
                // The notice stays; the next try delivers it.
                tracing::warn!(event = "wake.undelivered", conversation = %wake.recipient, to_conversation = %wake.sender, %error, "a wake's notice was not delivered");
                return;
            }
            tracing::info!(event = "wake.delivered", conversation = %wake.recipient, to_conversation = %wake.sender, message = %wake.message, notice = %notice.id, "a message's sender was woken");
        }
        if let Err(error) = control.forget_wake(wake.recipient.clone(), wake.message.clone()).await {
            tracing::warn!(event = "wake.not_forgotten", conversation = %wake.recipient, message = %wake.message, %error, "a settled wake stays, and its sender takes the same notice again, once");
        }
    }
}

/// What `wake`'s notice tells while `session`'s tree rests after
/// `finished`'s turn: none while its message waits or comes after it.
async fn told_at_rest(
    session: &AgentSession,
    wake: &PendingWake,
    finished: &Finished,
) -> Result<Option<Told>, jush_agent_store::StoreError> {
    // The queue is read first: a message leaves it only for the path.
    if session.waits(&wake.message) {
        return Ok(None);
    }
    let Some(message) = session.position(&wake.message).await? else {
        // Admitted before the wake was kept, a message on neither the queue
        // nor the path was removed by an edit.
        return Ok(Some(Told::Removed));
    };
    let ended = session.position(&finished.end.id).await?.is_some_and(|end| end > message);
    Ok(ended.then(|| Told::Ended(finished.clone())))
}

/// The error of `end`, the session's last turn, when it failed.
fn failure(session: &AgentSession, end: &EndedTurn) -> Option<String> {
    match end.outcome {
        TurnOutcome::Failed => session.failed_turn().map(|failure| failure.message),
        _ => None,
    }
}

/// The notice that wakes `wake`'s sender with `told`, from the root of the
/// recipient, whose title is `title` and whose round is `round`; a notice of
/// no end is dated `now`.
fn notice(wake: &PendingWake, title: Option<&str>, round: u64, told: &Told, now: Timestamp) -> AgentMessage {
    let root = root_of(&wake.recipient);
    let name = Finished::name_of(&wake.recipient, title);
    let (end, at, outcome, content) = match told {
        Told::Ended(finished) => (
            finished.end.id.clone(),
            finished.end.at,
            completion(&finished.end),
            format!(
                "The work a message from this agent started in {name} ended; its last turn {}",
                turn_end(finished)
            ),
        ),
        Told::Removed => (
            wake.message.clone(),
            now,
            CompletionOutcome::Aborted,
            format!("The {name} no longer holds a message from this agent: the user edited the conversation before it."),
        ),
        Told::Deleted => (
            wake.message.clone(),
            now,
            CompletionOutcome::Aborted,
            format!("The {name} was deleted before the work a message from this agent started ended."),
        ),
    };
    AgentMessage {
        id: done_id(&root, &end, &wake.sender_node),
        sender: Some(Sender {
            id: root.clone(),
            number: 0,
            description: "root session".to_owned(),
            round,
            conversation: Some(SenderConversation { id: root, title: title.map(str::to_owned), wakes: false }),
        }),
        recipient_id: wake.sender_node.clone(),
        timestamp: at,
        content,
        event: AgentMessageEvent::Completion { outcome },
        // The notice opens the conversation, at its end.
        origin: None,
        labels: wake.message_labels.clone(),
        assignment_id: wake.message_assignment_id.clone(),
    }
}

/// The labels and the assignment each notice names, by its id. Several
/// messages of one agent the same turn took wake it with one notice of that
/// turn's end, which the agent takes once (`done_id`): each such notice
/// names the first's, so the agent finds them the same message.
struct Named(HashMap<BlockId, Attribution>);

impl Named {
    /// The labels and the assignment the notices `wakes` keep already name.
    fn of(wakes: &[PendingWake]) -> Self {
        let kept = wakes.iter().filter_map(|wake| wake.notice.as_ref());
        Self(kept.map(|notice| (notice.id.clone(), attribution(notice))).collect())
    }

    /// `notice`, naming the labels and the assignment the first notice of
    /// its id named.
    fn name(&mut self, mut notice: AgentMessage) -> AgentMessage {
        let first = self.0.entry(notice.id.clone()).or_insert_with(|| attribution(&notice)).clone();
        notice.labels = first.labels;
        notice.assignment_id = first.assignment_id;
        notice
    }
}

/// The labels and the assignment `notice` names.
fn attribution(notice: &AgentMessage) -> Attribution {
    Attribution {
        labels: notice.labels.clone(),
        assignment_id: notice.assignment_id.clone(),
    }
}

/// How a turn's end reads as a completion.
fn completion(end: &EndedTurn) -> CompletionOutcome {
    match end.outcome {
        TurnOutcome::Answered => CompletionOutcome::Completed,
        TurnOutcome::Failed => CompletionOutcome::Failed,
        TurnOutcome::Stopped => CompletionOutcome::Aborted,
    }
}
