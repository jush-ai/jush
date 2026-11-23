import { computed, onScopeDispose } from 'vue'
import { z } from 'zod'
import { BrowserTabsError, addedWorkTab, asTabsError, type BrowserTabsApi } from './live/tabs'
import { PluginCallError, type ConversationPlugin } from '@jush-ai/plugin-sdk'
import {
  browserTabsSchema,
  controlReasonSchema,
  tabMovedSchema,
  tabOpenedSchema,
  type NavigateTab,
  type OpenTab,
  type StopTab,
  type TabHistory,
} from './generated/plugin'

/**
 * Opening a tab may start the conversation browser, on a Cloud that was
 * stopped: the operation allows itself five minutes (`browser.tab.new`), and the
 * call waits a little longer than that, so the operation's own answer is what
 * ends the wait.
 */
const OPEN_TIMEOUT_MS = 310_000

/**
 * A refused call in the words the page shows for it, never the device's own error
 * text: its reason's, or for an answer whose reason is not one of the plugin's,
 * its HTTP status. A call no answer came for, or whose answer could not be
 * read, says which.
 */
function tabsError(error: PluginCallError): BrowserTabsError {
  const reason = controlReasonSchema.safeParse(error.reason)
  if (reason.success) {
    return BrowserTabsError.of(reason.data)
  }
  switch (error.answer.type) {
    case 'status':
      return new BrowserTabsError({ status: error.answer.status })
    case 'none':
      return new BrowserTabsError({ answer: 'none' })
    case 'unreadable':
      return new BrowserTabsError({ answer: 'unreadable' })
  }
}

/**
 * The conversation browser's tab list on its device `device`, its tab methods
 * and its `browser` user stream over the plugin, which is the conversation's
 * on that device, for a conversation's browser on the page, whose effect
 * scope the tab list is followed in; `device` null is the primary device's,
 * whichever that is. A call still waiting for the backend when the store
 * releases that browser, as when the plugin is turned off, is dropped and
 * never sent (`packages/web-ui/src/plugins/page.ts`).
 */
export function browserTabsApi(
  plugin: ConversationPlugin,
  device: string | null,
  select: (workTab: string) => void,
  held: () => readonly string[],
): BrowserTabsApi {
  const state = plugin.state(browserTabsSchema)
  // The device's list, the primary device's first; one whose browser never
  // reported has none of its tabs.
  const tabs = {
    value: computed(() => {
      const all = state.value.value
      if (all === null) {
        return null
      }
      const listed = device === null ? all.devices[0] : all.devices.find((each) => each.device === device)
      return listed ?? { device: device ?? '', list: 0, tabs: [] }
    }),
    error: state.error,
  }
  const session = new AbortController()
  onScopeDispose(() => session.abort())
  async function call<T>(method: string, params: object, answer: z.ZodType<T>, timeoutMs?: number): Promise<T> {
    try {
      return await plugin.call(method, params, answer, { timeoutMs, signal: session.signal })
    } catch (error) {
      if (session.signal.aborted) {
        // Dropped with the work: the abort, which nothing reports.
        throw error
      }
      // Anything but a refusal has no code.
      throw error instanceof PluginCallError ? tabsError(error) : asTabsError(error)
    }
  }
  /** A navigation's answer: the number of the last tab list before it started. */
  async function move(method: string, params: object): Promise<number> {
    const moved = await call(method, params, tabMovedSchema)
    return moved.list
  }
  return {
    tabs: {
      value: tabs.value,
      error: computed(() => {
        const error = tabs.error.value
        return error ? tabsError(error) : null
      }),
    },
    open: (url) => {
      const params: OpenTab = url === null ? {} : { url }
      return call('open', params, tabOpenedSchema, OPEN_TIMEOUT_MS)
    },
    select,
    workTab: (tab) => addedWorkTab(device, tab),
    held,
    navigate: (tab, url) => move('navigate', { tab, url } satisfies NavigateTab),
    history: (tab, action) => move('history', { tab, action } satisfies TabHistory),
    stop: (tab) => move('stop', { tab } satisfies StopTab),
    stream: plugin.stream('browser'),
    deviceStarting: () => plugin.deviceStarting.value,
    downloading: () => plugin.downloading.value,
  }
}
