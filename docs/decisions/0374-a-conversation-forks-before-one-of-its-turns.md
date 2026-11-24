---
status: proposed
---

# A conversation forks before one of its turns

## Problem

A conversation forks only through one of its completed assistant texts. A
turn that ends with commands has none after them: a help desk's agent says
"Checking", runs three commands, and posts its reply with a fourth. A fork of
everything before the next turn has to cut at "Checking", and loses the
commands, their output and the reply the team read.

## Decision

A fork takes either end:

```ts
const copy = await conversation.fork({ before: turnId })   // everything before that turn
const other = await conversation.fork({ through: blockId }) // through a completed text, as today
```

- `before` names a turn of the root by the id of the message that started
  it, the id a program sent the message under, which `run` answers as
  `turn.id`; the fork holds every block before that message, and not that
  turn. A turn no message started, one that continued after a command, has
  no such message, and the fork is refused; so is a turn whose message
  still waits in the queue, which has not started. Before the first turn,
  the fork holds no history.
- The cut is the one an edit makes before the message it edits
  (`before_input`, `crates/agent-transcript/src/cut.rs`), at a `user` block
  alone, never a steer inside a turn, so the history holds no call left
  unanswered: it ends where the source's own history went on to that
  message, after the turn before it, or after a turn a message sent at
  once interrupted.
- The fork's route takes `beforeTurn` or `blockId`, one of them. An attempt
  is its source and the end as the request named it, the text's block or
  the turn, so a retry under the same id finds it whatever the source's
  history became since, and the id with another end, of either kind, is
  refused as another attempt.

## Spike

- A turn said "Checking.", ran two commands and replied; the next was
  stopped while `sleep 30` ran; a third answered. A fork before the third
  held the first whole, its calls, their results and the reply, and the
  stopped turn with its call as `error`, its response and the `abort`, and
  not the third. A fork while the conversation's agents were not open read
  the same from the store.
- A retry under the same id answered the same conversation, also after a
  Stop gave the turn's message back and the turn left the source; the id
  with another turn, or with that turn's message as a text to fork
  through, was refused as another attempt. A turn a command's end opened,
  an unknown turn and a body with both ends were refused, and a turn whose
  message waited in the queue was refused as not started.

## Prior art

- ChatGPT and Claude branch a conversation at a message: the branch holds
  everything before it.

## Alternatives

- Fork through the last block of the previous turn, whatever its kind:
  a turn that ended on a command's output would hand the copy a call the
  model must still answer.

## Open questions

None.
