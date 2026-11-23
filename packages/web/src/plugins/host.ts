import { getActivePinia } from 'pinia'
import { watch } from 'vue'
import { z } from 'zod'
import type { DeviceScope, PageHost } from '@jush-ai/web-ui/plugins/page'
import { disposePluginStores } from '@jush-ai/web-ui/plugins/stores'
import { appOverlayStore } from '@jush-ai/web-ui/overlay/appOverlay'
import { workspaceName } from '@jush-ai/web-ui/workspaces/workspaces'
import { userStreamOf } from '@jush-ai/web-ui/transport/user-stream'
import { apiRequest, jsonBody, readResponse } from '../api/client'
import { conversationFiles } from '../conversation/files'
import { directRouteOn } from '../direct'
import { directStream } from '../direct/operations'
import { useWork } from '../conversation/work'
import { useConversations } from '../conversation/store'
import { useProduct } from '../state/product'
import { packageDownloading, packageInstalled } from '../state/installed'
import { useResources } from '../state/resources'
import { useSettingsAddress } from '../settings/address'
import { useWorkspaces } from '../state/workspaces'
import { executionFor } from '../targets/execution'
import { cloudDeviceIn, deviceLabel, targetKindOf } from '../state/devices'
import { pluginEnabled } from './enabled'
import { callFailure } from './errors'
import { PLUGIN_PAGES } from './generated/pages'
import { usePluginStates } from './states'
import { usePluginSettings } from '../settings/plugins'

/**
 * The plugin pages' host in the product. It supplies:
 *
 * - Each plugin's user state, from the product state the sync channel keeps,
 *   which drops a plugin the user turned off.
 * - Its conversation states, by revision.
 * - Its calls, over the plugin call routes. These calls and the states' reads
 *   wait in the HTTP client while the backend cannot be reached.
 * - Its user streams, on a device of the conversation.
 * - What a device of a conversation holds of its packages, from that device's
 *   in the product state, and whether that device is a Cloud that does not
 *   run, from the Cloud's state there.
 * - The shell's own services: the conversations' files, intents and work, and
 *   the settings dialog.
 * - The app's Pinia, which holds the plugins' stores; a plugin the user turns
 *   off has its stores disposed then, and only its own.
 */
/**
 * Opens a tab now, in the click that asked for it, and sends it to `url`
 * once that arrives; a URL that never comes closes it.
 */
async function openLink(url: string | Promise<string>): Promise<void> {
  // Without `noopener` the tab is returned to point it; its opener is
  // dropped before it loads anything.
  const tab = window.open('', '_blank')
  if (tab) {
    tab.opener = null
  }
  try {
    const target = await url
    if (tab) {
      tab.location.href = target
    } else {
      // A browser that refused even the blank tab may still allow a
      // navigation in a new one.
      window.open(target, '_blank', 'noopener,noreferrer')
    }
  } catch (error) {
    tab?.close()
    throw error
  }
}

/** The schemes a page runs or loads itself, which no app opens. */
const PAGE_SCHEMES = new Set(['about', 'blob', 'data', 'file', 'filesystem', 'http', 'https', 'javascript', 'vbscript'])

/**
 * Goes to `url`, a link to an app, as a page's link to one does: the browser
 * hands it to the app, asking first, and the page stays.
 */
function openApp(url: string): void {
  const scheme = /^([a-z][a-z\d+.-]*):/i.exec(url)?.[1]?.toLowerCase()
  if (scheme === undefined || PAGE_SCHEMES.has(scheme)) {
    throw new Error(`Not a link to an app: ${url}`)
  }
  window.location.href = url
}

export function productPageHost(): PageHost {
  const pinia = getActivePinia()
  if (!pinia) {
    throw new Error('the plugin pages\' host is made in the app, which has its Pinia')
  }
  const product = useProduct()
  const work = useWork()
  const resources = useResources()
  const settings = useSettingsAddress()
  const workspaces = useWorkspaces()
  const conversations = useConversations()
  const states = usePluginStates()
  /** The device of a conversation `on` names, its primary device without one, and `plugin`'s packages, once the product state names both. */
  function hostDevice(plugin: string, on: DeviceScope) {
    const state = product.snapshot
    const listed = conversations.items.find((entry) => entry.id === on.conversation && entry.persistence === 'synced')
    if (!state || !listed) {
      return null
    }
    const packages = state.plugins?.find((entry) => entry.id === plugin)?.packages ?? []
    return { deviceId: on.device ?? executionFor(listed).deviceId, packages }
  }
  // Each plugin is watched on its own, so a switch of one leaves the others' stores alone.
  for (const page of PLUGIN_PAGES) {
    watch(
      () => pluginEnabled(product.snapshot, page.plugin),
      (enabled) => {
        if (!enabled) {
          disposePluginStores(pinia, page.plugin)
        }
      },
    )
  }
  return {
    pages: () => PLUGIN_PAGES,
    blockDocument: (plugin, tag, id) => `/api/plugins/${encodeURIComponent(plugin)}/blocks/${encodeURIComponent(tag)}/${encodeURIComponent(id)}`,
    userState: (plugin) => product.snapshot?.pluginStates?.[plugin],
    // On once the backend has it on, as the page shows a switch at once and
    // the backend serves its blocks only after; off at once either way.
    enabled: (plugin) => pluginEnabled(product.snapshot, plugin) && pluginEnabled(product.read, plugin),
    features: (plugin) => product.snapshot?.plugins?.find((entry) => entry.id === plugin)?.features ?? {},
    setFeature: (plugin, feature, enabled) => usePluginSettings().switchFeature(plugin, feature, enabled),
    followState: states.follow,
    async call(plugin, method, params, on, options) {
      const scope = on === null ? '' : `/conversations/${encodeURIComponent(on.conversation)}`
      const device = on?.device == null ? '' : `?device=${encodeURIComponent(on.device)}`
      const path = `${scope}/plugins/${encodeURIComponent(plugin)}/calls/${encodeURIComponent(method)}${device}`
      try {
        // A call that cannot reach the backend waits for it in the HTTP client and goes then
        // (`packages/web-ui/src/plugins/page.ts`).
        const response = await apiRequest(path, {
          method: 'POST',
          signal: options?.signal,
          timeoutMs: options?.timeoutMs,
          ...jsonBody(params),
        })
        return await readResponse(response, z.unknown())
      } catch (error) {
        throw callFailure(error)
      }
    },
    stream(name, on) {
      // Over the direct channel while the device has one.
      return directStream(() => directRouteOn(on.conversation, on.device), name, userStreamOf(on.conversation, name, on.device))
    },
    installed(plugin, on) {
      const host = hostDevice(plugin, on)
      return host ? packageInstalled(product.snapshot, host.deviceId, host.packages) : []
    },
    downloading(plugin, on) {
      const host = hostDevice(plugin, on)
      return host ? packageDownloading(product.snapshot, host.deviceId, host.packages) : []
    },
    deviceStarting(on) {
      const state = product.snapshot
      const listed = conversations.items.find((entry) => entry.id === on.conversation)
      if (!state || !listed || state.cloud === undefined || state.cloud.state === 'running') {
        return false
      }
      // The Cloud starts whether it is the primary device or attached, as a move away from it leaves it.
      const device = on.device ?? executionFor(listed).deviceId
      return device !== null && device === cloudDeviceIn(state.devices)?.id
    },
    deviceName(on) {
      if (on.device !== null) {
        const device = product.snapshot?.devices?.find((each) => each.id === on.device)
        return deviceLabel(targetKindOf(device), device)
      }
      const listed = conversations.items.find((entry) => entry.id === on.conversation)
      return listed ? executionFor(listed).name : deviceLabel('device', null)
    },
    files: conversationFiles,
    intents: {
      open: (conversation, request) => work.openIn(conversation, request),
      canOpen: (intent) => work.canOpen(intent),
    },
    work: {
      tabs: (conversation, kind, device) =>
        work.stateFor(conversation).tabsState.tabs
          .filter((tab) => tab.kind === kind && (device == null || tab.device === device))
          .map((tab) => tab.data),
      // Its window comes to the front on it once the tab is there, as a selection does.
      select: (conversation, _kind, id) => work.request(conversation, id),
      canGo: (conversation, direction) => work.canGo(conversation, direction),
      go: (direction) => work.go(direction),
    },
    workspaces: {
      list: () => workspaces.list.map((workspace) => ({ id: workspace.id, name: workspaceName(workspace) })),
      current: () => workspaces.current,
    },
    devices: {
      list: () =>
        (product.snapshot?.devices ?? [])
          .filter((device) => device.kind === 'user')
          .map((device) => ({
            id: device.id,
            name: device.name,
            state: device.state,
            mark: device.mark,
            downloadPercent: device.downloadPercent,
          })),
    },
    openSettings: (section) => void settings.open(section),
    openLink,
    openApp,
    overlays: appOverlayStore,
    pinia,
  }
}
