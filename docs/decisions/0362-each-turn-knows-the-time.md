---
status: proposed
---

# Each turn knows the user's time

## Problem

Alex, in `Asia/Shanghai`, asks on a Monday morning for a summary of last
week's commits. The agent knows only what its training suggests the date
is: no system prompt, context block or message tells it the date, the time
or Alex's time zone. It guesses a year, or runs `date` on the conversation's
device, a server that answers in UTC, where it is still Sunday night. A
schedule that wakes the agent at 09:00, a subagent's report after an hour,
and a turn resumed after a restart tell it no time either. A Claude Code
subscription is the exception: Claude Code writes a date into its own
prompt, and the other providers' conversations do not have it.

## Decision

**The first request of each turn, and the first after the turn resumes,
carries a context block of the user's local time, after the turn's input:**

```text
Now: Tuesday, 2026-11-24 09:12 in the user's time zone, Asia/Shanghai (UTC+08:00).
```

- **Every turn.** A turn the user starts, and one a schedule, a subagent's
  report, a restart's resume or a permission decision starts, each gets the
  block before its first request. So does a subagent's turn and a side
  question's.
- **A resume** (the user's Resume, a retry after a failure, a restart)
  gets a new block: hours may have passed since the turn began.
- **A compaction** within a turn drops the turn's block with what it
  summarizes, so the next request has the block again.
- **The zone** is the one the user's page last reported, which schedules
  and history searches use too. The minute is the request's, by the
  backend's clock. Before any page reported a zone, or for one this build
  does not know, the block gives UTC and says so, so the model does not take
  UTC for the user's zone:

  ```text
  Now: Tuesday, 2026-11-24 01:12 UTC; the user's time zone is not known.
  ```
- **The zone database.** The backend reads zones from the system's database
  and, where there is none, from one built into it. Jush's container image
  has no system database, so until now every zone lookup there failed: a
  schedule's zone too, and this block would say the zone is not known.
- **The block is hidden** on the page, as every context block is.
- Nothing is added to the system prompt, so its cache holds across days;
  the block comes after the turn's input, as the execution context's does.

**The design to build from.** A new context source, `user_time`, in the
user's shard. To know whether the turn already has its block, a source
sees, for each of its blocks the model receives, whether the running
action wrote it. A turn, a resume, a retry and a rerun of a turn after a
restart each start an action of their own:

```rust
/// One of a source's blocks the model receives, oldest first.
pub struct SeenBlock<'a> {
    pub text: &'a str,
    /// Written by the node's running action.
    pub current: bool,
}

fn context<'a>(
    &'a self,
    node: NodeContext<'a>,
    turn: &'a TurnId,
    seen: &'a [SeenBlock<'a>],
) -> LocalBoxFuture<'a, Result<Option<ContextAnswer>, String>>;
```

The session remembers the context blocks the running action writes, and
marks those as current. The other sources read the texts as today. The
`user_time` source answers when none of its blocks is current. Nothing is stored
beyond the blocks themselves, and an older page or runner sees no change.

## Spike

None.

## Prior art

- **Codex** puts the date and the IANA zone in its environment context,
  a user-role message, and sends a new one when they change; an experiment
  adds a reminder of the time in UTC after user and tool messages
  ([`world_state/environment.rs`](https://github.com/openai/codex),
  `session/time_reminder.rs`).
- **Goose** adds a `<turn-context>` block with the local time to the minute
  to every turn, and its system prompt says only the latest one is current
  (`crates/goose/src/agents/moim.rs`).
- **OpenClaw** gives every turn the date and the user's zone outside the
  system prompt, so the prefix stays cached across days, and stamps each
  inbound message with its time
  ([Date and time](https://cdn.jsdelivr.net/npm/openclaw@2026.7.1-2/docs/date-time.md)).
- **OpenCode** and **Gemini CLI** give the date only, OpenCode again when it
  changes.
- Claude Code and Codex Desktop were reported to give the UTC date to users
  east and west of UTC, a day off for hours each day
  ([claude-code#4514](https://github.com/anthropics/claude-code/issues/4514),
  [codex#26524](https://github.com/openai/codex/issues/26524)). Jush takes
  the user's zone, never the server's or a device's.

## Alternatives

- **The date only, again when it changes** (OpenCode, Codex): no time of
  day and no sense of how long ago anything was.
- **A time on every message** (OpenClaw's envelopes): precise, but each
  input kind needs its own rendering, and a change of zone would change
  every message's text and lose the cache.
- **The date in the system prompt** (Claude Code): the whole cache goes
  each day.

## Open questions

None.
