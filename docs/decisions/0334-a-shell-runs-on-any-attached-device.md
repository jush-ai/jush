---
status: accepted
---

# A shell call runs on any device the conversation reaches

## Problem

Alex's conversation works on his laptop and has a build machine,
`build-box`, attached. To run the tests there, the agent calls `shell` on the
laptop with `jush device shell --device build-box 'make test'`. The script
travels three legs: the laptop's job runs the `jush` command, the backend
starts the script on `build-box`, and the script's output comes back through
the laptop's job to the agent.

Every leg can lose the result:

* The laptop's lid closes for a minute while the tests run. The `jush`
  command on the laptop ends at once with `i/o error: stream closed because
  of a broken pipe` and exit status 1. On `build-box` the tests run on to the
  end and pass, but nothing records it: the agent reads a failure, the output
  after the break is gone, and the agent runs the tests again. For a deploy
  or a migration, it runs twice.
* `build-box` goes away: the command fails within a second, which is right,
  but it says only that a stream closed, not which device left.

The far job is no command of the conversation: the terminal panel does not
show it, `jush shell` cannot reach it, and it wakes no one when it ends.
When its caller is gone, it runs on where nobody sees it.

## Decision

A command on another device is a command of the conversation on that device,
as one on its own device is. Its result reaches the conversation from the
device that runs it, whatever happens to the device that asked for it.

### The `shell` tool takes a device

```text
shell { description, script, device?, background?, service? }
```

* `device` names a device from `jush device list`, by name or id. Without it
  the call runs where it runs now: the conversation's device, or a
  subagent's own.
* With it, the job starts on that device, in the directory commands start in
  there, which `jush device list` shows, for the root and a subagent alike.
  A device the conversation does not reach yet is attached first, and asks
  the user once, as `jush device shell` does now; the call holds while the
  user decides, and calls in one response that name the same device ask
  once. The node's own device runs the call as one without a device, and
  asks nothing.
* The job is the node's command like any other: `background` and `service`
  hold as they do, its end and its silence wake the node, `jush shell`
  reaches it by its id, the terminal panel shows it, and its runner keeps it
  across a restart, after which it is taken up on its own device.
* Only its own device matters to it. The conversation's device can go away
  and come back; the job runs on and reports. A device that is offline when
  the call starts, past the wait a dropped connection gets, fails the call
  with its offline message, as an attached device is refused now, and is not
  attached for it.
* The terminal panel names the device while the command runs, from the
  command's live view; a command the page knows only from the list of
  ended ones shows no device.

What the model reads of the field:

```text
device: A device from `jush device list` to run on, attached first if
needed; the conversation's own when left out.
```

### `jush device shell` is a pipe between two devices

With `device` on `shell`, the use left to `jush device shell` is bytes that
flow between two devices, such as
`tar c . | jush device shell --device build-box "tar x -C /work"`. That use
needs both ends: once one is gone the stream is cut, and what the other end
does with a cut stream is worth nothing. So the command ends whole, at both
ends, when either goes:

* When the far device goes away, the calling command fails and says which
  device went, such as `build-box went away while it sent its output`, never
  a bare broken pipe: at once when its connection breaks, and for a device
  that goes silent, such as one whose network drops, once the backend gives
  up on its connection and it does not come back within the wait a dropped
  connection gets (10 seconds).
* When the calling device goes away and does not come back within that wait,
  the far job stops, so nothing runs on where nobody sees it, and the
  backend logs why. A blip of the network shorter than that ends nothing. A
  caller that comes back later finds its command ended with exit status 1.
* A stop of the calling command stops the far job, as now.

Its description says it is for streaming bytes between devices, and points to
`shell` with `device` for running a script on a device. The context block's
line on attached devices, the attach and pair confirmations, and the
announcement of a move name `shell`'s `device` for running a script.

### The design to build from

* `agent-tools`: `ShellInput` gains `device`. A call with it asks
  `DeviceResolver::named_device`, which attaches the device through the same
  operation `jush device attach` uses, and runs in the node's environment on
  that device that stays there, kept apart from the node's own environments
  (`EnvironmentScope::follows`). Its commands' live view carries the device's
  name (`CommandView::device`).
* `backend-device-access`: `run_on_device` ends the call once either device
  is away past its wait (`Devices::until_away`, `Devices::reached`).
* `runner-client`: a pipe knows the name of each device end, which its
  failure names when that device goes; a failure the runner states itself
  keeps its own words (`SourceBreak`).
* Nothing is stored that is not stored now: a command on another device is a
  command record with its device, as a subagent's are; a command taken up
  after a restart is taken up on the device its record names.

## Spike

On a development backend with two runners on one Mac, a conversation on the
first ran `jush device shell --device second 'for i in $(seq 40); do echo $i;
sleep 1; done'`, and one runner was killed after 12 seconds.

* **The far runner killed:** the command printed 1 to 12 and exited with
  status 1 within a second. Nothing hung.
* **The calling runner killed, back after 60 seconds:** the command printed 1
  to 12, then `error: i/o error: stream closed because of a broken pipe`, and
  exited with status 1 when its device left. The script on `second` ran on to
  40 and finished 29 seconds later. The turn ended once the first device came
  back, and the agent read a failure.

A hang with two devices that both stay up was not reproduced: it would need
the devices' direct channel to break while both runners live, which one
machine cannot make.

## Prior art

* **ssh** ends a remote command when the connection drops. **tmux** and
  **GNU Screen** exist to keep it running, and **mosh** keeps the session
  across a lost link: the process lives on the far machine and the client
  only shows it ([mosh](https://mosh.org/#techinfo)).
* **VS Code Remote** keeps a terminal's process on the remote host across a
  reconnect of the window
  ([persistent sessions](https://code.visualstudio.com/docs/terminal/advanced#_persistent-sessions)).
* **kubectl exec** and **docker exec** end with their client, as
  `jush device shell` does now; `docker exec -d` runs a command detached for
  that reason.
* **Claude Code** and **Codex** run their shell on one machine and have no
  device to choose.

Jush takes the tmux and VS Code model for a command on another device: a
`shell` call with `device` belongs to the far device and the conversation,
not to the connection that started it. `jush device shell` stays an ssh-like
pipe, which, unlike ssh, also ends its far side once its caller is gone.

## Alternatives

* **Only make `jush device shell` survive its caller.** A script on another
  device would still run inside a command on the conversation's device, still
  need that device up to start, and still show as one opaque command.
* **Make `jush device shell`'s far job a command of the calling agent that
  runs on when its caller is lost.** It needs a command a `jush` command
  starts, beside the ones a `shell` call starts, a pipe the runner turns into
  the job's own output midway, and the session's report of a command no call
  watches. For the one use left to the command, a stream that is already
  cut, none of that buys anything.
* **Drop `jush device shell`.** Streaming an archive between two devices has
  no other way, and models use it as they use `ssh host cmd | tar x`.
* **A tool per device.** Tools would come and go with attachments, and a
  model's tool list would change mid-conversation.

## Open questions

None.
