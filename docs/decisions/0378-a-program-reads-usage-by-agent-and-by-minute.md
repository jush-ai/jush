---
status: proposed, superseded in part by 0383
---

# A program reads usage by agent and by minute, of the conversations it names

## Problem

A program reports what one piece of work cost when it ends, such as the
review of a change request. The work runs one of two ways:

- A subagent of a team's conversation does it. Its calls are mixed in the
  ledger with those of the conversation's main agent and of its other
  subagents at the same time, and `core.usage.report` cannot tell them
  apart: it groups by conversation, sender, model, provider, account, turn
  and purpose, not by agent.
- A conversation that several pieces of work share in turn does it. The
  program splits each call's cost between the pieces of work attached to
  the conversation when the call was made, and an hour is too coarse to
  tell which they were.

The program also reads the whole core to report on one conversation, as the
report takes no filter.

## Decision

```ts
const report = await core.usage.report({
  from: review.startedAt,
  to: new Date(),
  bucket: 'minute',
  conversations: [conversation.id],
  agents: [3, 5],
  groupBy: ['agent', 'model'],
})
```

- `groupBy` takes `agent`: the number the call's conversation knows its
  agent by, 0 for the main agent, as each row of the ledger keeps it; null
  for a side question's call and for one no agent made, such as a title's.
  A number names an agent within its conversation: grouped by agent alone,
  agent 3 of two conversations is one group.
- `conversations` keeps the calls of those conversations, and `agents`
  those of the agents of those numbers there; without them, the report
  keeps every call, and an empty list keeps none, as `IN ()` would. `agents`
  needs `conversations`, as a number names an agent within its
  conversation alone, and keeps no call of no agent. An id that is no
  conversation's and a number that is no agent's are refused with 400.
- `bucket` takes `minute`: the clock's minute in the zone's offset then. A
  call falls in the minute it ended, and a minute without a call of the
  report has no bucket.
- A subagent's work includes the subagents it started. The program finds
  them in `conversation.agents`, each subagent's entry naming its `number`
  and its `parent`, and names them in `agents`; the report knows no tree.
- The route is `GET /usage/report?…&conversations=&agents=`, each a comma
  list.
- The report reads a bucket at a time, as it does an hour's. Its first row
  is found through the ledger's time index; with `conversations`, through
  its conversation index, each conversation's first row in one seek, so a
  conversation with few calls is not found by reading every other row of
  the period.
- An earlier core refuses the new parameters and `minute` with 400.

## Spike

- A conversation's main agent started a reviewer, which started a helper,
  and another subagent beside it. `conversation.agents` named the helper's
  parent, the reviewer, and the reviewer and the helper made 3 and 1 calls,
  which a report of agents 1 and 2 held alone, by minute in two buckets. A
  report of the other conversation held only its calls, and one of agent 0
  of both the main agents'. Empty lists held nothing; `agents` without
  `conversations`, an id that is no conversation's and a negative number
  were refused with 400.
- A core with 300,000 calls of 40 conversations over 31 days answered the
  month by minute, 44,641 buckets, in 1.3 s, while it answered 11,088 other
  requests, the longest waiting 70 ms; by hour in 420 ms. Two
  conversations' calls of two agents by minute took 171 ms. A conversation
  with no call in the month answered in 22 ms: its first row is one seek of
  the conversation index, where the time index read all 300,000 rows in one
  turn of the database's thread, 250 ms, while other requests waited.

## Prior art

- Langfuse, open source, answers metrics by a time dimension of `minute`,
  `hour`, `day` and coarser, with filters in the same query
  ([Metrics API](https://langfuse.com/docs/metrics/features/metrics-api)).
- OpenAI's Usage API answers buckets of `1m`, `1h` or `1d`, and filters by
  project, user, API key and model
  ([Usage API](https://platform.openai.com/docs/api-reference/usage)).
- Anthropic's Usage and Cost API answers buckets of `1m`, `1h` or `1d`, and
  filters by model, workspace and API key
  ([Usage and Cost API](https://docs.claude.com/en/api/usage-cost-api)).
- Jush departs in naming agents, which no vendor sees.

## Alternatives

- A list of every call, with its time: as exact for splitting, but a second
  interface beside the report, which a minute serves well enough.
- `agents` with their descendants: the report would carry the tree's rule,
  which the program reads in `conversation.agents` already.

## Open questions

None.
