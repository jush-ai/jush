---
status: proposed
---

# A detach leaves the work of a subagent that works on the device

Supersedes: 0075 (A detach applies at once and stops what runs there), the
part that every command of the conversation on the device is stopped, a
command that asks for the detach included.

## Problem

Alex's conversation runs on `alex-mbp`, and a subagent runs the test suite
there. Alex moves the conversation to `lab-server`; the subagent stays on
`alex-mbp` to finish, as a move leaves each subagent where it works, and
`alex-mbp` stays attached. Alex then detaches `alex-mbp`. The detach stops
every command of the conversation on `alex-mbp`, the subagent's test run
included, but not the subagent: it reads that its command was stopped and
runs its next command on `alex-mbp`, which the conversation still reaches
through it. The work is stopped halfway, and the subagent carries on
without what it was doing.

## Decision

A detach stops the conversation's commands on the device except those a
live subagent runs on the device it works on. The subagent goes on there,
and the device is released once it ends, as when a detach finds a subagent
working there (0357). Until then `jush device list` shows the device with
the role `subagent`, which `jush device detach` refuses, as today: to stop
the work there, the user stops the subagent.

| A command on the detached device | After the detach |
|---|---|
| The root's, or a side question's, one that ran there before a move included | Stopped, as now |
| A subagent's that names the device, while the subagent works elsewhere | Stopped, as now |
| A subagent's on the device it works on, while the subagent is live | Runs on |
| A subagent's on the device it worked on, once the subagent ended | Stopped, as now |
| A `jush device shell --device` | Stopped, as now |

Each job registers with its conversation whose it is, which replaces the
job's `follows` flag:

```rust
pub enum JobAgent {
    /// The root's or a side question's, or a `jush device shell`'s.
    Following,
    /// A subagent's, on a device one of its calls names.
    Named,
    /// The subagent `node`'s, on its own device.
    OwnDevice(NodeId),
}
```

A node's job access knows which from its scope: whether the node follows
the conversation, and whether the environment is one a call names
(`EnvironmentScope::named`). A job adopted after a restart registers the
same way. A detach keeps an `OwnDevice` job only while that subagent is
live and its placement is the device (`DeviceShard::agent_works_on`), so the
commands a closing subagent leaves stopping are stopped as before. Nothing
is stored, and a runner and a page see no change.

## Spike

None.

## Prior art

- A move already leaves a subagent's commands where they run and the
  subagent where it works (`crates/agent-server/src/node/mod.rs`).
- OpenSSH's `~.` ends one session; other sessions to the host go on
  ([ssh(1), ESCAPE CHARACTERS](https://man.openbsd.org/ssh#ESCAPE_CHARACTERS)).

## Alternatives

- **Stop the subagent with its commands.** The device leaves at once, but
  the subagent's work is lost, though the move left it there to finish; the
  user can stop a subagent on its own. 0075 stopped every command because a
  user detaches a device to end the conversation's work there; that work was
  the conversation's then. A subagent left on the device after a move is
  work the user let finish there, and stopping it is a step of its own.
- **Keep stopping every command.** The subagent goes on without the work it
  was doing, which neither the user nor the agent asked for.

## Open questions

None.
