---
status: proposed
---

# A program manages the schedules of its conversations

## Problem

A program answers in a team's group chats, each chat a conversation, and
keeps routines for them: a report every weekday at 10:00 in one chat, the
person on duty rotated every Monday in another. A schedule wakes a
conversation that way, but only the conversation's own agent makes and
changes one, with `jush schedule`; the app turns one off and on, and deletes
it.

- Someone in one chat asks for a routine in another chat, whose agent the
  program would have to ask in turn.
- An admin's page lists every routine, turns one off, or runs one now, with
  no agent in between: nothing lets the program do that.
- A dashboard counts each routine's runs: when each fired, the turn it
  started, and how it ended. A program reads a schedule's last run only, in
  the metadata of a conversation it opened.
- Thirty routines move from the program's own scheduler, and each would take
  a turn of its conversation's agent to make.
- A run's turn does work that only some people may ask for, and the program
  cannot tell whose request the schedule came from.

## Decision

### The program acts on a conversation's schedules

A program makes, reads, changes, runs and deletes any conversation's
schedules, with the types `jush schedule` uses:

```ts
const chat = core.conversations.open(id)
const { schedule } = await chat.schedules.create({
  name: 'Daily report',
  trigger: { kind: 'time', timing: { kind: 'cron', expression: '0 10 * * 1-5' } },
  timeZone: 'Asia/Shanghai',
  prompt: 'Post the daily report.',
})
await chat.schedules.update(schedule.id, { paused: true })
await chat.schedules.run(schedule.id)
const runs = await chat.schedules.runs(schedule.id)
await chat.schedules.remove(schedule.id)
```

| Route | What it does | Answer |
| --- | --- | --- |
| `POST /conversations/:id/schedules` | makes one from a `SchedulePatch` over the defaults `jush schedule create` takes, on unless the patch pauses it | 201 `ScheduleAnswer` |
| `GET /conversations/:id/schedules/:schedule` | reads one, as `jush schedule show` shows it | `ScheduleDto` |
| `PATCH /conversations/:id/schedules/:schedule` | changes what the `SchedulePatch` names | `ScheduleAnswer` |
| `DELETE /conversations/:id/schedules/:schedule` | deletes it and its runs' records | 204 |
| `POST /conversations/:id/schedules/:schedule/runs` | runs it now, as `jush schedule run` does, of the repeat's task in `NewScheduleRun` | 201 `ScheduleRunDto` |
| `GET /conversations/:id/schedules/:schedule/runs?before=` | its runs before the run `before`, newest first, 50 at a time | `ScheduleRuns` |

`ScheduleAnswer` is `{ schedule, secret }`, where `secret` is a URL
trigger's secret, given when the change makes one, and null otherwise. A
conversation that does not exist answers 404 `conversation_not_found` on
every route, a schedule that does not, 404 `schedule_not_found`, and a
`before` that is no run of the schedule, 400 `invalid_query`, where an empty
page would read as the end of its runs. The page's switch sends
`{ paused }`, a `SchedulePatch`, to the same `PATCH`, and `ScheduleChange`
goes. Every schedule, the agent's and a program's, is made on unless its
patch pauses it, so `jush schedule create` no longer sets it on itself.

The message a run now sends says it ran "on request", where it said "by the
agent", since a program asks for one too.

The agent's `jush schedule` still acts on its own conversation only. A
program whose users ask one chat's agent for a routine in another gives that
agent a command of its own, whose handler calls these routes. A schedule
stays with the conversation it was made in: a program moves one by making it
in the other conversation and deleting it.

### Every schedule is in the product state

`ConversationSummary` lists the conversation's schedules, where it said only
whether it has any:

```rust
pub struct ConversationSummary {
    // `scheduled: bool` goes.
    /// The schedules that wake it, newest first.
    pub schedules: Vec<ConversationSchedule>,
    // …
}
```

`ConversationMetadata.schedules` goes. The sidebar's alarm clock and the
taskbar's Schedules menu read the summary, and a program reads every
conversation's schedules in `core.state`. A change of a schedule, and each
of its runs as it starts and ends, marks the conversation's entry, so
`core.on('state')` hears of it, as the app does.

### A run names its cause and its turn

```rust
pub struct ScheduleRunDto {
    // …
    /// What fired it: its time, a change, a call of its URL, another
    /// conversation that finished, or a run now that an agent or a program
    /// asked for.
    pub cause: RunCause,
    /// The turn its message started; null for a run that started none, as
    /// one skipped while its device was offline.
    pub turn: Option<TurnId>,
}
```

A run names its turn once its message goes, so the answer of a run now
names none yet, as a CronJob's new Job names no Pod: the program reads the
run again when the product state says the conversation changed. A turn
named before the message went, kept by a core that stopped then, would read
after the restart as a message taken out of the queue, and a once schedule
would be spent without its message.

`RunCause` moves from the store to the web contract, its one definition.
The turn's message keeps the schedule's id and its name when it fired
(`ScheduleMark`), so a program reads which schedule started a turn after the
schedule changed or went.

### A schedule names where its agent made and last changed it

```rust
pub struct ScheduleDto {
    // …
    /// The agent's turn whose `jush schedule create` made it; null for one a
    /// program made.
    pub created_in: Option<AgentTurn>,
    /// The agent's turn whose `jush schedule edit` last changed what it
    /// does, anything but whether it is on; null when no agent changed it
    /// since it was made, or a program changed it last.
    pub edited_in: Option<AgentTurn>,
}

/// An agent's turn: the agent, whose node is the conversation's id for its
/// root, and the turn, whose id for a turn a message started is the
/// message's.
pub struct AgentTurn {
    pub node: NodeId,
    pub turn: TurnId,
}
```

A program that sent the message knows who asked: the root's turn has the id
of the message that started it. Turning a schedule off and on changes
neither field, so a person who may only pause a routine never takes over its
message. A schedule made before this change names neither.

### Storage

The core database's `schedules` table gains `created_node`, `created_turn`,
`edited_node` and `edited_turn`. A step of the core database's history,
from the schema 0.2.6247 shipped, adds them, null for the schedules there.

An older page meets a summary without `scheduled`, and loads the page of the
new build, as it does after every update.

## Spike

A program on the SDK made four schedules in one conversation: a cron, a URL,
the defaults and one paused from the start. The conversation's agent made a
fifth and edited it with `jush schedule`, and the program then paused it,
changed its message, ran one now three times, paged its runs and deleted one.

- The URL trigger's answer carried its secret; the others carried none.
- The agent's schedule named the turns of its create and its edit. The pause
  kept both, and the program's change of its message cleared `editedIn`.
- Each run's turn, read once its message went, was the one its message
  started in the transcript.
- `core.on('state')` heard each change, and each run start and end.
- A core database that the 0.2.6247 SDK wrote, with a schedule its agent
  made, opened with the schedule on and naming no turn.

## Prior art

- Temporal's schedules
  ([`ScheduleHandle`](https://nodejs.temporal.io/api/interfaces/client.ScheduleHandle))
  are made, described, updated, paused, unpaused, triggered now and deleted
  through a client, and listed for a namespace, each with its recent actions
  and next times
  ([`ScheduleSummary`](https://nodejs.temporal.io/api/interfaces/client.ScheduleSummary)),
  as a conversation's summary lists its schedules with the last run and the
  next. Jush's schedule belongs to its conversation, so its routes sit under
  the conversation's.
- Kubernetes's CronJob is suspended with `spec.suspend`, run now with
  `kubectl create job --from=cronjob/<name>`, and keeps the jobs it started,
  each naming the CronJob that owns it.
- GitHub Actions runs a scheduled workflow now on `workflow_dispatch`, and
  each run names its event.

## Alternatives

- A `GET /schedules` that lists every schedule, with the summary keeping
  `scheduled`: a program would learn of a change by asking again, and the
  page and a program would read two lists.
- Fields of the program's own on a schedule, such as Temporal's memo, to
  say who asked: what Jush knows already, the turn, answers it, and a
  program keeps what else it needs under the schedule's id.
- `jush schedule` acting on another conversation: an agent would wake a
  conversation it does not work in, which the group's guard rests on.

## Open questions

None.
