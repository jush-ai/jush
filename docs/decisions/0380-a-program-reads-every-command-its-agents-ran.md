---
status: proposed, superseded in part by 0383
---

# A program reads every command its agents ran

## Problem

A team's program charts, every five minutes, the shell commands the agents
of its conversations ran: which conversation and which agent ran each,
its script, when it started and ended, and whether it failed. A month
holds a few hundred thousand, and a chart rebuilt from scratch needs the
last 30 days.

Each conversation keeps its commands in its own database: the calls in its
transcript, and each ended command's record. Nothing lists them across
conversations. The program pages through every conversation's transcript,
reads each long script block by block, and still lacks a background
command's end: its report carries the exit code, not when it ended. A call
that failed before its command started, such as one naming a device that
is offline, appears only as an error in a tool result.

## Decision

The core keeps a command history, as it keeps a usage ledger, and a program
reads it in the order its rows were written:

```ts
let page = await core.commands.history({ from: new Date(Date.now() - 30 * 86_400_000), limit: 500 })
for (;;) {
  for (const command of page.commands) chart.add(command)
  // { conversation: '…', agent: 1, command: '42', device: '…', turn: '…',
  //   sender: 'alex', assignment: 'chat:alex:17', title: 'Run the tests',
  //   script: 'bun test', startedAt: '…', endedAt: '…',
  //   end: { kind: 'exited', exitCode: 1 } }
  await chart.save(page.next)
  if (!page.more) break
  page = await core.commands.history({ after: page.next, limit: 500 })
}
// Five minutes later, from where it stopped:
page = await core.commands.history({ after: await chart.cursor() })
```

- A row is written for each command an agent's `shell` call started, when
  the command ends, however long after the call: it exited with its code,
  was stopped, or was lost, with the reason. A `shell` call that returned
  an error before its command started, such as one with a refused input or
  a device that is offline, has a row too, `not_started` with the error,
  and no command or start. A call the user's Send Now kept from running
  ran nothing and failed nothing, and has none. A command that ended
  before its job started has the end its conversation records: `stopped`
  for a Stop or a detach of its device, exited 127 when the conversation's
  device access refused it, such as after a switch of its device.
- A row names its conversation; its agent as the usage ledger names it, 0
  for the main agent and null for a side question; its command's id, which
  `conversation.history.commandOutput` takes; the device it ran on; the
  turn of the call; whom and which assignment the call's work was for
  (0372, 0379); the call's title and whole script, as the command's record
  holds them from its call on; when the command started, a wait for its
  device to come online included, and when it ended.
- `history` answers `{ commands, next, more }`. `after` continues after a
  page's `next`; `from` starts at the first row written of those that ended
  at or after a moment, and each row written after it follows; with
  neither, the first row; with both, the query is refused. `next` is where
  the page ends, always a place to read on from: the last row's, the
  `after` given for an empty page, the newest row's when nothing ended at
  `from` or after, or the place before the first row of a history that has
  none. It is opaque, and a cursor past every row this core's history
  wrote is refused, such as another core's. `more` says rows wait after
  the page. `limit` is 1 to 1,000, 100 by default: the core reads a page
  whole while it holds its database and sends it whole.
- Rows are in the order they were written, which is the order their
  commands ended: a reader that follows `next` reads each row once, also
  rows written while it reads.
- The history is a table of the core's database, `command_history`, beside
  `usage_ledger`. It keeps each script again, as the usage ledger keeps
  tokens the transcripts hold, so a reader opens no conversation's
  database.
- A command's row is written before its conversation records its end: a
  backend that stops in between finds the command running, ends it again,
  and the history keeps its first row, one row per command. So a row can
  be read a moment before `commandOutput` has the command's output.
- A conversation's deletion removes its rows with it, since they hold its
  scripts, as its transcript goes; the usage ledger's rows stay, as
  before. A row is not written for a conversation that is gone, as a
  command a deletion stopped ends after it. A fork copies no row: its
  commands ran once.
- Commands that ended before the update have no row. A program's
  `device.exec` (0350) and a schedule's watch command are not an agent's
  calls and have none.

## Spike

A program drove a scripted model through the SDK and read the history on
Bun, and on Node from a copy of the same core:

- Two commands of one step exited 0 and 3; a background command's row
  came once it ended, 3 seconds after it started and after its turn; a
  service the agent stopped with `jush shell stop`, and a foreground
  command the program's abort stopped, ended `stopped`.
- A call naming a device that does not exist and one asking for both the
  background and a service had rows `not_started` with their errors, and
  no command, device or start.
- A subagent's command counted as agent 1, a side question's as null, each
  with its message's sender and assignment.
- A background command whose runner and job processes were killed ended
  `lost`, "the device's runner started anew after it ended", once the
  runner was spawned again. Killing the runner alone lost nothing: the job
  went on under the next runner.
- Pages of 3 read all 12 rows once; `after` the last `next` and `from` an
  hour ahead answered no row and the newest row's place; `after` with
  `from`, a cursor that is no number or lies past the newest row, and a
  limit of 0 or 1,001 were refused. An empty history answered `next` "0",
  which `after` takes.
- Two rows written in the opposite order of their ends, the first ending a
  millisecond after `from` and the second at it, were both read from
  `from`.
- A command a Stop ended while its job waited to start, which a delay held
  open, kept its call's script and turn.
- A deletion removed its conversation's rows, and a background command it
  stopped left none; the next row's id came after the deleted ones'.
- Cores made by 0.2.6252, 0.2.6254 and 0.2.6342 open with an empty history.

## Prior art

- Shell history in Atuin, open source, keeps each command with when it
  ran, how long, its exit code, its directory and its host, in a SQLite
  table indexed by time ([Atuin](https://github.com/atuinsh/atuin)).
- GitHub Actions answers a run's jobs with their status, conclusion
  (success, failure, cancelled, skipped), start and completion
  ([Workflow jobs](https://docs.github.com/en/rest/actions/workflow-jobs)).
- OpenTelemetry names a command's line and exit code, and counts a
  non-zero code as an error
  ([CLI spans](https://opentelemetry.io/docs/specs/semconv/cli/cli-spans/),
  [process attributes](https://opentelemetry.io/docs/specs/semconv/registry/attributes/process/)).
- GitHub's audit log pages with an opaque cursor, `after`
  ([Audit log API](https://docs.github.com/en/enterprise-cloud@latest/admin/monitoring-activity-in-your-enterprise/reviewing-audit-logs-for-your-enterprise/using-the-audit-log-api-for-your-enterprise)),
  and Stripe's lists answer `has_more`.
- Jush departs from shell history in keeping a call that never started a
  command, which an agent's tool can make and a shell cannot.

## Alternatives

- An index of commands without their scripts, the program reading each
  script from its conversation: it stores less, but each read opens every
  conversation that ran a command since the last.
- The usage report's shape, totals by period: the program needs each
  command, not sums.
- Pages by time alone: two commands that end in the same millisecond, or a
  clock set back, make a reader skip or repeat a row; the cursor does not.

## Open questions

None.
