import { activeTab } from '../agent/tab-close'
import { pinnedData, type TabsState, type PinnedTabs } from '../agent/work-tabs'
import { resolveTab, type BoundTabKind, type WindowKind } from '../agent/tab-kinds/kind'

/**
 * The windows the conversation's tabs open in: each kind names its window
 * (`BoundTabKind.window`), and the kinds of one window share it.
 *
 * - A pinned kind is one tab of its window, named by the kind's id, there in
 *   every conversation.
 * - A tab of a kind the page does not have, or whose data its kind does not
 *   read, has no window; it stays saved, as the backend keeps it.
 * - A tab a kind's plugin opens for the user (`BoundTabKind.openings`) stands
 *   at the end of its window's strip until the work holds the tab it
 *   becomes, which then stands in its place. One the user closed shows
 *   neither, until its window closes the work's tab (`cancelledTabs`).
 * - A window shows the opening the user has selected (`chosen`), or the work's
 *   tab it became; else the newest of its tabs the history selected, else its
 *   first, as the tabs of one strip did.
 */

/** A tab of a work window, with its kind and checked data. */
export interface ShownTab {
  id: string
  kind: BoundTabKind
  data: unknown
  title: string
  /** What it shows, by name alone, such as a file's: what its pane says it reads while it waits, and a single view's title. */
  subject: string
  /** It is its kind's one tab, which no Close closes. */
  pinned: boolean
  /** The key of the kind's opening it is, until the work holds the tab it becomes; null for a tab of the work. */
  opening: string | null
}

/** The strip's id of the opening `key` of `kind`, which no work tab's id takes. */
export function openingTabId(kind: BoundTabKind, key: string): string {
  return `opening:${kind.kind}:${key}`
}

export interface WorkWindow {
  window: WindowKind
  tabs: ShownTab[]
  /** The tab it shows. */
  selected: string | null
  /** The tab of the work it shows, or would show but for an opening the user has selected: what the address names. */
  workTab: string | null
  /** The kind whose new tab its + opens, if one of its kinds opens tabs. */
  starts: BoundTabKind | null
  /** The kind whose rows its entry in the taskbar's Open lists, if one of its kinds has them. */
  offers: BoundTabKind | null
}

/**
 * The windows of `kinds` in their order, each with its tabs: its pinned kinds'
 * first, then the work's in order, then the openings. `chosen` holds, by
 * window, the id of the opening's tab the user has selected in it.
 */
export function workWindows(
  tabsState: TabsState,
  pinned: PinnedTabs,
  kinds: readonly BoundTabKind[],
  chosen: ReadonlyMap<string, string> = new Map(),
): WorkWindow[] {
  const windows = new Map<string, WorkWindow>()
  const windowOf = (kind: BoundTabKind): WorkWindow => {
    let found = windows.get(kind.window.id)
    if (!found) {
      found = { window: kind.window, tabs: [], selected: null, workTab: null, starts: null, offers: null }
      windows.set(kind.window.id, found)
    }
    return found
  }
  for (const kind of kinds) {
    const window = windowOf(kind)
    if (kind.start && !kind.pinned && !window.starts) {
      window.starts = kind
    }
    if (kind.openMenu && !window.offers) {
      window.offers = kind
    }
    if (kind.pinned) {
      const data = pinnedData(pinned, kind)
      window.tabs.push({ id: kind.kind, kind, data, title: kind.title(data), subject: kind.subject?.(data) ?? kind.title(data), pinned: true, opening: null })
    }
  }
  const cancelled = new Set(cancelledTabs(kinds))
  for (const tab of tabsState.tabs) {
    const resolved = resolveTab(tab, kinds)
    if (resolved.kind && !cancelled.has(tab.id)) {
      const { kind, data, title } = resolved
      windowOf(kind).tabs.push({ id: tab.id, kind, data, title, subject: kind.subject?.(data) ?? title, pinned: false, opening: null })
    }
  }
  const held = new Set(tabsState.tabs.map((tab) => tab.id))
  /** The work tab each opening became, by its tab's id, once the work holds it. */
  const became = new Map<string, string>()
  for (const kind of kinds) {
    for (const opening of kind.openings?.() ?? []) {
      const id = openingTabId(kind, opening.key)
      if (opening.tab !== null && held.has(opening.tab)) {
        became.set(id, opening.tab)
      } else if (!opening.cancelled && !opening.done) {
        windowOf(kind).tabs.push({ id, kind, data: null, title: opening.title, subject: opening.title, pinned: false, opening: opening.key })
      }
    }
  }
  for (const window of windows.values()) {
    const ids = window.tabs.map((tab) => tab.id)
    const picked = chosen.get(window.window.id)
    const shown = picked === undefined ? undefined : became.get(picked) ?? picked
    window.workTab = activeTab(window.tabs.filter((tab) => tab.opening === null).map((tab) => tab.id), tabsState.history)
    window.selected = shown !== undefined && ids.includes(shown) ? shown : window.workTab
  }
  return [...windows.values()]
}

/** The work tabs the openings the user closed became, which their windows close. */
export function cancelledTabs(kinds: readonly BoundTabKind[]): string[] {
  return kinds.flatMap((kind) => (kind.openings?.() ?? []).flatMap((opening) => opening.cancelled && opening.tab !== null ? [opening.tab] : []))
}

/** The id of the work tab the opening whose tab is `id` became, once the work holds it, else null. */
export function openingBecame(windows: readonly WorkWindow[], kinds: readonly BoundTabKind[], id: string): string | null {
  const held = new Set(windows.flatMap((window) => window.tabs.filter((tab) => tab.opening === null).map((tab) => tab.id)))
  for (const kind of kinds) {
    const opening = (kind.openings?.() ?? []).find((each) => openingTabId(kind, each.key) === id)
    if (opening?.tab != null && held.has(opening.tab)) {
      return opening.tab
    }
  }
  return null
}

/** The window that holds the tab `id`, or null. */
export function windowHolding(windows: readonly WorkWindow[], id: string): WorkWindow | null {
  return windows.find((window) => window.tabs.some((tab) => tab.id === id)) ?? null
}
