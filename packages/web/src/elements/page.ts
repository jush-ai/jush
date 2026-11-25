// The program's page Jush's element runs in, as the web app's window is for
// its own views (`main.ts`), made by the element as it connects.
//
// - One conversation: the page's one `<jush-conversation>` reaches it below
//   its `src`, where the program's server answers the page
//   (`crates/core-api/src/page.rs`), through the page's one connection. Its
//   first read, `GET {src}/page`, names the conversation and what the page
//   may do, which its views' controls follow (`may`), read again until it
//   is answered. `src` is read once: another conversation is another page,
//   or a frame. An element that takes the place of one the page removed, as
//   a framework draws it again, shows the same conversation.
// - One store of each kind, one router and one text instance, which the
//   element's Vue app installs. The router's address is the conversation's,
//   with the tab of its window in front in the query. A navigation
//   elsewhere, a link to another conversation or to settings, is the
//   program's to carry out: the element dispatches a `navigate` event with
//   the address the web app would show, and the router stays.
// - What the web app's window has once around its views, its menus, toasts,
//   media viewer and dialogs (`PageHosts.vue`).
// - The user's attention to the page, which marks the conversation read, and
//   the file tree's choice, which the user's preferences keep, as the web
//   app's window follows them.
//
// ```text
// <jush-conversation src=/chats/42/jush> --> GET /chats/42/jush/page
//                                            GET /chats/42/jush/state
//                                            WS  /chats/42/jush/connect
//                                        --> navigate event
// ```

import { effectScope, ref, shallowReadonly, shallowRef, type Ref } from 'vue'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { pageAnswerSchema } from '@jush-agent/protocol'
import { allowOperations } from '@jush-agent/web-ui/agent/page-operations'
import { configurePageConnection } from '@jush-agent/web-ui/transport/page-connection'
import { waitToReconnect } from '@jush-agent/web-ui/transport/liveness'
import { createPageI18n, setPageLanguage } from '@jush-agent/web-ui/i18n/text'
import { lastShownLanguage, resolveLanguage } from '@jush-agent/web-ui/i18n/languages'
import { errorFields } from '@jush-agent/web-ui/infra/errors'
import { pageLog } from '@jush-agent/web-ui/infra/log'
import { apiRequest, apiUrl, readResponse } from '../api/client'
import { useConversations } from '../conversation/store'
import { keepFileTree } from '../conversation/file-tree'
import { shownConversation, type ShownConversation } from '../conversation/shown'
import { useWork } from '../conversation/work'
import { programPage, showInProgramPage } from '../program-page'
import { SETTINGS_ROUTE } from '../settings/address'
import { followAttention } from '../state/attention'
import { followAppearance, followLanguage } from '../state/look'
import { usePreferences } from '../state/preferences'
import { useProduct } from '../state/product'
import { useResources } from '../state/resources'

/** What the element's app installs before it knows its conversation. */
export interface ElementsApps {
  pinia: Pinia
  router: Router
  i18n: ReturnType<typeof createPageI18n>
}

/** The page's element and its conversation. */
export interface ElementsPage extends ElementsApps {
  shown: ShownConversation
  /** The page's element, the last one that connected. */
  element: HTMLElement
  /** The conversation the page shows, once its program's server named it. */
  conversation: Readonly<Ref<string | null>>
  /** The page could not learn its conversation: its program's server refused it, or answered no page. */
  failed: Readonly<Ref<boolean>>
}

/** The route of every address: the element draws its views itself, and the router holds the address. */
const NOWHERE = { render: () => null }

let apps: ElementsApps | null = null
let page: ElementsPage | null = null

/** What the element's app installs, made once. */
export function elementsApps(): ElementsApps {
  if (apps) {
    return apps
  }
  const languages = navigator.languages
  const start = lastShownLanguage() ?? resolveLanguage('system', languages)
  const i18n = createPageI18n(start)
  setPageLanguage(start)
  const pinia = createPinia()
  setActivePinia(pinia)
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/chat/:id?', component: NOWHERE },
      { path: '/settings/:section?/:detail?', name: SETTINGS_ROUTE, component: NOWHERE },
      { path: '/:pathMatch(.*)*', component: NOWHERE },
    ],
  })
  apps = { pinia, router, i18n }
  return apps
}

/**
 * The page, made by its element `element` as it connects with `src`, where
 * the program's server answers the page. A second element, or another `src`,
 * is refused: the page shows one conversation.
 */
export function elementsPage(src: string | undefined, element: HTMLElement): ElementsPage {
  if (src === undefined) {
    throw new Error('Jush\'s element needs its `src` attribute: where the program\'s server answers the page, such as /chats/42/jush')
  }
  if (page) {
    if (page.element !== element && page.element.isConnected) {
      throw new Error('A page shows one conversation, in one <jush-conversation>: show another in a frame')
    }
    if (programPage()?.src !== src.replace(/\/+$/, '')) {
      throw new Error(`A <jush-conversation>'s src is read once: ${src} is another page's, shown in another page or frame`)
    }
    page.element = element
    return page
  }
  const { pinia, router, i18n } = elementsApps()
  showInProgramPage(src)
  // The program's server decides who reaches the page: a refused upgrade is
  // its answer, which the connection's state shows, with no session to check.
  configurePageConnection({ url: () => apiUrl('/connect'), failedToOpen: () => {} })
  const shownId = ref<string | null>(null)
  const failed = ref(false)
  router.beforeEach((to) => {
    if (shownId.value !== null && to.path === `/chat/${shownId.value}`) {
      return true
    }
    page?.element.dispatchEvent(new CustomEvent('navigate', { detail: { address: to.fullPath }, bubbles: true }))
    return false
  })
  const conversations = useConversations(pinia)
  // The page's own scope: what follows the conversation lives as long as
  // the page.
  const scope = effectScope(true)
  const shown = scope.run(() => {
    useWork(pinia).attach(router)
    keepFileTree()
    const product = useProduct(pinia)
    const preferences = usePreferences(pinia)
    followAppearance(useResources(pinia))
    followLanguage(product, preferences, shallowRef(navigator.languages))
    // The page lives as long as the window.
    followAttention(new AbortController().signal)
    conversations.initialize()
    return shownConversation(shownId, router)
  })!
  const read = async (failures: number): Promise<void> => {
    try {
      const answer = await readResponse(await apiRequest('/page'), pageAnswerSchema)
      allowOperations(answer.allow)
      failed.value = false
      shownId.value = answer.conversation
      await router.replace(`/chat/${answer.conversation}`)
    } catch (error) {
      failed.value = true
      pageLog.warn('program_page.unread', errorFields(error))
      waitToReconnect(failures + 1, () => void read(failures + 1))
    }
  }
  void read(0)
  page = {
    pinia,
    router,
    i18n,
    shown,
    element,
    conversation: shallowReadonly(shownId),
    failed: shallowReadonly(failed),
  }
  return page
}
