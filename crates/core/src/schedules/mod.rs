//! The schedules of the user's conversations in their core: what a
//! conversation's entry lists and `jush schedule` and a program read and
//! change, the scheduler that owns every trigger, a run from its trigger to
//! its turn's end, the change watches, and the calls of the URL triggers.
//!
//! A schedule belongs to one conversation and wakes only it; the conversation's
//! deletion deletes it, and while the conversation is archived it wakes nothing,
//! which is read from the conversation's archived state and never written to
//! the schedule's own `paused`.
//!
//! A schedule may end. From its end nothing fires it, and a run of its end
//! tells the conversation so, after the schedule's other runs, and deletes
//! it; one that is off at its end is deleted without a word.
//!
//! The scheduler is one task per core. It reads the user's schedules when
//! the core starts and whenever one changes, fires what is due, ends what
//! reached its end, and sleeps until the nearest next time or end, or the
//! next change; nothing polls the table.

pub(crate) mod commands;
pub(crate) mod holidays;
mod hooks;
pub(crate) mod message;
mod runs;
pub(crate) mod plan;
mod watch;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use base64::Engine;
use jush_core_store::StorageError;
use jush_agent_session::EndedTurn;
use jush_core_store::schedules::{StoredRun, StoredSchedule};
use jush_core_sync::Part;
use jush_core_store::seal::Row;
use jush_shared_types::{Attribution, ScheduleId, ScheduleRunId, Timestamp};
use jush_web_api_protocol::ids::ConversationId;
use jush_web_api_protocol::schedules::{
    AgentTurn, ConversationSchedule, IfBusy, LastRun, MonthDay, NEW_NAME, RUNS_SHOWN, Repeat, RepeatUnit, RunCause,
    ScheduleDto, SchedulePatch, ScheduleRunDto, Task, Timing, Trigger, Weekday,
};
use rand::RngCore;
use tokio::sync::Notify;

use crate::Core;
use plan::{Plan, PlanError};

pub use hooks::{CALLS_PER_MINUTE, HookAnswer, HookCall, HookLimits};

/// What the core keeps of its schedules between their records' reads.
#[derive(Default)]
pub(crate) struct Schedules {
    /// Rearms the scheduler: a change leaves a permit, so a change made
    /// while it reads is not missed.
    wake: Notify,
    /// What each schedule has under way.
    pending: RefCell<HashMap<ScheduleId, Pending>>,
    /// When each change watch last tried its command, recorded or not, as
    /// for an offline device, from which its next check counts.
    tried: RefCell<HashMap<ScheduleId, Timestamp>>,
    /// The runs whose message is on its way to their conversation: their
    /// turn is recorded, but no turn of theirs is there to read yet.
    carrying: RefCell<HashSet<ScheduleRunId>>,
}

/// A run whose message is on its way to its conversation, until it drops.
pub(crate) struct Carrying<'a> {
    schedules: &'a Schedules,
    run: ScheduleRunId,
}

impl Drop for Carrying<'_> {
    fn drop(&mut self) {
        self.schedules.carrying.borrow_mut().remove(&self.run);
    }
}

impl Schedules {
    /// Marks the run `run` as carrying its message until the guard drops.
    pub(crate) fn carrying(&self, run: &ScheduleRunId) -> Carrying<'_> {
        self.carrying.borrow_mut().insert(run.clone());
        Carrying { schedules: self, run: run.clone() }
    }

    /// Whether the run `run`'s message is on its way to its conversation.
    pub(crate) fn carries(&self, run: &ScheduleRunId) -> bool {
        self.carrying.borrow().contains(run)
    }
}

/// What a schedule has under way in the core: a URL's burst waiting out its
/// quiet, and a change watch's check.
///
/// Pausing the schedule, editing its trigger or deleting it drops both, which
/// stops their tasks, and moves its generation on, so what either finishes after
/// is not recorded. The core's end drops them all.
#[derive(Default)]
struct Pending {
    generation: u64,
    burst: Option<hooks::Burst>,
    /// Stops the check under way when dropped: its command ends on its
    /// device, and its task records nothing.
    check: Option<tokio_util::sync::DropGuard>,
}

/// Why a change of a schedule was refused.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("This conversation has no such schedule.")]
    NotFound,
    #[error("No such conversation.")]
    ConversationNotFound,
    #[error("This schedule has no run {0}.")]
    NoRun(ScheduleRunId),
    #[error("{0}")]
    Cron(String),
    #[error("{0}")]
    TimeZone(String),
    #[error("{0}")]
    Holidays(String),
    #[error("The schedule has no task {0}.")]
    NoTask(String),
    #[error("The schedule has several tasks: give the one to run.")]
    TaskNeeded,
    #[error("A single trigger has no tasks.")]
    NoTasks,
    #[error("The task has no message to send.")]
    NoMessage,
    #[error("A limit on runs a day is for a change watch, a URL or a when-done trigger.")]
    LimitNotTaken,
    #[error("Only a URL trigger has a secret.")]
    NoSecret,
    #[error("A once schedule ends when it fires, and takes no end.")]
    EndNotTaken,
    /// The end, as the schedule's time zone reads it.
    #[error("The end {0} has passed: give a later one.")]
    EndPassed(String),
    #[error("{0}")]
    NotDoneOf(String),
    /// A run now that could not be recorded, which the log says why of.
    #[error("The run could not be recorded, and did not start.")]
    NotRecorded,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

impl From<PlanError> for ScheduleError {
    fn from(error: PlanError) -> Self {
        match error {
            PlanError::TimeZone(_) => Self::TimeZone(error.to_string()),
            PlanError::Cron { .. } => Self::Cron(error.to_string()),
            PlanError::Holidays(_) => Self::Holidays(error.to_string()),
        }
    }
}

/// What a write of a schedule is.
#[derive(Clone, Copy)]
enum Save {
    Create,
    /// A change, which may replace a URL trigger's secret.
    Edit { new_secret: bool },
}

/// A new secret: 32 random bytes in URL-safe base64.
fn new_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 128 random bits as 32 hexadecimal digits.
fn random_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A new schedule's id.
fn new_schedule_id() -> ScheduleId {
    ScheduleId::try_from(random_id()).expect("32 digits are not empty")
}

/// A new task at the cycle's first moment, Monday, the 1st, in January, at
/// 09:00, as Add Task makes it.
pub(crate) fn new_task(prompt: String) -> Task {
    Task {
        id: random_id(),
        time: "09:00".into(),
        days: vec![Weekday::Mon],
        day: MonthDay::Date { date: 1 },
        month: 1,
        holidays: None,
        prompt,
    }
}

impl Core {
    /// Whether work the schedule started at `generation` may still record
    /// what it found: nothing paused it, edited its trigger or deleted it
    /// since.
    fn schedule_current(&self, id: &ScheduleId, generation: u64) -> bool {
        self.schedules()
            .pending
            .borrow()
            .get(id)
            .is_some_and(|pending| pending.generation == generation)
    }

    /// Drops what the schedule has under way, its burst's wait and its
    /// watch's check, and moves its generation on.
    fn drop_pending(&self, id: &ScheduleId) {
        let (burst, check) = {
            let mut pending = self.schedules().pending.borrow_mut();
            let entry = pending.entry(id.clone()).or_default();
            entry.generation += 1;
            (entry.burst.take(), entry.check.take())
        };
        // Dropped outside the borrow: each stops its task.
        drop((burst, check));
    }

    /// The scheduler's wake-up.
    fn rearm_schedules(&self) {
        self.schedules().wake.notify_one();
    }

    /// The conversation's schedules changed: its entry in the product state
    /// carries them, and the scheduler looks at their times again.
    pub(crate) fn schedules_changed(&self, conversation: &ConversationId) {
        self.mark(Part::Conversation(conversation.clone()));
        self.rearm_schedules();
    }

    /// The conversation's schedules as `jush schedule list` shows them,
    /// newest first.
    pub async fn schedule_list(&self, conversation: &ConversationId) -> Result<Vec<ScheduleDto>, StorageError> {
        let stored = self.store().conversation_schedules(conversation.clone()).await?;
        let mut listed = Vec::with_capacity(stored.len());
        for schedule in stored {
            listed.push(self.schedule_dto(schedule).await?);
        }
        Ok(listed)
    }

    /// The conversation's schedules as its entry lists them, newest first.
    pub(crate) async fn conversation_schedules(
        &self,
        conversation: &ConversationId,
    ) -> Result<Vec<ConversationSchedule>, StorageError> {
        let stored = self.store().conversation_schedules(conversation.clone()).await?;
        let mut listed = Vec::with_capacity(stored.len());
        for schedule in stored {
            let next_at = self.next_fire(&schedule).await?;
            let last_run = self
                .store().schedule_runs(schedule.id.clone(), 1)
                .await?
                .into_iter()
                .next()
                .map(|run| LastRun {
                    at: run.at,
                    state: run.state,
                    note: run.note,
                });
            listed.push(ConversationSchedule {
                id: schedule.id,
                name: schedule.name,
                trigger: schedule.trigger,
                time_zone: schedule.time_zone,
                paused: schedule.paused,
                next_at,
                ends_at: schedule.ends_at,
                last_run,
            });
        }
        Ok(listed)
    }

    /// The conversation's schedule `id`, as it is stored, while the core's
    /// user owns the conversation; any other is not found, so neither the page
    /// nor an agent reaches another user's schedule by its id, which a URL
    /// trigger's address shows.
    pub(crate) async fn stored_schedule(
        &self,
        conversation: &ConversationId,
        id: &ScheduleId,
    ) -> Result<StoredSchedule, ScheduleError> {
        if let Some(schedule) = self.store().owned_schedule(conversation.clone(), id.clone()).await? {
            return Ok(schedule);
        }
        match self.store().conversation(conversation.clone()).await? {
            Some(_) => Err(ScheduleError::NotFound),
            None => Err(ScheduleError::ConversationNotFound),
        }
    }

    async fn schedule_dto(&self, schedule: StoredSchedule) -> Result<ScheduleDto, StorageError> {
        let runs = self
            .store().schedule_runs(schedule.id.clone(), RUNS_SHOWN)
            .await?;
        let next_at = self.next_fire(&schedule).await?;
        Ok(ScheduleDto {
            id: schedule.id,
            name: schedule.name,
            trigger: schedule.trigger,
            time_zone: schedule.time_zone,
            max_runs_per_day: schedule.max_runs_per_day,
            if_busy: schedule.if_busy,
            prompt: schedule.prompt,
            paused: schedule.paused,
            next_at,
            ends_at: schedule.ends_at,
            created_at: schedule.created_at,
            created_in: schedule.created_in,
            edited_in: schedule.edited_in,
            runs: runs.into_iter().map(run_dto).collect(),
        })
    }

    /// The conversation's schedule `id` as `jush schedule show` shows it.
    pub async fn schedule(&self, conversation: &ConversationId, id: &ScheduleId) -> Result<ScheduleDto, ScheduleError> {
        let stored = self.stored_schedule(conversation, id).await?;
        Ok(self.schedule_dto(stored).await?)
    }

    /// The runs of the conversation's schedule `id` before its run `before`,
    /// newest first, [`RUNS_SHOWN`] at most; the latest without `before`.
    pub async fn schedule_runs(
        &self,
        conversation: &ConversationId,
        id: &ScheduleId,
        before: Option<ScheduleRunId>,
    ) -> Result<Vec<ScheduleRunDto>, ScheduleError> {
        let stored = self.stored_schedule(conversation, id).await?;
        // A run the schedule never had would read as the end of its runs.
        if let Some(before) = &before
            && self.store().schedule_run(before.clone()).await?.is_none_or(|run| run.schedule != stored.id)
        {
            return Err(ScheduleError::NoRun(before.clone()));
        }
        let runs = self.store().schedule_runs_before(stored.id, before, RUNS_SHOWN).await?;
        Ok(runs.into_iter().map(run_dto).collect())
    }

    /// When the schedule next fires by itself, as its menu says it: before
    /// its end, which is no firing of its trigger.
    async fn next_fire(&self, schedule: &StoredSchedule) -> Result<Option<Timestamp>, StorageError> {
        let now = self.store().now();
        if !schedule.live(now) {
            return Ok(None);
        }
        let next = match &schedule.trigger {
            Trigger::Time { timing } => {
                let Ok(plan) = Plan::new(timing, &schedule.time_zone, schedule.since.to_jiff()) else {
                    return Ok(None);
                };
                let last = self.store().schedule_last_fired(schedule.id.clone()).await?;
                let after = last.map_or(now, |last| last.max(now));
                plan.next_after(after.to_jiff()).map(Timestamp::truncate)
            }
            Trigger::Change { every_minutes, .. } => {
                let due = Timestamp::truncate(self.watch_due(schedule, *every_minutes));
                // At its limit it waits for the next day.
                Some(match self.limit_reached(schedule, now).await? {
                    Some(tomorrow) => tomorrow.max(due),
                    None => due,
                })
            }
            // Neither fires at a time of its own.
            Trigger::Url {} | Trigger::Done { .. } => None,
        };
        Ok(next.filter(|next| schedule.ends_at.is_none_or(|end| *next < end)))
    }

    /// `jush schedule create`, and a program's: a new schedule of
    /// `conversation` from its defaults, with `patch` over them, which the
    /// agent's turn `by` made, with the labels and the assignment of its
    /// work, or a program for none.
    ///
    /// The defaults are *New Schedule*, on, every day with one task at 09:00
    /// in the user's time zone and no message, which waits while the
    /// conversation is busy. A URL trigger gets its secret at once; the answer
    /// carries it when there is one.
    pub async fn create_schedule(
        &self,
        conversation: &ConversationId,
        patch: SchedulePatch,
        by: Option<(AgentTurn, Attribution)>,
    ) -> Result<(ScheduleDto, Option<String>), ScheduleError> {
        let (by, created_for) = by.map_or((None, Attribution::default()), |(turn, attribution)| (Some(turn), attribution));
        let store = self.store();
        // The record's id, as the caller may spell it in another case, which
        // the conversation's tree and marks would not match.
        let record = store.conversation(conversation.clone()).await?.ok_or(ScheduleError::ConversationNotFound)?;
        let now = store.now();
        let names: HashSet<String> = store.conversation_schedules(record.id.clone())
            .await?
            .into_iter()
            .map(|schedule| schedule.name)
            .collect();
        let name = (1..)
            .map(|number| match number {
                1 => NEW_NAME.to_owned(),
                number => format!("{NEW_NAME} ({number})"),
            })
            .find(|name| !names.contains(name))
            .expect("the numbers run out after the names");
        let reported = jush_core_devices::command_context::reported(store).await?;
        let defaults = StoredSchedule {
            id: new_schedule_id(),
            conversation_archived: record.archived(),
            conversation: record.id,
            name,
            trigger: Trigger::Time {
                timing: Timing::Repeat {
                    repeat: Repeat {
                        every: 1,
                        unit: RepeatUnit::Days,
                        start: None,
                    },
                    tasks: vec![new_task(String::new())],
                },
            },
            time_zone: match plan::zone(&reported.locale.time_zone) {
                Ok(_) => reported.locale.time_zone,
                Err(_) => "UTC".into(),
            },
            max_runs_per_day: None,
            if_busy: IfBusy::Queue,
            prompt: String::new(),
            paused: false,
            since: now,
            has_secret: false,
            watch: None,
            created_at: now,
            updated_at: now,
            created_in: by,
            created_for,
            edited_in: None,
            ends_at: None,
            ended: false,
        };
        self.save_schedule(defaults, patch, Save::Create, None).await
    }

    /// `jush schedule edit`, and a program's change, by the agent's turn `by`
    /// or a program for none; with `new_secret`, a URL trigger's secret is
    /// replaced, and the old one stops working at once.
    pub async fn change_schedule(
        &self,
        conversation: &ConversationId,
        id: &ScheduleId,
        patch: SchedulePatch,
        new_secret: bool,
        by: Option<AgentTurn>,
    ) -> Result<(ScheduleDto, Option<String>), ScheduleError> {
        let stored = self.stored_schedule(conversation, id).await?;
        self.save_schedule(stored, patch, Save::Edit { new_secret }, by).await
    }

    /// Applies `patch` to `schedule`, checks the whole, and writes it; a
    /// change of what it does, anything but whether it is on, names `by` as
    /// its last editor.
    async fn save_schedule(
        &self,
        mut schedule: StoredSchedule,
        patch: SchedulePatch,
        save: Save,
        by: Option<AgentTurn>,
    ) -> Result<(ScheduleDto, Option<String>), ScheduleError> {
        let new = matches!(save, Save::Create);
        // Turning a schedule off or on changes nothing the checks below read,
        // so a schedule a later time zone or holiday table no longer takes can
        // still be turned off; nor does it change who last said what it does.
        let only_paused = matches!(save, Save::Edit { new_secret: false })
            && patch == SchedulePatch { paused: patch.paused, ..SchedulePatch::default() };
        if !new && !only_paused {
            schedule.edited_in = by;
        }
        let store = self.store();
        let now = store.now();
        let before = schedule.clone();
        if let Some(name) = patch.name {
            schedule.name = name.into_string();
        }
        if let Some(trigger) = patch.trigger {
            schedule.trigger = trigger;
        }
        if let Some(zone) = patch.time_zone {
            schedule.time_zone = zone;
        }
        if let Some(limit) = patch.max_runs_per_day {
            schedule.max_runs_per_day = limit;
        }
        if let Some(if_busy) = patch.if_busy {
            schedule.if_busy = if_busy;
        }
        if let Some(prompt) = patch.prompt {
            schedule.prompt = prompt;
        }
        if let Some(paused) = patch.paused {
            schedule.paused = paused;
        }
        if let Some(ends_at) = patch.ends_at {
            schedule.ends_at = ends_at;
        }
        // A new end is still to come.
        if let Some(Some(end)) = patch.ends_at
            && end <= now
        {
            let zone = plan::zone(&schedule.time_zone)?;
            let local = end.to_jiff().to_zoned(zone).strftime("%Y-%m-%d %H:%M").to_string();
            return Err(ScheduleError::EndPassed(format!("{local} {}", schedule.time_zone)));
        }
        // A once schedule ends when it fires: one given an end is refused,
        // and a trigger changed to once drops the end it had, as a trigger
        // changed to a time drops its daily limit.
        if matches!(schedule.trigger, Trigger::Time { timing: Timing::Once { .. } }) && schedule.ends_at.is_some() {
            if patch.ends_at.flatten().is_some() {
                return Err(ScheduleError::EndNotTaken);
            }
            schedule.ends_at = None;
        }
        // Each part is checked as a whole schedule needs it.
        if !only_paused {
            plan::zone(&schedule.time_zone)?;
            if let Trigger::Time { timing } = &schedule.trigger {
                Plan::new(timing, &schedule.time_zone, now.to_jiff())?;
            }
            // A task's place is checked even while it has no message or no
            // working day, so what the schedule holds is always a place the table has.
            for task in runs::tasks_of(&schedule.trigger).unwrap_or_default() {
                if let Some(place) = &task.holidays
                    && holidays::calendar(place).is_none()
                {
                    return Err(PlanError::Holidays(place.country.clone()).into());
                }
            }
        }
        // A when-done trigger names another conversation of the user that is
        // not archived as it is set, by its record's id; one archived or
        // deleted later runs nothing.
        if let Trigger::Done { conversation } = &schedule.trigger
            && before.trigger != schedule.trigger
        {
            let watched = match self.store().conversation(conversation.clone()).await? {
                Some(record) if !record.archived() => record.id,
                Some(_) => {
                    return Err(ScheduleError::NotDoneOf(format!(
                        "conversation {conversation} is archived, and only the user restores it"
                    )));
                }
                None => {
                    return Err(ScheduleError::NotDoneOf(format!(
                        "no conversation {conversation} (see `jush conversation list`)"
                    )));
                }
            };
            if watched == schedule.conversation {
                return Err(ScheduleError::NotDoneOf(
                    "--when-done names another conversation; this one's end is its own turn's end".into(),
                ));
            }
            if self.wakes_back(&schedule.id, &schedule.conversation, &watched).await? {
                return Err(ScheduleError::NotDoneOf(format!(
                    "conversation {watched} already runs when this one finishes, through when-done schedules, so each would run the other without end"
                )));
            }
            schedule.trigger = Trigger::Done { conversation: watched };
        }
        if schedule.max_runs_per_day.is_some() && !schedule.trigger.takes_limit() {
            if patch.max_runs_per_day.flatten().is_some() {
                return Err(ScheduleError::LimitNotTaken);
            }
            // A trigger that changed to a time drops the limit it had.
            schedule.max_runs_per_day = None;
        }
        // A schedule counts from when it was set or turned on, or given a
        // new end after its end came: what it would have fired while it was
        // off, or over, never fires.
        let revived = before.over(now) && !schedule.over(now);
        if new || before.trigger != schedule.trigger || before.time_zone != schedule.time_zone
            || (before.paused && !schedule.paused)
            || revived
        {
            schedule.since = now;
        }
        // A pause or another trigger drops what was pending, before the
        // change is written, so nothing from before it fires.
        let drops = !new && ((schedule.paused && !before.paused) || before.trigger != schedule.trigger);
        if drops {
            self.drop_pending(&schedule.id);
        }
        let url = matches!(schedule.trigger, Trigger::Url {});
        let mut secret_shown = None;
        let replaced = matches!(save, Save::Edit { new_secret: true });
        if replaced && !url {
            return Err(ScheduleError::NoSecret);
        }
        let secret = if url && (!schedule.has_secret || replaced) {
            let secret = new_secret();
            let sealed = self.seal_schedule(&schedule.id, secret.as_bytes());
            secret_shown = Some(secret);
            Some(Some(sealed))
        } else if !url && schedule.has_secret {
            Some(None)
        } else {
            None
        };
        schedule.updated_at = now;
        // The edge finds the user whose core holds the schedule a URL names
        // before anyone signed in.
        if url {
            self.starter().schedule_called_at_url(&schedule.id).await?;
        }
        store.put_schedule(schedule.clone(), secret).await?;
        if drops {
            store.drop_pending_calls(schedule.id.clone()).await?;
        }
        // A watch whose command changed, or that was turned on or given a
        // new end after its end came, records its output first: a change
        // while it was off fires nothing.
        if let (Trigger::Change { command: old, .. }, Trigger::Change { command, .. }) =
            (&before.trigger, &schedule.trigger)
            && (old != command || (before.paused && !schedule.paused) || revived)
        {
            store.set_watch_output(schedule.id.clone(), None).await?;
        }
        self.schedules().tried.borrow_mut().remove(&schedule.id);
        self.schedules_changed(&schedule.conversation);
        let stored = self.stored_schedule(&schedule.conversation, &schedule.id).await?;
        Ok((self.schedule_dto(stored).await?, secret_shown))
    }

    fn seal_schedule(&self, id: &ScheduleId, plaintext: &[u8]) -> Vec<u8> {
        self.store().key().seal(Row::ScheduleSecret(id.as_str()), plaintext)
    }

    /// The schedule's URL secret, opened; none for a schedule that has none.
    /// It checks a call of the URL; the agent is shown it once, when it is made.
    pub(crate) async fn schedule_secret(&self, id: &ScheduleId) -> Result<Option<String>, StorageError> {
        let Some(sealed) = self.store().schedule_secret(id.clone()).await? else {
            return Ok(None);
        };
        let corrupt = |reason: String| StorageError::Corrupt {
            table: "schedules",
            column: "secret",
            reason,
        };
        let bytes = self
            .store()
            .key()
            .open(Row::ScheduleSecret(id.as_str()), &sealed)
            .map_err(|_| corrupt("a sealed secret does not open".into()))?;
        String::from_utf8(bytes).map(Some).map_err(|error| corrupt(error.to_string()))
    }

    /// `jush schedule delete`, and the user's Delete: the schedule goes with
    /// its runs' records.
    pub async fn delete_schedule(&self, conversation: &ConversationId, id: &ScheduleId) -> Result<(), ScheduleError> {
        let schedule = self.stored_schedule(conversation, id).await?;
        self.forget_schedule(&schedule.id);
        if !self.store().delete_schedule(schedule.id.clone()).await? {
            return Err(ScheduleError::NotFound);
        }
        self.tell_schedules_deleted(std::slice::from_ref(&schedule.id)).await;
        self.schedules_changed(conversation);
        Ok(())
    }

    /// What the core holds of the schedule `id`, which is deleted: its
    /// burst and its check stop, and its URL's call count and its watch's
    /// last try go.
    fn forget_schedule(&self, id: &ScheduleId) {
        self.drop_pending(id);
        let gone = self.schedules().pending.borrow_mut().remove(id);
        drop(gone);
        self.services().hook_limits.forget(id);
        self.schedules().tried.borrow_mut().remove(id);
    }

    /// The ids of the conversation's schedules, which its deletion reads
    /// before its records go, to forget them once they have
    /// ([`Self::conversation_deleted_schedules`]).
    pub(crate) async fn conversation_schedule_ids(&self, conversation: &ConversationId) -> Vec<ScheduleId> {
        match self.store().conversation_schedules(conversation.clone()).await {
            Ok(schedules) => schedules.into_iter().map(|schedule| schedule.id).collect(),
            Err(error) => {
                tracing::warn!(event = "schedules.forget_failed", %conversation, %error, "the schedules of a deleted conversation were not read");
                Vec::new()
            }
        }
    }

    /// Tells the starter the schedules `ids` were deleted. A failure leaves
    /// a URL the starter routes to this core, which refuses its calls.
    pub(crate) async fn tell_schedules_deleted(&self, ids: &[ScheduleId]) {
        for id in ids {
            if let Err(error) = self.starter().schedule_deleted(id).await {
                tracing::warn!(event = "schedules.owner_kept", schedule = %id, error = &error as &dyn std::error::Error, "a deleted schedule's owner was not forgotten");
            }
        }
    }

    /// The conversation is deleted, and its schedules `ids` with it: what the
    /// core holds of them goes, a watch's check under way among it.
    pub(crate) fn conversation_deleted_schedules(&self, ids: &[ScheduleId]) {
        for id in ids {
            self.forget_schedule(id);
        }
        self.rearm_schedules();
    }

    /// `jush schedule run`, and a program's run now: a run whatever the
    /// timing, the daily limit or whether the schedule is paused, of the
    /// repeat's task `task`, which may be left out when the repeat has only
    /// one. Answers the schedule and the run as it started.
    pub async fn run_schedule_now(
        &self,
        conversation: &ConversationId,
        id: &ScheduleId,
        task: Option<String>,
    ) -> Result<(StoredSchedule, ScheduleRunDto), ScheduleError> {
        let schedule = self.stored_schedule(conversation, id).await?;
        let task = match (runs::tasks_of(&schedule.trigger), task) {
            (None, None) => None,
            (None, Some(_)) => return Err(ScheduleError::NoTasks),
            (Some([only]), None) => Some(only),
            (Some(_), None) => return Err(ScheduleError::TaskNeeded),
            (Some(tasks), Some(id)) => Some(tasks.iter().find(|each| each.id == id).ok_or(ScheduleError::NoTask(id))?),
        };
        if task.is_some_and(|task| jush_shared_types::is_blank(&task.prompt)) {
            return Err(ScheduleError::NoMessage);
        }
        let at = self.store().now();
        let run = self
            .fire(
                &schedule,
                runs::Fired {
                    cause: RunCause::Now,
                    at,
                    task: task.map(|task| task.id.clone()),
                    payload: runs::Payload::None,
                },
            )
            .await
            .ok_or(ScheduleError::NotRecorded)?;
        Ok((schedule, run_dto(run)))
    }

    /// The conversation was archived: its schedules wake nothing while it is,
    /// which their reads say by themselves, and what they had under way drops,
    /// as a pause drops it.
    pub(crate) async fn conversation_archived_schedules(&self, conversation: &ConversationId) {
        self.drop_pending_of(conversation).await;
        self.schedules_changed(conversation);
    }

    /// The conversation was unarchived: its schedules wake it again, counting
    /// from now, which the restore wrote with it.
    pub(crate) fn conversation_unarchived_schedules(&self, conversation: &ConversationId) {
        self.schedules_changed(conversation);
    }

    /// Drops what each of the conversation's schedules has under way.
    async fn drop_pending_of(&self, conversation: &ConversationId) {
        match self.store().conversation_schedules(conversation.clone()).await {
            Ok(schedules) => {
                for schedule in &schedules {
                    self.drop_pending(&schedule.id);
                    if let Err(error) = self.store().drop_pending_calls(schedule.id.clone()).await {
                        tracing::warn!(event = "schedules.calls_not_dropped", schedule = %schedule.id, %error, "a held schedule's pending calls stay");
                    }
                }
            }
            Err(error) => tracing::warn!(event = "schedules.read_failed", %conversation, %error, "the schedules were not read"),
        }
    }

    /// Fires the schedules of the user as their times come, until the core
    /// closes.
    ///
    /// It settles the runs a run of the backend before left running, then fires
    /// what is due, starts the change watches whose time came, ends the
    /// schedules whose end came, and sleeps until the nearest next time or
    /// end, or a change of the schedules. A time that passed while the backend
    /// was down fires once, and an end that did comes after it.
    pub(crate) async fn follow_schedules(self: Rc<Self>) {
        self.settle_left_runs().await;
        self.resume_pending_calls().await;
        loop {
            let next = match self.fire_due().await {
                Ok(next) => next,
                Err(error) => {
                    // The next change or the next start reads them again.
                    tracing::warn!(event = "schedules.read_failed", user = %self.user(), %error, "the schedules were not read");
                    None
                }
            };
            let now = self.store().now();
            let wait = next.map(|next| {
                Duration::try_from(next.to_jiff().duration_since(now.to_jiff())).unwrap_or(Duration::ZERO)
            });
            tokio::select! {
                () = async {
                    match wait {
                        Some(wait) => tokio::time::sleep(wait).await,
                        None => std::future::pending().await,
                    }
                } => {}
                () = self.schedules().wake.notified() => {}
                () = self.closed() => {
                    // The core ends: its bursts' waits and its checks stop.
                    let pending = std::mem::take(&mut *self.schedules().pending.borrow_mut());
                    drop(pending);
                    return;
                }
            }
        }
    }

    /// Fires every schedule whose time came, ends every one whose end came,
    /// and answers the nearest next time or end.
    async fn fire_due(&self) -> Result<Option<Timestamp>, StorageError> {
        let store = self.store();
        let now = store.now();
        let mut nearest: Option<Timestamp> = None;
        let mut keep = |at: Timestamp| nearest = Some(nearest.map_or(at, |nearest| nearest.min(at)));
        for schedule in store.schedules().await? {
            let over = schedule.over(now);
            // One whose end ran waits for a new end, which `edit` sets.
            if over && schedule.ended {
                continue;
            }
            if let Some(end) = schedule.ends_at
                && !over
            {
                keep(end);
            }
            if schedule.on() {
                self.fire_trigger(&schedule, now, &mut keep).await?;
            }
            if over {
                self.end_schedule(&schedule).await?;
            }
        }
        Ok(nearest)
    }

    /// Fires what of `schedule`'s trigger is due at `now`, before its end:
    /// a time missed before an end that passed while the core was down
    /// fires before the end. `keep` takes the trigger's next time.
    async fn fire_trigger(
        &self,
        schedule: &StoredSchedule,
        now: Timestamp,
        keep: &mut impl FnMut(Timestamp),
    ) -> Result<(), StorageError> {
        let store = self.store();
        let over = schedule.over(now);
        // A time at its end fires no more than a later one, as its menu's
        // next time says.
        let upto = match schedule.ends_at {
            Some(end) if over => Timestamp::truncate(end.to_jiff() - jiff::SignedDuration::from_millis(1)),
            _ => now,
        };
        match &schedule.trigger {
            Trigger::Time { timing } => {
                let Ok(plan) = Plan::new(timing, &schedule.time_zone, schedule.since.to_jiff())
                else {
                    return Ok(());
                };
                let last = store.schedule_last_fired(schedule.id.clone()).await?;
                let mut after = last.unwrap_or(schedule.since).to_jiff();
                // Each task due runs as its own run, once for its
                // latest missed time; in time order, so the last fired
                // is the latest.
                for (due, task) in plan.due(after, upto.to_jiff()) {
                    self.fire(schedule, runs::Fired::time(Timestamp::truncate(due), task.map(str::to_owned)))
                        .await;
                    after = after.max(due);
                }
                if !over && let Some(next) = plan.next_after(after) {
                    keep(Timestamp::truncate(next));
                }
            }
            // From its end, its watch runs no more.
            Trigger::Change { .. } if over => {}
            Trigger::Change { every_minutes, .. } => {
                if self.schedule_checking(&schedule.id) {
                    return Ok(());
                }
                let due = Timestamp::truncate(self.watch_due(schedule, *every_minutes));
                let held = self.limit_reached(schedule, now).await?;
                match held {
                    // The limit holds back the command only once it is
                    // due; until then nothing was skipped.
                    Some(tomorrow) if due <= now => {
                        self.count_held_back(schedule, 0).await?;
                        keep(tomorrow);
                    }
                    Some(_) => keep(due),
                    None if due <= now => self.start_watch(schedule.clone()),
                    None => keep(due),
                }
            }
            // A URL fires when it is called, a when-done trigger as its
            // conversation rests (`fire_done`).
            Trigger::Url {} | Trigger::Done { .. } => {}
        }
        Ok(())
    }

    /// `schedule`'s end came: what it had under way stops, and a run of its
    /// end tells its conversation, which deletes it once its message
    /// reached the conversation (`end_run`). One that is off at its end,
    /// paused or held while its conversation is archived, is deleted
    /// without waking it.
    ///
    /// The end's message comes after every message of the schedule's runs:
    /// the URL's calls that came before the end, and wait for their burst's
    /// quiet or a run's end, start their run first, and the end waits until
    /// no run of the schedule works, whose end passes the scheduler again.
    async fn end_schedule(&self, schedule: &StoredSchedule) -> Result<(), StorageError> {
        let Some(end) = schedule.ends_at else {
            return Ok(());
        };
        self.drop_pending(&schedule.id);
        if !schedule.on() {
            tracing::info!(event = "schedule.ended_off", user = %self.user(), schedule = %schedule.id, conversation = %schedule.conversation, "a schedule that was off reached its end");
            if let Err(error) = self.delete_schedule(&schedule.conversation, &schedule.id).await {
                tracing::warn!(event = "schedule.spent_not_deleted", conversation = %schedule.conversation, schedule = %schedule.id, %error, "a spent schedule stays");
            }
            return Ok(());
        }
        if matches!(schedule.trigger, Trigger::Url {}) {
            let calls = self.store().take_pending_calls(schedule.id.clone()).await?;
            if !calls.is_empty() {
                let fired = runs::Fired {
                    cause: RunCause::Url,
                    at: self.store().now(),
                    task: None,
                    payload: runs::Payload::Calls(calls.into_iter().map(hooks::Call::from).collect()),
                };
                self.fire(schedule, fired).await;
                return Ok(());
            }
        }
        if self.schedule_working(&schedule.id).await? {
            return Ok(());
        }
        let (woke, last) = self.store().schedule_wakes(schedule.id.clone()).await?;
        let done_of = match &schedule.trigger {
            Trigger::Done { conversation } => match self.store().conversation(conversation.clone()).await {
                Ok(Some(record)) => Some(message::Finished::name_of(&record.id, record.title.as_deref())),
                _ => None,
            },
            _ => None,
        };
        let trigger = message::trigger_words(schedule, done_of.as_deref());
        let fired = runs::Fired {
            cause: RunCause::End,
            at: end,
            task: None,
            payload: runs::Payload::End { woke, last, trigger },
        };
        self.fire(schedule, fired).await;
        Ok(())
    }

    /// Whether `watched` runs when `conversation` finishes, through the
    /// user's when-done schedules other than `schedule`, paused ones too,
    /// which run once turned on.
    async fn wakes_back(
        &self,
        schedule: &ScheduleId,
        conversation: &ConversationId,
        watched: &ConversationId,
    ) -> Result<bool, StorageError> {
        let watches: Vec<(ConversationId, ConversationId)> = self
            .store()
            .schedules()
            .await?
            .into_iter()
            .filter(|each| each.id != *schedule)
            .filter_map(|each| match each.trigger {
                Trigger::Done { conversation: of } => Some((of, each.conversation)),
                _ => None,
            })
            .collect();
        let mut reached = std::collections::HashSet::from([conversation.clone()]);
        let mut next = vec![conversation.clone()];
        while let Some(finished) = next.pop() {
            for (of, runs) in &watches {
                if *of == finished && reached.insert(runs.clone()) {
                    if runs == watched {
                        return Ok(true);
                    }
                    next.push(runs.clone());
                }
            }
        }
        Ok(false)
    }

    /// `conversation` finished working, its last turn ending at `end`: runs
    /// each schedule that waits for it, unless the day's limit holds it back.
    pub(crate) fn fire_done(&self, conversation: &ConversationId, end: EndedTurn, error: Option<String>) {
        let core = self.this();
        let conversation = conversation.clone();
        self.tasks().spawn_local(async move {
            if let Err(error) = core.fire_done_now(conversation.clone(), end, error).await {
                tracing::warn!(event = "schedule.done_not_fired", user = %core.user(), %conversation, %error, "the schedules waiting for a conversation to finish were not run");
            }
        });
    }

    async fn fire_done_now(
        &self,
        conversation: ConversationId,
        end: EndedTurn,
        error: Option<String>,
    ) -> Result<(), StorageError> {
        let control = &self.store();
        let now = control.now();
        let waiting: Vec<StoredSchedule> = control
            .schedules()
            .await?
            .into_iter()
            .filter(|schedule| matches!(&schedule.trigger, Trigger::Done { conversation: of } if *of == conversation))
            .filter(|schedule| schedule.live(now))
            .collect();
        if waiting.is_empty() {
            return Ok(());
        }
        let title = control.conversation(conversation.clone()).await?.and_then(|record| record.title);
        let finished = message::Finished { conversation, title, end, error };
        for schedule in waiting {
            if self.limit_reached(&schedule, now).await?.is_some() {
                self.count_held_back(&schedule, 1).await?;
                continue;
            }
            let fired = runs::Fired {
                cause: RunCause::Done,
                at: now,
                task: None,
                payload: runs::Payload::Done(finished.clone()),
            };
            self.fire(&schedule, fired).await;
        }
        Ok(())
    }

    /// When a change watch next runs its command: an interval after its
    /// last try, recorded or not, and at once before its first.
    fn watch_due(&self, schedule: &StoredSchedule, every_minutes: u32) -> jiff::Timestamp {
        let recorded = schedule.watch.as_ref().map(|watch| watch.checked_at);
        let tried = self.schedules().tried.borrow().get(&schedule.id).copied();
        match recorded.max(tried) {
            Some(last) => Plan::every(every_minutes, last.to_jiff())
                .next_after(last.to_jiff())
                .unwrap_or(last.to_jiff()),
            None => schedule.since.to_jiff(),
        }
    }

    /// When the schedule's daily limit is reached, the start of the next
    /// day, from which it runs again; none while runs are left today or it
    /// has no limit.
    async fn limit_reached(
        &self,
        schedule: &StoredSchedule,
        now: Timestamp,
    ) -> Result<Option<Timestamp>, StorageError> {
        let Some(limit) = schedule.max_runs_per_day else {
            return Ok(None);
        };
        let Ok(zone) = plan::zone(&schedule.time_zone) else {
            return Ok(None);
        };
        let (today, tomorrow) = plan::day_bounds(now.to_jiff(), &zone);
        let ran = self
            .store().schedule_runs_since(schedule.id.clone(), Timestamp::truncate(today))
            .await?;
        Ok((ran >= limit).then(|| Timestamp::truncate(tomorrow)))
    }

    /// Counts a trigger the daily limit held back in the day's one skipped
    /// run, *Skipped, the limit of 10 runs a day was reached, 7 calls
    /// since*, made by the first and grown by each later one.
    async fn count_held_back(&self, schedule: &StoredSchedule, calls: u32) -> Result<(), StorageError> {
        let store = self.store();
        let now = store.now();
        let Some(limit) = schedule.max_runs_per_day else {
            return Ok(());
        };
        let Ok(zone) = plan::zone(&schedule.time_zone) else {
            return Ok(());
        };
        let (today, _) = plan::day_bounds(now.to_jiff(), &zone);
        match store.schedule_limit_run(schedule.id.clone(), Timestamp::truncate(today))
            .await?
        {
            Some(mut run) => {
                if let Some(jush_web_api_protocol::schedules::RunNote::Limit { calls: count, .. }) = &mut run.note {
                    *count += calls;
                }
                store.update_schedule_run(run).await?;
            }
            None => {
                store.insert_schedule_run(runs::skipped(
                        schedule,
                        runs::cause_of(&schedule.trigger),
                        now,
                        jush_web_api_protocol::schedules::RunNote::Limit { limit, calls },
                    ))
                    .await?;
            }
        }
        self.mark(Part::Conversation(schedule.conversation.clone()));
        Ok(())
    }
}


fn run_dto(run: StoredRun) -> ScheduleRunDto {
    ScheduleRunDto {
        id: run.id,
        cause: run.cause,
        at: run.at,
        task_id: run.task,
        state: run.state,
        note: run.note,
        line: run.line,
        turn: run.turn,
    }
}
