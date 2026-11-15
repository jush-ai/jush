---
status: accepted
---

# A browser download says where it goes, and `download` lists them

## Problem

An agent exports a report on a dashboard with `jush browser click t1 e12`.
The page downloads `report.csv`, and the click's result says only:

```text
Clicked button "Export" [ref=e12].
URL: https://example.com/dashboard
```

The file is there, in the browser profile's hidden
`.browser/downloads/` folder, under a name such as
`b99d72a4-968f-4c43-a4ce-475549306156`. Nothing tells the agent it exists,
so it clicks again, or looks for an export API. When the browser stops, the
folder is emptied and the file is gone.

`jush browser download` acts and saves what the action downloads, under a
name such as `jush-download_<random>`. It helps only an agent that knew in
advance that the action would download.

## Decision

### What the model reads

Each `jush browser` command of the agent ends with one line on stderr for
every download it has not been told of yet, whatever the command and
whatever came of it, without waiting for the download:

```text
Clicked button "Export" [ref=e12].
URL: https://example.com/dashboard
Download started: report.csv, into /Users/alex/.jush/…/scratch/1f2e/downloads (`jush browser download` lists it)
```

* A download a command starts is told by that command, as the click above
  is, when it begins before the command answers. One that begins later, from
  a timer, a frame, a new tab or a slow server, is told by the agent's next
  command.
* The agent is told of the user's downloads too: they share the browser,
  and no reliable sign says who started a download.
* On stderr, the line never changes a command's value: `eval … | jq` and
  `--json` still read one value, and a command that failed, such as an
  `open` whose page became a download, still tells it.
* `download` tells nothing: it lists them.

`jush browser download` lists the conversation's downloads on this device,
the newest first: the name, the state, the size and the path.

```text
$ jush browser download
name          state        size             path
report.csv    complete     48213 bytes      /Users/alex/.jush/…/downloads/report.csv
dataset.zip   in progress  12 of 300 MB     -
Folder: /Users/alex/.jush/…/scratch/1f2e/downloads
```

* `--wait` waits until each download in progress has ended, then lists
  them, so an agent that needs the file does not poll.
* The state is `inProgress` (`in progress` in text), `complete` or
  `canceled`. Chrome reports an interrupted download as canceled, with no
  reason, and a download in progress when its browser stops, such as when
  the last tab closes, ends canceled.
* A download in progress has no path yet: its file appears under its name
  once complete.
* The list is the conversation's downloads and the folder's other files,
  hidden ones aside, so it shows the files of earlier browsers and residents
  too. A complete download whose file was moved away is no longer listed.
* `--json` gives the same rows and the folder.
* Its description becomes: *List the downloads, where each is saved and
  how far it is; --wait waits for those in progress.*

Nothing else in the prompt or the descriptions changes.

### Where and under what name

* Every download of the conversation's browser on a device goes to
  `downloads/` of the conversation's scratch directory, the folder the
  main agent's `$JUSH_SCRATCH` names, beside `.browser/`. It is kept when
  the browser stops. The scratch directory's cleanup removes it, by time
  unused and by its size limit, and so do archiving and deleting the
  conversation.
* Its name is the one the page suggested, numbered as Chrome numbers a name
  already taken: `report (1).csv`. The name is taken when the download
  begins, past the folder's files and the other downloads in progress, so
  the line names the file it becomes. The page's name is data: only its last
  component, without control characters, never `.` or `..`, its stem
  shortened to leave room for a number within 255 bytes.
* A download in progress stays under Chrome's id in the profile's
  `downloads/`, and moves to its name once complete, so a partial file never
  has the name.
* The user's downloads go the same way and show in the list. A watched
  tab's download bubble shows every download that starts in the tab while
  it is watched, the agent's too, as a browser's bubble shows every download
  of its window.

### Design

* `crates/plugin-browser/command-chrome`:
  * A `tabs::downloads` module owns every download of a conversation on
    the device, across its browsers: each browser's environment follows
    `Browser.downloadWillBegin` and `downloadProgress` into it, and it takes
    the names, moves a complete file, and keeps the list. It finds a
    download's tab apart from following, so a slow tab never holds a
    download's progress. `live/downloads.rs` reads the bubble from it.
  * Each command of the agent takes the downloads not told yet and prints
    the line for each on stderr.
  * `page/download.rs` becomes the list, and the `download` operation's
    target, `--output` and `--overwrite` go. Saving an image's or a video's
    bytes, which `download --xy` did, is `assets export`'s.
* The downloads folder is `jush_command_protocol::browser_downloads_directory`,
  next to `browser_profile_directory`.
* Older runners keep today's behavior until they update.

## Spike

* **No confirmation, anywhere.** On a macOS device, a click on a download
  link, one click that starts three downloads and a download a timer starts
  1.5 seconds after the click all saved at once, with no prompt from Chrome.
  Each file was named by Chrome's id, and the click's result said nothing of
  it.
* **Built, on a development Cloud:**
  * Clicks on download links each told their download in their own result,
    a link whose server answered after 3 seconds included: a click waits for
    the navigation it starts. The second `hello.txt` was told, and saved, as
    `hello (1).txt`.
  * An `open` of a file's URL failed with `net::ERR_ABORTED` and told the
    download. Downloads from a `target=_blank` link, a link in a frame and
    `content fetch` were told by the command that started them.
  * `eval … | jq` read one value while a download was untold.
  * A 2 MB download listed as in progress, and as canceled once the last
    tab closed. A moved file and `.DS_Store` were not listed.
  * In a watched tab, the user's download showed in the bubble and was told
    by the agent's next command, and the agent's download showed in the
    bubble beside it.

## Prior art

* **Playwright MCP** saves each download under the page's name into its
  output directory and tells it in the next tool response: *Downloading file
  report.csv …*, then *Downloaded file report.csv to "…"*
  (`packages/playwright-core/src/tools/backend/tab.ts` and `response.ts` in
  [microsoft/playwright](https://github.com/microsoft/playwright)), every
  download, whatever started it. Jush tells the start the same way, and the
  rest in a list the agent reads when it needs it.
* **browser-use** gives each file the page's name, numbered
  `invoice (1).txt` (`name_download` in `browser_use/utils.py` of
  [browser-use/browser-use](https://github.com/browser-use/browser-use)), in
  `downloads/` of the agent's workspace, and says so in its system prompt.
  Jush puts no line in the prompt.
* **Chrome's Downloads page** (`chrome://downloads`) lists every download
  with its state and where it is, as `download` does.

## Alternatives

* **Waiting in the action for each download to end**, and printing its
  path. A large download would hold a click for minutes, and a click's
  result would wait on something the agent may not need.
* **Keeping `download`'s act-and-save form beside the list.** It is the
  same as a click followed by `download --wait`, and two ways to do one
  thing.
* **Telling an action only the downloads it started**, those that begin
  while it holds its tab. A download that begins after the action answers,
  in a frame, in a new tab, or from a slow server, would be counted as the
  user's and never told; one the user starts while the agent waits on the
  tab would be counted as the agent's and left out of the user's bubble.
* **A line in the prompt** saying where downloads go, as browser-use does.
  The model is still not told that a given click downloaded a file.

## Open questions

1. A link to a PDF opens Chrome's viewer and downloads nothing. Save the
   PDF a tab shows from `download`, in a later decision? Proposed: later.
