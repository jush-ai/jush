---
status: accepted
---

# A conversation hears when another one finishes

Supersedes: 0088 (An agent sends a message to another conversation), the
part that says there is no reply mechanism and that Jush never waits for
or forwards a reply.

## Problem

Alex's conversation *Coordinate the release* starts *Fix the cart total*
with `jush conversation new` and asks it to say when the fix is done.
The agent there fixes the cart, answers, and forgets to run `jush
conversation send` back. *Coordinate the release* never learns that the
fix is done, and waits until Alex looks.

The same happens without any message. Alex works in *Fix the cart total*
directly, and *Coordinate the release* should merge each time that
conversation finishes. Nothing tells it when. `jush conversation list`
says only when each conversation last changed, which also changes while it
works, and no command says whether a conversation still works, so even a
schedule that watches a command's output (`--watch`) has nothing to watch.

## Decision

A conversation hears when another one finishes in two ways:

* **Waking when one message is done.** `jush conversation send
  --wake-when-done` and `jush conversation new --wake-when-done` wake the
  agent that sent the message when the work the message starts or joins
  ends, with the outcome and last answer of its last turn, as a subagent's
  end wakes its parent.
* **Listening to every turn.** A schedule with the trigger `--when-done
  <conversation>` runs each time that conversation finishes working,
  whoever asked for the work, until the schedule is deleted.

`jush conversation list` also shows each conversation's status, so an agent
can see at once whether a conversation still works.

### Waking when one message is done

```text
Conversation "Coordinate the release", a command of its agent:

  $ jush conversation new --wake-when-done <<'EOF'
    Fix the cart total rounding and run the checkout tests.
  EOF
  Started conversation “Fix the cart total” (6f1c…). This agent wakes when
  the work the message starts there ends.

Later, the same conversation's main agent's transcript:

  ✓ Finished: Fix the cart total                                   12:41
```

* The agent wakes with a notice, which shows as a receipt row, as a
  subagent's end does: *Finished*, *Failed* or *Stopped*, then the other
  conversation's title, which opens it at its end. The model reads the
  outcome and the last answer of its last turn:

  ```text
  The work a message from this agent started in conversation “Fix the
  cart total” (6f1c…) ended; its last turn ended with an answer:

  The rounding is fixed and the checkout tests pass.
  ```

  A failed turn says its error message instead, and a stopped one says
  that the user stopped it. An answer over 16 KB is cut, and the notice
  says that `jush conversation history --conversation 6f1c…` reads the
  rest: the notice goes into the agent's context window, which the vendor
  limits, as a schedule's message bounds a command's output.
* The work ends when the recipient rests after the message, as a
  `--when-done` schedule fires: no turn runs, and no subagent or command
  it waits for. A busy recipient takes the message into its running work,
  and an idle one starts a turn with it. A recipient that starts a subagent
  and ends its turn to wait has not finished: its sender wakes when the
  subagent's result is handled and the recipient rests. An agent that sends
  several messages with `--wake-when-done` into one stretch of work wakes
  once.
* The notice goes to the agent that sent the message. When that agent has
  closed since, it goes to the nearest agent above it that still runs, up
  to the main agent, as a permission decision does. Like any agent message,
  it steers a busy agent and wakes an idle one.
* The recipient's agent reads that its sender will hear the end: "When the
  work it asks for ends, your last answer goes back to that conversation;
  send to it only what it needs before then." A message without
  `--wake-when-done` keeps today's line.
* A recipient deleted before the work ends sends a notice saying so once it
  is deleted, and so does one whose user edited the conversation before the
  message, which removes it. A sender deleted or archived before then hears
  nothing.
* A notice that cannot be delivered, such as to a sender whose device is
  away, is tried again at the next rest of any of the user's conversations
  and at the backend's start, and its agent takes it once.
* Without `--wake-when-done`, nothing changes: the recipient answers, if it
  does, with `jush conversation send`.

### Listening to every turn

```text
  $ jush schedule create --name "Merge the cart fix" \
      --when-done 6f1c… <<'EOF'
    Review what it did and merge it if the tests pass.
  EOF

Schedules menu of "Coordinate the release":

  ┌──────────────────────────────────────────────────────┐
  │ ▢ Merge the cart fix                                 │
  │   When “Fix the cart total” finishes · Last at 12:41 │
  └──────────────────────────────────────────────────────┘
```

* The schedule runs each time the conversation finishes working: when it
  rests after work that ended a turn, whether the user's message, a
  schedule or another conversation asked for it. Work that ends no turn,
  such as a compaction, runs nothing. One rest is one run, however many
  turns the work held, and the run carries the last turn's end.
* The run's message follows the schedule's line, the prompt and the same
  outcome and answer as a notice:

  ```text
  Schedule “Merge the cart fix”, conversation “Fix the cart total” (6f1c…)
  finished at 12:41

  Review what it did and merge it if the tests pass.

  The last turn of conversation “Fix the cart total” (6f1c…) ended with an
  answer:

  The rounding is fixed and the checkout tests pass.
  ```
* As for a command watch or a URL, `--max-runs-per-day` limits its runs,
  and `--if-busy` says what a run does while this conversation works.
* The trigger names a conversation of the user other than this one that is
  not archived. A conversation archived later finishes no work, so the
  schedule runs nothing; one deleted later leaves the schedule, which runs
  nothing.
* A trigger that would close a loop is refused: when this conversation's
  end already runs the named one, through the user's when-done schedules,
  paused ones too, each would run the other without end.
* A rest that both ends the work of this conversation's message and ends a
  working stretch wakes it twice, once for each. An agent that listens to
  every turn does not need `--wake-when-done` too.
* The Schedules menu draws the trigger with a speech-bubbles icon and the
  words *When “<title>” finishes*, with the title the sidebar shows now in
  any workspace, in every language Jush speaks; for a conversation the
  sidebar does not list, *When an archived or deleted conversation
  finishes*.

### Who may do it

Neither asks a permission: the `conversation` group asks none, since it
reaches only the user's own conversations, which the user sees in the
sidebar, stops and archives, and a schedule wakes no one but its own
conversation.

### A conversation's status in the list

`jush conversation list` gains a column `STATUS`: the status the sidebar
shows, `running`, `compacting`, `resuming`, `awaiting_commands`,
`interrupted`, `error`, `stopped`, `completed` or `idle`.

### The design to build from

* `crates/backend-user-shard/src/conversation/organize.rs`: `SendArgs` and
  `NewArgs` gain a flag.

  ```rust
  /// Wake this agent when the work the message starts ends, with how its
  /// last turn ended and its last answer.
  #[serde(rename = "wake-when-done")]
  wake_when_done: Option<bool>,
  ```

  The message's sender says so to the recipient, which words the line the
  recipient's model reads (`SenderConversation::wakes`).
* A wake a message asked for is kept in the control database, once the
  message is admitted, until its sender is woken, so a restart of the
  backend loses none:

  ```rust
  /// A wake a message asked for, until its sender is woken.
  pub struct PendingWake {
      /// The conversation the message went to, and its block there.
      pub recipient: ConversationId,
      pub message: BlockId,
      /// Who wakes at the end.
      pub sender: ConversationId,
      pub sender_node: NodeId,
      /// The notice to deliver, once the work ended.
      pub notice: Option<AgentMessage>,
  }
  ```

  The shard settles them where it fires a when-done schedule, in
  `follow_turn_end`, at the rest after work: each open wake whose message's
  block comes before the last turn's end (`AgentSession::position`, through
  the store when the window let it go) keeps its notice, which then goes,
  and the record is deleted once it is delivered. A check after keeping the
  wake settles a rest that came before it. A message on neither the queue
  nor the path was removed by an edit. The notice is kept before it goes,
  so a failed delivery is tried again as the same message. The recipient's
  deletion reads its wakes before it stops the work and delivers them after
  its commit.
* The notice is an `AgentMessage` whose event is `Completion`, the notice
  of an end that its receiver asked for, with the sender naming the other
  conversation (`Sender::conversation`). Its block's id is
  `done:<recipient>:<end block>:<sender node>`, so a delivery repeated
  after a restart is the same block. `CompletionOutcome` stays: `Completed`
  for an answer, `Failed`, and `Aborted` for a turn the user stopped, a
  deletion or an edit. Its time is the end block's, so a repeated delivery
  is equal. It opens a turn the user asked for when the work it continues
  is the user's, as a subagent's end does. Delivery uses
  `PermissionShard::admit`, as `deliver` does.
* `crates/web-api-protocol/src/schedules.rs`: `Trigger` gains:

  ```rust
  /// When another conversation of the user finishes working: it rests
  /// after work that ended a turn.
  Done { conversation: ConversationId },
  ```

  `takes_limit` holds for it. `RunCause` gains `Done`. The shard fires
  it from `follow_turn_end`, at the rest that ended a turn, for every
  schedule of the user whose trigger names that conversation.
* One function words a turn's end, title, id, outcome and answer cut to
  16 KB, for both the notice and the run's message
  (`schedules/message.rs`).
* `web-ui`: `triggerWords` and `triggerIcon` take the new kind; the title
  comes from the sidebar's rows (`SendTargets.describe`), so a rename shows
  at once. The receipt row of a `Completion` from another conversation says
  *Conversation finished*, *failed* or *stopped*, names its title and opens
  it at its end. The gallery's Schedules specimen gains such a schedule,
  and its transcript a notice.
* Storage: the trigger is stored as JSON, so a new kind needs no migration
  of its own; the runs' table is rebuilt, since its check names the causes.
  The pending wakes are a new table, created empty. An older backend never
  meets either.

## Spike

None. Each part exists: a schedule's run already waits for its turn's end
through restarts, and an agent message already reaches any conversation of
the user.

## Prior art

* OpenClaw's `sessions_send`
  ([sessions-send-tool.ts](https://github.com/openclaw/openclaw/blob/main/src/agents/tools/sessions-send-tool.ts))
  returns the reply of the turn a send starts once: inline when the send
  waits for it, otherwise as a later input of the sending session. With
  `watch: true` it also registers a durable watch of the other session
  ([session-state-events.ts](https://github.com/openclaw/openclaw/blob/main/src/sessions/session-state-events.ts)),
  whose changes reach the watcher; the end of a run is recorded there but
  does not notify on its own.
* Claude Code's agent teams, which are not open source, send the lead an
  idle notification after every turn of a teammate. Users report that it
  carries no result and that the lead loops on them
  ([an analysis](https://dev.to/nwyin/reverse-engineering-claude-code-agent-teams-architecture-and-protocol-o49),
  [issue 47930](https://claudeissues.com/issue/47930-bug-agent-teams-lead-session-loops-on-idle-notifications-and-duplicate-task-assi)).
* Jush departs from both. A wake is asked for, never sent by default, so
  a message that only steers wakes nobody. It carries the answer, so the
  sender need not read the transcript. Listening to every turn is a
  schedule, which the user sees, pauses and deletes in the Schedules menu,
  and which limits its runs a day, rather than a watch hidden in the
  agent's state.

## Alternatives

* **Wake for every message's turn by default**, as OpenClaw does: a message
  that only steers, such as "Rebase on the new main", would wake its
  sender for nothing, and a recipient that also sends back would arrive
  twice.
* **A blocking `jush conversation wait <id>`**, run in the background so
  its end wakes the agent: it would hold nothing through a restart of the
  backend, end with an error the agent must notice and rerun, and listen to
  one turn only.
* **A watch of its own, `jush conversation watch <id>`**: a second kind of
  thing that wakes a conversation, which the user could not see or stop
  where schedules are.
* **Fire the schedule at every turn's end rather than at the rest**: a
  conversation working through three queued messages would wake the
  listener three times while it still works.

## Open questions

None.
