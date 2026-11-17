/**
 * The `browser` page's store (`packages/web-ui/src/plugins/stores.ts`): each
 * conversation's browser on each of its devices on the page (`./tabs.ts`),
 * kept while the plugin is on, by conversation and device (`browserKey`).
 *
 * A browser tab's content watches its conversation's browser while it is
 * mounted, and a tab's title or icon reaches it without watching. One no
 * content watches is kept, with its view's connection and each tab's last
 * picture, so a return to the conversation shows them at once; past the
 * limit, the least recently watched is released.
 */
import { effectScope, onScopeDispose, watch } from 'vue'
import { KeptSessions, definePluginStore, type KeptSession, type PageContext, type PluginStore } from '@jush-agent/plugin-sdk'
import { browserTabsApi } from '../tabs'
import { BrowserTabsController, browserTabDataSchema, type BrowserTabsOptions } from './tabs'

/** The kind's name, which the page's tabs and the plugin's work tabs carry. */
export const BROWSER_TAB_KIND = 'browser'

/** The store's key of the conversation's browser on `device`, its primary device's for null. */
export function browserKey(conversation: string, device: string | null): string {
  return `${conversation}\n${device ?? ''}`
}

/** One conversation's browser as the store keeps it. */
export interface KeptBrowser extends KeptSession {
  readonly tabs: BrowserTabsController
}

export type BrowserStore = PluginStore<KeptSessions<KeptBrowser>>

/** The store of a page whose browsers take `options`. */
export function browserStore(options: BrowserTabsOptions): BrowserStore {
  return definePluginStore('browsers', (page) => {
    const browsers = new KeptSessions((key) => {
      const [conversation = '', device = ''] = key.split('\n')
      return keptBrowser(page, conversation, device === '' ? null : device, options)
    })
    // The store ends as its plugin is turned off.
    onScopeDispose(() => browsers.dispose())
    return browsers
  })
}

function keptBrowser(page: PageContext, conversation: string, device: string | null, options: BrowserTabsOptions): KeptBrowser {
  // Its own scope, which ends when the store releases it rather than with whatever made it.
  const scope = effectScope(true)
  const held = () => page.work.tabs(conversation, BROWSER_TAB_KIND, device).flatMap((data) => {
    const parsed = browserTabDataSchema.safeParse(data)
    return parsed.success ? [parsed.data.tab] : []
  })
  const tabs = scope.run(() => {
    const controller = new BrowserTabsController(
      browserTabsApi(
        page.plugin.conversation(conversation, device),
        device,
        (workTab) => page.work.select(conversation, BROWSER_TAB_KIND, workTab),
        held,
      ),
      page.errors,
      options,
    )
    // The work's tabs end the openings whose tabs it held.
    watch(held, () => controller.settleOpenings())
    return controller
  })!
  return {
    tabs,
    // What its view watches follows its contents' show and hide (`BrowserTabsController.show`).
    watched: () => {},
    dispose: () => {
      tabs.dispose()
      scope.stop()
    },
  }
}
