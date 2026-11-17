---
status: proposed
---

# The sidebar counts what needs the user and goes to each without reading it

Supersedes: 0136 (a shown conversation is read once the user acts on the
page), for a conversation the new entry shows: its first action is one in
the conversation, not one anywhere on the page.

## Problem

Alex runs five conversations in the Work workspace and comes back from a
meeting. Three rows wear a dot, green, green and red, spread over two
projects, one of them folded. To find them Alex scrolls the list and opens
each; opening one reads it at once, so a conversation Alex only glanced at,
meaning to come back to, loses its dot and is forgotten. When nothing is
unread, nothing in the sidebar says how many conversations still run, short
of scanning the rows for breathing dots.

## Decision

Below Archived, the sidebar has one more entry, the **unread entry**. It
says, of the workspace's conversations, in this order:

1. **Unread:** how many wear a dot that says something is new, red for a
   failure, yellow for a request, green for an answer, the dot of the most
   urgent beside the count: `3 Unread`.
2. **Otherwise running:** how many run, with the accent dot, breathing while
   a model works: `2 Running`.
3. **Otherwise** `Nothing Running`, faint, and it does nothing.

While the list is loading or failed to load, the entry shows its frame
alone: it says nothing it does not know.

```text
  [✎] New                 ⇧⌘O
  [⌕] Search               ⌘K
  [✦] Skills
  [▤] Archived
  (●) 3 Unread                  <- red, the most urgent of the three
                                   or (●) 2 Running, or Nothing Running
  PROJECTS
```

- **A click goes to the next one**, which the page shows: the most urgent
  first, red, then yellow, then green, and among equals the most recently
  active first. A conversation a click shows stays unread, so the next click
  goes on to the one after the conversation shown, and after the last back
  to the first, as Slack's Jump to Next Unread does. When the shown one has
  been read since, and so left the order, the next click still goes to the
  one that came after it. When the next one is the one shown, the click does
  nothing.
- **Running conversations** are gone through in the same order, the most
  recently active first, but each opens as its row's click opens it: none
  has news to keep.
- **A conversation it shows is read at the first action in the
  conversation**: a click or a tap, a key or a wheel in the main pane below
  the conversation's header bar, or in a frame of it such as the composer's
  field; the rest of 0136 holds. Nothing else reads it: an action in the
  sidebar, a menu or a dialog over the page, the header bar, or on a phone
  the swipe from the screen's edge that brings the sidebar back. The
  composer does not take the focus as the conversation shows, so the next
  key still reaches the entry. Opening a conversation any other way, from
  its row, a link or a notification, reads it as it shows, as today.
- **The entry's own click reads nothing**, the conversation shown before it
  included, so going through the news never reads what was on screen.
- **Nothing moves** as the entry's state changes: it is always there, one row
  high, and only its dot and words change.
- The entry counts the workspace the sidebar shows, as its rows do; another
  workspace's news is the dot on its tile (0317).

### Design

`web-ui` owns the entry and its order, `packages/web` the reading:

```ts
/** What the unread entry says, from the sidebar's rows (`sidebar/unread-entry.ts`). */
export type UnreadEntry =
  | { kind: 'unread'; count: number; tone: NewsTone }
  | { kind: 'running'; count: number; working: boolean }
  | { kind: 'idle' }
  | { kind: 'unknown' }

/** A conversation's place in the entry's order. */
export interface EntryPlace { id: string; rank: number; updatedAt: string }

/**
 * The entry's state, and the place its click goes to after `shownId`;
 * `last`, where the click went before, places a shown conversation read
 * since.
 */
export function unreadEntry(rows: readonly SidebarConversation[]): UnreadEntry
export function nextOf(
  rows: readonly SidebarConversation[],
  shownId: string | null,
  last: EntryPlace | null,
): EntryPlace | null
```

- The counts and the order read the rows' dots (`conversationDotTone`),
  the one rule the rows, the tiles and the tab read, so the entry counts
  what the tab's title counts for the workspace.
- `AppSidebar` shows the entry and emits `glance` with a conversation that
  has news, `select` with one that only runs. On `glance` the host shows it
  without the action that a change of the shown conversation is (`attend()`
  in `App.vue`), and marks the page as **glancing** at it
  (`state/attention.ts`): while it glances, an action attends the page only
  when it is in the conversation, and the glance ends there. A row's
  `select` ends it too and reads the conversation (`opened`), as does
  showing another conversation by any other way. The entry's element is
  marked (`data-unread-entry`), and no click on it attends the page.
- The composer's `focus-on-show` is off while the page glances at its
  conversation.
- An older page has no entry; nothing is stored.

## Spike

None.

## Prior art

- Slack goes to the next unread channel with ⌥⇧↓, and its Mark as Read
  preference can leave a channel's unseen messages unread as it shows. The
  entry departs in reading at the first action in the conversation, as Jush
  does since 0136.
- Linear's Inbox counts what needs the user at the top of its sidebar.

## Alternatives

- **A menu of the unread conversations under the entry.** One more surface
  to keep in step with the rows, which already list them; a click that goes
  to each in turn needs none.
- **Read a conversation the entry shows at once, as a row's click does.**
  Then a glance at one meant for later loses its dot, which is the problem.
- **Hide the entry while nothing is unread or running.** The list below it
  would move as conversations start and stop.

## Open questions

None.
