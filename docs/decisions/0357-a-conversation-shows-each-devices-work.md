---
status: proposed
---

# A conversation shows the work of each device it reaches

Supersedes:

- 0322 (A live tab mirrors its device): *On each list of the conversation's
  primary device*, which becomes each list of a device the conversation
  reaches, and the work of one live kind as one set of tabs.

## Problem

Alex's conversation runs on `alex-mbp` and has `lab-server` attached. The
agent runs `jush browser open` on `lab-server` to sign in to a dashboard, then
`jush browser show` so that Alex can enter a code. Nothing appears. The
browser on `lab-server` opened the page and reported its tabs, and the backend
kept the report, but the work shows only the primary device's browser. The
command succeeded, so the agent believes Alex is looking at the page.

The same rule hides other work of an attached device:

- **Simulators.** A simulator the agent boots on `lab-server` gets no tab,
  so Alex can neither watch nor operate it.
- **Widgets.** `jush widget create` run on `lab-server` draws its two
  screenshots with the browser of `alex-mbp`, then reads the pictures on
  `lab-server`, where they are not. Both screenshots are skipped, and the
  agent never sees the widget it made.
- **Files in a reply.** A link or a picture in a reply names a path of the
  primary device. A report the agent writes on `lab-server` can reach Alex
  only as an uploaded attachment, and `![chart](/srv/out/chart.png)` shows
  nothing.
- **Commands.** The Running window shows a command on `lab-server`, but its
  tab does not say where it runs.

Each of these has one cause: a plugin's package calls, its user stream and
the report it reads go to the conversation's primary device, and the work has
no notion of which device a tab shows.

## Decision

### What Alex sees

The devices a conversation reaches are its primary device, the attached ones,
and any device a running subagent works on. A subagent works on a device the
conversation does not otherwise reach when Alex moves the conversation and
then detaches the old device while a subagent still runs there.

Each device's browser is a window of its own, and so are each device's
simulators, as each Chrome profile is a window of its own. A tab or a
window of a device other than the primary one carries the device's name as a
tag. The primary device's tag shows only while something of another device
is shown in the same place: beside it on the taskbar, or in the same strip.
With the primary device alone, nothing changes from today.

```text
 Browser windows of alex-mbp and lab-server:
 | + | ... | [G] Orders - Example  alex-mbp  2 | [G] Dashboard  lab-server |

 The primary device's browser alone:
 | + | ... | [G] Orders - Example  2 |
```

- **Windows.** A window of a device kind holds that device's tabs, and its
  title bar shows the same tag as its taskbar button. Its + opens a tab on
  its device. The taskbar's Open lists the kind once and opens it on the
  primary device.
- **A device away.** While a device cannot be reached, only its windows say
  so, with the page every window shows today for an offline device.
- **A device leaves.** Once the conversation no longer reaches a device, its
  windows leave. When its commands there have ended and no subagent works there
  any longer, the backend releases the conversation there, as a detach does
  now, so its browser and simulators end, and their tabs are dropped. A move
  keeps every window: the old primary device stays attached.
- **The agent's `show`** on `lab-server` brings the window of `lab-server`'s
  browser to the front.
- **Widgets.** A widget's screenshots are drawn and read on the device that
  ran the command.
- **Files in a reply.** A link or a picture may name a file of another device
  the conversation reaches: `[report.md](device:<device id>/srv/out/report.md)`.
  A link opens the file in the File view, in a tab that carries the device's
  tag, as a file of the primary device opens. It has no *Show in Files*, since
  Files shows the primary device's working directory. A picture is copied when
  the reply ends, as the primary device's pictures are.
- **Commands.** A command's tab in Running and Services carries its device's
  tag by the same rule.

The Files and Changes windows do not change: they show the conversation's
working directory, which only the primary device has.

### Design

**One list of the devices a conversation reaches.** Device access already
lists them for admission (`conversation_devices`): the primary device, the
attached ones, then the live subagents' placements. The plugin host reads
reports through it (`ProductPort::reached_devices`) instead of its own query
of the records, and the page's list of devices adds the devices of its
running subagents, which each subagent's entry already names.

**A device leaves once nothing reaches it.** The release that follows a
detach waits for the conversation's commands on the device to end, as now,
and then, while only a subagent reaches the device, for the subagent tree to
change, and checks again. Once nothing reaches the device, it drops the
device's tabs, without retiring them, and the plugins' reports from it, so a
later attach starts from what the device reports then, and sends the release.
The drop leaves the detach's transaction, which cannot know of subagents.

**A work tab names its device.** A tab that shows something of one device
says which, so the page groups windows and draws tags without knowing any
kind's data, and the mirroring of one device's list leaves the other devices'
tabs alone:

```rust
pub struct WorkTab {
    pub id: String,
    pub kind: String,
    /// The device whose thing it shows, such as a browser's tab or a
    /// simulator; none for a tab of no device, which follows the
    /// conversation's primary device.
    pub device: Option<DeviceId>,
    pub data: Map<String, Value>,
}

impl WorkDocument {
    /// Makes the tabs of `kind` on `device` the tabs `listed`, as one
    /// change; the tabs of the kind on other devices stay as they are.
    pub fn mirror(&mut self, kind: &str, device: &DeviceId, listed: Vec<CreateWorkTab>) -> Mirrored;
}
```

A tab's id names its device, `browser-<device>-t1`, since each device's
browser numbers its own tabs from `t1`. A migration drops the stored browser
and simulator tabs without retiring their ids: each device lists its tabs
again when its runner reconnects, and they come back under the new ids.

**A plugin's port reaches the device it is asked about:**

| Request | Where its package calls, reads and report go |
|---|---|
| `Command`, the agent's job | The job's device, as its file reads go today |
| `PageCall`, `TabClosed` | The device the page names, or the tab's device |
| `PackageMessage`, `PackageLost` | The device it came from |

`Request::PageCall` gains `device: Option<DeviceId>`, none for the primary
device. `PortMessage::ReadReport` reads the report of the request's device,
and `PortMessage::ReadReports` every device's the conversation reaches.
Keeping a report answers only that it was kept: every device's list is
mirrored. `PluginPort::on(device)` sends the port's requests to another
device. `ProductPort::package_call` takes the device, and admits it as
`look_at_reachable_device` does today: the primary device, an attached one, or
one a live subagent works on.

**A page opens a user stream on a device.** `open_user_stream` and
`user_call` take the device, which `admit_stream` already takes. The page's
`stream(name, conversation, device)`, and what the host says of a device's
packages (`installed`, `downloading`, `deviceStarting`), name it too.

**The page draws a window per device kind and device.** A tab kind says
whether its tabs make a window per device:

```ts
interface WindowKind {
  id: string
  name(): string
  icon: Component
  openable?: boolean
  /** Each device's tabs of it make a window of their own, as each Chrome profile does. */
  perDevice?: boolean
}
```

The browser's and the simulators' windows say `perDevice`. A window of
another device has no *Pin* in its menu. A pinned kind stands for the primary
device's window. The tag rule is one function of web-ui, which the taskbar,
the window's title bar and every tab strip read.

**The plugins:**

- **Browser and simulators.** Each keeps one list per device and mirrors it
  into that device's tabs. The pruning of the agents' tabs (0318) runs on
  each device that has the conversation's tabs.
- **Widget.** The draw is a package call of the command's job, so it runs
  where the screenshots are read.
- **Files in a reply.** `filePath.ts` reads the `device:` form beside
  `commit:`; a link that names the primary device is an ordinary link. It
  opens the `deviceFile` intent, whose `device-file` tab sits in the Files
  window. A tab kind names the device of a tab an intent opens
  (`TabKind.deviceOf`), so the work's tab carries it. The file is read through
  the attached device's source that the attachment picker uses today. The
  reply's pictures are copied through `look_at_reachable_device` with the
  link's device. The agent's guide adds the form to its line on links.
- **Commands.** The tab reads the device its record already names; a command
  whose call named no device is the primary device's.

**What an older page, runner or data meets.** A page loaded before the
upgrade ignores a tab's device and shows every device's tabs of a kind in one
window, without tags, until it reloads. A runner needs no change: its packages
already report to the backend from any device. Stored tabs move by the
migration above.

## Spike

None ran. Two points were read in the code, and the build checks them on the
product:

- An attached device's browser package reports its tabs, and the backend keeps
  the report per device (`PluginPort::keep_report`).
- `admit_stream` admits an attached device for a look, as
  `look_at_reachable_device` does, so a user stream can use the same
  admission.

## Prior art

- **VS Code Remote Development**: each remote is a window of its own, and its
  title and status bar name the remote, `[SSH: lab-server]`
  ([Remote Development](https://code.visualstudio.com/docs/remote/remote-overview)).
  Jush shows several devices in one conversation, so the name is a tag on the
  window's button and tabs rather than in the title.
- **Chrome profiles**: each profile's windows are their own, and the Windows
  taskbar marks each with the profile's picture. Jush follows the one window
  per browser, and marks it with the device's name, the name the user gave it.
- No open-source agent harness shows the browsers of several machines at
  once.

## Alternatives

- **One browser window, each tab tagged with its device.** Two browsers
  then mix in one strip, and the window's + must ask which device. Each
  device's browser is its own program with its own sign-ins, so one window per
  device says what it is.
- **Show only the primary device, and refuse `show` elsewhere.** It changes
  little, and Alex still cannot use an attached device's browser.
- **A tag on every tab and window, always.** With one device it says what
  everything already shows.

## Open questions

- A link names its device by id, which outlives a rename; the agent finds the
  id in its context block and `jush device list`.
