import { computed, ref, type ComputedRef, type Ref } from 'vue'
import { Activity, CircleStop, SquareTerminal, X } from '@lucide/vue'
import type { ContextMenuGroup } from '../ui/context-menu'
import type { OwnWindow } from '../windows/SessionWindows.vue'
import { t as windowsText } from '../windows/locales/text'
import { deviceTags, type SessionDevice } from '../windows/device-tags'
import { commandKindOf, isTerminalRunning, runningOfKind, terminalStatus, type CommandKind, type TerminalRecord } from './terminals'
import { t } from './locales/text'

/**
 * A conversation's Running and Services windows: each a tab for every running
 * command of its kind, oldest first, and the ended one it shows (`active`),
 * which its Completed list picks (`ended`). An ended command's tab closes, a
 * running one's stays until the command is ended. Each counts its running
 * commands, and is used once the dock holds a command of its kind.
 *
 * With any tab, one is selected, as in a web browser: the command picked
 * while its tab stays, else the oldest, so a window never stands on no tab.
 * A tab is tagged with its command's device among the conversation's
 * `devices` (`deviceTags`): the device its call named, or the primary
 * device for the main agent's call that named none, as its live frames say.
 * A subagent's command that named none runs on that subagent's device, and
 * one the page knows only from the list of ended commands names no device:
 * neither is tagged, rather than tagged wrong.
 */
export function useCommandWindows(
  terminals: () => readonly TerminalRecord[],
  end: (id: string) => void,
  devices: () => readonly SessionDevice[],
): {
  active: Ref<Record<CommandKind, string | null>>
  /** The tabs of the window of `kind`. */
  tabs: (kind: CommandKind) => TerminalRecord[]
  /** The tab the window of `kind` shows. */
  shown: (kind: CommandKind) => string | null
  /** The ended commands of `kind`, newest first, which its Completed list shows. */
  ended: (kind: CommandKind) => TerminalRecord[]
  /** Shows the command `id` in the window of `kind`. */
  pick: (kind: CommandKind, id: string) => void
  windows: ComputedRef<OwnWindow[]>
} {
  const active = ref<Record<CommandKind, string | null>>({ command: null, service: null })

  function tabs(kind: CommandKind): TerminalRecord[] {
    const running = runningOfKind(terminals(), kind)
    const shown = terminals().find((terminal) => terminal.id === active.value[kind])
    return shown && !running.includes(shown) && commandKindOf(shown) === kind ? [...running, shown] : running
  }

  function shown(kind: CommandKind): string | null {
    const all = tabs(kind)
    return all.some((tab) => tab.id === active.value[kind]) ? active.value[kind] : all[0]?.id ?? null
  }

  function ended(kind: CommandKind): TerminalRecord[] {
    return terminals()
      .filter((terminal) => !isTerminalRunning(terminal.phase) && commandKindOf(terminal) === kind)
      .toSorted((a, b) => Date.parse(b.endedAt ?? b.startedAt) - Date.parse(a.endedAt ?? a.startedAt))
  }

  function pick(kind: CommandKind, id: string): void {
    active.value = { ...active.value, [kind]: id }
  }

  function close(kind: CommandKind, id: string): void {
    active.value = { ...active.value, [kind]: tabs(kind).filter((tab) => tab.id !== id).at(-1)?.id ?? null }
  }

  function menu(kind: CommandKind, terminal: TerminalRecord): () => ContextMenuGroup[] {
    return () => [
      [{ icon: CircleStop, label: t('terminals.endCommand'), disabled: terminal.phase !== 'running' || terminal.stopping === true, run: () => end(terminal.id) }],
      [{ icon: X, label: t('terminals.closeTab'), disabled: terminal.phase === 'running', run: () => close(kind, terminal.id) }],
    ]
  }

  /** The device of `terminal` by its id, a device the conversation does not list by the name its call gave; null when not known. */
  function deviceOf(terminal: TerminalRecord): string | null {
    const known = devices()
    if (terminal.device !== undefined) {
      return known.find((device) => device.name === terminal.device)?.id ?? terminal.device
    }
    const mainAgentLive = terminal.subagentId === undefined && terminal.toolUseId !== undefined
    return mainAgentLive ? known.find((device) => device.primary)?.id ?? null : null
  }

  function window(kind: CommandKind, id: string, icon: typeof Activity, name: string): OwnWindow {
    const count = runningOfKind(terminals(), kind).length
    const shownTabs = tabs(kind)
    const tags = deviceTags(shownTabs.map(deviceOf), devices())
    return {
      id,
      icon,
      name,
      count,
      used: terminals().some((terminal) => commandKindOf(terminal) === kind),
      // A running command has the accent's steady dot; a service that runs is
      // in its settled state, so Services has none.
      dot: kind === 'command' && count > 0 ? 'steady' : undefined,
      anchors: true,
      tabs: shownTabs.map((terminal, at) => ({
        id: terminal.id,
        title: terminal.title,
        tag: tags[at] ?? null,
        busy: terminal.stopping === true,
        status: terminalStatus(terminal.phase),
        closable: terminal.phase !== 'running',
        menu: menu(kind, terminal),
      })),
      current: shown(kind),
      select: (tab) => pick(kind, tab),
      closeTab: (tab) => close(kind, tab),
    }
  }

  const windows = computed(() => [
    window('command', 'running', SquareTerminal, windowsText('names.running')),
    window('service', 'services', Activity, windowsText('names.services')),
  ])
  return { active, tabs, shown, ended, pick, windows }
}
