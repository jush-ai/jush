---
status: proposed
---

# Jush's `find` refuses to walk a whole disk or home folder without a depth bound

## Problem

An agent looks for a file it has not seen and runs `find / -name
report.pdf` or `find ~ -name '*.key'`. On a Mac the walk takes minutes,
prints hundreds of thousands of lines into the call's output, and, as it
enters Desktop, Documents, Downloads or iCloud Drive, can make macOS ask
the user, one dialog per folder, to let the runner read their private
files. The user did not ask for any of it, and the agent learns nothing it
could not have learned from the one folder the file belonged in.

## Decision

`find`, which the job's shell runs as Jush's own utility (0068), refuses a
starting point that is a whole disk or a home folder unless the command
bounds its depth with `-maxdepth`. It prints why and what to do instead to
standard error, and exits with status 1 without walking anything:

```text
$ find ~ -name '*.key'
find: '/Users/alex' is a home folder: walking all of it takes minutes and
can make macOS ask the user for access to their private folders. Search
the folder the file belongs in, or bound the depth with -maxdepth. mdfind
finds a file by name from Spotlight's index at once.
```

- **Refused starting points**, compared once the path is resolved
  (symbolic links followed, `/c/` read as `C:\` on Windows as the job's
  other paths are), and on a Mac as the same folder by another path:
  - a disk's root: `/`, on a Mac also the data volume's root
    `/System/Volumes/Data`, which holds the user's files under another
    path, and on Windows a drive's root such as `C:\`;
  - the home folder of the user the runner runs as, also as
    `/System/Volumes/Data/Users/alex` on a Mac;
  - the folder that holds the home folders, when the home folder is in the
    system's own: `/Users` on a Mac, `/home` on Linux, `C:\Users` on
    Windows. A service's home elsewhere, such as `/var/lib/app`, has an
    ordinary folder above it.
- **Bounded walks still run:** any `-maxdepth` lets every starting point
  through, such as `find / -maxdepth 2 -name '*.conf'`: the agent chose how
  deep to go.
- **What is not a walk runs:** `find --help` and `find --version` in the
  home folder, and starting points `-files0-from` reads from a list, which
  are the list's own choice.
- **Every other starting point runs as before**, such as `find
  ~/Downloads -name '*.pdf'` or `find /usr/local -name libz.dylib`.
- **The hint names a tool the system has:** `mdfind` on a Mac, `locate`
  on Linux only where it is installed, and nothing more on Windows.
- **Where it applies:** every `find` a job's shell runs, the agent's own
  and one in a script it runs, since both are Jush's. A program such as
  Python that starts `/usr/bin/find` itself runs the system's, as it does
  today.
- **Other commands are left alone.** `grep -r`, `du`, `ls -R`, `fd` and
  `rg` are the system's own utilities (0068), whose arguments Jush does not
  read.

`find` reads its starting points as it always has, macOS's leading options
such as `-x` and `-f path` included, and asks the host about each before
it walks any, so there is one reading of its command line:

```rust
// vendor/uutils-findutils: findutils::find
pub trait Dependencies {
    /// Why the walk must not start at `path`, which it would walk to any
    /// depth, when the host refuses it.
    fn refuse_start(&self, _path: &OsStr) -> Option<String> {
        None
    }
}
impl StandardDependencies {
    pub fn refusing(self, refuse: fn(&OsStr) -> Option<String>) -> Self;
}

// crates/runner-shell/src/utilities.rs
let dependencies = StandardDependencies::new().refusing(find_scope::refusal);
findutils::find::find_main(&args, &dependencies)
```

Nothing is stored and nothing crosses a protocol, so an older backend,
page or runner meets no change; an older runner keeps walking.

## Spike

Jush's `find` walks a large source tree in 1.0 s against `/usr/bin/find`'s
1.9 s on the same Mac, so what the refusal spares is the extent of the
walk, not a slow walker. A job on a Mac ran each form:

| Command | Result |
|---|---|
| `find -x / …`, `find -f / …`, `find -E ~ …` | refused |
| `find /System/Volumes/Data …`, `…/Data/Users …`, `…/Data/Users/alex …` | refused |
| `find /Users …`, and `find -name …` in the home folder | refused |
| `find / -maxdepth 1 -name tmp`, `find /usr/local/bin …` | ran |
| `find --version` in the home folder | ran |

## Prior art

- **Claude Code** gives the model Glob and Grep tools that search the
  project, and its Bash tool's description tells the model not to run
  `find` or `grep`; reported issues show models running them through Bash
  anyway.
- **Gemini CLI** bounds its glob and grep tools to the workspace's folders
  (`packages/core/src/tools/glob.ts`, `validatePathAccess`); its shell tool
  runs any `find`.
- **Codex** tells the model to prefer `rg --files`; its shell runs any
  `find`.
- **Cline** refuses to list a root or a home folder in its own file-listing
  tool (`apps/vscode/src/services/glob/list-files.ts`), answering with an
  empty list.
- **GNU `rm`** refuses `rm -rf /` unless told `--no-preserve-root`.

Jush has one shell tool and no search tool, so the guard sits where the
others put their search tools' bounds: in its own `find`. Unlike Cline it
says why it refused and what to run instead, so the agent does not read
the refusal as "not found".

## Alternatives

- **A sentence in the shell tool's description.** Claude Code's experience
  shows models still run the command, and every call pays for the words.
- **A search tool beside the shell.** It would be a second way to do what
  the shell does, and models still reach for `find` in the shell.
- **Stopping the walk after a time or a count.** The minutes and the
  privacy dialogs come before any limit that would not also stop a
  legitimate search of a large project.
- **Refusing only without `-name` or another filter.** A filter does not
  shorten the walk, only the output.

## Open questions

None.
