<script setup lang="ts">
import { computed, h, ref, shallowRef, watch, type Component } from 'vue'
import { ArrowLeftToLine, ArrowRightToLine, CopyX, ListX, X } from '@lucide/vue'
import Menu from '../ui/Menu.vue'
import MenuItem from '../ui/MenuItem.vue'
import Sheet from '../ui/Sheet.vue'
import { appOverlayStore } from '../overlay/appOverlay'
import { usePhone } from '../ui/phone'
import type { ContextMenuGroup } from '../ui/context-menu'
import { t as shared } from '../locales/text'
import type { TabUpdate } from '../plugins/page'
import Taskbar, { type TaskbarItem, type TaskbarOpen } from './Taskbar.vue'
import WindowStage, { type StageWindowView } from './WindowStage.vue'
import WorkWindowBody from './WorkWindowBody.vue'
import type { WindowTab } from './WorkWindow.vue'
import { cancelledTabs, openingBecame, openingTabId, windowHolding, workWindows, type ShownTab, type WorkWindow } from './work-windows'
import { ICON_PX } from '../ui/icon-metrics'
import type { BoundTabKind } from '../agent/tab-kinds/kind'
import type { FrontCause, SessionWork } from './session-work'
import { tabsToClose, type TabCloseScope } from '../agent/tab-close'
import { rearranged } from '../ui/drag-order'
import { windowIcon } from './taskbar-icon'
import type { WindowStageModel } from './stage'
import { t } from './locales/text'

/**
 * A conversation's windows around it: the stage, with the conversation beside
 * the column, and the taskbar under it.
 *
 * Two sources fill it:
 *
 * - The work (`work`): the conversation's tabs, grouped into their kinds'
 *   windows (`work-windows.ts`). A window whose kinds are not pinned stands
 *   while it holds a tab, minimized when one comes without being shown; a
 *   pinned kind's window stands while the user keeps it open.
 * - The session's own windows (`own`), such as Agents, Running and a side
 *   question, which the conversation's view fills (`own` slot). Those with a
 *   count stand in the taskbar's system area once the conversation has had
 *   what they show (`used`), and stay, so the taskbar keeps its buttons
 *   while what runs comes and goes; one whose window is open stands too, as
 *   after a switch to a version of a message whose path has none.
 *
 * A tab a kind's plugin opens, such as the browser's new tab, stands in its
 * window from the click, selected, until the work holds the tab it becomes,
 * which is selected in its place (`work-windows.ts`).
 *
 * The stage alone says which windows stand and which is in front; each
 * window of the work shows the newest of its own tabs. The host's address
 * follows: it names the tab of the work's window highest in front that is not
 * minimized, or none (`inFront`). What asks for a tab from outside, a pill, a
 * link, a kind's `shows` or a navigation of the history, comes as the work's
 * `request`, which brings the tab's window to the front on it, opening it in
 * the middle of the area the first time, or, naming none, minimizes the
 * work's windows. Closing never opens or raises another window: the stage
 * hands the front to the newest window still shown. A conversation opened
 * anew shows the layout it keeps.
 *
 * On a phone the taskbar gives way to a list the top bar's Tabs button opens
 * (`listOpen`), and a window shows alone over the conversation.
 */
export interface OwnWindow extends StageWindowView {
  /** What its system button counts, shown above zero; absent, it is no system button and shows only while open. */
  count?: number
  /** Whether the conversation has had what its system button shows, which shows the button from then on. */
  used?: boolean
  /** The dot its system button wears (`TaskbarItem.dot`). */
  dot?: TaskbarItem['dot']
  select(tab: string): void
  closeTab?(tab: string): void
  /** Its window closed: it forgets the tab it showed. */
  closed?(): void
}

const props = defineProps<{
  stage: WindowStageModel
  work?: SessionWork
  own: readonly OwnWindow[]
  /** The px the composer keeps under the floating windows. */
  bottom: number
}>()

defineSlots<{
  default(): unknown
  /** The content of the session's own window `id`. */
  own(props: { id: string }): unknown
  /** What the session's own window `id` adds to its title bar. */
  trailing(props: { id: string }): unknown
  /** The system area's menus after its buttons, such as Schedules. */
  system(): unknown
  /** On a phone, the list's rows after the windows', which open what `system` opens. */
  list(): unknown
}>()

/** On a phone, the list of windows the top bar's Tabs button opens. */
const listOpen = defineModel<boolean>('listOpen', { default: false })

const phone = usePhone()
const area = ref<InstanceType<typeof WindowStage> | null>(null)

/**
 * The opening's tab each window shows, by window, which the page alone
 * knows: a new opening's from its click until the user selects another tab
 * or the work's tab it became is picked in the work.
 */
const chosen = shallowRef<ReadonlyMap<string, string>>(new Map())
function choose(id: string, tab: string | null): void {
  // A choice that changes nothing writes nothing: the watchers that choose
  // read the windows the choice makes, and would run again for their own
  // write.
  if ((chosen.value.get(id) ?? null) === tab) {
    return
  }
  const next = new Map(chosen.value)
  if (tab === null) {
    next.delete(id)
  } else {
    next.set(id, tab)
  }
  chosen.value = next
}

const windows = computed<WorkWindow[]>(() => (props.work ? workWindows(props.work.tabsState, props.work.pinned, props.work.kinds, chosen.value) : []))
const workIds = computed(() => new Set(windows.value.map((window) => window.window.id)))
function workOf(id: string): WorkWindow | undefined {
  return windows.value.find((window) => window.window.id === id)
}
function ownOf(id: string): OwnWindow | undefined {
  return props.own.find((window) => window.id === id)
}

/** Whether the area has its size, which places a window opening for the first time. */
const ready = computed(() => area.value?.measured ?? false)

/**
 * Opens the window `id` for the first time where `WindowStage` says, or where
 * it stood when it closed; false while the area is not measured yet.
 */
function ensure(id: string): boolean {
  if (!props.stage.find(id)) {
    if (!area.value || !ready.value) {
      return false
    }
    props.stage.add(id, area.value.opening(), ownOf(id)?.anchors ? 'anchored' : 'floating')
  }
  return true
}

// A window of kinds that are not pinned stands while it holds a tab, an
// opening's included; one that comes without a request for its tab waits
// minimized on the taskbar. It runs before the watcher that says what is in
// front, made after it, so a window its last closed tab leaves empty is gone
// by then.
watch([windows, ready], ([now]) => {
  for (const window of now) {
    if (window.tabs.some((tab) => !tab.pinned)) {
      ensure(window.window.id)
    } else if (window.tabs.length === 0 && props.stage.find(window.window.id)) {
      props.stage.remove(window.window.id)
    }
  }
}, { immediate: true })

/**
 * The strip's key of each work tab an opening became: the opening's, so the
 * strip keeps the one tab in place rather than closing one and opening
 * another.
 */
const keys = new Map<string, string>()
/** The strip's key of the tab `id`. */
function keyOf(id: string): string {
  const kept = keys.get(id)
  if (kept !== undefined) {
    return kept
  }
  for (const kind of props.work?.kinds ?? []) {
    const opening = kind.openings?.().find((each) => each.tab === id)
    if (opening) {
      const key = openingTabId(kind, opening.key)
      keys.set(id, key)
      return key
    }
  }
  return id
}

/** Every opening's tab the windows held, which a new one is not. */
let openingsSeen = new Set<string>()
/** The stage the openings were seen on: another is another conversation's, whose openings are its own. */
let openingsStage: WindowStageModel | null = null
// A new opening is selected in its window, as a web browser selects the tab
// its + opens; the work's tab it became is then picked in the work, and an
// opening that ended without a tab leaves its window to its other tabs. The
// tab of one the user closed closes once the work holds it.
watch(windows, (now) => {
  const work = props.work
  const unwanted = work ? cancelledTabs(work.kinds).filter((tab) => work.tabsState.tabs.some((held) => held.id === tab)) : []
  if (unwanted.length) {
    work?.closeTabs(unwanted)
  }
  // Another conversation came: what was chosen, seen and keyed was the last one's.
  const came = openingsStage !== props.stage
  if (came) {
    openingsStage = props.stage
    chosen.value = new Map()
    keys.clear()
  }
  const seen = new Set<string>()
  for (const window of now) {
    for (const tab of window.tabs) {
      if (tab.opening !== null) {
        seen.add(tab.id)
        if (!came && !openingsSeen.has(tab.id)) {
          choose(window.window.id, tab.id)
        }
      }
    }
  }
  openingsSeen = seen
  for (const [id, tab] of chosen.value) {
    const became = work ? openingBecame(now, work.kinds, tab) : null
    if (became !== null) {
      work?.pick(became)
      choose(id, null)
    } else if (!seen.has(tab)) {
      choose(id, null)
    }
  }
}, { immediate: true })

/**
 * The request the windows last took. The request comes in `props.work`,
 * which the parent makes anew only when it renders again: what taking it
 * changes in the windows runs this watcher again first, with the request it
 * took.
 */
let taken: number | null = null
// A request brings its tab's window to the front on it once the window holds
// the tab, as a click would; one naming none puts the work's windows away.
watch([() => props.work?.request ?? null, windows, ready], ([request]) => {
  const work = props.work
  if (!work || !request || request.id === taken) {
    return
  }
  const take = () => {
    taken = request.id
    work.took(request.id)
  }
  if (request.opens) {
    const { kind, data } = request.opens
    const opens = work.kinds.find((candidate) => candidate.kind === kind)
    take()
    if (opens?.open) {
      opens.open(data)
      show(opens.window.id)
    }
    return
  }
  if (request.tab === null) {
    take()
    for (const placed of props.stage.state.windows) {
      if (workIds.value.has(placed.id) && placed.placement !== 'minimized') {
        props.stage.minimize(placed.id)
      }
    }
    return
  }
  const window = windowHolding(windows.value, request.tab)
  if (window && ensure(window.window.id)) {
    take()
    choose(window.window.id, null)
    work.pick(request.tab)
    props.stage.show(window.window.id)
  }
}, { immediate: true })

/**
 * The work's tab of the window highest in front that is not minimized and
 * shows one; none while no window of the work shows. An opening's tab is
 * the page's alone, which the address cannot name: while a window shows
 * one, the address keeps naming the work's tab behind it.
 */
const inFront = computed(() => {
  const shown = props.stage.state.windows
    .filter((placed) => placed.placement !== 'minimized' && workIds.value.has(placed.id))
    .sort((a, b) => b.order - a.order)
  for (const placed of shown) {
    const tab = workOf(placed.id)?.workTab ?? null
    if (tab !== null) {
      return tab
    }
  }
  return null
})
/** The stage the address last followed: the first word of another one writes the address in place. */
let followed: WindowStageModel | null = null
/** The tab last said in front: once no window holds it, it closed. */
let said: string | null = null
/** The window in front was closed since the windows last said what is. */
let closing = false
// What is in front is the address: said once the windows know their tabs and
// have taken what the address asked of them.
watch([() => props.stage, inFront, ready, () => props.work?.request ?? null, () => props.work?.tabsRead ?? false], ([stage, tab]) => {
  const work = props.work
  if (!work || !ready.value || work.request?.fromAddress || (!work.tabsRead && !work.beforeFirstMessage)) {
    return
  }
  const gone = closing || (said !== null && said !== tab && !windowHolding(windows.value, said))
  const cause: FrontCause = followed !== stage ? 'came' : gone ? 'closed' : 'moved'
  followed = stage
  said = tab
  closing = false
  work.inFront(tab, cause)
}, { immediate: true })

/** Every tab of the work, by id, as it reads now. */
const tabsById = computed(() => new Map(windows.value.flatMap((window) => window.tabs.map((tab) => [tab.id, tab] as const))))
/**
 * Each tab's mark, as a window's tab and a taskbar's list draw it: one
 * component per tab and kind, which draws the tab's data as it reads now, so
 * a tab's data read anew, as each change of the work reads it, redraws the
 * mark and never mounts it again.
 */
const marks = new Map<string, { kind: BoundTabKind; mark: Component }>()
function markOf(tab: ShownTab): Component {
  let kept = marks.get(tab.id)
  if (kept?.kind !== tab.kind) {
    const { id, kind } = tab
    // An opening has no data yet: its window's icon stands for its mark.
    const opening = tab.opening !== null
    kept = {
      kind,
      mark: {
        render: () => {
          const now = tabsById.value.get(id)
          return now ? opening ? h(kind.window.icon, { size: ICON_PX.markIn28 }) : h(kind.mark, { data: now.data }) : null
        },
      },
    }
    marks.set(id, kept)
  }
  return kept.mark
}
// A closed tab's mark and key go with it.
watch(tabsById, (now) => {
  for (const id of marks.keys()) {
    if (!now.has(id)) {
      marks.delete(id)
    }
  }
  for (const id of keys.keys()) {
    if (!now.has(id)) {
      keys.delete(id)
    }
  }
})

/** The tab menu's commands that close several of the window's tabs at once. */
const closeMany: readonly { scope: Exclude<TabCloseScope, 'self'>; label: () => string; icon: Component }[] = [
  { scope: 'right', label: () => t('closeRight'), icon: ArrowRightToLine },
  { scope: 'left', label: () => t('closeLeft'), icon: ArrowLeftToLine },
  { scope: 'others', label: () => t('closeOthers'), icon: CopyX },
  { scope: 'all', label: () => t('closeAll'), icon: ListX },
]

/**
 * A tab's menu: what its kind offers, then the Close commands over the
 * window's tabs, as a web browser's tab menu. A pinned tab closes by none.
 */
function tabMenu(window: WorkWindow, tab: ShownTab): (() => ContextMenuGroup[]) | undefined {
  // An opening's page is not there yet: only its Close commands apply.
  const commands = () => tab.opening === null ? tab.kind.commands?.(tab.data, tab.id) ?? [] : []
  if (tab.pinned && commands().length === 0) {
    return undefined
  }
  const close = (ids: string[]) => closeTabs(window, ids)
  return () => {
    const closable = window.tabs.filter((each) => !each.pinned)
    return [
      commands().map((command) => ({ icon: command.icon, label: command.label, disabled: command.disabled, run: () => command.run() })),
      tab.pinned ? [] : [
        { icon: X, label: shared('common.close'), run: () => close([tab.id]) },
        ...closeMany.map((item) => {
          const ids = tabsToClose(closable, tab.id, item.scope)
          return { icon: item.icon, label: item.label(), disabled: ids.length === 0, run: () => close(ids) }
        }),
      ],
    ]
  }
}

/** Whether the window only ever shows one view, its one pinned tab, such as Files: no tab of its own opens or closes. */
function isSingle(window: WorkWindow): boolean {
  return window.starts === null && window.tabs.length === 1 && window.tabs[0]!.pinned
}

/** The window's tabs; a single view's is named by what it shows, such as a file's name, as its title bar and button say. */
function workTabs(window: WorkWindow): WindowTab[] {
  const offline = props.work?.deviceOffline !== null
  const single = isSingle(window)
  return window.tabs.map((tab) => ({
    id: tab.id,
    key: tab.opening === null ? keyOf(tab.id) : tab.id,
    title: single ? tab.subject : tab.title,
    mark: markOf(tab),
    icon: tab.opening === null ? tab.kind.icon?.(tab.data, tab.id) ?? null : null,
    // An opening is at work until its tab comes, as a loading page's tab.
    busy: !offline && (tab.opening !== null || (tab.kind.busy?.(tab.data, tab.id) ?? false)),
    closable: !tab.pinned,
    menu: tabMenu(window, tab),
  }))
}

/** Closes the tabs `ids` of `window`: the work's leave the work, and an opening ends. */
function closeTabs(window: WorkWindow, ids: readonly string[]): void {
  const closing = window.tabs.filter((tab) => ids.includes(tab.id))
  for (const tab of closing) {
    if (tab.opening !== null) {
      tab.kind.cancel?.(tab.opening)
    }
  }
  const held = closing.filter((tab) => tab.opening === null).map((tab) => tab.id)
  if (held.length) {
    props.work?.closeTabs(held)
  }
}

const views = computed<StageWindowView[]>(() => props.stage.state.windows.flatMap((placed): StageWindowView[] => {
  const window = workOf(placed.id)
  if (window) {
    return [{
      id: placed.id,
      icon: window.window.icon,
      name: window.window.name(),
      tabs: workTabs(window),
      current: window.selected,
      opensTabs: window.starts !== null,
      single: isSingle(window),
    }]
  }
  const own = ownOf(placed.id)
  return own ? [own] : []
}))

function select(id: string, tab: string): void {
  const window = workOf(id)
  if (window) {
    if (window.tabs.some((each) => each.id === tab && each.opening !== null)) {
      choose(id, tab)
    } else {
      choose(id, null)
      props.work?.pick(tab)
    }
    props.stage.show(id)
  } else {
    ownOf(id)?.select(tab)
  }
}

function closeTab(id: string, tab: string): void {
  const window = workOf(id)
  if (window) {
    closeTabs(window, [tab])
  } else {
    ownOf(id)?.closeTab?.(tab)
  }
}

function reorder(id: string, from: number, to: number): void {
  const window = workOf(id)
  const work = props.work
  if (!window || !work) {
    return
  }
  const tabs = window.tabs.filter((tab) => !tab.pinned)
  const offset = window.tabs.length - tabs.length
  const moved = tabs[from - offset]
  const before = tabs[to - offset]
  // An opening stays at the end until its tab comes.
  if (moved?.opening === null && before?.opening === null) {
    work.moveTab(moved.id, work.tabsState.tabs.findIndex((tab) => tab.id === before.id))
  }
}

/** The window's + and the taskbar's +: a new tab, which the plugin of a device's live tabs makes as an opening. */
function newTab(id: string): void {
  const starts = workOf(id)?.starts
  const data = starts?.start?.()
  if (!starts || data === undefined) {
    return
  }
  if (starts.open) {
    starts.open(data)
    show(id)
    return
  }
  props.work?.add(starts.kind, data)
}

/** Closes the window `id`: a window of the work closes its tabs with it, as a web browser's does. */
function close(id: string): void {
  const window = workOf(id)
  if (window) {
    closing ||= inFront.value !== null && windowHolding(windows.value, inFront.value)?.window.id === id
    const tabs = window.tabs.filter((tab) => !tab.pinned).map((tab) => tab.id)
    props.stage.remove(id)
    closeTabs(window, tabs)
    return
  }
  props.stage.remove(id)
  ownOf(id)?.closed?.()
}

/**
 * Escape on the window `id`: one that holds only what it always shows, such
 * as Files, closes, as a glance ends; one that holds the user's own tabs, such
 * as the browser's pages or a simulator, goes to the taskbar, so a key never
 * ends a page or a device.
 */
function putAway(id: string): void {
  if (workOf(id)?.tabs.some((tab) => !tab.pinned)) {
    props.stage.minimize(id)
  } else {
    close(id)
  }
}

/** Opens the window `id` from the taskbar, or the + menu: on its tab, a pinned kind's, or a new tab of a kind that opens them. */
function open(id: string): void {
  const window = workOf(id)
  if (window) {
    const tab = window.selected
    if (tab !== null) {
      ensure(id)
      if (window.tabs.some((each) => each.id === tab && each.opening === null)) {
        props.work?.pick(tab)
      }
      props.stage.show(id)
    } else {
      newTab(id)
    }
    return
  }
  const own = ownOf(id)
  if (own) {
    ensure(id)
    props.stage.show(id)
  }
}

function press(id: string): void {
  if (props.stage.find(id)) {
    props.stage.toggle(id)
  } else {
    open(id)
  }
}

function pick(id: string, tab: string): void {
  if (!props.stage.find(id)) {
    open(id)
  }
  select(id, tab)
  props.stage.show(id)
}

function stateOf(id: string): TaskbarItem['state'] {
  const placed = props.stage.find(id)
  if (!placed) {
    return 'closed'
  }
  if (placed.placement === 'minimized') {
    return 'minimized'
  }
  return props.stage.state.focused === id ? 'front' : 'shown'
}

function itemOf(id: string): TaskbarItem | null {
  const window = workOf(id)
  if (window) {
    const tabs = workTabs(window)
    return {
      id,
      icon: window.window.icon,
      name: window.window.name(),
      tabs: props.stage.find(id) ? tabs.map((tab) => ({ id: tab.id, title: tab.title, mark: tab.mark, icon: tab.icon, closable: tab.closable })) : [],
      current: window.selected,
      pinned: props.work?.pins.includes(id) ?? false,
      state: stateOf(id),
    }
  }
  const own = ownOf(id)
  if (!own) {
    return null
  }
  return {
    id,
    icon: own.icon,
    name: own.name,
    tabs: own.tabs.map((tab) => ({ id: tab.id, title: tab.title, mark: tab.mark, icon: tab.icon, closable: tab.closable })),
    current: own.current,
    pinned: false,
    state: stateOf(id),
    count: own.count,
    dot: own.dot,
  }
}

/** The user's pins, then the windows that stand, in the order they came. */
const items = computed<TaskbarItem[]>(() => {
  const pins = (props.work?.pins ?? []).filter((id) => workIds.value.has(id))
  const standing = props.stage.state.windows.map((window) => window.id).filter((id) => !pins.includes(id) && ownOf(id)?.count === undefined)
  return [...pins, ...standing].flatMap((id) => itemOf(id) ?? [])
})
const system = computed<TaskbarItem[]>(() => props.own
  .flatMap((window) => window.count === undefined ? [] : itemOf(window.id) ?? [])
  .filter((item) => ownOf(item.id)?.used || item.state !== 'closed'))
/** What Open lists: every window of the work but those that say it does not (`openable`). */
const opens = computed<TaskbarOpen[]>(() => windows.value.filter((window) => window.window.openable !== false).map((window) => ({
  kind: window.window.id,
  icon: window.window.icon,
  label: window.window.name(),
  open: props.stage.find(window.window.id) !== undefined,
  newTab: window.starts !== null && props.stage.find(window.window.id) !== undefined,
  menu: window.offers?.openMenu,
})))

function openFromMenu(id: string): void {
  const window = workOf(id)
  if (window?.starts && props.stage.find(id)) {
    newTab(id)
  } else if (props.stage.find(id)) {
    props.stage.show(id)
  } else {
    open(id)
  }
}

function pin(id: string, pinned: boolean): void {
  const others = (props.work?.pins ?? []).filter((each) => each !== id)
  props.work?.keepPins(pinned ? [...others, id] : others)
}

/** The taskbar's buttons of a group dragged to a new order: the pins, or the windows that stand. */
function arrange(ids: string[], pinned: boolean): void {
  if (pinned) {
    props.work?.keepPins(rearranged(props.work.pins, ids))
  } else {
    props.stage.arrange(ids)
  }
}

function onScreen(id: string): boolean {
  const placed = props.stage.find(id)
  return placed !== undefined && placed.placement !== 'minimized' && (!phone.value || props.stage.state.focused === id)
}

/** The window `show` asked for before the area was measured, which shows once it is. */
let waiting: string | null = null
watch(ready, (measured) => {
  if (measured && waiting !== null) {
    const id = waiting
    waiting = null
    show(id)
  }
})

/** Shows the window `id` in front, opening it the first time where windows open. */
function show(id: string): void {
  if (ensure(id)) {
    props.stage.show(id)
  } else {
    waiting = id
  }
}

defineExpose({ show })

/** On a phone, the list's rows: every button the taskbar would show. */
const listed = computed(() => [...items.value, ...system.value])

function listPick(id: string): void {
  listOpen.value = false
  open(id)
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <div class="relative min-h-0 flex-1">
      <WindowStage
        ref="area"
        :model="stage"
        :windows="views"
        :bottom="bottom"
        :phone="phone"
        @select="select"
        @close-tab="closeTab"
        @reorder="reorder"
        @new-tab="newTab"
        @close="close"
        @escape="putAway"
      >
        <slot />
        <template #content="{ id }">
          <WorkWindowBody
            v-if="workOf(id)"
            :window="workOf(id)!"
            :on-screen="onScreen(id)"
            :device-offline="work?.deviceOffline ?? null"
            :dismiss="phone ? () => stage.minimizeAll() : null"
            @update-tab="(tab: string, data: unknown, how?: TabUpdate) => work?.updateTab(tab, data, how)"
            @update-pinned="(kind: string, data: unknown, how?: TabUpdate) => work?.updatePinned(kind, data, how)"
            @close-tab="(tab: string) => closeTab(id, tab)"
          />
          <slot v-else name="own" :id="id" />
        </template>
        <template #trailing="{ id }">
          <slot v-if="!workOf(id)" name="trailing" :id="id" />
        </template>
      </WindowStage>
    </div>
    <Taskbar
      v-if="!phone"
      :items="items"
      :system="system"
      :opens="opens"
      @press="press"
      @pick="pick"
      @close-tab="closeTab"
      @pin="pin"
      @arrange="arrange"
      @close="close"
      @open="openFromMenu"
      @opened="show"
    >
      <template #system>
        <slot name="system" />
      </template>
    </Taskbar>
    <Sheet v-if="phone" :open="listOpen" :label="t('taskbar')" :overlay-store="appOverlayStore" @close="listOpen = false">
      <Menu>
        <MenuItem
          v-for="item in listed"
          :key="item.id"
          :icon="windowIcon(item)"
          :label="item.tabs.find((tab) => tab.id === item.current)?.title ?? item.name"
          :value="item.count !== undefined ? item.count ? String(item.count) : undefined : item.tabs.length > 1 ? String(item.tabs.length) : undefined"
          value-faint
          @select="listPick(item.id)"
        />
        <slot name="list" />
        <MenuItem
          v-for="entry in opens.filter((open) => open.menu || !open.open)"
          :key="`open-${entry.kind}`"
          :icon="entry.icon"
          :label="entry.label"
          @select="entry.menu || listPick(entry.kind)"
        >
          <template v-if="entry.menu" #submenu>
            <component :is="entry.menu" @opened="listOpen = false; show(entry.kind)" />
          </template>
        </MenuItem>
      </Menu>
    </Sheet>
  </div>
</template>
