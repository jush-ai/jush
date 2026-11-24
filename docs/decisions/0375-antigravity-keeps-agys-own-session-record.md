---
status: accepted
---

# Antigravity keeps agy's own session record, and Jush compacts within agy's limit

Supersedes: 0352 (An Antigravity subscription runs through Jush's copy of
Google's agy), these parts: **History**, where a new process's steps are
always written from the conversation's blocks, and **Catalog**, where a
model's context size is models.dev's alone. 0354 (Antigravity's model reads
the tool results agy saves, and its turns report their output), its
**Usage**, where a run's input is `usage_update`'s `used` divided by the
turn's totals.

## Problem

Alex works with Gemini 4 Neon through Antigravity on a long refactor. Over
two days the conversation's transcript grows to about 690,000 tokens, and
Jush never compacts it. `agy` does, on its own: once its context passes
its limit it writes a summary and goes on from about 14,000 tokens. Each
time Alex comes back after five idle minutes, the process is gone, and the
new one is written the whole 690,000-token transcript. Its first request
carries all of it, `agy` compacts again at once, and the next request
starts over from a summary. In two days this happened 14 times. The
conversation's page reads a cache hit rate of 3%, where `agy` itself counts
50 to 98%.

Three causes add up:

- **Two compactions.** Jush compacts at its threshold of the model's window
  in use, which for Gemini 4 Neon is models.dev's 1,048,576 tokens, so at
  about 1,015,000. `agy` compacts long before, at a limit of its own, and
  that limit cannot be changed from `agy acp` (see Spike). So Jush's
  transcript grows without bound, and `agy`'s summary is thrown away with
  each process.
- **A rebuilt session.** A new process loads steps written from Jush's
  blocks: reasoning is left out and a medium becomes a line of text. The
  request it sends begins with other bytes than the last request of the
  process before, and pays for the history again.
- **Usage of one request in four.** `agy acp` reports a request's split of
  uncached input and cache read only at a turn's end. Every earlier run
  reports all its tokens as uncached input, so the page counts nearly every
  request as a cache miss.

## Decision

Jush alone compacts, within the room `agy` gives a model, as for Claude
Code; a new process loads the session the last one left, where the
conversation goes on from it, as magpie does.

### The window in use is `agy`'s limit

The catalog gives each model of an Antigravity account, as its context
size, the smaller of models.dev's window and the limit at which `agy`
compacts it:

| `agy`'s models | Limit |
| --- | --- |
| Gemini 3.8 Flash | 256,000 |
| Gemini 4 (Neon, Carbon, Argon, Hydrogen) | 512,000 |
| Claude Opus and Sonnet | 840,000 |
| GPT-OSS 120B | 80,000 |
| Any other | 256,000 |

Jush's threshold of a 512,000-token window is 479,000, so Jush compacts
before `agy` would. A model the table does not name gets 256,000, the
default `agy` gives a model its configuration names no limit for.

The table is written in the provider's code. `agy` offers no way to read
its limits: neither ACP, nor any command of its print mode, nor its log
names them, and the configuration it takes them from comes from Google's
backend, which Jush never calls with the account's tokens (0352). The
table holds the limits of `agy` 1.5.16, and Jush runs Google's newest
`agy` (`crates/provider-subscription-cli/src/releases.rs`), so a later
release can move one.

`agy` still compacts by itself where the table is out of date, or where one
request grows past its limit in a single step, such as a tool result of
40,000 tokens at 475,000. The session then goes on from `agy`'s summary,
and the database keeps it. The provider sees the compaction's line in the
transcript and logs it,
`antigravity.agy_compacted`, with the model and the tokens of the request
before it, so a limit that moved shows in the logs.

### A new process loads the session the last one left

When the machine closes an idle process, `agy` ends and writes its
write-ahead log into its database (measured), a few megabytes at most. The
provider reads that database before the process's directory goes, as the
directory always goes with its process, and the runtime keeps it, with what
the process held: its session's id, its system prompt and the user messages
it received. A runtime keeps one, its session's last.

The next process of the session loads that database, written into its own
directory, where the request continues it: the same system prompt, the user
messages the process received, and new ones after them. It needs no first
process to make the session. Otherwise, as after Jush compacted, after a
message was edited or sent again, or after the backend restarted, the
session is made and written from the blocks as today. The runtime lets the
database go once a process started from it opens, or fails to.

So when Alex comes back after five idle minutes, the request of the new
process starts with the bytes the last request of the old one sent,
reasoning and media included, and Google's cache serves them. After Jush
compacts, the session is written from the blocks once, and the requests
after it are cached again.

A step that names a file in the old directory, such as a tool result's
offloaded image or a document the user attached, names it under that
directory. The new process cannot read it, as a missing file fails today.
A session written from the blocks names such a medium in text alone, so
nothing is lost against today. The step is kept as written, since rewriting
its path would change the bytes the cache holds.

### Usage is read from `agy`'s transcript

`agy` writes each session's steps, as it finishes them, to a transcript of
JSON lines in the process's directory
(`.gemini/antigravity-cli/brain/<session>/.system_generated/logs/transcript.jsonl`).
A response's line carries its request's tokens, and is written as soon as
the model asks for a call, before the call runs (measured):

```text
{"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-4-neon-low",
 "input_tokens":2160,"cache_read_tokens":39840,"output_tokens":12,"content":"Done."}
```

At the end of each run the provider reads the lines added since the last
read, by running `tail -c +<offset>` on the machine, so only the new bytes
leave it. The run reports its last response's tokens: uncached input, cache
read and output, with the thinking. A run that made no request reports
none, such as one that delivers a result while `agy` goes on to the next
call of the same response. The page's cache rate then counts every
request, and the context the agent compacts by is the last request's, as
before. A run whose transcript cannot be read reports what it reports
today.

### What changes for stored data

Nothing is stored: the kept database lives in its runtime, and a
conversation's blocks are unchanged. A conversation from before this change
goes on as any other; its next process is written from its blocks once.

## Spike

Measured with `agy` 1.5.16 on Linux x86_64, through `agy acp` with Jush's
agent and `excludeDefaultComponents: true`, on a Google AI Pro account.

| Point | Result |
| --- | --- |
| `AGY_CLI_AUTO_COMPACT_OVERRIDE` under `agy acp` | Ignored: at 8,000, at 50K and at 5%, Gemini 3.8 Flash and Gemini 4 Neon went on to about 69,000 tokens uncompacted. Under `agy -p` the same variable at 50K refused a 70,000-token message as over budget, so only print mode reads it. |
| `agy -p` with stream-json instead | Its input takes `{"event":"user","message":{"content":…}}` and only `text` blocks: an `image` block fails with `content block type "image" is not supported (only "text")`. `control_request` fails as not supported. A user's image could not reach the model, so the provider stays on ACP. |
| Where the limit comes from | `agy` embeds a default configuration that maps each model to a profile with a `max_token_limit`: 256,000, 512,000, 80,000, and for Claude 840,000 through the `claude-1m_840k` variant. A model's profile is found by the id its response steps record. A configuration the server sends can override it. |
| The limit at work | Gemini 3.8 Flash compacted before a request that would reach about 259,000 tokens and not before one of about 253,000; Gemini 4 Neon before one of about 588,000 and not before one of about 506,000. `agy` compares the last request's tokens plus the new message, before sending it. |
| A database in a new directory | The database of a Gemini 4 Neon session with reasoning, copied alone into a new directory with Jush's agent, loaded in a new process: the model named what it had said, and 39,782 of 42,144 input tokens were cached. Moved into a session made empty, its `steps` rows did the same, 39,941 of 42,215. |
| The process's directory | A session loaded in a copy of its directory read 53,385 of 55,652 tokens from the cache, the same as in its own. Likely because Jush's agent leaves out `agy`'s workspace block (0352), which names the directory. |
| Cache after idling | The same session, loaded after 14 idle minutes, read 53,593 of 55,855 from the cache. One other session read none after seven minutes, so the cache can be gone sooner. |
| The database while a process runs | Its write-ahead log grew by 0.8 to 1 MB a turn of about 20,000 new tokens, to 7.1 MB over eight turns, with no checkpoint. When the process ended the database held it all in 1.2 MB. Reading the database at each run would copy the whole log each time. |
| The transcript | A turn whose model called an MCP tool had its response's line, with `input_tokens`, `cache_read_tokens`, `output_tokens` and `tool_calls`, in `transcript.jsonl` before the call reached the tool. |

## Prior art

- **Claude Code** (`crates/provider-claude-code/src/session.rs`): the CLI
  prints its own record as it writes it, Jush keeps it on the blocks and
  writes it back as a session file, the CLI's compaction is turned off, and
  Jush compacts. `agy acp` prints no record and lets no compaction be turned
  off, so Jush keeps inside the limit and keeps the database instead.
- **magpie's `agy-cli`**
  ([magpie-community/plugins](https://github.com/magpie-community/plugins)):
  keeps the `agy` session on the machine and resumes it with
  `session/resume` where the client's history still matches it, replays the
  history as text, up to 112 KiB, where it does not, lets `agy` compact,
  and reads each step's usage from `agy`'s database. It found that `agy`'s
  cache follows the process's directory and starts a resumed session in its
  old one; with `agy`'s whole prompt, which names the workspace, that holds,
  and with Jush's agent it does not apply, so Jush loads the database in
  the new process's own directory. Where it does not match, Jush writes the
  whole history from its blocks, never cut to a budget.
- **Google** says the 256,000 default was chosen for reasoning quality,
  latency and quota, and tracks settings for it in
  [antigravity-cli#878](https://github.com/google-antigravity/antigravity-cli/issues/878).

## Alternatives

- **Let `agy` compact, as magpie does.** The conversation would have two
  histories, Jush's and `agy`'s summary of it.
- **Move to `agy -p` stream-json** to set the limit: images from the user
  cannot be sent (Spike).
- **Keep `agy`'s rows on the blocks, as Claude Code's record**, so a session
  survives the backend's restart: the rows are only in the database, whose
  log grows by about 1 MB per 20,000 tokens while the process runs, and
  reading it at each run would copy all of it again.

## Open questions

None.
