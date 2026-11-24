//! A session's `agy` process, which a run reads (`run.rs`) and the runtime
//! keeps between its turns.
//!
//! A new process holds the request's history before it answers the request's
//! input, the user messages the transcript ends with (`history.rs`):
//!
//! - with no history, as in a conversation's first turn, the process makes
//!   the session itself;
//! - with the history the session's last process left, the database it left
//!   ([`Parked`]), the process loads that database as it is, so its first
//!   request starts with the bytes the last one sent, which Google's cache
//!   holds;
//! - with any other history, a first process makes the session with Jush's
//!   agent and ends, the provider writes the history into the session's
//!   database, and the turn's process loads it.
//!
//! A process closed between turns, as the machine closes an idle one, ends,
//! and `agy` writes its write-ahead log into its database; the provider reads
//! that database before the directory goes, and the runtime keeps it, the
//! last one, until a process started from it opens or the runtime goes. The
//! machine is up when its process closes, and a directory lasts no longer
//! than its process, as before.
//!
//! `agy` never answers from a summary of its own: a history at or over the
//! limit at which it compacts by itself is refused as too long, which the
//! agent answers by compacting and asking again. The provider refuses a
//! history it estimates over the limit before writing it into a session, and
//! a run whose `agy` compacted the session: a `usage_update` that comes
//! before the model said anything of its request, with tokens over the limit
//! or with a compaction among the transcript's new lines. The run's failure
//! closes the process, so no later one loads the summarized session.
//!
//! Each run reads the lines `agy` added to the session's transcript since the
//! last read (`transcript.jsonl`, only the new bytes), for the tokens of each
//! model request, which ACP reports only summed over a turn.
//!
//! The process answers the input in one prompt, `session/prompt`. The model's
//! text and reasoning arrive as `session/update`s, and a call of a tool over
//! the MCP channel (`tools.rs`). A call ends the run and keeps the process:
//! its turn waits for the call's result, which the next run delivers. The
//! prompt's answer ends the turn.
//!
//! A call of one of `agy`'s own tools, such as `read_file`, is `agy`'s to run:
//! it arrives as `session/update`s, which the run reports as a vendor's step.
//! Before a call `agy` does not allow by itself, `agy` asks Jush, which allows
//! a call of its own MCP server, and a `read_file` of a path where nothing is
//! on the process's machine, which fails with `file does not exist` and
//! leaves the turn going: `read_file` then reads only what `agy` saved of a
//! result and the media Jush puts in the session's workspace (`media.rs`),
//! which it reads without asking. `agy`'s other tools ask nothing and reach
//! no file (measured). A denial ends the turn with no answer, which fails the
//! run.
//!
//! A kept process goes on with a later request only where it continues what
//! the process received, with the same system prompt: new user messages
//! after a turn's end, or the result of the call it holds. `agy` takes input
//! only between turns, so a steer that comes with a result is answered by a
//! new process, whose history holds the result.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use futures_util::FutureExt as _;
use futures_util::future::{LocalBoxFuture, Shared as SharedFuture};
use jush_device_interface::{Process, ProcessEnd};
use jush_provider_common::openai_request::reasoning_effort;
use jush_provider_common::quota::ProbeAsk;
use jush_provider_common::{
    ErrorCode, InferenceItem, InferenceRequest, Medium, OpaqueStep, ProviderFailure, ResultPart, ToolCall,
    UserPart, encode_body,
};
use jush_provider_subscription_cli::{
    CliStarter, CliWork, ConfigDir, HeldProcess, KeptProcess, Placed, Placement, Unanswered, exit_message,
    tool_result,
};
use jush_shared_types::{OpaqueStatus, OpaqueView, Timestamp, text_tokens};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::acp::{self, Line, Output, Refusal};
use crate::failure::{self, Reported};
use crate::history;
use crate::media;
use crate::models;
use crate::start::{self, CONVERSATIONS};
use crate::tools::{self, Channel, ChannelEvent};
use crate::{CLI, Shared};

/// How long the process that makes a session may take to end once its input
/// closed: `agy` ends in milliseconds, and one that does not must not hold
/// the run for good.
const MAKER_END: Duration = Duration::from_secs(10);

/// A session's process.
pub(crate) struct Live {
    held: HeldProcess,
    exit: SharedFuture<LocalBoxFuture<'static, ProcessEnd>>,
    output: Output,
    channel: Channel,
    /// The process's directory, its `HOME`, on its machine.
    home: String,
    /// The media the process was given as files.
    files: media::Files,
    /// `agy`'s id of the session.
    session: String,
    /// The conversation's session the process serves, which the logs name.
    node: String,
    system_prompt: String,
    /// The model the session runs now, at its level, as `agy` names it.
    model: Option<String>,
    /// The user messages the process received, in order.
    sent: Vec<Vec<UserPart>>,
    /// The call whose result the next run delivers.
    held_call: Option<ToolCall>,
    /// The prompt in flight.
    prompting: Option<Prompting>,
    next_id: u64,
    /// The tokens the session holds after its latest model request, as `agy`
    /// reports them.
    context: Option<u64>,
    /// Whether the piece being streamed is reasoning.
    thinking: bool,
    /// The latest piece of text, when it words a failure as `agy` does: the
    /// model's text when anything follows it, else what the turn's last step
    /// says when the turn ends (`failure.rs`).
    warning: Option<String>,
    /// What the run is told next, before anything the process says: what
    /// followed a held piece of text, given as the model's first.
    after: Option<Next>,
    /// Whether Jush denied a call of the prompt in flight.
    denied: bool,
    /// The name and status of each of `agy`'s own calls of the prompt in
    /// flight, by their ids, which an update of a call need not repeat.
    steps: HashMap<String, (String, OpaqueStatus)>,
    /// What `agy` reported of the latest turn when it ended.
    turn: Option<TurnUsage>,
    /// Whether the latest turn ended with the model's answer, and nothing
    /// was given since: the session's database then holds a conversation the
    /// next process can go on with.
    settled: bool,
    /// Whether the model said anything of its request since the process was
    /// last given input or ran a call of its own: a `usage_update` before it
    /// may be a compaction's.
    answering: bool,
    /// The tokens of a `usage_update` that came before the model said
    /// anything, which the run checks for a compaction.
    unanswered_usage: Option<u64>,
    /// Where the session's directory goes when the process is closed.
    parking: Parking,
    /// How far the session's transcript was read.
    transcript: Transcript,
}

/// The session a process closed between turns left: its database, which
/// holds the session as the process ended it, and what the process held.
#[derive(Clone)]
pub(crate) struct Parked {
    session: String,
    database: Bytes,
    /// How many steps the database holds.
    steps: u64,
    system_prompt: String,
    sent: Vec<Vec<UserPart>>,
}

/// A runtime's kept session, the last one its processes left.
pub(crate) type Parking = Rc<RefCell<Option<Parked>>>;

impl Parked {
    /// Whether `request`, whose input starts at `input`, goes on from the
    /// session: the same system prompt, and its history holds the user
    /// messages the process received and none after them.
    fn continues(&self, request: &InferenceRequest, input: usize) -> bool {
        let history: Vec<&Vec<UserPart>> = user_messages(&request.items[..input]).collect();
        self.system_prompt == request.system_prompt
            && history.len() == self.sent.len()
            && history.iter().zip(&self.sent).all(|(message, sent)| *message == sent)
    }
}

/// How far a session's transcript was read: the bytes read, and the last step
/// whose line was taken.
#[derive(Default)]
struct Transcript {
    offset: u64,
    /// None until a line was taken: the steps of the history it was loaded
    /// with are below `first`.
    last: Option<u64>,
    first: u64,
}

/// A step's line of the session's transcript, as the run reads it.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct StepLine {
    step_index: u64,
    #[serde(rename = "type")]
    kind: String,
    input_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl StepLine {
    /// Whether it is the model's response to a request, which carries the
    /// request's tokens.
    pub(crate) fn is_response(&self) -> bool {
        self.kind == "PLANNER_RESPONSE"
    }

    /// Whether it is `agy`'s own compaction of the session.
    pub(crate) fn is_compaction(&self) -> bool {
        self.kind == "CHECKPOINT"
    }

    /// The request's tokens: uncached input, cache read and output, the
    /// thinking with it; none for a line that names no input.
    pub(crate) fn usage(&self) -> Option<jush_shared_types::TokenUsage> {
        Some(jush_shared_types::TokenUsage {
            input_tokens: self.input_tokens?,
            output_tokens: self.output_tokens.unwrap_or(0),
            cache_read_tokens: self.cache_read_tokens.unwrap_or(0),
            cache_write_tokens: 0,
        })
    }
}

/// The tokens of a turn, summed over its model requests, as `agy` reports
/// them when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TurnUsage {
    pub(crate) input: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
    /// Thinking included (measured).
    pub(crate) output: u64,
}

impl TurnUsage {
    /// What a `session/prompt` answer reports, if anything.
    fn of(result: &Value) -> Option<Self> {
        let usage = result.get("usage")?;
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        Some(Self {
            input: count("inputTokens"),
            cache_read: count("cachedReadTokens"),
            cache_write: count("cachedWriteTokens"),
            output: count("outputTokens"),
        })
    }
}

/// What a run reads of the process next.
pub(crate) enum Next {
    /// A piece of the model's text.
    Text(String),
    /// A piece of the model's reasoning; `opens` when it starts a block.
    Reasoning { opens: bool, text: String },
    /// The model called a tool, whose result the next run delivers.
    Called(ToolCall),
    /// A call of `agy`'s own tool started or changed.
    Step(OpaqueStep),
    /// The turn ended with the model's answer.
    Answered,
    /// The turn failed; the process is to be closed.
    Failed(ProviderFailure),
    /// The run was given up.
    Cancelled,
}

impl Live {
    /// Starts a process for `request` and gives it the request's input.
    pub(crate) async fn start(
        shared: &Arc<Shared>,
        placement: &dyn Placement,
        request: &InferenceRequest,
        parking: &Parking,
    ) -> Result<Self, ProviderFailure> {
        let work = shared.auth.work().await.map_err(|failure| failure.failure())?;
        let input = input_start(&request.items).ok_or_else(no_input)?;
        // A start given up leaves the session kept; one that failed after
        // taking it lets it go, so that the next writes the history instead.
        let parked = (input > 0)
            .then(|| parking.borrow().clone())
            .flatten()
            .filter(|parked| parked.continues(request, input));
        let resumes = parked.is_some();
        let session = match parked {
            Some(parked) => Some((parked.session, parked.database, parked.steps)),
            None if input == 0 => None,
            None => Some(make_session(shared, placement, work.clone(), request, input).await?),
        };
        let started = Self::open(shared, placement, request, parking, work, input, session).await;
        if started.is_ok() || resumes {
            parking.borrow_mut().take();
        }
        started
    }

    /// Starts the turn's process with `session`, the id, database and step
    /// count of the session it loads, if any, and gives it the request's
    /// input, which starts at `input`.
    async fn open(
        shared: &Arc<Shared>,
        placement: &dyn Placement,
        request: &InferenceRequest,
        parking: &Parking,
        work: CliWork,
        input: usize,
        session: Option<(String, Bytes, u64)>,
    ) -> Result<Self, ProviderFailure> {
        let loaded = session.as_ref().map(|(id, _, _)| id.clone());
        let first = session.as_ref().map_or(0, |(_, _, steps)| *steps);
        let system_prompt = request.system_prompt.clone();
        let start: CliStarter = Arc::new(move |site| {
            let session = session.as_ref().map(|(id, database, _)| (id.as_str(), database.clone()));
            start::turn(site, &system_prompt, session)
        });
        let Placed {
            process,
            companion,
            config,
            site,
            ..
        } = placement
            .start(work, start, &|_| {})
            .await
            .map_err(|error| ProviderFailure::no_answer(error.0))?;
        let Process {
            output,
            control,
            exit,
        } = process;
        let exit = exit.shared();
        let held = HeldProcess::of(control, exit.clone(), Some(config));
        let companion = companion.ok_or_else(|| {
            ProviderFailure::no_answer("Antigravity's MCP channel was not started")
        })?;
        let mut live = Self {
            held,
            exit,
            output: Output::new(output),
            channel: Channel::start(companion, request.tools.clone()),
            home: start::home(&site),
            files: media::Files::default(),
            session: String::new(),
            node: request.session_id.clone(),
            system_prompt: request.system_prompt.clone(),
            model: None,
            sent: Vec::new(),
            held_call: None,
            prompting: None,
            next_id: acp::FIRST_FREE,
            context: None,
            thinking: false,
            warning: None,
            after: None,
            denied: false,
            steps: HashMap::new(),
            turn: None,
            settled: false,
            answering: false,
            unanswered_usage: None,
            parking: parking.clone(),
            transcript: Transcript {
                first,
                ..Transcript::default()
            },
        };
        let opened = acp::open(
            live.held.control(),
            &mut live.output,
            &start::workspace(&site),
            loaded.as_deref(),
        )
        .await;
        let opened = answered(shared, opened).await?;
        live.session = match loaded {
            Some(id) => id,
            None => session_id(&opened)?,
        };
        // What `agy` wrote of the history it loaded is not the run's.
        live.transcript.offset = live.transcript_size().await.unwrap_or(0);
        live.sent = user_messages(&request.items[..input]).cloned().collect();
        // The prompt waits for `agy` to have Jush's tools, which it asks for
        // once the session is made: a turn takes the tools it has as it starts.
        let exited = tokio::select! {
            listed = live.channel.listed() => {
                listed.map_err(failure)?;
                false
            }
            _ = live.exit.clone() => true,
        };
        if exited {
            // What `agy` wrote last says why it ended.
            while live.output.line().await.is_some() {}
            return Err(live.ended().await);
        }
        live.prompt(shared, request).await?;
        Ok(live)
    }

    /// Whether the process can go on with `request`: it still runs, with the
    /// same system prompt, the transcript continues what it received, and the
    /// request gives it its input, the result of the call it holds or new user
    /// messages after its turn's end.
    pub(crate) fn serves(&self, request: &InferenceRequest) -> bool {
        let running = self.exit.clone().now_or_never().is_none();
        let messages: Vec<&Vec<UserPart>> = user_messages(&request.items).collect();
        let continues = messages.len() >= self.sent.len()
            && messages.iter().zip(&self.sent).all(|(message, sent)| *message == sent);
        let gains = match &self.held_call {
            Some(call) => {
                messages.len() == self.sent.len()
                    && result_of(&request.items, &call.tool_use_id).is_some()
            }
            None => {
                self.prompting.is_none()
                    && messages.len() > self.sent.len()
                    && input_start(&request.items).is_some()
            }
        };
        running && self.system_prompt == request.system_prompt && continues && gains
    }

    /// Gives the process `request`'s input: the held call's result, or the
    /// new user messages as the next prompt.
    pub(crate) async fn give(
        &mut self,
        shared: &Arc<Shared>,
        request: &InferenceRequest,
    ) -> Result<(), ProviderFailure> {
        self.channel.offer(request.tools.clone());
        self.settled = false;
        self.answering = false;
        match self.held_call.take() {
            Some(call) => {
                let (output, is_error) = result_of(&request.items, &call.tool_use_id)
                    .ok_or_else(|| failure(format!("The result of the call {} is missing", call.tool_use_id)))?;
                let output = self.files.result(own_dir(&self.held)?, &self.home, output).await?;
                self.channel
                    .deliver(vec![(call.tool_use_id, tool_result(&output, is_error))]);
                Ok(())
            }
            None => self.prompt(shared, request).await,
        }
    }

    /// Sends the new user messages of `request` as a prompt, in the model
    /// and at the level the request asks for.
    async fn prompt(
        &mut self,
        shared: &Arc<Shared>,
        request: &InferenceRequest,
    ) -> Result<(), ProviderFailure> {
        let model = agy_model(request);
        if self.model.as_ref() != Some(&model) {
            let id = self.id();
            self.ask(shared, id, acp::set_model(id, &self.session, &model)).await?;
            self.model = Some(model);
        }
        let messages: Vec<Vec<UserPart>> = user_messages(&request.items)
            .skip(self.sent.len())
            .cloned()
            .collect();
        let mut blocks: Vec<Value> = Vec::new();
        for content in &messages {
            let content = self.files.message(own_dir(&self.held)?, &self.home, content).await?;
            blocks.extend(prompt_blocks(&content));
        }
        let id = self.id();
        let line = acp::prompt(id, &self.session, blocks);
        self.write(line).await?;
        self.sent.extend(messages);
        self.prompting = Some(Prompting {
            id,
            model_id: request.model_id.clone(),
            sent_at: shared.clock.now(),
        });
        self.thinking = false;
        self.warning = None;
        self.denied = false;
        self.steps.clear();
        self.turn = None;
        self.settled = false;
        Ok(())
    }

    /// What the process does next for the run.
    pub(crate) async fn next(&mut self, shared: &Arc<Shared>, cancel: &CancellationToken) -> Next {
        if let Some(next) = self.after.take() {
            return next;
        }
        match self.event(shared, cancel).await {
            Next::Text(text) => Next::Text(text),
            // `agy` words a failure as the turn's last piece of text, so one
            // held when anything else follows was the model's.
            next => match self.warning.take() {
                Some(held) => {
                    self.after = Some(next);
                    Next::Text(held)
                }
                None => next,
            },
        }
    }

    /// The next event of the process for the run.
    async fn event(&mut self, shared: &Arc<Shared>, cancel: &CancellationToken) -> Next {
        loop {
            let line = tokio::select! {
                biased;
                () = cancel.cancelled() => return Next::Cancelled,
                event = self.channel.next() => match event {
                    ChannelEvent::Opened(call) => {
                        self.held_call = Some(call.clone());
                        return Next::Called(call);
                    }
                    ChannelEvent::Ended(reason) => return Next::Failed(failure(reason)),
                },
                line = self.output.line() => line,
            };
            let Some(line) = line else {
                return Next::Failed(self.ended().await);
            };
            match line {
                Line::Notification { method, params } if method == "session/update" => {
                    if let Some(next) = self.update(&params) {
                        return next;
                    }
                    if let Some(used) = self.unanswered_usage.take() {
                        if let Some(prompt) = &self.prompting
                            && self.compacted(used, &prompt.model_id).await
                        {
                            let limit = models::agy_limit(&prompt.model_id);
                            tracing::warn!(
                                event = "antigravity.compaction_refused",
                                node = %self.node,
                                model = %prompt.model_id,
                                tokens = used,
                                limit,
                                "agy compacted the conversation by itself; the run refuses it as too long"
                            );
                            return Next::Failed(over_limit(used, limit));
                        }
                        self.context = Some(used);
                    }
                }
                Line::Notification { .. } => {}
                Line::Request { id, method, params } => {
                    let line = if method == "session/request_permission" {
                        let allow = tokio::select! {
                            () = cancel.cancelled() => return Next::Cancelled,
                            allow = allows(self.held.dir(), &params["toolCall"]) => allow,
                        };
                        let (line, allowed) = acp::permission(id, &params, allow);
                        self.denied |= !allowed;
                        line
                    } else {
                        acp::refuse(id)
                    };
                    if let Err(failure) = self.write(line).await {
                        return Next::Failed(failure);
                    }
                }
                Line::Answer { id, answer } => {
                    if let Some(prompt) = self.prompting.take_if(|prompt| id == Value::from(prompt.id)) {
                        return self.answered(shared, &prompt, answer).await;
                    }
                }
            }
        }
    }

    /// What an update of the session says to the run, if anything.
    fn update(&mut self, params: &Value) -> Option<Next> {
        let update = params.get("update")?;
        let text = || {
            update
                .get("content")
                .and_then(|content| content.get("text"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        match update.get("sessionUpdate")?.as_str()? {
            "agent_message_chunk" => {
                let text = text()?;
                self.thinking = false;
                self.answering = true;
                if failure::warns(&text) {
                    return self.warning.replace(text).map(Next::Text);
                }
                Some(Next::Text(match self.warning.take() {
                    Some(held) => held + &text,
                    None => text,
                }))
            }
            "agent_thought_chunk" => {
                let text = text()?;
                self.answering = true;
                let opens = !std::mem::replace(&mut self.thinking, true);
                Some(Next::Reasoning { opens, text })
            }
            "usage_update" => {
                let used = update.get("used").and_then(Value::as_u64);
                // `agy` reports the tokens of the request that summarizes the
                // session as soon as it wrote the summary, before the model
                // says anything of the request it compacted for (measured).
                match used {
                    Some(used) if !self.answering && self.prompting.is_some() => {
                        self.unanswered_usage = Some(used);
                    }
                    _ => self.context = used.or(self.context),
                }
                None
            }
            "tool_call" | "tool_call_update" => {
                let step = self.step(update)?;
                self.thinking = false;
                // A call of `agy`'s own that ended starts the model's next
                // request.
                self.answering = step.status == OpaqueStatus::Running;
                Some(Next::Step(step))
            }
            // `agy`'s title of the session and the settings it offers.
            _ => None,
        }
    }

    /// The step a call update of `agy`'s own tool says; none for a call of
    /// Jush's MCP server, which the channel brings.
    fn step(&mut self, update: &Value) -> Option<OpaqueStep> {
        if own_call(update) {
            return None;
        }
        let id = update.get("toolCallId")?.as_str()?.to_owned();
        let known = self.steps.get(&id).cloned();
        let name = match update.get("name").and_then(Value::as_str) {
            Some(name) => name.to_owned(),
            None => known.as_ref()?.0.clone(),
        };
        // An update names only what changed, so one without a status keeps
        // the step's.
        let status = match update.get("status").and_then(Value::as_str) {
            Some("completed") => OpaqueStatus::Completed,
            Some("failed") => OpaqueStatus::Failed,
            Some(_) => OpaqueStatus::Running,
            None => known.map_or(OpaqueStatus::Running, |(_, status)| status),
        };
        self.steps.insert(id.clone(), (name.clone(), status));
        Some(OpaqueStep {
            call_id: Some(id),
            status,
            view: OpaqueView::Other {
                vendor: CLI.name.to_owned(),
                name,
            },
            entries: Vec::new(),
        })
    }

    /// The end of the turn, by the prompt's answer and, after a piece of text
    /// that words a failure, the session's last step: an error step fails the
    /// turn, and any other step gives the piece as the model's text first.
    async fn answered(&mut self, shared: &Arc<Shared>, prompt: &Prompting, answer: Result<Value, Refusal>) -> Next {
        let Some(warning) = self.warning.take() else {
            return self.turn_end(shared, answer).await;
        };
        let reported = match self.last_step().await {
            Ok(Some((kind, payload))) => failure::reported(kind, &payload, &warning),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(event = "antigravity.last_step_unread", node = %self.node, %error, "a session's last step was not read");
                Some(Reported::unread(&warning))
            }
        };
        let Some(reported) = reported else {
            tracing::info!(event = "antigravity.warning_was_text", node = %self.node, "a piece of text worded as agy words a failure ended a turn whose last step is no error");
            self.after = Some(self.turn_end(shared, answer).await);
            return Next::Text(warning);
        };
        if reported.signs_out() {
            shared.auth.signed_out().await;
        }
        if reported.exhausted() && quota_left(shared, prompt).await {
            return Next::Failed(reported.refused_with_quota_left());
        }
        Next::Failed(reported.failure())
    }

    /// The type and payload of the session's last step, read from `agy`'s
    /// database and its write-ahead log, which holds what `agy` has not
    /// checkpointed yet.
    async fn last_step(&self) -> Result<Option<(u64, Vec<u8>)>, String> {
        let dir = own_dir(&self.held).map_err(|failure| failure.message)?;
        let name = format!("{CONVERSATIONS}/{}.db", self.session);
        let database = dir.read(&name).await.map_err(|error| error.to_string())?;
        // A log `agy` checkpointed and removed holds nothing more.
        let wal = match dir.read(&format!("{name}-wal")).await {
            Ok(wal) => Some(wal),
            Err(error) if error.code() == Some("ENOENT") => None,
            Err(error) => return Err(error.to_string()),
        };
        tokio::task::spawn_blocking(move || history::last_step(&database, wal.as_deref()))
            .await
            .map_err(|error| error.to_string())?
    }

    /// The end of the turn by the prompt's answer alone.
    async fn turn_end(&mut self, shared: &Arc<Shared>, answer: Result<Value, Refusal>) -> Next {
        let result = match answer {
            Ok(result) => result,
            Err(refusal) => return Next::Failed(refused(shared, refusal).await),
        };
        self.turn = TurnUsage::of(&result);
        match result.get("stopReason").and_then(Value::as_str) {
            Some("end_turn") if self.denied => Next::Failed(failure(
                "Antigravity ended the turn because Jush did not let its model read a file outside the tool results it saved."
                    .to_owned(),
            )),
            Some("end_turn") => {
                self.settled = true;
                Next::Answered
            }
            Some("max_tokens" | "max_turn_requests") => Next::Failed(ProviderFailure::ended_early(
                jush_provider_common::EarlyEnd::OutputLimit,
                "max_tokens",
                result.to_string(),
            )),
            Some("refusal") => Next::Failed(ProviderFailure::ended_early(
                jush_provider_common::EarlyEnd::Filter,
                "refusal",
                result.to_string(),
            )),
            _ => Next::Failed(ProviderFailure::protocol(
                "Antigravity ended its turn for a reason Jush does not know",
                result.to_string(),
            )),
        }
    }

    /// The tokens the session holds after its latest model request.
    pub(crate) fn context(&self) -> Option<u64> {
        self.context
    }

    /// What `agy` reported of the latest turn when it ended.
    pub(crate) fn turn(&self) -> Option<TurnUsage> {
        self.turn
    }

    /// Closes the process, SIGTERM first, and waits for its end. A process
    /// between turns leaves its directory to its runtime, for the next
    /// process to load the session from.
    pub(crate) async fn close(mut self) {
        let between_turns = self.settled && self.held_call.is_none() && self.prompting.is_none();
        if !between_turns {
            self.held.close().await;
            return;
        }
        // The directory goes once its database was read.
        let Some(dir) = self.held.close_keeping_dir().await else {
            return;
        };
        match left_database(&*dir, &self.session).await {
            Ok((database, steps)) => {
                *self.parking.borrow_mut() = Some(Parked {
                    session: self.session,
                    database,
                    steps,
                    system_prompt: self.system_prompt,
                    sent: self.sent,
                });
            }
            Err(error) => {
                tracing::warn!(event = "antigravity.left_session_unread", node = %self.node, %error, "the session a closed process left was not read; the next process writes its history");
            }
        }
        dir.remove().await;
    }

    /// The lines `agy` added to the session's transcript since the last read,
    /// of the steps the process took; none when it cannot be read, which is
    /// logged.
    pub(crate) async fn transcript(&mut self) -> Option<Vec<StepLine>> {
        match self.read_transcript().await {
            Ok(lines) => Some(lines),
            Err(error) => {
                tracing::warn!(event = "antigravity.transcript_unread", node = %self.node, %error, "a session's transcript was not read");
                None
            }
        }
    }

    async fn read_transcript(&mut self) -> Result<Vec<StepLine>, String> {
        let (read, lines) = self.new_lines().await?;
        self.transcript.offset += read;
        if let Some(line) = lines.last() {
            self.transcript.last = Some(line.step_index);
        }
        Ok(lines)
    }

    /// The whole lines `agy` added to the session's transcript since the last
    /// read, of steps after the last one taken, and the bytes they span,
    /// without taking them.
    async fn new_lines(&self) -> Result<(u64, Vec<StepLine>), String> {
        let dir = own_dir(&self.held).map_err(|failure| failure.message)?;
        let path = transcript_path(&self.session);
        let from = (self.transcript.offset + 1).to_string();
        let read = dir.output("tail", &["-c", &format!("+{from}"), &path]).await?;
        // Only whole lines: one being written is read whole next time.
        let whole = read.iter().rposition(|byte| *byte == b'\n').map_or(0, |end| end + 1);
        let mut after = self.transcript.last.map_or(self.transcript.first, |last| last + 1);
        let mut lines = Vec::new();
        for text in read[..whole].split(|byte| *byte == b'\n').filter(|text| !text.is_empty()) {
            let Ok(line) = serde_json::from_slice::<StepLine>(text) else {
                continue;
            };
            if line.step_index < after {
                continue;
            }
            after = line.step_index + 1;
            lines.push(line);
        }
        Ok((whole as u64, lines))
    }

    /// Whether `agy` compacted the session for the request in flight, which
    /// reported `used` tokens before the model said anything of it: the
    /// tokens at or over the model's limit, or a compaction among the
    /// transcript's new lines, which also shows one under a limit `agy` moved.
    async fn compacted(&self, used: u64, model_id: &str) -> bool {
        if used >= u64::from(models::agy_limit(model_id)) {
            return true;
        }
        match self.new_lines().await {
            Ok((_, lines)) => lines.iter().any(StepLine::is_compaction),
            Err(error) => {
                tracing::warn!(event = "antigravity.transcript_unread", node = %self.node, %error, "a session's transcript was not read");
                false
            }
        }
    }

    /// The size of the session's transcript now; none when it has none yet.
    async fn transcript_size(&self) -> Option<u64> {
        let dir = self.held.dir()?;
        let size = dir.output("wc", &["-c", &transcript_path(&self.session)]).await.ok()?;
        std::str::from_utf8(&size).ok()?.split_whitespace().next()?.parse().ok()
    }

    /// Writes request `line`, of id `id`, and reads until its answer.
    async fn ask(&mut self, shared: &Arc<Shared>, id: u64, line: Bytes) -> Result<Value, ProviderFailure> {
        let asked = acp::ask(self.held.control(), &mut self.output, id, line).await;
        answered(shared, asked).await
    }

    /// The failure of a process whose output ended: what it wrote to its
    /// standard error last, else how it ended.
    async fn ended(&self) -> ProviderFailure {
        let end = self.exit.clone().await;
        let unasked = self.channel.unasked();
        if !unasked.is_empty() {
            return failure(format!(
                "Antigravity exited before asking for the results of {}",
                unasked.join(", ")
            ));
        }
        let stderr = self.output.stderr();
        if matches!(end, ProcessEnd::Lost(_)) || stderr.is_empty() {
            return ProviderFailure::no_answer(exit_message(&CLI, &end));
        }
        failure(stderr)
    }

    async fn write(&self, line: Bytes) -> Result<(), ProviderFailure> {
        self.held
            .control()
            .write_stdin(line)
            .await
            .map_err(|error| ProviderFailure::no_answer(format!("Antigravity's input could not be written: {error}")))
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

impl KeptProcess for Live {
    fn node(&self) -> String {
        self.node.clone()
    }

    fn close(self) -> LocalBoxFuture<'static, ()> {
        Box::pin(Live::close(self))
    }
}

/// The transcript of the session `session`, in its process's directory.
fn transcript_path(session: &str) -> String {
    format!(".gemini/antigravity-cli/brain/{session}/.system_generated/logs/transcript.jsonl")
}

/// The database of the session `session` a closed process left in its
/// directory `dir`, as one file, and how many steps it holds.
async fn left_database(dir: &dyn ConfigDir, session: &str) -> Result<(Bytes, u64), String> {
    let name = format!("{CONVERSATIONS}/{session}.db");
    let database = dir.read(&name).await.map_err(|error| error.to_string())?;
    let wal = match dir.read(&format!("{name}-wal")).await {
        Ok(wal) => Some(wal),
        Err(error) if error.code() == Some("ENOENT") => None,
        Err(error) => return Err(error.to_string()),
    };
    tokio::task::spawn_blocking(move || history::settled(&database, wal.as_deref()))
        .await
        .map_err(|error| error.to_string())?
}

/// Makes a session of Jush's agent, with the request's system prompt, in a
/// process of its own, and writes the history before `input` into its
/// database: the session's id, the filled database and how many steps it
/// holds.
async fn make_session(
    shared: &Arc<Shared>,
    placement: &dyn Placement,
    work: CliWork,
    request: &InferenceRequest,
    input: usize,
) -> Result<(String, Bytes, u64), ProviderFailure> {
    // A history `agy` would compact before the first request is refused
    // before `agy` spends a summary on it.
    let limit = models::agy_limit(&request.model_id);
    let estimate = text_tokens(&request.system_prompt) + history::tokens(&request.items);
    if estimate >= u64::from(limit) {
        tracing::warn!(
            event = "antigravity.history_refused",
            node = %request.session_id,
            model = %request.model_id,
            tokens = estimate,
            limit,
            "a history over agy's limit was refused as too long before it was written"
        );
        return Err(over_limit(estimate, limit));
    }
    let system_prompt = request.system_prompt.clone();
    let Placed {
        process,
        config,
        site,
        ..
    } = placement
        .start(work, Arc::new(move |site| start::maker(site, &system_prompt)), &|_| {})
        .await
        .map_err(|error| ProviderFailure::no_answer(error.0))?;
    let Process {
        output,
        control,
        exit,
    } = process;
    let exit = exit.shared();
    let mut held = HeldProcess::of(control, exit.clone(), Some(config));
    let mut output = Output::new(output);
    let made = async {
        let opened = acp::open(held.control(), &mut output, &start::workspace(&site), None).await;
        let session = session_id(&answered(shared, opened).await?)?;
        held.control().close_stdin().await.map_err(|error| {
            ProviderFailure::no_answer(format!("Antigravity's input could not be written: {error}"))
        })?;
        tokio::time::timeout(MAKER_END, exit.clone())
            .await
            .map_err(|_| ProviderFailure::no_answer("Antigravity did not end after making the session"))?;
        let dir = held
            .dir()
            .ok_or_else(|| failure("Antigravity's session has no directory".to_owned()))?;
        let database = dir
            .read(&format!("{CONVERSATIONS}/{session}.db"))
            .await
            .map_err(|error| ProviderFailure::no_answer(format!("Antigravity's session could not be read: {error}")))?;
        Ok::<_, ProviderFailure>((session, database))
    };
    let made = made.await;
    held.close().await;
    let (session, database) = made?;
    let items = request.items.clone();
    let (filled, steps) = encode_body(CLI.name, move || history::fill(&database, &items[..input]))
        .await?
        .map_err(failure)?;
    Ok((session, filled, steps))
}

/// The refusal of a history of `tokens`, at or over `limit`, the tokens at
/// which `agy` compacts by itself: the agent compacts and asks again, so the
/// conversation keeps one history, Jush's.
fn over_limit(tokens: u64, limit: u32) -> ProviderFailure {
    ProviderFailure {
        message: format!(
            "The conversation has about {tokens} tokens, at or over the {limit} at which Antigravity summarizes it by itself."
        ),
        code: Some(ErrorCode::ContextLengthExceeded),
        diagnostics: None,
        retry_after: None,
    }
}

/// The directory of the process `held`, which every turn's process has.
fn own_dir(held: &HeldProcess) -> Result<&dyn ConfigDir, ProviderFailure> {
    held.dir()
        .ok_or_else(|| failure("Antigravity's process has no directory".to_owned()))
}

/// Whether `call`, a call as `agy` describes it, is of Jush's MCP server.
fn own_call(call: &Value) -> bool {
    call.get("name")
        .and_then(Value::as_str)
        .is_some_and(tools::is_server_tool)
}

/// Whether Jush allows the call `call` that `agy` asks about: one of its own
/// MCP server, or a `read_file` of paths where nothing is on the machine of
/// the process's directory `dir`, which reveals nothing of the machine.
async fn allows(dir: Option<&dyn ConfigDir>, call: &Value) -> bool {
    if own_call(call) {
        return true;
    }
    if call.get("name").and_then(Value::as_str) != Some("read_file") {
        return false;
    }
    let (Some(dir), Some(paths)) = (dir, read_paths(call)) else {
        return false;
    };
    for path in paths {
        if !matches!(dir.exists(path).await, Ok(false)) {
            return false;
        }
    }
    true
}

/// The paths a `read_file` call names: the model's, and each `agy` resolved,
/// the file it reads, with `..` and every link, `/proc/self` among them,
/// resolved (measured). None when it names no resolved path, which is the
/// one that counts, or one is not an absolute path of the machine, which
/// runs Linux.
fn read_paths(call: &Value) -> Option<Vec<&str>> {
    let mut paths = vec![call.get("rawInput")?.get("Path")?.as_str()?];
    for location in call.get("locations")?.as_array()? {
        paths.push(location.get("path")?.as_str()?);
    }
    (paths.len() > 1 && paths.iter().all(|path| path.starts_with('/'))).then_some(paths)
}

/// A prompt in flight: its request's id, the model it runs, by Jush's id,
/// and when it was sent.
struct Prompting {
    id: u64,
    model_id: String,
    sent_at: Timestamp,
}

/// Whether a usage read shows quota left in every limit of `prompt`'s model,
/// its group's. A read that fails, or reports no limit of the model, shows
/// none, so the account counts as used up, as Google said.
async fn quota_left(shared: &Arc<Shared>, prompt: &Prompting) -> bool {
    // Google refuses a request as it arrives, so a read that began after the
    // prompt was sent sees the account at least as used as Google did. A
    // probe answers the read of another request of the account when one is
    // running, which may have begun before this prompt was sent and says
    // nothing of it; then it reads again. A read that begins after that one
    // ended began after the prompt was sent, so two reads always suffice.
    for _ in 0..2 {
        let Ok(snapshot) = shared.quota.probe(ProbeAsk::User).await else {
            return false;
        };
        let read: Vec<_> = snapshot
            .windows
            .iter()
            .filter(|window| window.requested_at.is_some_and(|at| at >= prompt.sent_at))
            .collect();
        if read.is_empty() {
            continue;
        }
        let limits: Vec<_> = read
            .into_iter()
            .filter(|window| {
                window
                    .scope
                    .as_ref()
                    .is_some_and(|scope| scope.model_ids.iter().any(|id| *id == prompt.model_id))
            })
            .collect();
        return !limits.is_empty()
            && limits.iter().all(|window| window.used_percent.is_some_and(|used| used < 100.0));
    }
    false
}

/// What `agy` answered `asked`, or the run's failure where it did not.
async fn answered(
    shared: &Arc<Shared>,
    asked: Result<Result<Value, Refusal>, Unanswered>,
) -> Result<Value, ProviderFailure> {
    match asked {
        Ok(Ok(answer)) => Ok(answer),
        Ok(Err(refusal)) => Err(refused(shared, refusal).await),
        Err(Unanswered::Ended(stderr)) if stderr.is_empty() => {
            Err(ProviderFailure::no_answer("Antigravity ended before it answered"))
        }
        Err(Unanswered::Ended(stderr)) => Err(failure(stderr)),
        Err(Unanswered::Failed(message)) => Err(ProviderFailure::no_answer(message)),
    }
}

/// The failure of a request `agy` refused; a refusal for the sign-in signs
/// the account out.
async fn refused(shared: &Arc<Shared>, refusal: Refusal) -> ProviderFailure {
    match shared.auth.refused(&refusal).await {
        Some(signed_out) => signed_out.failure(),
        None => failure(format!("Antigravity refused the request: {}", refusal.message)),
    }
}

/// The id of the session `session/new` answered.
fn session_id(answered: &Value) -> Result<String, ProviderFailure> {
    answered
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ProviderFailure::protocol("Antigravity's new session has no id", answered.to_string()))
}

/// Where the input of `items` starts: the user messages they end with. None
/// when they end with anything else, which `agy` cannot answer: it takes no
/// prompt without a message.
fn input_start(items: &[InferenceItem]) -> Option<usize> {
    let start = items
        .iter()
        .rposition(|item| !matches!(item, InferenceItem::UserMessage { .. } | InferenceItem::UserSteer { .. }))
        .map_or(0, |last| last + 1);
    (start < items.len()).then_some(start)
}

/// The failure of a request that ends without a message for a new process.
fn no_input() -> ProviderFailure {
    failure(
        "Antigravity cannot go on with a turn its process no longer holds; send a message to go on"
            .to_owned(),
    )
}

/// The user messages and steers of `items`, in order.
fn user_messages(items: &[InferenceItem]) -> impl Iterator<Item = &Vec<UserPart>> {
    items.iter().filter_map(|item| match item {
        InferenceItem::UserMessage { content } | InferenceItem::UserSteer { content } => Some(content),
        _ => None,
    })
}

/// The latest result the transcript holds for `tool_use_id`.
fn result_of<'a>(items: &'a [InferenceItem], tool_use_id: &str) -> Option<(&'a [ResultPart], bool)> {
    items.iter().rev().find_map(|item| match item {
        InferenceItem::ToolResult {
            tool_use_id: id,
            output,
            is_error,
        } if id == tool_use_id => Some((output.as_slice(), *is_error)),
        _ => None,
    })
}

/// The model `request` asks for at its level, as `agy` names it, such as
/// `gemini-4-neon-high`.
fn agy_model(request: &InferenceRequest) -> String {
    match reasoning_effort(request.thinking.as_ref()) {
        Some(level) => format!("{}-{level}", request.model_id),
        None => request.model_id.clone(),
    }
}

/// A user message as ACP content: text, and an image with its bytes; any
/// other medium is named in text, a file in the workspace once it is one
/// (`media.rs`).
fn prompt_blocks(content: &[UserPart]) -> Vec<Value> {
    content
        .iter()
        .map(|part| match part {
            UserPart::Text(text) => json!({ "type": "text", "text": text }),
            UserPart::Image(Medium::Bytes(bytes)) => json!({
                "type": "image",
                "data": STANDARD.encode(&bytes.data),
                "mimeType": bytes.media_type,
            }),
            other => json!({ "type": "text", "text": history::part_text(other) }),
        })
        .collect()
}

/// A failure Jush found in what `agy` did, which no retry fixes.
fn failure(message: String) -> ProviderFailure {
    ProviderFailure {
        message,
        code: None,
        diagnostics: None,
        retry_after: None,
    }
}
