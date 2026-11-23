---
status: proposed
---

# A program's plugin gives the shell commands of its own

## Problem

A program's plugin gives the agents its commands under `jush`, such as
`jush acme ticket create`. A program that already names its commands after
itself, `acme ticket create`, has them written in its scripts, its runbooks
and the scripts its agents wrote, which run them in subprocesses. Moved to
Jush, every one of them must be rewritten to `jush acme …`, and from then
on its people and its agents know two spellings of each command.

The plugin interface already places a command tree either under `jush` or
at the shell's own level (`Placement::Root`), and a device's runner already
makes a command at that level run in every job and every process it starts.
Only the SDK's `definePlugin` cannot say so.

## Decision

A top-level entry of `definePlugin`'s `commands` takes a `placement`:
`'jush'`, the default, puts the group under `jush`, and `'root'` makes it a
command of the shell, run by its own name:

```ts
const acme = definePlugin({
  id: 'acme',
  name: 'Acme',
  description: 'Gives the agent `acme`, which files and reads tickets.',
  commands: {
    acme: {
      placement: 'root',
      summary: "Acme's tickets",
      index: 'Files and reads Acme tickets.',
      commands: {
        ticket: { summary: 'Tickets', commands: { create: leaf({ … }) } },
      },
    },
  },
})
```

- The agent and every process its commands start run `acme ticket create`,
  as they run `jush`: `acme --help`, its permissions, a call your program
  is asked again after it restarts, and its failures behave as a `jush`
  command's do.
- The capability index lists it beside Jush's groups. Its opener says
  `<command> --help`, where it said `jush <group> --help`, since not every
  command it lists is under `jush`: "Commands, run in the shell;
  `<command> --help` for details:".
- A top-level command's name is the program's own. The core refuses `jush`,
  a name another plugin takes, and the shell words and common tools it
  reserves (`crates/device-interface/src/reserved.rs`), such as `ls`, `git`
  or `node`.
- A device's shell answers its own builtins and the utilities it carries,
  more names than the core reserves, such as `history`. A device left the
  whole manifest uninstalled for such a name, so that every command of the
  core failed there and its connection ended over and over; it now leaves
  out that one command and keeps the rest, since the shell would answer the
  name first and an alias of it would shadow the system's program in a
  subprocess.

The manifest and the core do not change: the manifest's
`Commands.placement` already says `root`.

## Spike

A plugin with `acme` at the root: the agent ran `acme ticket create`, and so
did a `bash -c` it started; `acme --help` showed the group, the index listed
`- acme: Files and reads Acme tickets.`, and `jush acme` was refused as an
unknown group. A root named `jush`, and two plugins with one root name, were
refused as the process started. A root named `history`, which the core takes
and the device's shell answers itself, made every command of the core fail
on the device ("job manifest is not installed") while its connection ended
again and again.

## Prior art

- A package manager puts a package's executables on `PATH` under the names
  the package gives them, npm's
  [`bin`](https://docs.npmjs.com/cli/v10/configuring-npm/package-json#bin)
  and Python's console scripts: the program is run by its own name, not
  through the manager.
- Git runs `git-<name>` on `PATH` as `git <name>`: the opposite direction,
  one spelling under the tool's name, which this decision keeps as the
  default.

## Alternatives

- A script on each device, `acme`, that runs `jush acme "$@"`: the index,
  the help and the errors would still say `jush acme`, and each device
  would need the script installed and kept.
- A separate list beside `commands`, such as `rootCommands`: one more field
  for what the manifest says with one value per tree.

## Open questions

None.
