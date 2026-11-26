//! The starter lanes of a program's core
//! (`crates/web-api-protocol/src/starter.rs`): a program opens one on its
//! page connection to the core, and the core's starter sends its
//! notifications on it and waits for the program's answers.
//!
//! Several lanes may be open, as several programs drive the core: a
//! notification goes on the newest of a connection that declares the
//! plugins' current versions, and on the newest of any while none is. One
//! sent while none is open, or whose lane ends before the program answers,
//! reached no one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::ws::close_code;
use jush_core::starter::Notified;
use jush_core::{Lane, LaneClose, LaneEnd, LaneMessage, LaneReader};
use jush_web_api_protocol::push::PushMessage;
use jush_web_api_protocol::starter::{StarterAnswer, StarterRequest};
use tokio::sync::{mpsc, oneshot};

/// One core's starter lanes.
#[derive(Default)]
pub struct StarterLane(Mutex<Opens>);

#[derive(Default)]
struct Opens {
    /// How many lanes were opened for the core.
    count: u64,
    /// Each open lane's notifications to send, oldest first.
    open: Vec<Open>,
}

/// A lane a program opened, until it ends.
struct Open {
    /// Its number among the core's lanes.
    number: u64,
    /// Its connection declares its plugins in a version: it takes a
    /// notification while no lane of the current versions is open.
    versioned: bool,
    notifications: mpsc::UnboundedSender<Notification>,
}

/// A notification to send, and where its answer goes.
struct Notification {
    message: PushMessage,
    answer: oneshot::Sender<Notified>,
}

impl StarterLane {
    fn opens(&self) -> std::sync::MutexGuard<'_, Opens> {
        // No section panics while it holds the lock.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether a lane is open.
    pub fn is_open(&self) -> bool {
        !self.opens().open.is_empty()
    }

    /// Sends `message` on the newest lane of the current versions, else the
    /// newest, and answers how the program says it went; nowhere while no
    /// lane is open, or when the lane ends first.
    pub async fn notify(&self, message: PushMessage) -> Notified {
        let (answer, answered) = oneshot::channel();
        let sent = {
            let opens = self.opens();
            let lane = opens.open.iter().rev().find(|open| !open.versioned).or(opens.open.last());
            match lane {
                Some(open) => open.notifications.send(Notification { message, answer }).is_ok(),
                None => false,
            }
        };
        if !sent {
            return Notified::Nowhere;
        }
        answered.await.unwrap_or(Notified::Nowhere)
    }

    /// Ends the open lanes, as the core closes.
    pub fn end(&self) {
        self.opens().open.clear();
    }

    /// Opens a lane, which takes the notifications until a newer one opens;
    /// with `versioned`, of a connection that declares its plugins in a
    /// version, which takes them only while no lane of the current versions
    /// is open.
    pub(crate) fn claim(self: &Arc<Self>, versioned: bool) -> Claim {
        let mut opens = self.opens();
        opens.count += 1;
        let (notifications, received) = mpsc::unbounded_channel();
        let number = opens.count;
        opens.open.push(Open { number, versioned, notifications });
        Claim {
            lane: self.clone(),
            number,
            notifications: received,
        }
    }
}

/// An open of the starter lane, from its claim until it ends.
pub(crate) struct Claim {
    lane: Arc<StarterLane>,
    number: u64,
    notifications: mpsc::UnboundedReceiver<Notification>,
}

impl Claim {
    /// Sends the core's notifications on `lane` and hands the program's
    /// answers back, until the program closes the lane, its connection
    /// ends, or the core closes.
    pub(crate) async fn serve(mut self, mut lane: Lane, mut reader: LaneReader) {
        let mut waiting: HashMap<u32, oneshot::Sender<Notified>> = HashMap::new();
        let mut next = 0u32;
        let end = loop {
            tokio::select! {
                notification = self.notifications.recv() => {
                    // The core closes: its starter ended the lane.
                    let Some(Notification { message, answer }) = notification else {
                        break LaneEnd::Close(LaneClose { code: close_code::AWAY, reason: "core_closing" });
                    };
                    next = next.wrapping_add(1);
                    let request = StarterRequest::Notify { id: next, message };
                    let text = serde_json::to_string(&request).expect("a starter request serializes");
                    if lane.send(text).await.is_err() {
                        break LaneEnd::Gone;
                    }
                    waiting.insert(next, answer);
                }
                message = reader.next() => {
                    let Some(message) = message else {
                        break LaneEnd::Gone;
                    };
                    let answer = match message {
                        LaneMessage::Text(text) => serde_json::from_str::<StarterAnswer>(&text).ok(),
                        LaneMessage::Binary(_) => None,
                    };
                    let (id, outcome) = match answer {
                        Some(StarterAnswer::Sending { id, to }) => (id, Notified::Sending(to as usize)),
                        Some(StarterAnswer::Shown { id }) => (id, Notified::Shown),
                        Some(StarterAnswer::Nowhere { id }) => (id, Notified::Nowhere),
                        None => {
                            break LaneEnd::Close(LaneClose { code: close_code::POLICY, reason: "unexpected_message" });
                        }
                    };
                    if let Some(answer) = waiting.remove(&id) {
                        // A notification whose sender left needs no answer.
                        let _ = answer.send(outcome);
                    }
                }
            }
        };
        // The notifications still waiting reached no one: their senders hear
        // so as their answers drop.
        drop(waiting);
        lane.end(reader, end);
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.lane.opens().open.retain(|open| open.number != self.number);
    }
}
