/**
 * A program's page of one conversation, which `@jush-agent/web-components`'
 * `<jush-conversation src>` shows: the program's server answers it below
 * `src`, after its own sign-in, by handing each request here.
 *
 * ```text
 * page --> /chats/42/jush/... --> program's server --> page.fetch / accept
 *                                                      |
 *          /cores/{core}/pages/{conversation}/... <----+  + its operations
 * ```
 *
 * The core answers the page as one of that conversation alone, which does
 * what `allow` names and nothing else (`crates/core-api/src/page.rs`). The
 * paths below `src` are the element's and the core's: the program passes
 * them on without reading them.
 */
import type { PageOperation } from '@jush-agent/protocol'
import type { AcceptedSocket } from './core'
import { below, PROCESS_ORIGIN, segment, type CoreRoutes } from './routes'
import { StreamSocket } from './socket'

/** The header that names the page's operations on the process's channel. */
const OPERATIONS_HEADER = 'jush-page-operations'

export type { PageOperation }

/** A conversation's page as the program binds it. */
export interface PageOptions {
  /** The element's `src`, where the program's server answers the page, such as `/chats/42/jush`. */
  base: string
  /**
   * What the page may do beyond reading its conversation, such as `send`
   * or `files`; the page shows no control of the rest, and the core
   * refuses it. An operation a later Jush adds is on no page until it is
   * named here.
   */
  allow: readonly PageOperation[]
}

/** A program's page of one conversation, which holds nothing: make one for each request. */
export class ConversationPage {
  private readonly operations: string

  constructor(
    private readonly routes: CoreRoutes,
    private readonly conversation: string,
    private readonly options: PageOptions,
  ) {
    this.operations = options.allow.join(',')
  }

  /** Answers one of the page's requests below `base`. */
  fetch(request: Request): Promise<Response> {
    const url = new URL(request.url)
    const forwarded = new Request(PROCESS_ORIGIN + this.path(below(url.pathname, this.options.base)) + url.search, request)
    // The page's own header, if it sent one, is not its operations.
    forwarded.headers.set(OPERATIONS_HEADER, this.operations)
    return this.routes.process.fetch(forwarded)
  }

  /**
   * Carries the page's connection, `socket`, opened at `url` below `base`,
   * to the core, both ways, until either end closes; `socket` is a
   * WebSocket of the standard interface, as Deno's `upgradeWebSocket` and
   * the `ws` package give.
   */
  accept(socket: AcceptedSocket, url: string | URL): void {
    const query = new URL(url, PROCESS_ORIGIN).search
    const core = new StreamSocket(this.routes.process.connect(this.path(`/connect${query}`), 'websocket', {
      'sec-websocket-version': '13',
      [OPERATIONS_HEADER]: this.operations,
    }))
    const early: (string | ArrayBuffer | Uint8Array)[] = []
    let open = false
    let pageClosed = false
    socket.binaryType = 'arraybuffer'
    socket.addEventListener('message', (event) => {
      const data = event.data as string | ArrayBuffer | Uint8Array
      if (open) {
        core.send(data)
      } else {
        early.push(data)
      }
    })
    socket.addEventListener('close', () => {
      pageClosed = true
      core.close()
    })
    core.addEventListener('open', () => {
      open = true
      for (const data of early.splice(0)) {
        core.send(data)
      }
    })
    core.addEventListener('message', (event) => socket.send(event.data))
    core.addEventListener('close', (event) => {
      if (!pageClosed) {
        socket.close(event.code === 1006 || event.code === 1005 ? 1011 : event.code, event.reason)
      }
    })
  }

  /** The process's path of the page's route `path`. */
  private path(path: string): string {
    return `/cores/${segment(this.routes.id)}/pages/${segment(this.conversation)}${path}`
  }
}
