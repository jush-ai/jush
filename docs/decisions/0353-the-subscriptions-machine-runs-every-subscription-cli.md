---
status: proposed
---

# Claude Code's machine becomes the subscriptions machine, which runs every subscription's CLI

## Problem

A subscription provider runs its vendor's own CLI as a process, signed in
with the user's subscription account. Claude Code is the only one today, so
everything that hosts such a CLI is written for it alone:

- The managed machine that runs the processes is the "Claude Code machine":
  `ManagedMachine::ClaudeCode` and `SandboxMachine::ClaudeCode`,
  `devices.machine = 'claude_code'` with the device name `Claude Code`, the
  manager's `JUSH_MANAGED_CLAUDE_CODE_MEM_MIB`, and "Claude Code’s machine"
  in the pages and errors.
- The `jush.claude-code` package installs one CLI only: its artifact line is
  `Claude Code` and its errors say so.
- The placement that starts a process on the machine, with its account
  directories, sign-in handovers, idle processes and start stages, lives in
  `provider-claude-code`, and the backend's side of it in
  `backend-user-shard/src/conversation/claude_cli.rs` downcasts to
  `ClaudeCodeProvider`.

A second subscription CLI, such as a vendor's `alex-cli` that signs in with
its own accounts, would need a second machine, a second package and a
second placement, each doing what the first does, or it would run on a
machine named after another vendor.

## Decision

The machine, the package and the placement serve any subscription CLI.
Claude Code becomes the first CLI on them and behaves exactly as it does
today. No user sees a change but the machine's name.

### The machine

| Before | After |
| --- | --- |
| `ManagedMachine::ClaudeCode`, `SandboxMachine::ClaudeCode` | `::Subscriptions` |
| `devices.machine` `claude_code` | `subscriptions` |
| device name `Claude Code` | `Subscriptions` |
| `JUSH_MANAGED_CLAUDE_CODE_MEM_MIB` | `JUSH_MANAGED_SUBSCRIPTIONS_MEM_MIB` |
| "Claude Code’s machine" | "the Subscriptions machine" |
| `claude_code_access()`, `claude_machine()` | `subscriptions_access()`, `subscriptions_machine()` |
| log events `claude_code_machine.*` | `subscriptions_machine.*` |

It is still one machine for the deployment, held by the master's shard, with
the same image, idle rule, network rule and memory limit. Every CLI's
accounts keep their directory `accounts/<id>` on it: an account's id is
unique across families, so no directory moves.

### The package

`jush.claude-code` becomes `jush.subscription-cli`, with the operations
`subscription-cli.ensure` and `subscription-cli.status`. The release record
names the CLI it is of, and `status` reads which CLI it lists. A CLI is named
by its name, such as `Claude Code`, which is also its line in the runner's
cache, as it is today:

```rust
// crates/command-package-subscription-cli-protocol
pub struct Release {
    /// The CLI's name, such as `Claude Code`.
    pub cli: String,
    pub version: String,
    /// Jush's platform keys, such as `darwin-arm64` and `linux-x64-musl`,
    /// which the vendor's keys are read into.
    pub platforms: BTreeMap<String, Artifact>,
}

pub struct StatusRequest {
    pub cli: String,
}
```

The artifact's form stays one executable file. A CLI that ships otherwise
adds its form when it comes.

### The placement

A new crate, `provider-subscription-cli`, holds what every subscription CLI's
provider shares, moved out of `provider-claude-code`: `Placement`, `CliSite`,
`CliSystem`, `CliStart`, `CliStarter`, `CliWork`, `Handover`, `Placed`,
`ConfigDir`, `StartError`, `StartStage`, `StartSteps`, `WorkLease`,
`AccountWork`, `AccountMachine`, the machine's `IdleProcesses` with their
seats, and `close_process`. Claude Code keeps what is its own: its kept
process, its prepared one, and how it logs them. Two things are added, which
say what differs between CLIs:

```rust
// crates/provider-subscription-cli
/// A subscription's CLI, as the Subscriptions machine runs it.
pub struct Cli {
    /// Its name, such as `Claude Code`.
    pub name: &'static str,
    /// The directory of an account's directory that holds each process's
    /// own directory, such as `projects`.
    pub process_parent: &'static str,
}

/// A vendor's releases of its CLI, as the backend follows them.
pub trait CliReleases: Send + Sync {
    fn latest(&self, refresh: bool) -> BoxFuture<'_, Result<Release, ReleaseError>>;
    fn believed(&self) -> Option<Release>;
}
```

- `provider-claude-code` declares `CLI`, and `ClaudeReleases` implements
  `CliReleases`. The backend's services hold each CLI's release reader by
  its name.
- A family says which CLI its requests run and builds the runtime of a
  provider of its own over a placement, since only the family knows its
  provider's type. The backend downcasts to no provider type:

  ```rust
  // crates/provider-registry: ProviderFamily
  fn cli(&self) -> Option<&'static Cli>;
  fn process_runtime(
      &self,
      provider: Arc<dyn Provider>,
      placement: Rc<dyn Placement>,
  ) -> Option<Box<dyn ProviderRuntime>>;
  ```

- `AccountMachine::state()` goes, and `AccountMachine::run` names the CLI
  whose work it runs. Claude Code's `MachineState`, its feature flags'
  refresh times and the MCP protocol its CLI asks for, is Claude Code's
  alone, so `ClaudeCodeFamily` owns it and hands it to each provider it
  builds.
- `Capabilities`, whose one field named the package, goes with
  `Provider::capabilities()`: every CLI provider would name the same
  package, and the family says which CLI. The web API's `cliPackage`
  becomes `cliName`, the CLI's name.

### The backend

`conversation/claude_cli.rs` becomes `conversation/subscription_cli.rs`:
`ClaudeMachine` becomes `SubscriptionsMachine` and `ClaudeCli` becomes
`SubscriptionCli`. The versions it keeps are keyed by the machine and the
CLI, the install it shows in Settings by the provider entry as now, and its
messages name the CLI from `Cli::name`, such as "Claude Code 2.1.0 could not
be installed: …". `Services::claude_releases` becomes
`Services::cli_releases`, every CLI's reader.

### Upgrade

An operator upgrades as usual. What the new release meets:

- **The control database:** a migration rewrites `devices.machine` and the
  device's name, and the column's check names `subscriptions`.
- **A machine that runs across the upgrade:** the earlier manager's records,
  of a gVisor sandbox it leaves and of a Simple Isolation process, name
  `claude_code`. The new manager reads that value as `subscriptions` in both,
  and reads an absent value in the first as it does today.
- **A rollback:** the record of a gVisor sandbox the new manager leaves names
  no machine when it leaves the Subscriptions machine. The earlier manager
  reads an absent machine as its Claude Code machine, so it starts and takes
  the sandbox over; it would fail to start on a name it does not know. A
  Simple Isolation process record names `subscriptions`, which the earlier
  manager cannot read: it stops that process, and the next request starts
  the CLI again.
- **The CLI already on the machine:** the runner's cache keeps one record
  per digest, naming the package it was installed for, and never rewrites
  it on an install. It already renames what an earlier release's records
  name, `demi.<name>`, as it reads them; it now renames `jush.claude-code`
  to `jush.subscription-cli` too, so the CLI is listed in its line and
  nothing downloads. A machine that runs across the upgrade keeps the
  earlier runner until its next boot. There `status` lists no version, so
  the first start on each runner connection installs as on a machine with
  none: it waits for the vendor's newest release, then runs `ensure`, which
  finds a version the cache has by its digest. If the vendor has published a
  newer one meanwhile, that start waits for its download, and if the
  vendor's release cannot be read, it fails. This lasts until the machine
  stops idle and boots with the new runner.
- **An operator who set `JUSH_MANAGED_CLAUDE_CODE_MEM_MIB`:** the upgrade's
  configuration check refuses the new release before it switches, naming the
  variable, as it does any `JUSH_MANAGED_*` variable no setting reads. The
  operator renames it to `JUSH_MANAGED_SUBSCRIPTIONS_MEM_MIB`, which
  `docs/cloud/setup.md` lists.
- **The log:** earlier rows keep the event names they were written with.

## Spike

None. The steps above were read from the code: the cache's records in
`crates/runner-command-packages/src/cache.rs`, the left sandbox's record in
`crates/machine-manager-protocol/src/lib.rs`, the Simple Isolation process
record in `crates/machine-manager/src/runtime/simple.rs`, and the upgrade's
configuration check in `crates/server/src/moving.rs`.

## Prior art

- [Zed's agent server store](https://github.com/zed-industries/zed/blob/main/crates/project/src/agent_server_store.rs)
  runs several vendors' agent CLIs through one store: one
  `ExternalAgentServer` trait, one installer for every downloaded archive,
  each agent in a directory under its id. Jush does the same with one package
  and one placement, and departs in where the CLI runs: on a managed machine
  of the deployment, not on the user's computer.
- The [Agent Client Protocol registry](https://github.com/agentclientprotocol/registry)
  describes each agent by its id and a per-platform download, as the release
  record does here.

## Alternatives

- **A machine, package and placement for each CLI.** Each would repeat the
  first's code, and each would hold a memory limit and an idle machine of
  its own for what one machine already does.
- **Keep the name "Claude Code machine" and run other CLIs on it.** The name
  would say what the machine is not.
- **Keep `jush.claude-code` and add a package for each CLI.** Two packages
  with one purpose.

## Open questions

None. The names, the machine's "Subscriptions" ("the Subscriptions machine"
in a sentence), the package `jush.subscription-cli` and the crate
`provider-subscription-cli`, were agreed.
