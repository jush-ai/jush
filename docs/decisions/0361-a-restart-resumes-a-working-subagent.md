---
status: proposed
---

# A restart resumes a conversation whose subagent works

## Problem

Alex's conversation runs on `lab-server`. A subagent it spawned on
`alex-mbp` before a move still works there, in a browser with three pages
open, and `alex-mbp` was detached since (0360). The root agent is idle,
waiting for the subagent. Alex's server updates.

- **Nothing resumes it.** Before the old backend stops, it marks each
  conversation whose root's turn runs, and the new one resumes them. This
  root's turn does not run, so the conversation is not marked. Its subagent
  waits until someone opens the conversation, which brings its tree back.
- **Its device lets go of it.** When `alex-mbp`'s runner connects to the new
  backend, the welcome names the conversations it is to hold. That list
  counts the conversations whose records name the device, and those whose
  open tree has a subagent there. This conversation is neither, as its tree
  is not open yet, so `alex-mbp` closes its browser. When the subagent goes
  on, its three pages are gone.

## Decision

**A conversation whose agents work when the backend stops, its root or only
its subagents, goes on after the restart, and its devices hold it
meanwhile.**

- **The mark.** The shutdown marks each live tree whose root's turn runs,
  as today, and now also each one whose root rests while its subagents
  work (`Tree::agents_work`). A tree that only waits for its commands is
  not marked: a runner's hello brings it back, as today. The mark says
  which it is, in a new column, `interrupted_turns.root`; a migration makes
  every stored mark a root's.
- **A root's mark** is taken and resumed as today: the conversation shows
  as resuming (0279), and the root's turn goes on if it ended interrupted.
- **A subagents' mark** restores the tree, which brings the subagents back,
  and each one's interrupted turn goes on. The root stays as it was: it
  does not show as resuming, and an interrupted turn an earlier crash left
  on it still waits for the user's Resume. The mark is taken once the tree
  is open; a restore cut short and done again brings back the same.
- **The welcome.** A runner's welcome also names, as held, each
  conversation a restart is resuming whose tree is not open yet: one whose
  mark is still stored, or one whose root's mark was taken and whose tree
  is being restored (the shard's `resuming` set). The welcome reads them in
  that order, and the open trees last, so a conversation that moves from
  one to the next while the welcome is read is named by one of them.
- With the user's `resumeAfterRestart` preference off, nothing goes on by
  itself, as today, and a device whose hello comes after the mark is taken
  lets go of the conversation, as today.
- A crash marks nothing, as today: work that brought the backend down does
  not run again by itself, and a device whose subagent's conversation is
  not open yet lets go of it at its hello, as today.

A runner sees no change. A page sees the subagents go on without the
conversation being opened.

**What it costs.** A conversation being resumed is named as held to every
device, not only to those it reaches. A device that holds nothing for it
does nothing. One it was detached from just before the restart, whose
release had not reached it, keeps it until its next hello.

## Spike

None ran. The behavior it fixes was reproduced on the slot's product with
two runners: a subagent's browser on a detached device was released at the
device's hello after each of three restarts, and kept when the device was
attached.

## Prior art

- A turn the shutdown interrupts already resumes by its mark (0279), and a
  runner already keeps the jobs the backend took up after a restart.

## Alternatives

- **Record the devices subagents work on, and name them in the welcome.**
  The browser stays, but the subagent still waits for someone to open the
  conversation, and it needs a table of its own.
- **Restore every conversation with a live subagent at start.** Finding
  them means opening every conversation's database.

## Open questions

None.
