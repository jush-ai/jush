//! A session's runtime and its runs (`live.rs`).
//!
//! The runtime keeps at most one process. A run takes it when it serves the
//! request, else starts a new one, and hands it back only where the
//! conversation can go on in it: at a call of a tool, whose result the next
//! run delivers, and at the turn's end. It hands it back before the run's
//! last event, since the agent may stop reading there. Every other end closes
//! it, SIGTERM first.
//!
//! Once the session's action ends, the process is idle until the next run
//! takes it, and takes a seat among its machine's idle processes, which may
//! close it meanwhile (`crates/provider-subscription-cli/src/idle.rs`).
//!
//! `agy` reports each model request's tokens when it ends, input, cache and
//! output together, and a turn's split into them only at the turn's end,
//! summed over its requests. A run reports its last request's as its input:
//! the context the next request carries, which the agent compacts by. The
//! turn's last run also reports the turn's output, and splits its input as the
//! turn's is split. The agent sums a turn's output and speed over its runs, so
//! both are the turn's; the input the usage ledger counts misses the requests
//! `agy` makes inside a run, such as one that reads a saved result, since
//! counting two requests in one response would read as twice the context.

use std::rc::Rc;
use std::sync::Arc;

use async_stream::stream;
use futures_util::StreamExt as _;
use futures_util::future::LocalBoxFuture;
use futures_util::stream::Stream;
use jush_provider_common::{
    InferenceRequest, ProviderEvent, ProviderRun, ProviderRuntime, RequestLimits,
    ToolResultKinds,
};
use jush_provider_subscription_cli::{Placement, ProcessSlot};
use jush_shared_types::{Model, TokenUsage};

use crate::live::{Live, Next, TurnUsage};
use crate::{CLI, Shared};

/// A session's runtime.
pub(crate) struct AgyRuntime {
    shared: Arc<Shared>,
    placement: Rc<dyn Placement>,
    /// The process kept from the last run.
    slot: ProcessSlot<Live>,
}

impl AgyRuntime {
    pub(crate) fn new(shared: Arc<Shared>, placement: Rc<dyn Placement>) -> Self {
        Self {
            shared,
            placement,
            slot: ProcessSlot::default(),
        }
    }
}

impl ProviderRuntime for AgyRuntime {
    fn run(&mut self, request: InferenceRequest) -> ProviderRun<'_> {
        run(self, request).boxed_local()
    }

    fn fresh(&self) -> Box<dyn ProviderRuntime> {
        Box::new(Self::new(self.shared.clone(), self.placement.clone()))
    }

    fn close(&mut self) -> LocalBoxFuture<'_, ()> {
        Box::pin(self.slot.close())
    }

    /// The action ended: the machine is no longer held for it, and the
    /// process kept from its last run is idle.
    fn idle(&mut self) {
        self.slot
            .idle(&*self.placement, &self.shared.entry, self.shared.auth.account().ok());
    }

    /// `agy` takes an image in a tool's result, as MCP image content, and
    /// documents no limit of a request's size.
    fn request_limits(&self, _model: &Model) -> RequestLimits {
        RequestLimits {
            tool_results: ToolResultKinds::IMAGES,
            ..RequestLimits::default()
        }
    }
}

/// One run: the process kept for the session when it serves the request,
/// else a new one, read until a call of a tool or the turn's end.
fn run(runtime: &mut AgyRuntime, request: InferenceRequest) -> impl Stream<Item = ProviderEvent> + '_ {
    stream! {
        let cancel = request.cancel.clone();
        // The machine stays awake from the action's first request to its
        // end, however long its process waits on a tool's result.
        runtime.slot.act(&*runtime.placement);
        let mut live = match runtime.slot.take() {
            Some(mut live) if live.serves(&request) => {
                match cancel.run_until_cancelled(live.give(&runtime.shared, &request)).await {
                    None => {
                        live.close().await;
                        return;
                    }
                    Some(Err(failure)) => {
                        yield ProviderEvent::Error(failure);
                        live.close().await;
                        return;
                    }
                    Some(Ok(())) => live,
                }
            }
            previous => {
                if let Some(previous) = previous {
                    previous.close().await;
                }
                let started = Live::start(&runtime.shared, &*runtime.placement, &request);
                match cancel.run_until_cancelled(started).await {
                    // Giving the start up drops what it started, which
                    // closes it.
                    None => return,
                    Some(Err(failure)) => {
                        yield ProviderEvent::Error(failure);
                        return;
                    }
                    Some(Ok(live)) => live,
                }
            }
        };
        loop {
            match live.next(&runtime.shared, &cancel).await {
                Next::Text(text) => yield ProviderEvent::TextDelta(text),
                Next::Reasoning { opens, text } => {
                    if opens {
                        yield ProviderEvent::ThinkingStart;
                    }
                    yield ProviderEvent::ThinkingDelta(text);
                }
                Next::Step(step) => yield ProviderEvent::Opaque(step),
                Next::Called(call) => {
                    let usage = usage(live.context(), None);
                    runtime.slot.keep(live);
                    yield ProviderEvent::ToolCall(call);
                    yield ProviderEvent::response(usage, CLI.name, &request.model_id);
                    return;
                }
                Next::Answered => {
                    let usage = usage(live.context(), live.turn());
                    runtime.slot.keep(live);
                    yield ProviderEvent::response(usage, CLI.name, &request.model_id);
                    return;
                }
                Next::Failed(failure) => {
                    yield ProviderEvent::Error(failure);
                    live.close().await;
                    return;
                }
                Next::Cancelled => {
                    live.close().await;
                    return;
                }
            }
        }
    }
}

/// The usage a run reports, whose tokens add up to `held`, the tokens of its
/// last request, which the agent takes for the context: `held` as its input;
/// for the turn's last run, which has `turn`, the turn's output, and the rest
/// of `held` as input, split as the turn's input is.
fn usage(held: Option<u64>, turn: Option<TurnUsage>) -> Option<TokenUsage> {
    let held = held?;
    let Some(turn) = turn else {
        return Some(TokenUsage {
            input_tokens: held,
            ..TokenUsage::default()
        });
    };
    let input = held - turn.output.min(held);
    let total = turn.input + turn.cache_read + turn.cache_write;
    let share = |part: u64| {
        if total == 0 {
            0
        } else {
            u64::try_from(u128::from(input) * u128::from(part) / u128::from(total)).unwrap_or(input)
        }
    };
    let cache_read_tokens = share(turn.cache_read);
    let cache_write_tokens = share(turn.cache_write);
    Some(TokenUsage {
        input_tokens: input - cache_read_tokens - cache_write_tokens,
        output_tokens: turn.output,
        cache_read_tokens,
        cache_write_tokens,
    })
}
