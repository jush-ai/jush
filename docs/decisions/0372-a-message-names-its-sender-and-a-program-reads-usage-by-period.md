---
status: proposed
---

# A message names its sender, and a program reads usage by period

## Problem

A program serves many people through one core: each person's message in a
team's chat starts a turn. It bills each team and each person, and charts
the cost by day and by model. Jush keeps a row for every call of a model
(the usage ledger: the conversation, the turn, the provider, the account,
the model, the tokens, the cost and the time), but answers only its totals
by provider and model, and one conversation's cost of the last hour or day.
So the program keeps a ledger of its own, and does not know who sent what
Jush counted.

## Decision

### The sender

```ts
await conversation.run('What does the invoice total?', { senderId: 'person-7' })
await conversation.steer('And last month too.', steerId, { senderId: 'person-8' })
```

- `run`, `send`, `steer`, `sendToAgent` and a side question's `ask` and
  `followUp` take `senderId`, the program's own id of whoever sent the
  message, 1 to 255 characters. The message keeps it: its `user` or `steer`
  block names it, so a page can show who wrote what, and a queued message
  keeps it in the checkpoint.
- A call's sender is that of the latest message that handed its agent
  work: a message, a steer, or another agent's explicit message; or that of
  the work whose end opened its turn. So the calls of a turn are its
  message's sender's, and so are those of a retry, a resume and a
  compaction, until the next message or the next turn an end opens. A
  steer the turn reads makes the calls after it its sender's. A permission
  decision and a move hand no work and change nothing.
- A command's reports and a subagent's notice of its end name whom their
  work was for: the sender of the call that started the command, which its
  record keeps from its start to its end, and the sender the subagent
  worked for as it closed. So does the notice that another conversation's
  work ended, which `jush conversation send --wake-when-done` asked for:
  the sender its message named. A turn such a report or notice opens, once
  the agent rests, is that sender's: Alice's task whose command ends after
  Bob's message wakes a turn of Alice's. One read inside a running turn
  joins it and changes nothing. Of reports that open a turn together, the
  first that opened it names the sender, and reports that wait with a
  notice open the turn before it; a service's loss, which waits for the
  next turn, opens none, and joins the turn a notice opens after the
  notice. Several messages of one agent that one turn of another
  conversation took wake it once, for the first's sender.
- What an agent's command does is for the sender of the call that ran it,
  which the command keeps, also once a restart takes it up again: a child
  it spawns or resumes, its explicit message (`jush agent send`, `jush
  conversation new` and `send`), whose recipient works for that sender
  from then, and a call to a vendor it makes, an image or a video.
- The session names the sender in each model request, and the usage ledger
  keeps it in each row's `sender_id`. A call outside an agent's work, a
  title's or the suggested replies', names none.
- Stored data grows by the sender alone, null or absent before: the
  ledger's rows, the blocks (format 21: a message, a steer, a report, and
  whether a notice opened its turn) and the checkpoints (format 12) of a
  conversation, its running and ended commands, a kept wake, and a media
  job's kept row. Nothing stored is rewritten.
- The model does not read it; a program that wants the agent to know who
  speaks writes it into the message.
- It is named `senderId`, not `userId`: a core's user is the person it
  belongs to.

### The report

```ts
const report = await core.usage.report({
  from: new Date('2026-11-01'),
  to: new Date('2026-12-01'),
  bucket: 'day',
  timeZone: 'Asia/Shanghai',
  groupBy: ['conversation', 'sender', 'model'],
})
// report.buckets[0] = { start, end, groups: [{ conversation, sender, model,
//   calls, inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens,
//   cost: { usd: 0.42 }, apiPrice: 0.40, unpricedCalls: 0 }] }
```

- `bucket` is `hour` or `day`, counted in `timeZone`, UTC unless named.
- `groupBy` takes any of `conversation`, `sender`, `model`, `provider`,
  `account`, `turn` and `purpose`; a group holds the rows that share each.
- `cost` is what the vendors said the calls cost, by currency, never
  converted; `apiPrice` is what the tokens would cost on the vendors' APIs,
  as the ledger fixes it when it writes a row, and `unpricedCalls` counts
  the calls with tokens whose model has no price. A refused call counts as
  a call with no tokens.
- A bucket holds the groups that made calls, and a period without calls has
  no bucket; the first and the last start and end with the period, so two
  reports of adjoining periods hold no bucket twice. A dimension a call has
  no value of, such as the sender of a title's call, is null.
- An hour is the clock's hour in the zone's offset then, cut where the
  offset changes, as it does by half an hour or at a quarter past in a few
  zones; a day runs from one start of the zone's day to the next.
- The route is `GET /usage/report?from=&to=&bucket=&timeZone=&groupBy=`; the
  report reads the period's rows by the ledger's time index and sums them.

## Spike

- A program ran four turns in two conversations, sent by `alice`, by `bob`,
  by `alice` again and by no one named. Grouped by sender, the day of
  `Asia/Shanghai` (16:00 to 16:00 UTC) held `alice` with 2 calls, `bob`
  with 1 and null with 5: the unnamed turn and the four suggested replies.
  A zone, a dimension or a period the report does not know was refused
  with 400.
- `alice`'s message started a command in the background and the turn
  ended; the command's end opened a turn of its own id, whose call was
  `alice`'s, as was the running command's record. A subagent the agent
  started for `alice` made `alice`'s calls. `bob` steered a turn of
  `alice`'s while a command ran: the call before the steer was `alice`'s,
  the one after `bob`'s, and the transcript's `user` and `steer` blocks
  named each.
- In `bob`'s turn the agent sent `alice`'s running subagent work with
  `jush agent send`: the subagent's call after reading it was `bob`'s. A
  conversation `jush conversation new` started in `alice`'s turn made
  `alice`'s calls. A side question asked by `carol` and followed up by
  `dave` made one call of each.
- After `bob`'s message, each of these opened a turn whose call was
  `alice`'s: the end of a command `alice`'s turn started in the background,
  the end of a subagent working for her, also of one a command of hers
  spawned, and the end of another conversation's work her turn asked to be
  woken by. The end of her command while `bob`'s turn ran joined that turn
  at its next step, whose call was `bob`'s. A subagent `bob`'s message sent
  more work to woke the root, at its end, for `bob`.
- Hours of `Pacific/Chatham` and `Australia/Lord_Howe` across their clock
  changes in 2026 and 2027 held each row within their bounds, none
  overlapping: `[13:15, 14:00)`, `[14:00, 14:15)` and `[14:15, 15:15)` UTC
  across Chatham's change back at 14:00.
- A core 0.2.6332 made, with a conversation of two calls, opened: its
  blocks of format 20 read, a new turn sent by `carol` was hers, the old
  rows null. Cores 0.2.6252 and 0.2.6254 made opened too.
- A core with 300,000 ledger rows over 31 days, 10,000 calls a day,
  answered a month by day, grouped by sender and model, in 410 ms, and by
  hour (721 buckets) in 460 ms. Meanwhile it answered 23 and 576 other
  requests, the longest waiting 28 ms: a bucket at a time, where reading
  the month in one turn of the thread kept them waiting for all of it.

## Prior art

- OpenAI's Usage API answers buckets of `1m`, `1h` or `1d`, grouped by
  project, user, API key or model
  ([Usage API](https://platform.openai.com/docs/api-reference/usage)), and
  its requests take a `user` (now `safety_identifier`) naming the end user.
- Anthropic's Usage and Cost API answers buckets grouped by model,
  workspace or API key
  ([Usage and Cost API](https://docs.claude.com/en/api/usage-cost-api)), and
  its requests take `metadata.user_id`.
- LiteLLM's proxy, open source, writes each call's spend row with the end
  user its request named in `user`, and reports spend by end user
  ([Spend Tracking](https://docs.litellm.ai/docs/proxy/cost_tracking)): the
  sender on each of Jush's ledger rows is that column.
- Langfuse, open source, takes a `userId` on a trace, which holds a task's
  nested calls, and sums their cost by user and by a daily time dimension
  ([Metrics API](https://langfuse.com/docs/metrics/features/metrics-api)):
  the calls after a message play the trace's part, since an agent's work
  has no trace a program opens.
- Jush departs in naming the id `senderId`, since its user is the core's.

## Alternatives

- Group by turn alone, and let the program read the sender from its turn
  ids: every program would keep a map of turns to people.
- Keep each turn's sender by the message that opened it, and join the
  ledger's rows to it by turn: a turn that continues after a command or a
  subagent, and a subagent's turns, have turn ids of their own, so most of
  a long task's calls would name no one.
- Count a turn a report or a notice opens for the latest message's
  sender: `alice`'s task whose command ends after `bob`'s message would go
  on as `bob`'s, who asked for none of it.
- Let a report or a notice read inside a running turn make the rest of
  that turn its sender's: `bob`'s turn would become `alice`'s at a boundary
  he cannot see.

## Open questions

None.
