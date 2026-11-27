---
status: proposed
---

# A schedule ends at a time, and its end wakes the conversation once

## Problem

Alex's agent asks for access to a design document, then waits for it with
a schedule: `jush schedule create --watch './can-read.sh' --every 1m`. The
access may come in an hour, in a week, or never. A schedule has no end, so
the watch runs its command every minute until the agent deletes it. When
the access never comes, the conversation is compacted, or the agent
forgets, the watch runs for good, and nobody learns that the wait stopped
making sense. To bound the wait, the agent makes a second, once schedule
that deletes the first: two schedules that depend on each other, which the
agent forgets as easily.

## Decision

A schedule may name its end. At its end it stops, and it wakes its
conversation once with a message that says it ended, not that its trigger
fired, so the agent decides whether to remind someone, wait again or give
up. Until its end, it runs as it does today.

```text
$ jush schedule create --name "Design doc readable" --watch './can-read.sh' --every 1m --for 7d <<'EOF'
The design doc is readable now; go on with the review.
EOF
```

Seven days later, with no change of the command's output, the
conversation reads:

```text
Schedule “Design doc readable”, its end came at 2026-10-13 09:30: nothing
fires it any more, and it is deleted once this turn ends. Made at
2026-10-06 09:30, it never woke this conversation; its trigger was the
output of `./can-read.sh` changing, checked every 1 min.

Its message, which its runs send when its trigger fires:
The design doc is readable now; go on with the review.
```

A schedule that woke the conversation before says *it woke this
conversation 2 times, the last at 2026-10-09 10:03*. The message the
schedule's runs send stands last, named as what it waited for, so the
model does not read it as what to do now.

* `--for 7d` ends it a while from now, as `--every` counts (`10m`, `2h`,
  `1d`); `--until "2026-10-13 18:00"` at a local time in `--tz`, as `--at`
  takes it. `edit` takes either to set a new end, from now, and
  `--no-end` to remove it, as `--no-limit` removes a limit. `show` prints
  the end; `list` prints it beside the next run.
* Every trigger takes an end but a once one (`--at`), which ends when it
  fires. A trigger changed to a once one drops its end, as a trigger
  changed to a time drops its daily limit; one given with an end is
  refused. An end that has passed is refused.
* Until its end, a schedule fires as it does today, however often; a time
  at its end does not fire. From its end, nothing fires it: its times, its
  watch, its URL, which takes and holds the calls as a paused schedule's,
  and the conversation it waits for. A watch's check under way stops.
  Calls of its URL that came before its end, and still wait for their
  burst's quiet or a run's end, start their run then, before the run of
  its end.
* Its end is a run of its own, `cause: 'end'`, which wakes the
  conversation as any run does, queued or steering as the schedule says,
  after every run of the schedule that still works has ended, so its
  message comes after theirs. Once its message reached the conversation,
  the schedule is deleted with its runs when the turn it started ends,
  whatever became of that turn, as a once schedule is once it fired; an
  agent or a program that sets a new end before that keeps it, and the new
  end ends it again.
* A schedule that is off at its end, paused by the user or the agent or
  held while its conversation is archived, is deleted without waking it,
  as nothing of it wakes the conversation while it is off.
* A schedule whose end's run was skipped for a device that is offline, or
  failed before its message went, as in a conversation with no model,
  stays, and its menu row says why, as a once schedule's does; nothing
  fires it until `edit` gives it a new end, from which it counts as one
  turned on does. A run that failed so names no turn, once schedules'
  included, whose failed run deleted them before.
* An end that passed while the core was down wakes the conversation once
  it starts, after the last time the schedule missed, as a time does.
* The Schedules menu says the end after the trigger: *When output changes
  · 1 min · Until Oct 13 09:30*, its weekday instead within the coming week,
  *Until Thu 09:30*.

### The design to build from

* The web API: `ScheduleDto` and `ConversationSchedule` gain
  `endsAt: Timestamp | null`; `SchedulePatch` gains `endsAt`, null to
  remove it, as `maxRunsPerDay` takes null. `RunCause` gains `end`. A
  program's `conversation.schedules.create` and `update` take it as they
  take every field.
* The core's database: `schedules` gains `ends_at INTEGER`, null for none,
  and `schedule_runs.cause` takes `end`. Whether a schedule's end ran is
  read from its runs: a run of cause `end` at or after its end.
* The scheduler wakes at the nearest end among the schedules, beside the
  nearest time.
* The message is the run's first line, the trigger in words, so an agent
  whose conversation was compacted knows what it waited for, and the
  message its runs sent.
* An older core's database gains the column, empty, and the cause; an
  older page reads no end and shows none.

## Spike

None: the end is a time the scheduler already sleeps until, and its run
is a run as the others are.

## Prior art

* Amazon EventBridge Scheduler's schedules take an `EndDate`, after which a
  rate or cron schedule stops, and with `ActionAfterCompletion: DELETE` the
  schedule is deleted after its last run; nothing is told of the end
  ([Deleting a schedule](https://docs.aws.amazon.com/scheduler/latest/UserGuide/managing-schedule-delete.html)).
* Temporal's schedules take an end time in their spec, after which they
  take no action ([Schedules](https://docs.temporal.io/schedule)).
* Airflow's sensors, which wait for a condition, take a `timeout`; at it
  the sensor fails, or with `soft_fail` is skipped, so what follows can
  tell a wait that timed out from one that was met
  ([`airflow.sensors.base`](https://airflow.apache.org/docs/apache-airflow/2.10.2/_modules/airflow/sensors/base.html)).
* Jush takes EventBridge's end that deletes the schedule, and tells the
  conversation as Airflow tells what follows the sensor: the agent that
  waited is the one that acts on the wait's end.

## Alternatives

* **An end that deletes the schedule without waking the conversation**, as
  EventBridge's: the agent never learns that the wait ended, which is the
  case this decision is for.
* **A schedule that stays, ended, after its end**, so `edit` could wake
  it again: a state beside on and off that every list and menu then
  shows, for what making a schedule again does.
* **A message of its own for the end (`--on-end`)**, as Power Automate's
  steps run after one that timed out: every schedule with an end then
  needs two messages, while the end's first line and the schedule's own
  message tell the agent what it waited for and that the wait ended.
* **Ending a schedule when its trigger first fires**, as RFC 5545's `COUNT`
  ends a recurrence: a wait that was met behaves as it does today; the
  agent deletes the schedule when it has what it waited for.

## Open questions

None.
