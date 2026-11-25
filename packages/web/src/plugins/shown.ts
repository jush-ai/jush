import type { AnyPluginPage } from '@jush-agent/plugin-sdk'
import { mayOpen } from '@jush-agent/web-ui/agent/page-operations'
import { PLUGIN_PAGES } from './generated/pages'

/**
 * The plugins' pages as a conversation's work uses them: on a program's
 * page, without the kinds of the windows the program did not let it open,
 * so neither the taskbar nor a link opens them.
 */
export function shownPages(): readonly AnyPluginPage[] {
  return PLUGIN_PAGES.map((page) => {
    const kinds = page.kinds ?? []
    return kinds.every((kind) => mayOpen(kind.window.id)) ? page : { ...page, kinds: kinds.filter((kind) => mayOpen(kind.window.id)) }
  })
}
