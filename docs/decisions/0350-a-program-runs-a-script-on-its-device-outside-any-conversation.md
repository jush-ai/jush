---
status: proposed
---

# A program runs a script on its device outside any conversation

## Problem

A program does work on its devices that is no conversation's. Each time a
chat message arrives, it runs a filter on the device that serves the chat's
agent: the message goes in on standard input, and the filter prints whether
the agent should hear of it. Every hour it fetches a list of repositories
with the device's own SSH key. It posts a review comment with a token that
only the program holds, which must not land in a file on the device.

Today a program reaches a device's shell only through a conversation's
agent: it sends a message, a model reads it, and the agent runs a command.
That costs a model's turn for each filter run, writes every run into the
conversation's transcript, and runs the command only as the agent chooses to
write it.

## Decision

`device.exec(script, options)` runs `script` on a paired device as the
agent's shell runs a command, and resolves once it exits:

```ts
const { exitCode, stdout, stderr } = await device.exec('bun filter.js', {
  cwd: '/work/filters/release-notes',
  env: { CHAT_TOKEN: token },
  stdin: JSON.stringify(event),
  timeout: 10_000,
})
```

- The script runs in the device's job shell, the one the agent's commands
  and a schedule's change watch run in: the device's environment with `env`
  above it, in `cwd`, the home directory the device's runner reported by
  default. Each run is a job of its own, so the runs on one device run side
  by side and none waits for another.
- An exit status other than 0 is a result: the status the agent's shell
  reports for the same command, such as 1 with why on standard error for a
  `cwd` that does not exist.
- The promise rejects only when there is no exit status, with a `JushError`
  whose `code` says why:
  - `device_offline`: the device's runner was not connected, and the script
    never ran.
  - `command_timed_out`: the script ran past `timeout` milliseconds and was
    stopped on the device, with `SIGTERM` and, 5 seconds later, `SIGKILL`;
    the error's `body.output` holds what it printed before.
  - `device_lost`: the device lost the job, as a runner that started afresh
    does; its runner was away when the script ran past `timeout`, and stops
    the job when it is back; or its runner's connection broke before the
    core sent it the whole of `stdin`.
  - `backend_closing`: the core closed while the script ran. The device
    stops the script when it connects to the core again, which knows no such
    job.
- A device whose runner drops while the script runs keeps running it, as it
  keeps the agent's commands, and the result comes once the runner is back,
  unless `timeout` passes first. Without `timeout`, or with 0, a run waits
  as long as its script runs.
- A runner that ends, or whose connection breaks, after the core sent it the
  whole of `stdin` but before its script read all of it, loses the rest: the
  script reads the input's end there, as a program whose `ssh` client went
  does, and the run answers the script's status, since the core cannot tell.
- `options.signal` stops the run, and the promise rejects with its reason,
  as a `fetch`'s does. The run of a program whose connection to the core
  ends is stopped too.
- The route takes a paired device, as browsing does, and answers 404
  `device_not_found` for a Cloud.
- The run is no conversation's: no transcript, command list or turn holds
  it. `env` reaches the job's shell process, and no file on the device; the
  output is kept on the device until the core has read it, as every job's
  is.
- The script has no `jush` commands: those act for an agent of a
  conversation, and the run has none.

The standard streams are text: `stdin` is a string, and `stdout` and
`stderr` are read as UTF-8, an invalid byte as U+FFFD. A device keeps 16 MiB
of a job's output; beyond that the middle is left out, and `leftOutBytes`
counts what is missing. The request, the script and its input with it, is a
JSON body, at most the 1 MiB every JSON body of the contract may be.

### Contract

`POST /devices/:id/exec` answers once the script ended:

```rust
pub struct DeviceExec {
    pub script: String,
    /// None for the home directory the device's runner reported.
    pub cwd: Option<String>,
    /// Above the device's own environment.
    pub env: BTreeMap<String, String>,
    pub stdin: String,
    /// None for no limit.
    pub timeout_ms: Option<u64>,
}

pub struct DeviceExecResult {
    pub exit_code: i32,
    pub output: ExecOutput,
}

pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub left_out_bytes: u64,
}
```

A refusal answers 409 `device_offline`, `device_lost` or
`command_timed_out`, whose `ErrorBody` carries `output`, or 503
`backend_closing`. A script that ran past its time is the caller's, not the
backend's failure, so it answers no 5xx, which the backend logs as its own.
The SDK gives the answer's output as `exitCode`, `stdout`, `stderr` and
`leftOutBytes` of one object. It sends a `timeout` of 0, or one that is not
finite, as none, and a fraction rounded up.

### How the core runs it

The core reaches the device through device access, the owner's way to a
device whose runner is connected, which touches no conversation's files and
wakes nothing. One function runs a script to its end, for the change watch
and for this route (`crates/core-device-access/src/scripts.rs`): the watch
gives it the conversation's device and context, and 60 seconds.

The function drives one job on the device as `jush device shell` drives its
far job, not through the agent's shell environment, whose records, pages and
command numbers no script has:

- The script's standard input is a pipe the core fills, as a `jush device
  shell` job's is. A pipe that fails before the script ends, as one does when
  its runner's connection breaks before the core sent the whole input, ends
  the run as lost and stops the job. A pipe counts as whole once the core
  sent its end, so what a runner that ends later held of it is past the
  core's sight.
- Its output is what the job's views carried when they hold all of it; else
  the core reads the output the device keeps, at its end. A read that fails
  leaves each stream's first and newest bytes the views carried, with what
  lies between counted in `leftOutBytes`. The agent's shell does the same
  with the same code (`crates/runner-client/src/job_views.rs`).
- A stop, at `timeout` or at the caller's going, stops the job as a stop of
  the agent's command does; while the runner is away, the job is lost at
  once instead, since only that runner could report the stop. The core's
  close ends the runner's connection before such a stop reaches it; the
  runner's next connection stops the job, since the core it connects to
  knows no such job.
- The exit status of a script a signal ended is the one the agent's shell
  reports, 130 for `SIGTERM` or `SIGKILL` and 128 for another, and 127 for a
  job bash could not run or whose shell process ended under it, with why
  after what it printed: the two share the mapping and the line.

A job's command context names its scope, a conversation, or the provider
entry for work such as installing a CLI. The scope of this work is `device`,
and no command reads its context. A job whose scope is no conversation's
gets no scratch directory and no `JUSH_SCRATCH` from the runner now: the
runner's next connection removes every directory no conversation owns, so
one it made for such a job could go while the job ran.

A job's shell process ends the job's input when the runner that relayed it
goes: the runner that takes the job over has no part of that input, and a
script left waiting for it would never end.

An older runner runs these scripts too: a job with a pipe on its standard
input and a scope that is no conversation's both run there already. It still
makes a scratch directory for the `device` scope, and its job shells wait
for the input of a runner that went.

## Spike

Whether a run per message is cheap enough, and whether stops, drops and
timeouts end as the decision says, was run on the built change: a program
connected to `jush-core serve`, and a runner on the same Mac, both
development builds.

| Case | What it showed |
| --- | --- |
| `cat` with a line on standard input, one after another | 17 ms each at the median, 25 ms at most |
| 40 runs a second for 10 seconds | 17 ms each at the median, 28 ms at the 99th percentile |
| 50 runs started at once | all ended within 106 ms |
| A Bun script that reads standard input and prints JSON | 26 ms each at the median |
| `timeout: 800` on `sleep 30` | `command_timed_out` (409) after 838 ms with what it printed, its shell gone, and no warning in the core's log |
| `timeout: 0`, `timeout: Infinity` and `timeout: 1.5` | all ran, the first two with no limit |
| `signal` aborted after 500 ms | the script's shell gone 50 ms later |
| The program's process killed while its script runs | the script's shell gone within 50 ms |
| The runner killed 1 second into a 3-second script, then started again | the script's own output and status 0 once the runner was back; a run asked for meanwhile answered `device_offline` |
| The runner killed, and not back by `timeout` | `device_lost` at the timeout, the script still running; the runner stopped it 400 ms after it was back |
| 20 MB printed, then a last line | 15.8 MB of it with the last line at its end; `leftOutBytes` 4,204,032 |
| `env` holding a secret | the secret in no file under the runner's home |
| The core closed while `sleep 600` ran | the close took 4 ms, and the run rejected with `backend_closing`; the script ran on until the core was opened again, whose runner connection stopped it 313 ms later |
| The runner killed as a 20 MB script ended, so the core's read of its kept output failed | status 0 with the first 8 KiB and the newest 8 KiB, its last line at the end; `leftOutBytes` 19,983,628 |
| The job's shell process killed after the script printed a line | status 127 with the line, and why on standard error |
| `exit 3`, `head -c 10` and `sleep 1; exit 5`, each given 1 MB on `stdin` it did not read to its end | their own statuses, 3, 4 and 5 |
| The runner killed while `sleep 2; wc -c` waited with 1 MB on `stdin`, then started again | `wc` read the input's end 0.7 s after the runner was back, at 655,360 bytes, and the run answered its status 0 |
| 1,057 runs started around a runner's kill | 1,050 answered 0, 7 `device_lost`, and none hung |
| A script that writes into `$JUSH_SCRATCH` | the variable is not set |

The change watch runs through the same function now: a watch on
`date +%H:%M` recorded its first output and woke its conversation a minute
later with the output before and after.

## Prior art

- Codex's app server
  ([`command/exec`](https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/command_exec.rs))
  runs a command without a thread or a turn, with `cwd`, `env` over the
  server's environment and `timeoutMs`, and answers `exitCode`, `stdout`
  and `stderr`. A timeout answers exit code 124, and the end of a client's
  connection terminates its commands
  ([`command_exec.rs`](https://github.com/openai/codex/blob/main/codex-rs/app-server/src/command_exec.rs)).
  Jush takes a script, since its jobs run scripts in its own shell on every
  system, and refuses a timeout, since 124 is a status a script may exit
  with.
- Cloudflare's [Sandbox SDK](https://developers.cloudflare.com/sandbox/guides/execute-commands/)
  `exec` answers `exitCode`, `stdout` and `stderr` for any exit status, and
  throws only for a command that could not run; `timeout` is milliseconds.
- Node's `child_process.execFile` takes `cwd`, `env`, `timeout` in
  milliseconds and `signal`; its promise rejects on an exit status other
  than 0, as E2B's `commands.run` does with `CommandExitError`. Jush answers
  the status, which a filter's script uses as its answer.

## Alternatives

- A conversation's agent runs it: a model's turn for each run, written into
  the transcript, as the problem says.
- A raw process (the runner's `spawn`): it ends with the connection that
  started it, runs a program rather than a script in Jush's shell, and would
  be a second way to run a command on a device.
- A streaming run, with input written and output read while it runs: no
  work asked for it. A program that needs one runs a server on the device.

## Open questions

None.
