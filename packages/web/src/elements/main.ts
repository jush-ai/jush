// `@jush-agent/web-components`: the web app's conversation view, with its
// work's windows and taskbar, as a standard custom element, a second entry
// of the web app's build, for a program's page in any framework, or none:
//
// ```html
// <link rel="stylesheet" href="/assets/jush-web-components.css" />
// <script type="module" src="/assets/jush-web-components.js"></script>
// <jush-conversation src="/chats/42/jush"></jush-conversation>
// ```
//
// It shows the one conversation the program's server answers below `src`,
// by handing each request to `@jush-agent/sdk`'s `conversation.page()`, with
// the controls of what the program let the page do (`page.ts`). It renders
// in the page's own tree, without a shadow root, so its menus and dialogs,
// which go to the body, keep the styles the stylesheet gives.

import { defineCustomElement, type App } from 'vue'
import ConversationElement from './ConversationElement.vue'
import { elementsApps } from './page'
import '../style.css'

/** Installs the page's stores, router and text in an element's own app. */
function configureApp(app: App): void {
  const { pinia, router, i18n } = elementsApps()
  app.use(pinia).use(router).use(i18n)
}

export const JushConversation = defineCustomElement(ConversationElement, { shadowRoot: false, configureApp })

if (!customElements.get('jush-conversation')) {
  customElements.define('jush-conversation', JushConversation)
}
