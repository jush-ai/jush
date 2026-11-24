---
status: accepted
---

# Antigravity never lets `agy` compact a history: a history over its limit is a context-length refusal

Supersedes: 0375 (Antigravity keeps agy's own session record, and Jush
compacts within agy's limit), the part of **The window in use is `agy`'s
limit** where a session goes on from `agy`'s summary after `agy` compacted
by itself.

## Problem

Alex has a long conversation with Gemini 4 Neon through Antigravity. Months
ago, before 0375, `agy` compacted it by itself several times, so the usage
each turn reported was that of `agy`'s summary, about 200,000 tokens, while
Jush's transcript grew to about 690,000. Jush compacts by the latest usage a
provider reported, so it never did.

The backend restarts for an upgrade, and the kept session goes with it.
Alex asks what to do next. The provider makes a new session and writes all
690,000 tokens of the transcript into it. Before the first request, `agy`
finds them over its limit of 512,000 for the model and compacts them into a
summary. The model answers from about 12,000 tokens: it does not know what
the conversation was doing, tries `read_file` on Alex's project paths four
times, fails each time, and looks through the repository from scratch.

A new conversation reaches the same state the first time `agy` compacts by
itself, which 0375 leaves possible: a tool result that carries a request
from under Jush's threshold of 479,000 to over 512,000 in one step, or an
`agy` release whose limit is below the one the provider's table holds. From
then on the usage `agy` reports measures its summary, not Jush's history,
and the next session written from the blocks is compacted again.

## Decision

`agy` never answers from a summary of its own. A request whose history is
over `agy`'s limit fails as a context-length refusal
(`ErrorCode::ContextLengthExceeded`), as a vendor's API refuses a prompt
too long. The agent already handles that refusal: it compacts once and
asks again, and a summary request refused so is asked again over a smaller
part of the history (`crates/agent-session/src/turn.rs`,
`crates/agent-session/src/compaction.rs`).

The provider refuses at two points:

- **Before it writes a session.** When it writes the history into a new
  session, it estimates the request's tokens first: the system prompt's,
  the history's and the new input's, a token for each 4 bytes, as Jush's
  estimates count text (`jush_shared_types::text_tokens`). At or over
  `agy`'s limit for the model (the provider's table of 0375), it refuses
  without starting `agy`, so `agy` spends no quota on a summary that would
  be thrown away.
- **When `agy` compacts.** An estimate can fall short of `agy`'s count, and
  a session already running can grow past the limit in one step, such as a
  saved result its `read_file` reads back. `agy` reports the compaction
  request's tokens as a `usage_update` as soon as the summary is written,
  before the model says anything of the request it compacted for (Spike).
  So a `usage_update` that comes before the model said anything since the
  process was given input, or since a call of `agy`'s own ended, is
  checked: tokens at or over the limit, or a `CHECKPOINT` among the
  transcript's new lines, which also shows a compaction under a limit an
  `agy` release lowered, end the run with the refusal and close the
  process. Nothing the model says from the summary reaches the
  conversation, and no later process loads the summarized session.

After the refusal the agent compacts, and the next request finds no kept
session that continues it, since the history's user messages changed: the
provider writes the compacted history into a new session, which fits.

What the user sees: the turn shows Jush's own compaction where it was due,
and the model answers from Jush's summary. A conversation already in the
state of the problem compacts at its next request that writes a session,
such as the first after the backend restarts.

Each refusal is logged with the tokens and the limit
(`antigravity.history_refused` before writing,
`antigravity.compaction_refused` when `agy` compacted). A compaction the
run sees only in the transcript lines it reads at its end, as when the
`CHECKPOINT` line was not yet written at the `usage_update`, is logged as
today (`antigravity.agy_compacted`), so the table can be corrected.

## Spike

Measured with `agy` 1.5.16 through `agy acp`, with an agent of
`excludeDefaultComponents: true` and GPT-OSS 120B, whose limit is 80,000.

| Point | Result |
| --- | --- |
| What ACP says when `agy` compacts | A first prompt of 54,370 tokens, then one of about 30,000. During the second, `agy` wrote its summary, then sent `usage_update` with `used: 85351`, the compaction request's tokens, 0.8 s before the model's first thought of the request, and `used: 4594` after the answer. The transcript's `CHECKPOINT` line held the same 85,351. |
| The same in the problem's conversation | The `CHECKPOINT` of its first request after the restart held 690,031 tokens, over Gemini 4 Neon's 512,000. |

## Prior art

- **Vendors' APIs** refuse a prompt over the window, and Jush maps that to
  `context_length_exceeded`, which the agent recovers from by compacting.
  This makes `agy` behave the same.
- **Claude Code** (0375's prior art): its own compaction is turned off, so
  Jush alone compacts. `agy acp` offers no switch, so the provider refuses
  where `agy` would compact.

## Alternatives

- **Report the compaction's tokens as the run's usage**, so the agent's
  next estimate is over its threshold: the model still answers that request
  from `agy`'s summary, and the usage, which the conversation's cost reads,
  would count tokens no request sent.
- **Only close the process after `agy` compacted**, so the next request is
  written from the blocks: the usage the agent reads is still `agy`'s
  summary's, so it does not compact, and the next session is compacted by
  `agy` again.
- **Estimate only, before writing**: the estimate counts 4 bytes a token,
  which can fall short of `agy`'s count, and does nothing for a session
  that grows past the limit while it runs.

## Open questions

None.
