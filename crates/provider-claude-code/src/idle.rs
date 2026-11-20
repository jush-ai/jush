//! A Claude Code process among its machine's idle ones
//! (`crates/provider-subscription-cli/src/idle.rs`): a runtime's kept process,
//! and how a closed one is logged.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use jush_provider_subscription_cli::{Closing, IDLE_FOR, Idle, Placement};
use tokio_util::task::AbortOnDropHandle;

use crate::Shared;
use crate::live::LiveCli;

/// Logs that a process `idle` waiting as closed after five idle minutes.
pub(crate) fn log_unused(shared: &Shared, node: &str, idle: Idle) {
    tracing::info!(
        event = "subscription_cli_process.idle_closed",
        provider = %shared.id,
        node,
        process = idle.as_str(),
        reason = "unused",
        ms = u64::try_from(IDLE_FOR.as_millis()).unwrap_or(u64::MAX),
        "an idle CLI process was closed"
    );
}

/// A runtime's process kept from its last action, seated idle until the next
/// run takes it; meanwhile its seat may close it.
pub(crate) struct Kept {
    live: Rc<RefCell<Option<LiveCli>>>,
    _closer: AbortOnDropHandle<()>,
}

impl Kept {
    /// Seats `live`, which no action uses any more, among its machine's idle
    /// processes.
    pub(crate) fn seat(shared: &Arc<Shared>, placement: &dyn Placement, live: LiveCli) -> Self {
        let seat = placement.idle().seat(&shared.id, shared.auth.account().ok(), Idle::Kept);
        let live = Rc::new(RefCell::new(Some(live)));
        let closer = {
            let live = live.clone();
            let shared = shared.clone();
            let closing = seat.closing();
            tokio::task::spawn_local(async move {
                let closing = closing.await;
                drop(seat);
                let Some(live) = live.borrow_mut().take() else {
                    return;
                };
                if closing == Closing::Unused {
                    log_unused(&shared, &live.node(), Idle::Kept);
                }
                // A close of its own, which a run taking the process
                // meanwhile cannot cut short.
                tokio::task::spawn_local(live.close());
            })
        };
        Self {
            live,
            _closer: AbortOnDropHandle::new(closer),
        }
    }

    /// The process, which leaves its seat; none when its seat closed it.
    pub(crate) fn take(self) -> Option<LiveCli> {
        self.live.borrow_mut().take()
    }

    /// Whether the process, while it is still kept, could go on with
    /// `request`, whatever its input ([`LiveCli::continues`]).
    pub(crate) fn continues(&self, request: &jush_provider_common::InferenceRequest) -> bool {
        self.live
            .borrow()
            .as_ref()
            .is_some_and(|live| live.continues(request))
    }
}
