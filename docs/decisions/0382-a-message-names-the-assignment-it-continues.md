---
status: proposed, superseded in part by 0383
---

# A message names the assignment it continues

## Problem

A program follows something for a conversation and wakes it when that
changes. Alex asks the agent of a team chat to review a merge request; the
agent runs `acme review start`, and the program's plugin starts polling the
request. Hours later a commit lands, and the program sends the
conversation a message to review it again.

The work that message starts is Alex's review going on, as the turn a
background command's end opens is (0379). But a message the program sends
opens an assignment of its own id: each update of the request counts as
new work, and a review's cost is split across them. The plugin's call knew
the assignment, `call.invocation.caller.assignmentId`, and the program has
no way to hand it on.

## Decision

A message may name the assignment it continues, as it names its sender:

```ts
// As the review starts, the call names the work it is part of.
const caller = call.invocation.caller
// Hours later.
await conversation.send('A commit landed on !42; review it again.', 'acme:review-42:3', {
  senderId: caller?.senderId,
  assignmentId: caller?.assignmentId,
})
```

- `MessageOptions` takes `assignmentId` beside `senderId`, as `run`,
  `send`, `steer`, `sendToAgent`, `sideQuestions.ask` and `followUp` take
  them. A message that names one opens a turn in that assignment: its
  calls, the commands they start and the work that continues them are
  counted in it, in `usage.report` and in `commands.history`. A message
  that names none opens an assignment of its own id, as before. Its id
  stays its own, which it is taken once under.
- The sender is the program's to name, as before; the assignment names no
  sender.
- A steer the running turn reads joins the turn's assignment, as before;
  one that opens a turn of its own opens it in the assignment it names. A
  message to a subagent that is running joins its turn's; one that resumes
  it resumes it in the assignment it names.
- An assignment's id is a message's id, by the rules of one. Jush does not
  look it up: the program names the work its messages belong to.
- Frames and routes carry it as `assignmentId`: a lane's `send` and
  `steer`, the first message a conversation is created with,
  `POST /conversations/{id}/agents/{node}/messages`, and a side question's
  ask and follow-up. A page's view of a pending steer shows it. Nothing
  stored changes: a queued message and a pending steer hold the assignment
  as the work Jush continues does.
- A page a program serves names one as it names a sender: the program
  decides what its pages send.
- A core of an earlier release refuses a request that names one, whose
  fields it does not know; one that names none is the same as before.

## Spike

A program's plugin kept the caller of `acme review start`, which Alex's
message `chat:alex:1` ran, and the program, after Bob's message, woke the
conversation with Alex's sender and assignment, over a scripted model:

- The call named `assignmentId: 'chat:alex:1'` and `senderId: 'alex'`.
- The turn the program's message opened, and the command it ran, were
  counted in `chat:alex:1`, Alex's, in `usage.report` and in
  `commands.history`; Bob's turn stayed in `chat:bob:1`.
- A steer it sent while nothing ran opened a turn in `chat:alex:1`; one it
  sent while Bob's next turn ran joined Bob's turn's assignment under
  Alex's sender.
- `sendToAgent` with Alex's assignment resumed a subagent that had ended in
  `chat:alex:1`, and so was the turn its end opened.

## Prior art

- W3C Trace Context carries a trace's context in each request, and
  OpenTelemetry propagates it across processes and queues, so the work a
  callback or a consumer does joins the trace that asked for it
  ([Trace Context](https://www.w3.org/TR/trace-context/),
  [context propagation](https://opentelemetry.io/docs/concepts/context-propagation/)).
- Langfuse lets a caller name the trace a call joins, by its id.

## Alternatives

- A token the core keeps for a command's call, which a later message
  hands back: the core keeps state for each, with a lifetime it chooses,
  and a program that starts following something outside a command call has
  none.
- A message that names an earlier message, whose assignment the core
  looks up: the core reads other conversations for it and decides what a
  message that is gone means, for the same answer.

## Open questions

None.
