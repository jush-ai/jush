/**
 * Debug This Page in the product: the page's debug lane, which holds the link
 * an agent runs code in this tab through (`crates/web-api-protocol/src/debug.rs`).
 *
 * ```text
 *  menu item --> dialog --> lane { kind: 'debug', resume } --> started { token }
 *                                     ^                              |
 *   reload, lost connection ----------'          sessionStorage <----'
 * ```
 *
 * - Debug This Page opens the dialog and, unless the page is debugged
 *   already, a lane that makes a new link.
 * - The tab keeps the link's token in `sessionStorage`, so a page that
 *   follows in the tab, after a reload, a navigation or a discard, resumes
 *   the link as it loads. A closed tab the browser reopens later, and a copy
 *   of a tab, restore that storage too, and drop the token (`continuesTab`).
 * - A lane the connection lost opens again with the token. One the backend
 *   refuses, or closes as the link is stopped, expires or moves to another
 *   tab, ends the page's debugging: the token and the mark go.
 * - The backend's `run` is the body of an async function; what it returns
 *   goes back as JSON (`plain`), and what it throws as an error.
 */
import { computed, ref, shallowRef } from 'vue'
import {
  DEBUG_ROUTE,
  MAX_PAGE_MESSAGE_BYTES,
  debugToPageSchema,
  schemaRefusal,
  type DebugFromPage,
  type PageFacts,
} from '@jush-ai/protocol'
import { useCopy } from '@jush-ai/web-ui/composables/useCopy'
import { coded, environmental, reportError } from '@jush-ai/web-ui/infra/errors'
import { PAGE_ID } from '@jush-ai/web-ui/infra/log'
import type { DebugPhase } from '@jush-ai/web-ui/sidebar/debug-page'
import { t } from '@jush-ai/web-ui/sidebar/locales/text'
import { waitToReconnect, type ReconnectWait } from '@jush-ai/web-ui/transport/liveness'
import { connectionFailures, openLane, type Lane, type LaneRefusal } from '@jush-ai/web-ui/transport/page-connection'
import { outcome, wellFormed } from './written'

/** The `sessionStorage` key that keeps the token of the tab's link. */
const TOKEN_KEY = 'jush.debugLink'

/** The `sessionStorage` key that keeps when the tab's last page left it. */
const LEFT_KEY = 'jush.debugLeft'

/**
 * How soon after the tab's last page left a new page of it goes on with the
 * link: a reload or a navigation in the tab loads the app well within it,
 * even on a slow network, while a closed tab the browser reopens, which
 * restores its `sessionStorage` too, usually comes later.
 */
const CONTINUE_MS = 30_000

/** How long Stop Debugging waits for the backend to end the link before it lets go of it here. */
const STOP_WAIT_MS = 10_000

/**
 * The closes after which the link is over for this tab. A lane closed for a
 * message one side could not read opens again: the link outlives a defect.
 */
const LINK_OVER = new Set(['debug_stopped', 'debug_expired', 'debug_moved'])

const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor as new (body: string) => () => Promise<unknown>

/** The page's answer to the run `id` of `source`, within what a page sends in one message. */
async function run(id: number, source: string): Promise<string> {
  let result: { returned: unknown } | { thrown: unknown }
  try {
    result = { returned: await new AsyncFunction(source)() }
  } catch (thrown) {
    result = { thrown }
  }
  const text = JSON.stringify({ type: 'ran', id, outcome: outcome(result) } satisfies DebugFromPage)
  // The writing counts characters; the message's limit is in UTF-8 bytes.
  const bytes = new TextEncoder().encode(text).length
  return bytes > MAX_PAGE_MESSAGE_BYTES
    ? JSON.stringify({ type: 'ran', id, outcome: { kind: 'too_large', bytes } } satisfies DebugFromPage)
    : text
}

/** What the page is now. */
function facts(): PageFacts {
  return {
    id: PAGE_ID,
    url: wellFormed(`${location.pathname}${location.search}${location.hash}`),
    title: wellFormed(document.title),
    visible: document.visibilityState === 'visible',
    build: import.meta.env.JUSH_VERSION,
  }
}

/** The backend's refusal of the lane, as the page shows a failure. */
function refused(refusal: LaneRefusal): Error {
  // A refusal's params are each a number or a string (`ErrorBody`).
  const params = Object.fromEntries(
    Object.entries(refusal.error.params ?? {}).filter((entry): entry is [string, string | number] =>
      typeof entry[1] === 'string' || typeof entry[1] === 'number'),
  )
  const error = coded(new Error(refusal.error.message), refusal.error.code, params)
  return Object.assign(error, { requestId: refusal.error.requestId })
}

/**
 * Whether this page goes on with the tab's link: the tab's last page left it
 * just before, as a reload or a navigation in the tab does, or the browser
 * discarded the tab and loads it again. Answers false for a closed tab the
 * browser reopened later, and for a copy of a tab.
 */
function continuesTab(): boolean {
  const left = Number(sessionStorage.getItem(LEFT_KEY))
  sessionStorage.removeItem(LEFT_KEY)
  const discarded = 'wasDiscarded' in document && document.wasDiscarded === true
  return discarded || (left > 0 && Date.now() - left < CONTINUE_MS)
}

function create() {
  // Decided once, as the page loads, whether or not it is signed in yet.
  if (!continuesTab())
    sessionStorage.removeItem(TOKEN_KEY)
  addEventListener('pagehide', () => {
    if (sessionStorage.getItem(TOKEN_KEY) !== null)
      sessionStorage.setItem(LEFT_KEY, String(Date.now()))
  })
  /** Where the page's debugging stands; null while it is not debugged. */
  const phase = shallowRef<DebugPhase | null>(null)
  /** What the dialog shows: the last phase, which stays while it closes as debugging ends. */
  const shown = computed<DebugPhase>((last) => phase.value ?? last ?? { status: 'starting' })
  const isOpen = ref(false)
  /** The lane that holds the link, or makes it. */
  let lane: Lane | null = null
  /** The wait before the lane opens again, after the connection was lost. */
  let waiting: ReconnectWait | null = null
  /** Resolves as the lane closes after Stop Debugging. */
  let stopped: (() => void) | null = null
  const url = computed(() => (phase.value?.status === 'on' ? phase.value.url : undefined))
  const { copy, copied } = useCopy(() => url.value)

  /** The page is no longer debugged. */
  function end(): void {
    sessionStorage.removeItem(TOKEN_KEY)
    phase.value = null
    isOpen.value = false
  }

  function receive(from: Lane, data: unknown): void {
    let value: unknown = null
    try {
      value = typeof data === 'string' ? JSON.parse(data) : null
    } catch {
      // Read as no message below.
    }
    const parsed = debugToPageSchema.safeParse(value)
    if (!parsed.success) {
      reportError(t('debug.unreadable'), schemaRefusal(parsed.error), { site: 'debug.unreadable' })
      from.close(1003, 'invalid_message')
      return
    }
    const message = parsed.data
    switch (message.type) {
      case 'started':
        sessionStorage.setItem(TOKEN_KEY, message.token)
        phase.value = {
          status: 'on',
          url: `${location.origin}/api${DEBUG_ROUTE}/${message.token}`,
          expiresAt: new Date(message.expiresAt),
        }
        break
      case 'run':
        void run(message.id, message.source).then((answer) => from.send(answer))
        break
      case 'describe':
        from.send(JSON.stringify({ type: 'described', id: message.id, page: facts() } satisfies DebugFromPage))
        break
    }
  }

  function closed(from: Lane, event: CloseEvent): void {
    if (lane !== from)
      return
    lane = null
    if (stopped !== null) {
      stopped()
      return
    }
    const kept = sessionStorage.getItem(TOKEN_KEY)
    if (from.refusal !== null) {
      // A link the tab kept is over; a new one could not be made.
      if (kept !== null)
        end()
      else
        phase.value = { status: 'failed', error: refused(from.refusal) }
      return
    }
    if (LINK_OVER.has(event.reason)) {
      end()
      return
    }
    if (kept !== null) {
      // The connection was lost: the link waits for the tab, which opens
      // the lane again after the connection's own wait.
      waiting = waitToReconnect(Math.max(1, connectionFailures.value), () => {
        waiting = null
        connect(kept)
      })
      return
    }
    phase.value = {
      status: 'failed',
      error: environmental(coded(new Error('The debug lane closed before its link was made'), 'unreachable')),
    }
  }

  function connect(resume: string | null): void {
    const opened = openLane({ kind: 'debug', resume }, 'debug lane')
    lane = opened
    opened.addEventListener('message', (event) => receive(opened, event.data))
    opened.addEventListener('close', (event) => closed(opened, event))
  }

  /** Lets go of the lane and of a wait to open it again. */
  function release(): void {
    waiting?.cancel()
    waiting = null
    const holding = lane
    lane = null
    holding?.close()
  }

  function start(): void {
    release()
    phase.value = { status: 'starting' }
    connect(null)
  }

  return {
    shown,
    isOpen,
    copied,
    copy,
    /** The page is debugged: an agent may run code in it. */
    debugging: computed(() => phase.value?.status === 'on'),

    /** Debug This Page: the dialog, and a new link unless the page has one. */
    open(): void {
      isOpen.value = true
      if (phase.value === null || phase.value.status === 'failed')
        start()
    },
    close(): void {
      isOpen.value = false
    },
    retry: start,

    /**
     * Lets go of the link as the page signs out: the backend's lane ended
     * with the session, and a lane that waited to resume it would keep the
     * connection trying without one.
     */
    forget(): void {
      release()
      if (phase.value !== null)
        end()
    },

    /** Resumes the link this tab kept, as the signed-in page loads (`continuesTab`). */
    resume(): void {
      const kept = sessionStorage.getItem(TOKEN_KEY)
      if (kept === null || lane !== null || waiting !== null)
        return
      phase.value = { status: 'starting' }
      connect(kept)
    },

    /** Stop Debugging: ends the link, or lets go of it here once the backend does not answer. */
    async stop(): Promise<void> {
      const holding = lane
      if (holding !== null) {
        await new Promise<void>((done) => {
          const timer = setTimeout(done, STOP_WAIT_MS)
          stopped = () => {
            clearTimeout(timer)
            done()
          }
          holding.send(JSON.stringify({ type: 'stop' } satisfies DebugFromPage))
        })
        stopped = null
      }
      release()
      end()
    },
  }
}

let debugging: ReturnType<typeof create> | null = null

/** The page's debugging, made at its first use, in the setup of the app. */
export function usePageDebugging(): ReturnType<typeof create> {
  debugging ??= create()
  return debugging
}
