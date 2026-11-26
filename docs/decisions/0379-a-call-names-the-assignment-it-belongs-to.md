---
status: proposed, superseded in part by 0383
---

# A call names the assignment it belongs to

## Problem

A program charts what each kind of work costs a day: a person's message, a
schedule it made, a chat event it listens to. Each hands the agent work
that goes on after its turn ends: a command in the background reports, a
subagent ends, the agent's own schedule fires, a stopped turn resumes. The
program counts the turns those open as the work that started them.

`core.usage.report` groups by turn, but a turn Jush opens has an id of its
own, and nothing says which earlier turn it continues. The program reads
each conversation's transcript and orders its input blocks to tell.

## Decision

Each call of a model names its assignment: the id of the message that
handed the agent the work the call is part of.

```ts
await conversation.send('Summarize the incident', 'listener:ops-alerts:8812', { senderId: 'person-7' })

const report = await core.usage.report({
  from, to, bucket: 'day', timeZone: 'Asia/Shanghai',
  groupBy: ['conversation', 'assignment', 'model'],
})
// group.assignment === 'listener:ops-alerts:8812' for the turn the message
// opened, and for the turns its background command and subagents open later.
```

- An assignment opens with a message that opens a turn: one the program or
  a page sends to the conversation or to a subagent, a steer that no
  running turn took, a side question and each of its follow-ups, and the
  message of a run of a schedule the program or the user made. Its id is
  the message's: the id the program sent it under, or the run's `turn`.
- A follow-up takes an id, as `send` does, which it is taken once under
  and which the conversation's messages share:
  `sideQuestions.followUp(side, input, messageId, { senderId })`, its
  options moving from the third argument to the fourth. It had none, so
  its assignment's id was one only Jush knew.
- What continues work keeps its assignment, as it keeps its sender (0372):
  - a turn a command's report, a subagent's end or another conversation's
    end opens: the assignment of the work that ended;
  - a run of a schedule an agent made: the assignment of the turn that made
    it, whose sender it keeps too, where it named none before; a run of one
    that names none, as one made before this, opens its own;
  - a retry, a resume and a compaction;
  - a subagent an agent starts, a command's calls to a vendor, and the work
    `jush agent send`, `jush conversation send` and `new` hand another
    agent: the assignment of the call that asked for it.
- A steer the running turn reads joins its assignment: its sender's calls
  after it are counted in the assignment the turn had.
- What continues work that names no assignment names none either, as work
  begun before the update does: a message that opens an assignment names
  its own id, so no call is counted under an id only Jush knows, such as
  that of a subagent's first message or of a compaction's instruction.
- A title's and the suggested replies' calls name no assignment, as they
  name no sender.
- `groupBy` takes `assignment`. A program counts assignments by the ids
  that open them, which it knows: its own messages' ids, and its schedules'
  runs' `turn`.
- It is not named a task, which a schedule's repeat already names, nor a
  trace, which names a vendor's request.
- Stored data grows by the assignment alone, absent before: the blocks
  (format 22: a message, a steer, a report and an agent's message), the
  checkpoints (format 13: a queued message, and the agent messages and
  reports that wait), a conversation's running and ended commands, the
  usage ledger's rows, a kept wake, a media job's kept row (format 3), and
  a schedule an agent made, with its sender. Nothing stored is rewritten;
  what was counted before names no assignment.
- The model does not read it.

## Spike

A program drove a scripted model through the SDK, on Bun and Node, and
grouped each conversation's calls by agent, sender, assignment and purpose:

- A background command of the message `listener:ops-alerts:8812` ended
  after Bob's message: its turn's 3 calls are in that assignment, Bob's in
  his. One that ended inside Bob's running turn left its calls in his.
- A subagent's calls, and the turn its end opened, are in the assignment
  of the message that started it, as are those of the conversation that
  `jush conversation send --wake-when-done` and `jush conversation new`
  started, and the turn the first one's end opened.
- After Bob's turn ran `jush agent send 1`, the subagent's calls were in
  Bob's assignment; `sendToAgent(child, …, 'reminder-7')` twice resumed it
  once, in `reminder-7`, and so was the turn its end opened.
- A run of a schedule the agent made, started with `schedules.run`, is in
  the assignment of the turn that made it, and its sender's; a run of the
  program's schedule is in the run's `turn`.
- A steer's calls are in the running turn's assignment, under the steer's
  sender. A stopped turn's resume, and a compaction after it, are in the
  turn's.
- A side question is in its own id, and a follow-up sent twice under one
  id ran once, in that id. Titles and suggested replies name none.
- Cores made by 0.2.6252, 0.2.6254 and 0.2.6342 open, and the calls they
  counted name no assignment.

## Prior art

- OpenTelemetry carries a trace's id from the request that starts it to
  every span below, across queues and background work, and its backends sum
  a trace's cost and time
  ([Traces](https://opentelemetry.io/docs/concepts/signals/traces/)).
- Langfuse, open source, groups a task's nested model calls under one
  trace and reports their cost by trace
  ([Tracing](https://langfuse.com/docs/observability/overview)).
- Jush departs in naming the assignment by the message that opened it,
  which the program chose, rather than by an id of Jush's.

## Alternatives

- Tags a message carries, which the turns after it inherit, as LiteLLM
  counts spend by a request's tags: the program would still map each tag to
  an assignment to count them, and the assignment's id already names its
  tag.
- Each turn naming the turn it continues: the program would follow the
  chain itself, for every call.

## Open questions

None.
