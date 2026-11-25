// Where the views run: the web app's own page, which reaches its server at
// `/api` behind a Jush session and reads the server's whole product state,
// or a program's page, which shows them in Jush's custom element
// (`elements/main.ts`). That reaches one conversation below the address
// where the program's server answers it, reads what the core lets the page
// read of the product state, and does what the program let the page do,
// with no session of Jush's.

import { shallowRef } from 'vue'

/** The program's page the views run in. */
export interface ProgramPage {
  /** The element's `src`, where the program's server answers the page, such as `/chats/42/jush`. */
  src: string
}

// A ref, so what a store computed from it before the elements started, such
// as whose the browser's data is, follows.
const shown = shallowRef<ProgramPage | null>(null)

/** Makes this page a program's, whose server answers it below `src`; once, before the views start. */
export function showInProgramPage(src: string): void {
  shown.value = { src: src.replace(/\/+$/, '') }
}

/** The program's page the views run in, or null in the web app's own. */
export function programPage(): ProgramPage | null {
  return shown.value
}
