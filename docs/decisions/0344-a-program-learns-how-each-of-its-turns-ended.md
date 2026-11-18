---
status: proposed
---

# A program learns how each of its turns ended, whatever it missed

## Problem

A program sends two messages: `run('Summarize the logs')` starts a turn, and
`run('Draft the notes')` waits in the queue behind it. The program's
connection to the core drops for ten seconds, and both turns end meanwhile.
When the connection is back, the first `run()` resolves with the text of
both turns, "The logs show three timeouts.\n\nNotes for Monday: …", and with
the second turn's outcome: had the first failed and the second finished, the
program reads that the first finished.

The SDK takes a turn as ended once the conversation's `lastTurn`, the latest
ended turn its metadata names, ends after the turn's message. `lastTurn`
names only the latest turn, and only by the block that ended it, so once two
turns ended unseen, nothing the program reads says where the first ended or
how. The blocks hold it, but which block ends a turn is the core's rule
(`ends_turn` in `agent-transcript`, the summary's walk in `core-store`),
which a program does not have.

The same view decides that a message left the queue without running: the
root idle, and the message neither queued, nor starting, nor in the
transcript. That is a guess from what the lane holds, and a lane that holds
less than the conversation, as after a drop, makes it wrong.

## Decision

The conversation answers for each of its turns by the id of the message that
started it, the id the program sent it under.

### A turn keeps one id

A turn's input blocks name it: the message's `user` block, and the context,
steers, command reports and agent messages written while it runs. Today a
Resume, the user's or the one Jush makes after a backend restart, runs under
an id of its own, so one turn's blocks carry two ids, and a reader that
follows a turn's id stops at the Resume. A Resume also marks the latest stop
it holds as continued, which after a failed turn can be an earlier turn's:
that turn's Stopped marker then leaves the transcript.

A Resume now writes the turn it continues, the root's last turn, from its
start, and marks only the stop at its resume point. A Retry, which took over
the turn it reruns only once it had cut the old answer, writes it from its
start too, so a turn being rerun or continued never reads as ended. A turn
resumed before this change keeps the Resume's own id, and is read as it
stood before the Resume.

What else follows from one id:

- The message can be edited while its Resume runs, as while its Retry does.
- `jush conversation history cost` counts a Resume's requests toward the
  turn, and `jush conversation history timing` shows the turn as one row,
  without the time it stood stopped or failed until the Resume.

### `lastTurn` names its turn

`LastTurn` gains the turn it ended:

```rust
pub struct LastTurn {
    /// The block that ended the turn, so a turn that a Resume continues
    /// ends anew.
    pub id: BlockId,
    /// The turn it ended, which the turn's input blocks name: for a turn a
    /// message started, the message's id, which a Resume of the turn keeps;
    /// a turn Jush started for a command's report or an agent's message has
    /// an id of its own.
    pub turn_id: TurnId,
    pub outcome: TurnOutcome,
}
```

The turn is the one of the last input block before the end, which the index
of block types finds without reading the turn's blocks. An end that no
turn's input precedes ends no turn.

### A turn is read by its id

`GET /conversations/{id}/turns/{turnId}` answers the root's turn `turnId`:

```ts
type TurnState =
  // The message waits in the root's queue.
  | { state: 'queued' }
  // Its turn runs: from taking the message to its end, and again while a
  // Resume continues it.
  | { state: 'running' }
  // It ended, at the block `end`.
  | { state: 'ended'; end: string; outcome: 'finished' | 'failed' | 'stopped' }
```

The turn lasts from the `user` block that opened it on the root's current
path until an input block of another turn. `ended` names the block that
ended it and how, by the rule `lastTurn` follows: the turn's last
`response`, its last `error` unless that ended no turn, or its last `abort`
unless a Resume continued it. A conversation that holds no turn of the id
answers 404 `turn_not_found`: a message taken out of the queue or cleared, a
message a Stop gave back, one stopped before its turn wrote anything, which
never ended, and an id never sent. A turn Jush started for a command's report
or an agent's message has no message, and answers 404 too.

A conversation whose tree runs answers `queued` and `running` from the
root's session: `running` until the closing save of the turn's action has
committed, as the root shows it. Every other answer comes from the store,
which by then holds the turn's end. A message no `user` block holds is
`queued` while the root's checkpoint holds it in its queue, as a restored
root's queue is until its session takes it back. A conversation no tree runs
answers from the store alone, and its root's last turn is `running` while
the checkpoint's phase is not idle, as when the process died within it. A
root restored after that answers the turn `running` from its session until
the interruption it writes of the turn is saved: until then the store holds
an earlier `response` of the turn as its last block.

### A schedule's run ends by the same reading

A schedule's run waits for the turn its message started. It read that turn
by a rule of its own: the blocks after the message through the next
message, ended once the root rested. It now reads the turn as the route
does:

- The run ends with the turn: failed with the error that ended it, or done
  with the turn's last text, which a stopped turn gives too.
- A message that left the queue without running, as one the user took out,
  skips the run. Before, the run stayed running for good.

### `run()` asks when it cannot tell

- `run()` resolves when `lastTurn.turnId` is its message's, with the blocks
  after the message up to `lastTurn.id`.
- When the lane's view leaves its turn unaccounted for, the root idle and the
  message neither queued, nor starting, nor the last ended turn's, it reads
  the turn. `ended` resolves it with the blocks up to `end`, once the
  transcript holds that block. `queued` and `running` wait on.
  `turn_not_found` fails it: "The message left the queue without running".
- The view decides only when to ask; the conversation's answer decides how
  `run()` ends. It asks once for each such view, and the latest answer
  counts. A read that fails, or answers `queued` or `running` while the view
  shows neither, is made again after the backoff the lane reconnects with,
  since an idle conversation may change no more.
- A turn longer than the page the lane reads after a drop ends past what the
  lane holds: `run()` reads the pages before it until it holds the message.

### Older pages and processes

The web app ignores `turnId`, as its schemas drop the fields they do not
name. A program drives a process of its own release, as the SDK starts one
from its own platform package; a program that connects to an older process,
which sends no `turnId`, fails to read its conversations' metadata, as with
any field a release adds.

## Spike

None. The lost end was reproduced before the design: a queued `run()`
across a dropped connection resolved with both turns' text. The rest reads
what the core stores already.

## Prior art

- Codex's app-server protocol keeps a thread's turns as a collection a
  client reads back: each
  [`Turn`](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/thread_data.rs)
  has an `id` and a `status`, `completed`, `interrupted`, `failed` or
  `inProgress`, and a client that resumes a thread reads them with
  `thread/resume` or `thread/turns/list`. Jush reads one turn by the id the
  program already holds, since a program asks only for the turns it waits
  for.
- OpenAI's Responses API in background mode lets a client that lost its
  stream poll `GET /v1/responses/{id}` until the status leaves `queued` and
  `in_progress`
  ([Background mode](https://developers.openai.com/api/docs/guides/background)).
  A Jush turn is read the same way, by an id the client chose, with Jush's
  own outcomes.
- ACP answers a `session/prompt` request with the turn's `stopReason` when
  the turn ends, on the connection that sent it. A connection that drops
  loses the answer, which is the case this decision covers.

## Alternatives

- **The SDK finds a turn's end in the blocks.** It would be a second
  implementation of `ends_turn` and of which input starts a turn, in
  TypeScript beside the core's, and the two would drift.
- **The metadata names every ended turn, or the last few.** Every page would
  carry a list that grows with the conversation, or a limit that a long
  enough drop still overruns.
- **The core replays the turn ends a lane missed.** A lane keeps versions of
  collections, not a log of events; a log for turn ends alone would be a
  second history beside the transcript.
- **Each terminal block stores its turn's outcome.** The outcome follows from
  the block's type and flags, which a stored copy would duplicate.
- **A turn is read by its request.** Every block names the request its
  `user` block starts, so the route could answer by request with no change
  to the core. A request also holds the turns Jush starts after the
  message's turn ended, for a command's report: a program that missed the
  end would get the later turn's answer, and one that saw it the first.
- **Readers follow a Resume's own id back to the turn it continues.** The
  summary, the session and the store would each need that chain, where one
  id per turn needs none.

## Open questions

None.
