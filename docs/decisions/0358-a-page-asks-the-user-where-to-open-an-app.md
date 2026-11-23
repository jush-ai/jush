---
status: proposed
---

# A page that opens an app asks the user where to open it

## Problem

A sign-in page asks the browser to open its desktop app, as a team chat's
"Authorize in App" button does with a link such as
`chat://client/authorize?token=…` in a hidden frame. In Jush's browser
nothing happens: no app opens, no question shows, and the page waits. The
agent finds no link to hand the user either, since the page builds it in a
script.

Chrome on a desktop asks first, in a dialog that names the app ("Open
Chat?"), and opens the app once the user agrees. Jush's Chrome runs without
a window, so it opens that dialog where no one sees it, and the request
ends there. A link to the system's settings, a frame's, a script's and a
plain link's alike, opened nothing in a test.

Worse, the tab is stuck from then on. While Chrome's dialog is open, the
tab takes no input: no click or key of the user's or the agent's reaches
the page, so the user clicking "Authorize" again, or the agent trying
another way, does nothing at all. Reloading the page does not end it;
only leaving the site does.

## Decision

A page that asks to open an app asks the user in the browser's panel, over
the picture, as a page's JavaScript dialog does:

```text
+--------------------------------------------------------------+
|  Open Chat?                                                  |
|  auth.example.com wants to open this app.                    |
|  +--------------------------------------------------------+  |
|  | chat://client/authorize?token=8f2c…             [Copy] |  |
|  +--------------------------------------------------------+  |
|                 [Cancel]  [Open Here]  [Open on alex-mbp]    |
+--------------------------------------------------------------+
```

- The link shows on one line, cut where it is longer, with Copy beside it.
- The host is the tab's page's, as a JavaScript dialog's title names it.

- **Open on alex-mbp** opens the link on the device the browser runs on, as
  the device's own Chrome would.
- **Open Here** opens the link on the computer or phone the user is looking
  from, through the browser Jush runs in, which may ask the user once more
  in its own dialog.
- **Cancel** drops the request. It is the default, which Enter and Escape
  take, as in Chrome's own dialog: the page chooses when to ask, so a key
  the user meant for the page opens nothing.
- The title names the app the device opens such links with. A device
  without one, such as a Cloud, says so in the dialog: "alex-mbp has no app
  for chat: links". Its button is unavailable then.
- The request belongs to the tab. It waits while no one watches the tab
  and shows once someone does. A newer request waits behind it and is asked
  next, as Chrome queues its dialogs, so the request on screen never
  changes under the user's hand. The first answer, from any viewer, ends it
  for every viewer.
- Every request asks: no "always allow" choice is remembered.
- A request Chrome refuses is not asked, as Chrome shows nothing for it:
  one a page makes again without a click of the user's, such as on every
  load or on a timer, and one for a scheme Chrome never hands on.
- Chrome's own dialog, when it opened one, holds the tab until the user
  answers (see "What Chrome does"). Then, whatever the answer, the tab goes
  to `about:blank` and back, as the Back button goes, which closes Chrome's
  dialog. The page loads again with what Chrome keeps of it on Back, the
  text typed into its fields included, and `about:blank` stays the tab's
  next page until it goes elsewhere. A request Chrome opened no dialog for
  holds nothing and reloads nothing.
- The top-level page going to another site ends every request, as it
  closes Chrome's dialogs; going to another page of the same site ends
  those that hold nothing. Closing the tab ends them all.

### What Chrome does

Chrome drops the navigation itself, then decides, and says what it decided
in the console of the page that asked, except when it asks:

| Chrome's decision | Its console says | The tab |
| --- | --- | --- |
| Refused: asked again without a click, or a scheme it never hands on | `Not allowed to launch '…'` | Free |
| No app for the link | `Failed to launch '…'` | Free |
| Started without asking, as for `mailto:` | `Launched external handler for '…'` | Free |
| Its dialog | Nothing | Held |

A page asking again without a click is refused because Chrome stops
accepting requests after one until the user clicks or types in a tab. So a
page that asks on every load, as a launching page does, is asked once: the
release loads it again, it asks again, and Chrome refuses. Chrome accepts
requests again after any extension's call too, and the live view's capture
makes a few as it starts showing a tab; a request made while it starts can
thus be asked once more.

Jush reads these messages rather than guess what Chrome would decide,
which differs by system and by what the page did before: on Linux, Chrome
opens its dialog for a link no app takes too, naming `xdg-open`. A request
waits 250 ms before it is asked: Chrome refuses one as it receives it, a
few milliseconds after the page asked, and the wait keeps a refused
request from flashing up.

The agent cannot answer it, since opening an app reaches beyond the
browser. It reads the request where it reads a dialog. The result of the
click or key that caused it says

```text
The page asked to open "chat://client/authorize?token=8f2c…" in an app; info
shows it while the user is asked in the browser panel where to open it.
```

and `jush browser info` says, beside `Dialog:`,

```text
App: "chat://client/authorize?token=8f2c…" ("Chat" on this device), waiting
for the user to choose where to open it in the browser panel
```

so it can tell the user what to do, and hand the link on if the user asks.
While Chrome's dialog holds the tab, or the answer releases it, an action
that acts through the page's input, such as `click`, `type` or `scroll`,
fails at once with `app_blocked`, which says so, where it would otherwise
do nothing; reading the page still works.

### What counts as a link to an app

A document request of any frame of the tab's page, by a link, a script, a
frame's address or a server's redirect, for a link to a
scheme the browser does not load itself: not `http`, `https`, `about`,
`blob`, `data`, `file`, `filesystem`, `javascript`, `ws`, `wss`,
`view-source`, nor Chrome's own (`chrome`, `chrome-extension`, `devtools`
and the like), and none of the schemes Chrome never hands to a system
(`shell`, `vbscript`, `ms-help` and the rest of its list), nor a scheme of
one letter, which Windows reads as a drive. The browser module reads it
from the request Chrome starts for the frame, which names the whole link,
before Chrome drops it.

Two requests are missed, as today:

- One of a frame of another site that Chrome runs in a process of its own,
  which sends its events on a session of its own that the tab's observer
  does not follow.
- One whose link opens in another tab, such as `target="_blank"`, which is
  that tab's: Chrome asks there, in a tab Jush opens only once its page
  commits.

### Design

- `command-chrome`'s `tabs::apps` follows each tab's document requests
  (`Network.requestWillBeSent`) and keeps its links to an app in order, the
  oldest the tab's `AppRequest`, which the live view sends its viewers as it
  sends a dialog. It reads Chrome's decisions from the page's console
  (`Log.entryAdded`): a refusal drops the newest request of its scheme,
  since Chrome refuses as it receives; another decision frees the oldest,
  since Chrome decides in order. A request Chrome said nothing of holds the
  tab. A top-level commit compares the sites as Chrome's
  `WebContentsModalDialogManager` does: the same host, or the same domain
  under the public suffix list.
- The release takes the tab to `about:blank` with `Page.navigate`, accepts
  a `beforeunload` question the page asks meanwhile, and once
  `about:blank` committed goes back to the entry it left with
  `Page.navigateToHistoryEntry`, unless the agent or the user took the tab
  elsewhere meanwhile. A browser's navigation reaches a page that a
  script's navigation may not, such as a `data:` one, and Back keeps a
  form's text where a new load drops it.
- `driver::apps` says which links are for an app, finds the device's app
  for one, and opens one, each without a shell:
  - macOS: Launch Services' default application and its display name;
    `/usr/bin/open`;
  - Windows: the scheme's association's friendly app name
    (`AssocQueryStringW`); `ShellExecuteW`;
  - Linux: `xdg-mime query default x-scheme-handler/<scheme>` and the
    desktop entry's `Name`; `xdg-open`.

  The link is escaped before the system gets it as Chrome's
  `EscapeExternalHandlerValue` escapes it: a quote, a space or a caret
  would otherwise end the link on an app's command line and start an
  argument of the page's. Windows also gets it quoted, and runs the app in
  the system folder, as Chrome does. The device's opener runs beside the
  release, so an opener that waits for its app, as `xdg-open` does without
  a desktop, keeps nothing waiting.
- A link the device's system could not open is the viewer's notice
  `app_not_opened`, a toast in the panel.
- Open Here asks the page's host to open the link (`PageHost::openApp`),
  which sends the page to it, as a link to an app on any page does: the
  browser hands it to its app, asking first where it does, and the page
  stays as it was. It refuses a link of a scheme a page runs or loads
  itself, such as `javascript:` or `https:`.
- An action's navigation observation counts a link to an app as no
  navigation of the tab, so a click on such a link does not wait for one.

```rust
// plugin-browser protocol, browser/mod.rs
pub struct AppRequest {
    /// The link, as the page asked for it.
    pub url: String,
    /// The device's app for the link's scheme; None when it has none.
    pub app: Option<String>,
}

// live.rs
LiveModuleMessage::App { tab: TabId, request: Option<AppRequest> }
LiveViewerMessage::App { tab: TabId, url: String, open: AppAnswer }
pub enum AppAnswer { Device, Here, Cancel }

// operations.rs
InfoResult { …, app: Option<AppRequest> }
ActionResult { …, app: Option<String> }

// failure.rs
BrowserErrorCode::AppBlocked // "app_blocked"
```

An answer names the link it answers, so one that crosses a newer request
does nothing. `Here` and `Cancel` open nothing on the device; each ends the
request for the other viewers. The page asks its host for the name of the
device the browser runs on (`PageHost::deviceName`), as the user's devices
name it.

## Spike

1. **Does Chrome name the link to Jush?** On a Mac, a page asked for
   `x-apple.systempreferences:` three ways: a hidden frame, `location.href`
   and a link. Each time the tab's CDP events named the whole link in
   `Page.frameRequestedNavigation`, `Page.frameStartedNavigating` and
   `Network.requestWillBeSent`, then failed the load as
   `net::ERR_ABORTED`. System Settings did not open, and the console said
   nothing. Chrome's console names only the scheme in its newest versions,
   so the events are the only source of the link.
2. **Does each system name the app?** The lookup ran on each system with
   a chat app installed: Windows named it from the app's two schemes,
   Ubuntu named it from its desktop entry, and a scheme without an app gave
   none on both. A Windows `mailto:` without a default mail app is named
   "Pick an app", Windows' own chooser, which is what Windows would open.
3. **Does Chrome open `mailto:` itself?** Chrome allows `mailto:` without
   asking, so a windowless Chrome might open the mail app on its own and
   the dialog would open it twice. On a Mac, a `mailto:` link asked in the
   panel, and the system's log showed no app opened for it.
4. **What ends Chrome's hidden dialog?** On a Mac, after a link to System
   Settings, a click on the page's button changed nothing. Then:
   `Input.setIgnoreInputEvents(false)`, `Network.setBlockedURLs` and
   `Fetch` interception set before the link did not help, and neither did
   a reload or another page of the same site. Going to `about:blank` and
   back did: the next click reached the page. Chrome's
   `WebContentsModalDialogManager` closes its dialogs only when the
   top-level page commits a document of another site.
5. **Can the page be kept from asking Chrome?** A listener of the
   Navigation API's `navigate` event can cancel a link's or a script's
   navigation to such a link before Chrome sees it, but not a hidden frame's,
   which is what the chat app's sign-in page uses, nor a server's redirect.
6. **What does Chrome say of each decision?** On a Mac, with `Log`
   enabled: a link no app takes logged `Failed to launch`, `mailto:` logged
   `Launched external handler`, a link to System Settings logged nothing
   and held the tab, and the same link again from a script logged `Not
   allowed to launch … because a user gesture is required` and held
   nothing more. On Ubuntu, a link no app takes logged nothing, and mouse
   input no longer reached the page: Chrome had opened its dialog.
7. **Does a page that asks on load ask forever?** On a Mac, a page asked on
   every load. After an answer and the release, Chrome refused its next
   request, whether a viewer watched the tab or not, and whether the
   release went back or replaced the address. When the agent clicked the
   page's button within a second of showing the tab, while the live view's
   capture started, the next request was asked once more; a probe request
   then showed Chrome accepting requests again, which the capture
   extension's calls explain.
8. **Can Jush tell a held tab by its input?** `Input.dispatchMouseEvent`
   answers at once, with no error, whether the page gets the event or
   Chrome drops it, so only the page itself could tell, by an event it
   receives.

## Prior art

- **Chromium** asks in `ExternalProtocolDialog`, titled with the app's
  name and attributed to the asking origin, after the rules of
  `external_protocol_handler.cc`, which this follows for what counts as a
  link to an app. It departs by asking where to open the link, which a
  local browser never needs to.
- **VS Code Remote** opens a link the remote side hands it through
  `openExternal` on the user's own computer, since the remote machine has no
  screen. Open Here is that.
- Hosted browsers for agents, such as Browserbase and Steel, drop such links,
  which is the bug here.

## Alternatives

- **Open on the device only.** A Cloud has no apps, and the user who watches
  from a phone or another computer may want the app there.
- **Open here only.** An app that checks the device it runs on, as a sign-in
  that admits a managed device does, would then admit the wrong computer.
- **Let Chrome open apps without asking**, by its policy for allowed
  origins. Any page, or the agent, could then start apps on the user's
  device unasked.
- **Copy the link for the user to open.** It works, but asks the user to do
  by hand what a button does.
- **Keep Chrome from opening its dialog.** Chrome has no switch for it. Its
  policies can only let a link open without asking, and need an
  administrator to install. Canceling the navigation in the page (spike 5)
  misses frames and redirects, so the release would still be needed, as a
  second way to the same end.
- **Tell whether Chrome holds the tab by whether the device has an app.**
  It is wrong where Chrome decides otherwise: on Linux, for `mailto:`, and
  for a request it refuses, which then reloads the page for nothing, and
  for a page that asks on every load, asks again after every answer.
- **Probe the tab with a mouse event the page would see** (spike 8). The
  page would receive the probe whenever the tab is free.
- **Release by going back to the address with `location.replace`.** It
  keeps the history as it was, but a script may not go to a `data:` page,
  a page that came from a form is fetched again without the form, and the
  fields lose their text.

## Open questions

None.
