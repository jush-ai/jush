---
status: proposed
---

# A page the user chose to debug runs the code an agent sends it

## Problem

On Alex's laptop the model menu opens partly below the window's edge, and
its last models cannot be chosen; on a new tab it opens in place. The
page's log says only that the menu opened. To find out why, the agent asks
Alex to paste a script into the browser's console and to paste back what it
printed: the menu's box, the viewport's size, the zoom. Then another, for
the ancestors' transforms, and a third, for the layout's state. Each round
costs Alex a few minutes and the agent a turn, and a script that needed
one more field means another round.

The development setup cannot stand in for that page: the bug shows in a
state that the page reached over hours, on Alex's data, network and
browser. Only the page can answer, and today only Alex can ask it.

## Decision

Alex chooses **Debug This Page** in the account menu. The page shows a link.
Alex gives the link to an agent, which reads the page and runs code in it
with `curl`, until Alex stops it or 24 hours pass.

```text
  Account menu                 Debug This Page                         [x]
 +-----------------------+    +--------------------------------------------+
 | (A) Alex              |    | An agent you give this link to can run     |
 |-----------------------|    | code in this page, and see and do what you |
 | Settings          ,   |    | can here, until you stop debugging or      |
 | Debug This Page       |    | close the tab.                             |
 |-----------------------|    | The link stops working at May 7, 18:30.    |
 | Sign Out              |    | [https://jush.example.com/api/debug/k3…  ] |
 +-----------------------+    |                                            |
                              | [Stop Debugging]             [Copy Link]   |
                              +--------------------------------------------+
```

- **Per page.** The link reaches the one tab Alex chose it in. The tab
  keeps it through a reload or a navigation, so code that reloads the page
  or opens another of its addresses goes on debugging it; it ends as the
  tab closes, at Stop Debugging, or 24 hours after it began, whichever
  comes first.
- **Shown while on.** While a page is debugged its account row carries a mark,
  and the menu item opens the same window, with the link and Stop
  Debugging. The mark takes no room the row did not have, so nothing moves
  as debugging starts or ends.
- **What the agent does.** `GET` on the link answers what the page is and
  how to use the link, so an agent needs no instructions beyond the link:

  ```text
  $ curl https://jush.example.com/api/debug/k3…
  {"page":{"id":"pg_7f2c","url":"/chat/0a1b…","title":"Pricing page · Jush",
           "visible":true,"build":"0.2.6240"},
   "expiresAt":"2031-05-07T10:30:00.000Z",
   "usage":"POST JavaScript to this link + /eval … it runs in the page as the body of an async function …"}

  $ curl --data-binary @probe.js https://jush.example.com/api/debug/k3…/eval
  {"value":{"menuBottom":912,"viewportHeight":860,"zoom":1.1},"ms":4}
  ```

- **How code runs.** The body is the body of an async function, so it can
  `await` and `return`. The value comes back as JSON: an element as a short
  selector, a cycle as `"[Circular]"`, a function as `"[Function name]"`,
  anything iterable, such as a `Map`, a `Set` or a `NodeList`, as an array,
  `undefined` as `null`, and what lies deeper than 100 levels as `"[Deep]"`,
  since the backend's JSON reader stops at 128. Every string is made well
  formed, so a cut through an emoji does not make the answer unreadable, and
  the writing stops once the answer is larger than a page sends, so a large
  object graph does not hold the tab. A thrown error comes back as
  `{"error":{"name","message","stack"}}`. A run that has not settled after
  `?timeout=` seconds, 10 unless asked, at most 60, answers `408
  run_timed_out`; the page's own work goes on. The most is 60 because the
  request stays open while the code runs, and a proxy in front of the
  backend, such as Cloudflare's at 100 seconds, cuts a longer one.
- **What it can reach.** Whatever the page can, as the console can: it runs
  as Alex, in Alex's page. This is why it starts only from Alex's click,
  ends on its own and is shown while on.
- **Recorded.** Each run writes `debug.ran` in the backend's log: the page,
  the link's first 6 characters, the code's first 4 KiB, how long it took,
  how it ended and how large the answer was, also when the agent stopped
  waiting for it, since the code ran either way. Starting and ending write
  `debug.started` and `debug.ended`.

### Design to build from

The link rides the page connection as one more lane (`crates/web-api-protocol/src/connection.rs`):

```rust
pub enum LaneTarget {
    // …
    /// The page's debugging: a new link, or the one the tab kept, which
    /// the backend takes back only while it is the same user's and
    /// unexpired.
    Debug { resume: Option<String> },
}

/// What the backend sends on a debug lane.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DebugToPage {
    /// The lane's link, as it opened.
    Started { token: String, expires_at: Timestamp },
    /// Code to run, which the page answers by `id`.
    Run { id: u32, source: String },
    /// What the page is now, for `GET`, which the page answers by `id`.
    Describe { id: u32 },
}

/// What the page sends on a debug lane.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DebugFromPage {
    /// What a run returned or threw, as JSON.
    Ran { id: u32, outcome: RunOutcome },
    /// Its id, address, title, visibility and build.
    Described { id: u32, page: PageFacts },
    /// Stop Debugging: the link ends.
    Stop,
}
```

- **The backend** owns the links, in memory: by token, the user, the
  expiry and the lane that holds it now, if any. A token is 32 random
  bytes, base64url, so the link is its own key, as a share's is
  (`crates/backend-http/src/shares.rs`), and takes no session. A run while
  no lane holds the token, as during a reload, waits up to 10 seconds for
  one, then answers `409 page_not_connected`; an unknown or expired token
  answers `404 debug_link_not_found`. An answer larger than a page sends in
  one message, 1 MiB, is refused with `413`: a debugging answer is a few
  fields, and the agent can return less. The links are lost as the backend
  restarts; a tab that resumes one the backend does not know is refused,
  and its mark goes.
- **One tab holds a link.** A lane that resumes a link takes it from the
  lane that held it, which closes, and that page drops its mark: after a
  reload, the backend may not yet have seen the old page's connection end.
- **A closed tab.** The backend cannot tell a closed tab from a connection
  lost for a while, so the link stays until it expires, answering `409`.
  A browser that reopens a closed tab, or copies a tab, restores its
  `sessionStorage` too, so the page tells them apart itself: as a page
  leaves the tab it notes when, and a new page goes on with the link only
  when the last one left less than 30 seconds before, as after a reload or
  a navigation, or when the browser discarded the tab and loads it again.
  Any other page drops the token as it loads, signed in or not. A closed
  tab reopened within those 30 seconds goes on debugging, as if it had not
  closed.
- **Stop Debugging** sends `stop` on the lane; the backend ends the link and
  closes the lane. If the backend does not answer within 10 seconds, the
  page lets go of the link anyway.
- **The log keeps no token.** A request's line writes the path segment
  after `debug` as `{token}`, as it does for a share's link.
- **The routes:** `GET /api/debug/:token` and `POST /api/debug/:token/eval`.
- **The page** holds the token in `sessionStorage`, and opens the lane
  again with it as a page that goes on with the tab loads. It runs the
  source with `AsyncFunction`, which the app's page allows, as it sets no
  Content Security Policy.
- **An older page** has no menu item and opens no such lane; nothing else
  changes.

## Spike

None.

## Prior art

- [Chii](https://github.com/liriliri/chii) (`server/lib/WebSocketServer.js`),
  the successor of [weinre](https://github.com/apache/cordova-weinre): the
  page loads a script that opens `ws://…/target/<id>`, a DevTools front end
  opens `ws://…/client/<id>?target=<id>`, and the server passes the Chrome
  DevTools Protocol between them. Anyone who knows the target's id may
  connect. Jush departs in three ways:
  - the page uses its existing connection;
  - the agent runs one script per request, which `curl` makes, instead of
    a DevTools front end over a socket;
  - a link exists only from the user's click, and expires.
- Chrome's remote debugging port is off until the browser is started with
  `--remote-debugging-port`, and a browser under automation says so in a
  bar across the window ("Chrome is being controlled by automated test
  software"). Jush follows both: off until asked, and marked while on.

## Alternatives

- **A switch in Settings for all of the user's pages.** One link then
  reaches every page, also ones the user forgot are open, and the agent
  must choose among them. Rejected: the user knows which page shows the
  problem.
- **Relay the DevTools protocol, as Chii does.** It gives a whole
  inspector, which an agent with `curl` cannot drive; the protocol's
  `Runtime.evaluate` is what an agent would use of it anyway.
- **Read-only access.** Finding why a click does nothing takes pressing,
  scrolling and calling the page's functions; a read-only surface would
  send the agent back to asking the user.
- **Keep asking the user to paste scripts into the console.** What this
  decision replaces.

## Open questions

None. The account row's mark is a small amber dot on the avatar.
