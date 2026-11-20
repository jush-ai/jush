//! A session's `agy` process, which a run reads (`run.rs`) and the runtime
//! keeps between its turns.
//!
//! A new process holds the request's history before it answers the request's
//! input, the user messages the transcript ends with (`history.rs`):
//!
//! - with no history, as in a conversation's first turn, the process makes
//!   the session itself;
//! - with history, a first process makes the session with Jush's agent and
//!   ends, the provider writes the history into the session's database, and
//!   the turn's process loads it.
//!
//! The process answers the input in one prompt, `session/prompt`. The model's
//! text and reasoning arrive as `session/update`s, and a call of a tool over
//! the MCP channel (`tools.rs`). A call ends the run and keeps the process:
//! its turn waits for the call's result, which the next run delivers. The
//! prompt's answer ends the turn.
//!
//! A kept process goes on with a later request only where it continues what
//! the process received, with the same system prompt: new user messages
//! after a turn's end, or the result of the call it holds. `agy` takes input
//! only between turns, so a steer that comes with a result is answered by a
//! new process, whose history holds the result.

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
    InferenceItem, InferenceRequest, Medium, ProviderFailure, ResultPart, ToolCall, UserPart,
    encode_body,
};
use jush_provider_subscription_cli::{
    CliStarter, CliWork, HeldProcess, KeptProcess, Placed, Placement, Unanswered, exit_message,
    tool_result,
};
use jush_shared_types::Timestamp;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::acp::{self, Line, Output, Refusal};
use crate::failure::{self, Reported};
use crate::start::{self, CONVERSATIONS};
use crate::tools::{Channel, ChannelEvent};
use crate::{CLI, Shared, history};

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
    /// The tokens the session holds after its latest call, as `agy` reports
    /// them.
    context: Option<u64>,
    /// Whether the piece being streamed is reasoning.
    thinking: bool,
    /// The failure `agy` reported in the turn's text.
    reported: Option<Reported>,
}

/// What a run reads of the process next.
pub(crate) enum Next {
    /// A piece of the model's text.
    Text(String),
    /// A piece of the model's reasoning; `opens` when it starts a block.
    Reasoning { opens: bool, text: String },
    /// The model called a tool, whose result the next run delivers.
    Called(ToolCall),
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
    ) -> Result<Self, ProviderFailure> {
        let work = shared.auth.work().await.map_err(|failure| failure.failure())?;
        let input = input_start(&request.items).ok_or_else(no_input)?;
        let session = if input == 0 {
            None
        } else {
            Some(make_session(shared, placement, work.clone(), request, input).await?)
        };
        let loaded = session.as_ref().map(|(id, _)| id.clone());
        let system_prompt = request.system_prompt.clone();
        let start: CliStarter = Arc::new(move |site| {
            let session = session.as_ref().map(|(id, database)| (id.as_str(), database.clone()));
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
            reported: None,
        };
        let opened = acp::open(
            live.held.control(),
            &mut live.output,
            &start::home(&site),
            loaded.as_deref(),
        )
        .await;
        let opened = answered(shared, opened).await?;
        live.session = match loaded {
            Some(id) => id,
            None => session_id(&opened)?,
        };
        live.sent = user_messages(&request.items[..input]).cloned().collect();
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
        match self.held_call.take() {
            Some(call) => {
                let (output, is_error) = result_of(&request.items, &call.tool_use_id)
                    .ok_or_else(|| failure(format!("The result of the call {} is missing", call.tool_use_id)))?;
                self.channel
                    .deliver(vec![(call.tool_use_id, tool_result(output, is_error))]);
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
        let blocks: Vec<Value> = messages.iter().flat_map(|content| prompt_blocks(content)).collect();
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
        self.reported = None;
        Ok(())
    }

    /// What the process does next for the run.
    pub(crate) async fn next(&mut self, shared: &Arc<Shared>, cancel: &CancellationToken) -> Next {
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
                }
                Line::Notification { .. } => {}
                Line::Request { id, .. } => {
                    if let Err(failure) = self.write(acp::refuse(id)).await {
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
                if let Some(reported) = failure::reported(&text) {
                    self.reported = Some(reported);
                    return None;
                }
                self.thinking = false;
                Some(Next::Text(text))
            }
            "agent_thought_chunk" => {
                let text = text()?;
                let opens = !std::mem::replace(&mut self.thinking, true);
                Some(Next::Reasoning { opens, text })
            }
            "usage_update" => {
                self.context = update.get("used").and_then(Value::as_u64).or(self.context);
                None
            }
            // `agy`'s record of the MCP calls the channel brings, its title
            // of the session, and the settings it offers.
            _ => None,
        }
    }

    /// The end of the turn, by the prompt's answer.
    async fn answered(&mut self, shared: &Arc<Shared>, prompt: &Prompting, answer: Result<Value, Refusal>) -> Next {
        if let Some(reported) = self.reported.take() {
            if reported.signs_out() {
                shared.auth.signed_out().await;
            }
            if reported.exhausted() && quota_left(shared, prompt).await {
                return Next::Failed(reported.refused_with_quota_left());
            }
            return Next::Failed(reported.failure());
        }
        let result = match answer {
            Ok(result) => result,
            Err(refusal) => return Next::Failed(refused(shared, refusal).await),
        };
        match result.get("stopReason").and_then(Value::as_str) {
            Some("end_turn") => Next::Answered,
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

    /// The tokens the session holds after its latest call.
    pub(crate) fn context(&self) -> Option<u64> {
        self.context
    }

    /// Closes the process, SIGTERM first, and waits for its end.
    pub(crate) async fn close(mut self) {
        self.held.close().await;
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

/// Makes a session of Jush's agent, with the request's system prompt, in a
/// process of its own, and writes the history before `input` into its
/// database: the session's id and the filled database.
async fn make_session(
    shared: &Arc<Shared>,
    placement: &dyn Placement,
    work: CliWork,
    request: &InferenceRequest,
    input: usize,
) -> Result<(String, Bytes), ProviderFailure> {
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
        let opened = acp::open(held.control(), &mut output, &start::home(&site), None).await;
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
    let filled = encode_body(CLI.name, move || history::fill(&database, &items[..input])).await?;
    Ok((session, filled.map_err(failure)?))
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
/// other medium is named in text.
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
