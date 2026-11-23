import type { Component } from 'vue'
import type { z } from 'zod'
import type { WorkTab } from '../work-tabs'
import type { TitleText } from '../../ui/ui-text'

/** A command of a tab's menu that its kind offers, such as Reload. */
export interface TabCommand {
  label: TitleText
  icon?: Component
  /** It cannot run now, and shows disabled. */
  disabled?: boolean
  run(): void
}

/**
 * A window the conversation's tabs open in, such as Files or the browser: its
 * id, unique across plugins, its name and its icon, which its title bar and
 * its taskbar button show.
 */
export interface WindowKind {
  id: string
  name(): string
  icon: Component
  /**
   * Whether the taskbar's Open lists it; absent, it does. A window that only
   * shows what another place opens in it, such as Attachments, which an
   * attachment in the transcript opens, says false.
   */
  openable?: boolean
  /**
   * Each device's tabs of it make a window of their own, as each Chrome
   * profile's do: a device's browser or simulators. The page binds its kinds
   * once per device the conversation reaches, each to a copy of the window
   * whose `device` names it.
   */
  perDevice?: boolean
  /** The device a per-device window's copy is of. */
  device?: string
}

/**
 * The id of a per-device window `window`'s copy for `device`: the window's
 * own for the conversation's `primary` device, so the user's pin and the
 * layout the conversation keeps name the primary device's window, as they
 * named the one window before; `browser@<device>` for another device.
 */
export function deviceWindowId(window: WindowKind, device: string, primary: string): string {
  return device === primary ? window.id : `${window.id}@${device}`
}

/**
 * A tab the kind's plugin opens for the user, from the click until the work
 * holds it: it stands in its window's strip meanwhile, as a web browser's new
 * tab does before its page exists.
 */
export interface KindOpening {
  /** Which of the kind's openings it is, unique while the page has it. */
  key: string
  /** Its title in the strip, such as New Tab or a simulator's name. */
  title: string
  /** The id of the work tab it becomes, once the device made it; null before. */
  tab: string | null
  /**
   * The work has held its tab: the opening is over, and the tab, while the
   * work holds it, stands in its place.
   */
  done: boolean
  /**
   * The user closed it before the work held its tab, which the strip never
   * shows: its window closes the tab once the work holds it.
   */
  cancelled: boolean
}

/**
 * What the conversation's windows need to show the tabs of one kind. They
 * know nothing else about a kind: the protocol, stream or routes its tabs use
 * stay behind `content`.
 */
export interface BoundTabKind<Data = unknown> {
  kind: string
  /** A tab's `data`, checked where the saved state enters the page. */
  schema: z.ZodType<Data>
  /** The device a per-device window's kind is bound to, whose tabs alone it reads. */
  device?: string
  /** The window its tabs open in. */
  window: WindowKind
  /** The data of a new tab the window's + opens, or what `open` asks for; absent, it opens tabs only by intents. */
  start?(): unknown
  /**
   * Has the kind's plugin make the tab `opens` asks for in place of the work
   * creating it, as one of its `openings`.
   */
  open?(opens: unknown): void
  /**
   * The tabs its plugin opens for the user on this page, oldest first, read
   * reactively; one the work holds already may still be among them.
   */
  openings?(): readonly KindOpening[]
  /** Ends the opening `key`, as its tab's Close: it is `cancelled` from then on. */
  cancel?(key: string): void
  // A method, so kinds of different data fit one list of kinds.
  title(data: Data): string
  /** What the pane says it reads while it waits, such as a file's name; absent, the title. */
  subject?(data: Data): string
  /** The strip's mark. Props: `data`. */
  mark: Component
  /**
   * A picture in place of the mark, such as a web page's icon, as an image URL;
   * null while there is none.
   */
  icon?(data: Data, id: string): string | null
  /**
   * Whether the tab `id` is at work, read reactively: its mark is a spinner
   * meanwhile.
   */
  busy?(data: Data, id: string): boolean
  /**
   * The tab's content. Props: `tabId`, `data`, and `shown`, whether the tab is
   * the work's selection. Emits `update` with the tab's next `data` and,
   * for a change that is no step of the history, a `TabUpdate` saying so;
   * and `close` to have its window close the tab as its user would.
   */
  content: Component
  /**
   * The kind is its window's one view, such as Files, there in every
   * conversation, which is never created, closed or saved; its data starts
   * here and lives in the page's memory. Its id is the kind's id.
   */
  pinned?: { data(): Data }
  /**
   * What the tab's menu offers before its Close commands, read when the menu
   * opens.
   */
  commands?(data: Data, id: string): readonly TabCommand[]
  /**
   * What the strip shows after a pinned tab's title, such as its counts. Props:
   * `data`.
   */
  badge?: Component
  /**
   * The rows of its window's entry in the taskbar's Open, as a submenu, such
   * as the simulators a device has. Emits `opened` once the user chose one,
   * which shows the window.
   */
  openMenu?: Component
  /**
   * The content of an opening's tab, such as a simulator that boots. Props:
   * `opening`, its key, and `shown`, whether its window is on screen.
   */
  opening?: Component
}

/** A tab with its kind and checked data, or why no window can show it. */
export type ResolvedTab =
  | { tab: WorkTab; kind: BoundTabKind; data: unknown; title: string }
  | { tab: WorkTab; kind: null; data: null; title: string }

/**
 * The tab as the windows show it. A tab of an unknown kind, or whose data does
 * not fit its kind, stays in the strip and says so: the page never repairs or
 * drops a saved tab.
 */
export function resolveTab(tab: WorkTab, kinds: readonly BoundTabKind[]): ResolvedTab {
  const read = readTab(tab, kinds)
  return read ? { tab, kind: read.kind, data: read.data, title: read.kind.title(read.data) } : { tab, kind: null, data: null, title: tab.kind }
}

/**
 * The tab's kind among `kinds` and its data as the kind reads it: null while
 * none of them reads it, so no window can show it, as a tab of a device the
 * conversation no longer reaches. A kind bound to a device reads that
 * device's tabs alone.
 */
export function readTab<Kind extends { kind: string; schema: z.ZodType; device?: string }>(
  tab: WorkTab,
  kinds: readonly Kind[],
): { kind: Kind; data: unknown } | null {
  const kind = kinds.find((candidate) => candidate.kind === tab.kind && (candidate.device === undefined || candidate.device === tab.device))
  const parsed = kind?.schema.safeParse(tab.data)
  return kind && parsed?.success ? { kind, data: parsed.data } : null
}
