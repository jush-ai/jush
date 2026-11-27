---
status: proposed
---

# A program searches every conversation's history, in a time window

## Problem

Alex's program runs a review each morning: which conversations used the
browser yesterday, what users said about an experiment, what came of the
code reviews of the last two weeks. The program has hundreds of
conversations, many with subagents. `core.search` answers one match per
conversation and no time window; `conversation.search` finds a root's
messages alone, 50 at most, with no time window; nothing finds a
subagent's text. So the program lists every conversation and searches
each, which is slow, and still misses the subagents and the window.

## Decision

A program searches the texts of every conversation of its core, its
subagents' and its archived ones' included, for words, within a time
window, and pages through every hit:

```ts
const { hits, next, more } = await core.history.search('browser login', {
  from: new Date('2026-10-05T00:00:00+08:00'),
  to: new Date('2026-10-06T00:00:00+08:00'),
})
for (const hit of hits) {
  // hit.conversation, hit.agent (0 for the main agent), hit.node,
  // hit.blockId, hit.kind ('message' | 'answer' | 'command'), hit.time,
  // hit.text, the line around the first match, and hit.ranges
  const around = await core.conversations.open(hit.conversation)
    .history.transcript(hit.node, { around: hit.blockId })
}
// The next page: the same text and options, after the last.
const later = await core.history.search('browser login', {
  from: new Date('2026-10-05T00:00:00+08:00'),
  to: new Date('2026-10-06T00:00:00+08:00'),
  after: next,
})
```

* A text is what the core's search index holds of each agent: a message
  or a steer of the user or of the agent's parent (`message`), an
  agent's text (`answer`), and the title of a command a `shell` call ran
  (`command`). Scripts, command outputs and thinking are not searched, as
  `jush conversation history search` searches none of them. A side
  question, which the index does not hold, is not searched.
* The text holds words split at white space; a hit holds every word, or
  any with `any: true`, anywhere, case ignored, in every language, as the
  other searches find them. `kinds` keeps the kinds named; all three by
  default.
* `from` and `to` keep the texts written at or after `from` and before
  `to`, as `core.usage.report` reads its window; either may be left out.
* Hits come newest first, and a conversation gives every hit it has. A
  page holds at most `limit` hits, 1 to 200 and 50 by default; `more`
  says hits wait after it, and the same text and options with
  `after: next` read them: the cursor holds where the page ends, not what
  the search keeps. `next` is opaque, the place of the last text the page
  read or, for a page that read none, the `after` given. A text whose
  conversation is being deleted, or a subagent's whose database cannot be
  read, is left out, so a page may hold fewer than `limit`.
* A hit names its agent twice: `agent`, the number the conversation knows
  it by, as `core.commands.history` and `core.usage.report` name it, and
  `node`, the node `history.transcript` and `history.block` take.
* Who may search what is the program's to decide, as it gives its agents
  commands: Jush gives the search to the program and knows no user and no
  permission of it. A conversation's page cannot reach it.
* A text appears once the indexer took the save that wrote it, a moment
  after; a conversation deleted leaves the index with it. After an
  upgrade builds the index again, a conversation's texts appear as the
  indexer reaches it, as the product's search finds them.

### The design to build from

* The web API: `GET /history/search?q=&any=&kinds=&from=&to=&after=&limit=`,
  its kinds a list with a comma between two, as the usage report's
  query takes its lists, answering `HistoryHits { hits, next, more }`,
  each `HistoryHit { conversation, agent, node, blockId, kind, time,
  text, ranges }`. A bad cursor, an empty text, an unknown kind or a
  limit outside its range answers 400.
* The SDK: `core.history.search(text, options)`, as `core.search(text,
  options)` takes its text.
* The core's search index answers it from its texts table in one read:
  the conditions on the words are those an agent's history search builds,
  without its conversation and agent, with the window on the texts' time,
  ordered by time, conversation, agent and index, and the cursor is the
  last hit's four. The node of each agent a page names is read from its
  conversation's database once per page, but a main agent's, whose node
  is its conversation's id; the page's conversations' records leave out
  the texts of a conversation being deleted.
* Nothing is stored: the index holds every text already.

## Spike

None: the index holds each text with its conversation, agent, block and
time, which `jush conversation history search` reads per agent.

## Prior art

* Zulip, an open-source team chat, searches every channel a user can read
  with one call: `GET /messages` takes a narrow of `search` and other
  operators, answers each message with its channel and its time, and
  pages by an anchor, the message to read on from, with how many before
  or after it ([Get messages](https://zulip.com/api/get-messages)). Its
  anchor is a message's id; Jush's cursor is a text's place in the order
  of the search, since a text has no id of its own that stays.
* Slack's `search.messages` searches every channel the token can read,
  with `after:` and `before:` in the query, and answers each match with
  its channel and its timestamp, page by page
  ([search.messages](https://api.slack.com/methods/search.messages)).
* Jush takes the window as two fields rather than words of the query, as
  its usage report does, and a cursor rather than page numbers, as Zulip
  and its command history do, so a page read while texts are written
  loses none and repeats none.

## Alternatives

* **A conversation filter, or a list of conversations, on the search**:
  the program that decides which conversations a search may see can keep
  the hits of those; a list would grow with the conversations it names.
  Left for when a program needs the index to narrow it.
* **The scripts of commands in the index**: a search for the commands
  that used the browser would find their scripts, not only their titles,
  at the cost of an index several times larger, built again on upgrade.
  Not now.
* **The search through `jush conversation history search --all`**: the
  program, not its agents, searches; it gives its agents a command of its
  own.

## Open questions

None.
