---
status: proposed
---

# Only a command the agent no longer waits for folds into its run

Supersedes: the rule of commit `23e23be385` (a step that still runs stays in
its run's row), for the steps the agent still waits for. That change made no
decision record.

## Problem

Alex asks the agent to build the docs while the test suite runs. The agent
starts the suite, stops waiting for it, and goes on: it reads the config,
then builds the docs. Before `23e23be385`, the suite's call stood as a row
of its own under the run for as long as the suite ran, twenty minutes for a
slow one, and several such calls made a stack of rows that came and went
under the run.

`23e23be385` folded every step that still runs into its run's row. While a
turn runs, the row says only what the agent does now: the newest step, or
Requesting between steps. The count of what the run has done is gone for the
whole turn, and Alex sees `Build the docs` where the row said
`Ran 3 commands`, until the agent writes its answer. Two runs next to each
other, as on both sides of a compaction's divider, read one as a count and
the other as a step.

## Decision

A step joins its run's row once the agent no longer waits for it:

- **Joins the run:** a call that returned, also when its command runs on, as
  one the agent left running or stopped waiting for; thinking once it is
  written.
- **A row of its own under the run:** what the agent waits for, a call being
  written or waiting for its result, and thinking being written at the end of
  a running turn. The Requesting row under the run says the wait for the
  model, as before `23e23be385`.

While commands of a run still run after their calls returned, the run's row
says how many in place of its time, shimmers, and wears the time of the one
that has run longest:

```text
⧉ Ran 3 commands, 1 still running  [7s]  ›
⊡ Build the docs                   [4s]  ›
```

Once they end, the row says its time again: `Ran 4 commands for 13s`. A
command whose stop waits for its runner reads `1 stopping`.

The grouping reads no command's live frames, as since `23e23be385`: a
returned call joins its run whatever its command does, so the run's row is
all that changes when a command ends.

A step the reader opened while the agent waited for it folds into its run
when the agent stops waiting, as a call does when it returns; a command
that runs on goes on showing its output in the Terminals window and in its
row inside the opened run.

## Spike

None: the rule before `23e23be385` ran for weeks, and the run's row that
says how many commands still run is that change's.

## Prior art

None looked at: this restores the rule before `23e23be385` for the steps
the agent waits for, and keeps the folding that change brought only for the
commands it no longer waits for.

## Alternatives

- **The run's row says the count and the current step on one line:** two
  facts compete for one line, and a long step title cuts the count.
- **Keep `23e23be385`:** the count of what the run did is hidden for the
  whole turn.

## Open questions

None.
