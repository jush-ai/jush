---
status: proposed
---

# A file a job writes is recorded when it opens and when it closes, not at each write

Supersedes: 0065 (A foreground program writes its redirected file itself,
and its edit is read when it exits), the part where a program's edit is read
when the file is handed to it and when it exits: the redirection's own open
and close now bound it.

## Problem

An agent runs `find src -name '*.rs' > files.txt` in a tree of 23,000 Rust
files. Jush's `find` writes each name as it finds it, and every one of
those 23,000 writes is recorded as an edit of `files.txt` on its own: the
recorder locks the job's journal, reads and parses it from disk, reads the
whole file as the before side, writes, reads the whole file again as the
after side, and saves a snapshot and the journal. The command takes 13 s,
where `/usr/bin/find` writing the same file takes 2 s and Jush's `find`
writing into a pipe takes 1 s; a sample of the job's process shows the
walk itself under 1% of the time, the rest in the recorder's opens, reads
and renames.

Any writer that writes in small pieces pays the same: `for i in $(seq
8000); do echo "line $i"; done > lines.txt` takes 1.5 s, and 0.09 s when
its output passes through `| cat` first. Both also lose their diff: each
write's snapshots count towards the job's copy budget, which the file's
thousands of snapshots use up, so the edit shows the file without its
contents.

## Decision

A file a job writes through a descriptor it opened is one edit, read twice:

- **the before side when the descriptor opens:** a redirection such as
  `> f`, `>> f`, `2> f` or `exec 3> f`, or a utility's own open of a file
  to write, such as `tee f`, `sort -o f` or `find -fprint f`;
- **the after side when the job's last copy of that descriptor closes:**
  the end of the command or group the redirection belongs to, `exec 3>&-`,
  the job's end for a descriptor still open then, or the utility closing
  its file.

The writes in between go straight to the file, as a program's writes to a
redirected file already do (0065). A user sees the same edit under the
call, with the same diff, as soon as the command ends.

- **A program the shell hands the file to** takes a share of the hold until
  it exits, since the shell lets go of its own copy once the program has
  started; the command completes after that, as since 0065. The shell no
  longer reads the file again when it hands it over: one hold covers the
  shell's writes and the program's.
- **The same file opened again while it is held**, as `> f 2> f` does, or
  by two of the job's background tasks that append to it, joins the hold:
  one edit. A hold's sides are read under the job's lock on its holds, so
  an open, a close and the job's end of one file follow each other and the
  job's edits of the file make one chain.
- **Another change of a held file by one of the job's utilities**, a
  whole-file write (`cp tpl f`, `sed -i`), a rename or a removal, records
  its own edit as before. The hold is recorded up to it first and starts
  again after it from the file as the change left it: under the new name
  after `mv`, never after `rm`. So `exec 3> out.tmp; …; mv out.tmp
  out.txt` shows `out.txt` with everything written to it, before and after
  the rename.
- **Two jobs writing one file at once** each count the other's writes
  between their open and their close as their own, as two programs do
  since 0065. Until now a utility's or the shell's writes were recorded one
  by one.
- **Whole-file writes keep their own record:** `cp`, `mv`, `sed -i` and the
  native `file` commands already record one edit per operation, which
  this does not change.
- **A background task's copy** keeps its share, so the file stays held
  until the task closes it too. A file still held when the job ends is
  read then: what a task that outlives the job writes later is not the
  job's edit, as before (0061). A background task's program redirected to
  a file is now recorded up to the job's end, where it was not recorded at
  all.
- **The job's shell dying** while a file is held, killed by the system for
  memory, leaves no edit of the file, where the writes until then were
  recorded one by one. A program that dies holding its redirected file
  leaves its edit unread the same way since 0065.
- **The before side keeps the job's budget for copies:** once the job has
  spent it, a hold notes only the file's size and time, as an edit's before
  side does.

The recorder gains no interface; `Recorder::hold` and `Held::settle` are
what both kinds of writer use. The shell's scope owns the job's holds, by
the file's identity (`crates/runner-shell/src/holds.rs`):

```rust
pub(crate) struct Holds(Arc<Inner>);

struct Inner {
    /// Held while a hold's side is read; the recorder's lock is never held
    /// while this one is taken.
    table: Mutex<Table>,
    /// The identities held, which a write reads.
    held: RwLock<HashSet<FileIdentity>>,
}

struct Hold {
    /// None once the file is removed or replaced.
    held: Option<Held>,
    /// The opens whose copies are still open.
    shares: usize,
    /// Tells a share of a hold the job's end ended from one of a new hold
    /// on the same file.
    generation: u64,
}

/// One open's share, which the open's copies keep; its drop ends it.
pub(crate) struct Share { scope: Scope, identity: FileIdentity, generation: u64 }
```

- A redirection's `OpenFile::Controlled` carries a control of its own for
  each open, which its duplicates share; dropping the last of them
  releases the share. `ExecutionHost::open_file` returns that control with
  the file.
- `ExecutionHost::external_output` takes a share for a program handed a
  held file, which the program's completion drops.
- A utility's `Control::edit`, `edit_rename` and `edit_remove` record the
  holds on their path up to the change and start them again once its
  guard drops.
- A utility's `uucore::context::fs::File` carries the share
  `Control::open` returns for a file opened to write, which its
  `try_clone`s keep; dropping the last of them releases the share.
- `Scope::write` writes a held file without recording; a write to a file
  the job did not open to write, which no hold covers, is recorded as
  today.

Nothing stored or sent changes shape: an edit is the same segment in the
same journal, so an older backend and page read it as before.

## Spike

A probe ran one job per case on a Mac, against the recorder's journal, with
`main`'s code and with this design in the same debug build:

| Case | Today | This design |
|---|---|---|
| 8,000 `echo`s in a loop into one file | 2.81 s, no diff | 0.61 s, diff kept |
| `find crates -name '*.rs' > files.txt` | 0.88 s, no diff | 0.05 s, diff kept |
| `{ … } > f`, `>> f`, `exec 3> f` … `exec 3>&-`, `> f 2> f`, `exec > f` | one edit each | the same |
| A program (`/bin/echo`, `sh -c`) redirected to a file | one edit | the same |
| `tee f`, `sort -o f`, `find -fprint f`, `truncate`, `cp` | one edit each | the same |
| A pipeline, a subshell, a file the job found and changed | one edit each | the same |
| A background task that ends before the job | its writes included | the same |
| A background task that outlives the job | its later writes left out | the same |
| `exec 3> out.tmp`, writes, `mv out.tmp out.txt`, more writes | the writes after the rename left out | all of them |
| `cp tpl f`, `sed -i … f`, `rm f` while `f` is held | one edit each | the same |
| 40 background tasks appending to one file | one edit | the same; five runs, each one edit |
| Two background tasks writing one file with `>` | one edit | the same |

The appending tasks made 5 to 11 overlapping edits when a hold's after
side was read outside the job's lock, which is why it is read under it.

In the product, on the same debug runner, `find` over 23,000 files into a
file took as long as into a pipe, 1.5 s.

## Prior art

Version control and editors' local history record a file's state at
points the user sees, a save or a commit, not at each write the program
makes; VS Code's local history keeps an entry per save. Jush's points are
a command's opens and closes. No other agent harness records a shell
command's edits, so none has this cost: Claude Code and Codex record only
their own file-editing tools' changes.

## Alternatives

- **Buffer a utility's output to a file**, as C's standard library does
  for a file, so `find` writes in 64 KiB pieces. It speeds the utilities
  only, not `echo` in a loop, and each piece still reads the whole file
  twice, which grows with the file.
- **Keep the journal in memory between writes.** It removes the journal's
  reads and writes, but not the two reads of the whole file at each
  write, which grow with the file.
- **Read the job's files once when it ends.** A later job's or the user's
  changes would then count as this job's, which 0065 ruled out for the
  same reason.

## Open questions

None.
