---
status: proposed
---

# A message carries its program's labels

Supersedes: 0372 (A message names its sender, and a program reads usage by
period), its `senderId`; 0378 (A program reads usage by agent and by
minute), its `sender` dimension; 0379 (A call names the assignment it
belongs to) and 0382 (A message names the assignment it continues) where
they name the sender beside the assignment; and 0380 (A program reads
every command its agents ran), its rows' `sender`.

## Problem

A message names its sender, `senderId`, so a program's usage report sums
each person's calls (0372). Who sent a message is the program's notion,
one of several it counts by: the person, the chat group, the kind of work,
the customer. Jush keeps the one it was given a name for, and a program
that counts by another dimension has nowhere to put it.

## Decision

A message carries labels, the program's own key-value pairs, which Jush
keeps with the work and counts by without reading them:

```ts
await conversation.send('Review !42', 'chat:alex:17', {
  labels: { sender: 'alex', group: 'team-a', scene: 'review' },
})
const report = await core.usage.report({
  from, to, bucket: 'day', timeZone: 'Asia/Shanghai',
  groupBy: ['label:sender', 'label:scene', 'model'],
})
// group.labels: { sender: 'alex', scene: 'review' }
```

- `MessageOptions` takes `labels` in place of `senderId`, as `run`, `send`,
  `steer`, `sendToAgent`, `sideQuestions.ask` and `followUp` and a
  conversation's first message do. At most 16 labels; a key is 1 to 64
  letters, digits, `.`, `_` and `-`, the first a letter or a digit, which
  `groupBy` names; a value is 1 to 512 characters. The limits bound what
  each row of usage keeps, and the first character keeps out a key such as
  `__proto__`, which a JavaScript program's object reads as its prototype.
- Labels go where the sender went, by its rules (0372, 0379): a call
  carries the labels of the latest work its agent read, a steer's from the
  steer on, in place of the turn's rather than merged with them, and the
  work that continues it, its reports, subagents, wakes, retries and the
  runs of a schedule an agent made, keeps them. A message that names none
  carries none.
- `usage.report` groups by `label:<key>`, as many as it names, and each
  group answers `labels` with each key's value, null for a call without
  it. `commands.history` rows, the `user` and `steer` blocks, and a
  command's call (`invocation.caller.labels`) carry them.
- Jush gives a label no meaning, and the model does not read it.
- Stored data keeps labels where it kept the sender: the blocks (format
  23), the checkpoints (format 14), a conversation's running and ended
  commands, the usage ledger, the command history, a kept wake, a schedule
  an agent made and a media job's kept usage (format 4). The sender
  0.2.6342 and 0.2.6346 kept is not carried over, since no program has
  relied on it yet: their cores open, and what they counted names no
  label.

## Spike

A program on Bun sent messages labeled `{ sender, scene }` over a
scripted model, and read `usage.report` grouped by `label:sender` and
`label:scene`:

- A background command started in Alice's turn ended after Bob's message:
  the turn its report opened carried Alice's labels, as did the subagent
  she started, the other conversation her turn woke on, and the runs of
  the schedule her turn made; a command that ended inside Bob's running
  turn joined his.
- After Carol's steer, Dave's running turn's calls carried Carol's
  labels, in Dave's assignment.
- A plugin's call read `caller.labels`, and the program's later message
  with them and the assignment counted in the same group.
- `commands.history` rows carried each command's labels; 17 labels, a key
  of `bad key`, an empty value and a value of 513 characters were refused,
  512 taken, and `groupBy: ['sender']` refused.
- A core 0.2.6346 made, with senders in its blocks, a queued message's
  checkpoint, a running and an ended command, the ledger, the command
  history, a kept wake and an agent's schedule, opened on Bun and Node:
  every block read, the queued message ran, and its old calls grouped as
  `{ sender: null }`.

## Prior art

- Prometheus attaches labels to each sample and sums by any of them
  ([Data model](https://prometheus.io/docs/concepts/data_model/)). Its
  label names begin with a letter or `_`, and those that begin with `__`
  are its own; Jush's begin with a letter or a digit.
- OpenTelemetry's Baggage carries an application's key-value pairs with a
  request across services, apart from the trace's own context
  ([Baggage API](https://opentelemetry.io/docs/specs/otel/baggage/api/)).
  A service adds to the baggage it received; Jush's labels are replaced
  whole by the next work's, as the sender was, so a call never carries
  one person's label beside another's.
- OpenAI's requests take `metadata`, up to 16 pairs of a 64-character key
  and a 512-character value
  ([Create a model response](https://platform.openai.com/docs/api-reference/responses/create)).
- Jush keeps its own context, the assignment (0379), as a field, as a
  trace keeps its id, and the program's as labels.

## Alternatives

- One opaque string: a program that counts by two dimensions packs them
  into it and unpacks each group.
- A field for each dimension a program asks for: Jush would learn each
  program's notions, one release at a time.

## Open questions

None.
