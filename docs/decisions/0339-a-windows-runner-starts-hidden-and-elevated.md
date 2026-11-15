---
status: accepted
---

# A Windows runner starts without a window, and elevated when the user allows

## Problem

Alex pairs a Windows 11 computer. Each time Alex logs in, and again after
each runner update, a Windows Terminal window titled `powershell.exe`
opens and stays on the taskbar. Closing it stops the runner.

The scheduled task that starts the runner runs `powershell.exe -WindowStyle
Hidden`. Windows 11 hands a new console to Windows Terminal, its default
terminal, and Windows Terminal minimizes the window instead of hiding it
([microsoft/terminal#12464](https://github.com/microsoft/terminal/issues/12464)).
With the classic console host the window flashes before it hides.

The runner also runs with Alex's limited rights, although Alex's account is in
the Administrators group. An agent that installs a program for all users,
changes a system setting or deletes a scheduled task in the root folder gets
"Access is denied", and Alex opens a terminal with Run as administrator to do
it by hand.

## Decision

### No window

An installation on Windows holds `jush-start.exe` beside `run.ps1`: a
program of the Windows subsystem, which Windows starts without a console. It
runs the rest of its command line with `CREATE_NO_WINDOW`, waits for it, and
exits with its exit code. It reads nothing and decides nothing, so it does not
change between releases.

```text
  Task Scheduler ─▶ jush-start.exe            (no console, no window)
                     └▶ powershell.exe …        (CREATE_NO_WINDOW)
                         └▶ run.ps1 run --log
                             └▶ jush-runner.exe run …
```

- The scheduled task's action, and the `Run` value where Windows refuses
  the task, run `jush-start.exe` with the PowerShell of today's loop, named
  by its full path and without `-WindowStyle Hidden`.
- The runner carries `jush-start.exe` in its own executable. The runner's
  build script compiles it with the same `rustc`, target, linker and flags
  as the runner, so a release ships one file per target as before. It links
  it reproducibly, with no link time and no debug record, so every build
  gives the same bytes. Each runner writes it into its installation when
  the copy there differs, before it registers the task.

### Elevation

Windows runs a program of an account in the Administrators group with the
account's limited rights, until User Account Control (UAC) elevates it: then
it runs with the account's administrator rights, as a program started with
Run as administrator does. An elevated runner is still the user's own
account, with the user's home, files and settings. It never runs as the
built-in Administrator account or as SYSTEM.

The runner reports its elevation in its hello:

```rust
/// Whether a Windows runner runs elevated.
pub enum Elevation {
    /// It runs elevated, or the account is in the Administrators group and
    /// UAC is off.
    Elevated,
    /// The account is in the Administrators group and the runner runs with
    /// its limited rights: a request shows a UAC prompt.
    Elevatable,
    /// The account is not in the Administrators group. A prompt would ask
    /// for another account's password, and what it starts would run as that
    /// other account.
    Unavailable,
}

// The hello's new field; absent on macOS, Linux and an older runner.
pub elevation: Option<Elevation>,
```

- **Staying elevated.** The scheduled task runs "with highest
  privileges" (`HighestAvailable`) once an elevated process registered it.
  Windows then starts the runner elevated at each logon and each start of
  the task, with no prompt. Registering that task needs administrator
  rights; with limited rights Windows answers "Access is denied". A runner
  with limited rights never registers a task at the least privileges over
  one with the highest.
- **Started elevated.** A runner started elevated, as from an
  administrator's terminal or an administrator's SSH session, registers the
  task with the highest privileges at once, with no prompt, whatever a
  prompt was answered before: the person chose to start it elevated.
- **Asking once.** An `Elevatable` runner of the task whose task does not
  run with the highest privileges asks after it first connects, once an
  installation:
  1. It starts `jush-start.exe "…\jush-runner.exe" elevate --home …` with
     the verb `runas`. Windows shows the UAC prompt on the desktop. The
     elevated process inherits nothing of the environment, so it is told
     its installation, and it needs nothing of `run.ps1`, which the release
     before wrote.
  2. The elevated runner of the action `elevate` registers the task with the
     highest privileges, removes the scheduled tasks of before the rename,
     which only an elevated process can delete, and exits.
  3. When it succeeded, the runner hands over to the task, leaving its
     jobs, as an update does. The task starts the elevated runner once
     this one ended, and its hello says `Elevated`. A runner whose task
     runs with the highest privileges already, from a grant whose runner
     ended before it could hand over, hands over at once, without asking.
- **A prompt waits for its answer.** Windows keeps a UAC prompt on the
  desktop until someone answers it. The prompt's own task outlives the
  connection, so the runner serves meanwhile, a drop neither waits for the
  answer nor loses it, and a grant hands over whenever it comes. The runner
  says whether a prompt shows (`Outbound::ElevationPrompt { showing }`),
  and a request while one shows asks nothing more.
- **A prompt that showed is kept.** Windows keeps a prompt on the desktop
  after the runner that asked ended, as an update or a crash ends it, so a
  runner that asked at each start would pile prompts up. A prompt that
  shows writes `elevation-asked` in the installation, whatever its answer,
  and no runner asks by itself again; Request on the device's page asks
  again. A runner with no desktop to show the prompt on goes on with
  limited rights and asks at its next start.
- **A task an elevated process made.** Windows refuses a process with
  limited rights to change or delete a task an elevated process registered,
  as an installer run from an administrator's terminal did, at any run
  level. A limited runner then leaves that task as it is, with no `Run`
  value beside it, and asks to elevate: the elevated runner registers it
  anew. An elevated runner that is asked to uninstall removes its task
  itself before it lets the installation go, since the `uninstall` that
  asked may run with limited rights. An `uninstall` with limited rights
  that finds such a task, with no runner to remove it, asks through the
  prompt for an elevated PowerShell that removes it, as an uninstaller
  does; with no desktop, or after a No, it prints the command to run in an
  administrator's PowerShell. A task left behind would start whatever a
  program of the account puts in its place elevated, with no prompt.
- **The device's page.** On Windows, the Device group has a row:

  ```text
    Device
      Name                  alex-pc                    [Rename]
      System                Windows 11 24H2
      Runner                Up to date
      Administrator rights  No                         [Request]
      Paired                November 15, 2026
  ```

  - `Elevated` reads Yes.
  - `Elevatable` reads No, with Request, which is off while the device is
    offline.
  - `Unavailable` reads No, and the description says that the account
    is not in the Administrators group.

  Each state has a description under its value, so the row keeps its
  height. Request asks the runner again, whatever it kept, and settles once
  the prompt shows. While a prompt shows, whoever asked, Request is busy on
  every page and the description asks the person to allow it on the
  device's screen. A Yes shows Yes once the elevated runner says hello; a
  No gives Request back. Only a prompt that could not show says so in a
  toast. On the wire, Request is `Inbound::ElevationRequest { id }`,
  answered by `Outbound::ElevationAnswer { id, outcome }`, where the outcome
  is `shown` or `unavailable`. ShellExecuteEx fails at once when no prompt
  can show and holds one that shows until it is answered, so the runner
  answers `shown` when it has not failed within two seconds. A runner
  answers every request before it hands over, so a grant within those two
  seconds, or a task found elevated already, never reads as a device that
  went offline.
- **What an older part meets.** An older runner says nothing of its elevation,
  and the page shows no row for it. An installation of before keeps its
  task until the runner of this release registers it again with
  `jush-start.exe`, and asks then.

### What elevation changes

- Every command an agent runs on that computer runs elevated: it may
  install programs for all users and change system settings. The runners on
  macOS and Linux keep the user's own rights.
- A file an elevated process creates is owned by the Administrators group.
  The account's own programs still read and write it.
- An elevated process does not see the network drives mapped in the
  account's limited session, as Windows does for every elevated program.
- The task runs `jush-start.exe`, `run.ps1` and `jush-runner.exe` from the
  installation in the user's home, which every program of the account may
  write. A program of the account that replaces one of them is started
  elevated at the next start of the task, with no prompt. Keeping them
  writable by administrators alone would make every install, update and
  removal ask for elevation, so they stay the account's.

## Spike

- **The window.** On a Windows 11 24H2 computer whose default terminal is
  Windows Terminal, a scheduled task that runs `powershell.exe -WindowStyle
  Hidden` opened a Windows Terminal window. The same task that runs a
  Windows-subsystem program, which starts PowerShell with
  `CREATE_NO_WINDOW`, opened none.
- **The registration.** `Register-ScheduledTask` with the highest run level
  from a limited PowerShell answered "Access is denied".
- **The build.** A build script compiled a Windows-subsystem `main` with
  `$RUSTC`, `$RUSTC_LINKER` and `$CARGO_ENCODED_RUSTFLAGS` for
  `x86_64-pc-windows-msvc` under cargo-xwin, and `include_bytes!` embedded
  it: 218 KB, subsystem 2.
- **A prompt nobody answers.** The runner's prompt stayed on the desktop
  for 271 s, until it was answered; a No failed `ShellExecuteEx` with
  `ERROR_CANCELLED`. A prompt stayed on the desktop after the process that
  asked was ended.
- **A task an elevated process made.** A limited PowerShell could neither
  register again nor delete a task an elevated one had registered, at the
  least privileges or the highest. It could delete a task it had made
  itself, which an elevated process later registered again with the
  highest privileges.
- **Handing over.** Registering a running task again did not stop its
  instance, and a start queued behind it ran the new definition once the
  instance ended.
- **A path with spaces.** A task whose program's path holds spaces started
  it with the path quoted.
- **The same bytes.** Two builds of the starter differed in the link time
  and in the debug record's PDB identifier; without the debug record and
  with `/Brepro` they were the same.

## Prior art

- [Ollama](https://github.com/ollama/ollama) builds its tray app with `-H
  windowsgui`, and the app starts `ollama.exe serve` hidden
  (`app/server/server_windows.go`, `scripts/build_windows.ps1`). Jush's
  starter is the same split, with nothing in it but the start.
- [espanso](https://github.com/espanso/espanso) builds its whole program for
  the Windows subsystem and attaches to the parent's console for output
  (`espanso/src/main.rs`, `espanso/src/util.rs`). Jush does not: `run.ps1`
  waits for the runner and shows its output, and PowerShell does not wait
  for a Windows-subsystem program.
- Jush's installer already asks for elevation with `Start-Process -Verb
  RunAs` to install Git for Windows for every user, and installs it for the
  account alone otherwise.

## Alternatives

- **`conhost.exe --headless`** starts a console with no window, but the flag
  is undocumented, and security software reports it as a way to hide
  malware.
- **A VBScript launcher through `wscript.exe`.** Windows is removing
  VBScript.
- **The starter as a second file of the release.** It hides the window the
  same way, but every target's release record, the installers and the
  update would carry two files for one that never changes.
- **A Windows service.** It runs in session 0, away from the user's
  desktop, where the runner's browser, screenshots and emulators work.
- **`requireAdministrator` in the runner's manifest.** Windows would prompt
  at every start, including the starts after each update when nobody is at
  the computer.

## Open questions

None.
