---
status: proposed
---

# A tab the user opens is in the strip at once

Supersedes: 0322 (A live tab mirrors its device), the window that shows what
opens over its tabs with no tab for it, and the alternative it refused,
*Show a tab the page opens before the device has it*.

## Problem

Alex's conversation has one browser tab, *Start*. Alex clicks the browser
window's +. The window shows a blank page with an empty address bar at once,
but the strip still holds only *Start*, which still looks selected. The new
tab comes into the strip about a second later, once the device has made it
and the work holds it.

With no browser running, after Alex closed every tab, the taskbar's
*Browser* opens a window with an empty strip. The page says *Starting the
browser…* for about three seconds on a device on the same machine, and
longer on a device across the internet. A simulator that boots is the same:
its window shows *Booting…* with no tab for it for as long as the boot takes.

So the window shows a page that no tab in its strip stands for. Alex cannot
tell which tab the page is, or close it before it comes. 0322 counted on the
tab coming "within one round trip", but the device takes 600 to 750 ms only
to make a blank tab in a running Chrome, and seconds to start Chrome or to
boot a simulator.

## Decision

A tab the user opens is in the strip from the click, as a web browser's new
tab is, and the tab the device makes takes its place:

```text
  click +                    the device has made it
  [ Start ][ ◌ New Tab ×] +  [ Start ][ ◯ New Tab ×] +
  ----------------------     ----------------------
  | address bar        |     | address bar        |
  | Starting the       |     | the page           |
  | browser…           |     |                    |
```

- The window's +, the taskbar's *Open*, a link's *Open in Browser Window* and
  a simulator chosen in the taskbar's *Open* add a tab to the end of the
  window's strip, selected, with the spinner a loading page's tab shows. A
  browser tab is titled *New Tab* for a blank page and with its address for
  a link; a simulator's tab with the simulator's name.
- Its content is what the window shows over its tabs today, the address bar
  and what Jush does. Another tab selected meanwhile shows its own page.
- Once the work holds the device's tab, it is in that place, in the same
  frame: the strip does not move. The device lists a tab the user opened at
  the end, so its index is the opening tab's.
- Closing the opening tab ends the opening. When the device makes the tab
  afterwards, the window closes it, so it never comes into the strip.
- A refused opening leaves the strip and says so in a toast, as today. A
  window with no other tab closes.
- A blank browser tab is titled *New Tab* after it comes too, not
  *about:blank*, the title Chrome reports for it.

The opening tab is never in the work. It lives in the page that asked, as the
opening does today, so the device's list stays the work's only owner of live
tabs: another page shows the tab once the device has it.

Each kind with `open` lists what it opens, with the work tab each becomes once
the device made it, and lets the window end one:

```ts
interface TabKind {
  // The tabs the kind's plugin opens for this page, oldest first.
  openings?(tab: KindTab): readonly KindOpening[]
  // Ends the opening `key`: it is `cancelled` from then on.
  cancel?(key: string, tab: KindTab): void
  // The content of an opening's tab. Props: `conversation`, `opening`.
  opening?: Component
}

interface KindOpening {
  key: string
  title: string
  // The work tab it becomes, once the device made it.
  tab: string | null
  // Closed before the work held its tab.
  cancelled: boolean
}
```

- `workWindows` appends each opening whose tab the work does not hold to its
  window's tabs, marked as opening. The window selects the newest opening
  until the user selects another tab; once the work holds the tab it became,
  the window selects that tab and picks it in the work.
- The strip keys the work's tab by the opening's key, so it stays the one tab
  in place rather than one closing and another opening.
- The browser learns which tab is the opening's from the device's list, the
  first tab the user opened that the browser did not have when asked, so the
  work may hold the tab before the open answers. A simulator's is its
  device's.
- A cancelled opening's tab never shows; the window closes it in the work
  once the work holds it, which closes it on the device.
- The kind's `opening` component no longer stands over every tab, and its
  `failed` event is gone: a window with no tab left closes, as when its last
  tab closes.

Older pages and runners need nothing: the change is the page's own.

## Spike

On a development backend with a device on the same machine:

- With Chrome running, the window's + took 870 ms until the strip held the
  new tab. Chrome's `tab.new` took 742 ms of it.
- With no browser running, the taskbar's *Browser* took 2.9 s, Chrome's
  start 1.5 s of it.
- Of the 742 ms, about 460 ms are the two waits for a paint that sizing a
  new tab does before the device lists it: each Chrome snapshot that serves
  as the wait takes about 230 ms on macOS, on a new tab and an old one
  alike. They keep the first picture from showing the page at its old size,
  so they stay.
- On the development device the open answered 10 to 30 ms before the work
  held the tab. The answer and the work's change reach a page over a server
  apart, in either order, so the browser's opening also takes its tab from
  the device's list.

## Prior art

- Chrome adds a tab to the strip in the same frame as the click
  ([`TabStrip::AddTabAt`](https://source.chromium.org/chromium/chromium/src/+/main:chrome/browser/ui/views/tabs/tab_strip.cc)),
  and shows a throbber in it until the page commits, before any renderer
  exists.
- VS Code's editor tabs open at once and show a progress bar while the
  editor's input resolves.

## Alternatives

- **Make the device faster:** a running Chrome cannot make and size a tab in
  less than a few hundred milliseconds, and nothing makes a cold start or a
  simulator's boot instant.
- **Create the work tab at the click and let the device fill it in:** the
  work would have a second owner of live tabs while one opens, which 0322
  removed.

## Open questions

None.
