import { ref, type Ref } from 'vue'
import { SCREEN_EDGE_PX } from '@jush-ai/web-ui/ui/phone'
import { FRAME_INPUT, USER_ACTION_EVENTS } from '@jush-ai/web-ui/ui/user-input'

/**
 * Whether the user attends the page: from their first action on it while it
 * is visible and focused, a click or a tap, a key or a wheel, until it is
 * hidden or loses the focus. The conversation the page shows is read only
 * while it is attended, so what arrived while the user was away keeps its
 * marks until they act. Coming back to the page is no action, nor is the
 * pointer's move over it on the way.
 *
 * A focus lost from within a frame of the page tells the page's window
 * nothing, so whoever reads it asks `attending`, which checks the page is
 * still in front.
 *
 * A conversation the sidebar's unread entry shows is glanced at: the page is
 * not attended as it shows, and while it glances only an action in the
 * conversation attends it (`inConversation`), which ends the glance; an
 * action in the sidebar, a menu or a dialog over the page, or the
 * conversation's header bar does not, so the entry's next click leaves the
 * glanced one unread. Opening a conversation any other way ends the glance
 * (`opened`, `shown`). The entry's own click never attends the page, so it
 * leaves the conversation shown before it unread as well.
 */
export const attended: Ref<boolean> = ref(false)

/** The conversation the unread entry shows, until an action outside the sidebar or another conversation shows. */
let glancing: string | null = null

/** A notification was clicked while the page was not in front, which its worker brings it to next. */
let attendWhenInFront = false

/** Whether the page is visible and its window, or a frame within it, has the focus. */
function inFront(): boolean {
  return document.visibilityState === 'visible' && document.hasFocus()
}

/** Whether the user attends the page now; a page no longer in front stops being attended. */
export function attending(): boolean {
  if (attended.value && !inFront()) {
    attended.value = false
  }
  return attended.value
}

/** The user acted on the page, such as by changing the conversation it shows: it is attended while in front. */
export function attend(): void {
  if (inFront()) {
    attended.value = true
  }
}

/** The unread entry is about to show the conversation `id`, without reading it. */
export function glance(id: string): void {
  glancing = id
  attended.value = false
}

/** Whether the page glances at the conversation `id`, which its composer then leaves the focus for. */
export function glancingAt(id: string): boolean {
  return glancing === id
}

/** The user opens the conversation `id` from its row, which reads it, the one glanced at too. */
export function opened(id: string): void {
  if (glancing === id)
    glancing = null
  attend()
}

/**
 * The page shows the conversation `id`: the user's action, which attends the
 * page, unless the unread entry showed it.
 */
export function shown(id: string | null): void {
  if (id !== null && id === glancing)
    return
  glancing = null
  attend()
}

/** The user clicked a notification, which acts on another surface: the page is attended once it comes in front. */
export function attendNotification(): void {
  if (inFront()) {
    attended.value = true
  } else {
    attendWhenInFront = true
  }
}

/**
 * Whether `event` acts in the conversation: in the main pane, outside its
 * header bar, and on a phone not the swipe from the screen's edge that draws
 * the sidebar's drawer in. A menu or a dialog sits outside the main pane.
 */
function inConversation(event: Event): boolean {
  const target = event.target
  if (!(target instanceof Element) || !target.closest('main') || target.closest('header'))
    return false
  const x = event instanceof TouchEvent
    ? event.touches[0]?.clientX
    : event instanceof PointerEvent && event.pointerType === 'touch' ? event.clientX : undefined
  return x === undefined || x > SCREEN_EDGE_PX
}

/** Follows the user's actions on the page and its leaving, until `signal` aborts. */
export function followAttention(signal: AbortSignal): void {
  /** An action, `inside` the conversation or not. */
  const act = (inside: boolean): void => {
    if (glancing !== null && !inside)
      return
    if (inFront()) {
      glancing = null
      attended.value = true
    }
  }
  const actOn = (event: Event): void => {
    // The unread entry's click shows another conversation and reads none.
    if (event.target instanceof Element && event.target.closest('[data-unread-entry]'))
      return
    act(inConversation(event))
  }
  const front = (): void => {
    if (attendWhenInFront && inFront()) {
      attendWhenInFront = false
      attended.value = true
    }
  }
  for (const action of USER_ACTION_EVENTS) {
    window.addEventListener(action, actOn, { capture: true, passive: true, signal })
  }
  window.addEventListener(FRAME_INPUT, (event) => {
    // The page's frames, the composer's and the windows', are in the
    // conversation.
    if ((USER_ACTION_EVENTS as readonly string[]).includes((event as CustomEvent<string>).detail)) {
      act(true)
    }
  }, { signal })
  window.addEventListener('focus', front, { signal })
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState !== 'visible') {
      attended.value = false
    }
    front()
  }, { signal })
  // The window blurs when the focus moves into a frame of the page too,
  // which leaves the page in front; the document knows once the move ends.
  window.addEventListener('blur', () => {
    setTimeout(() => {
      if (!inFront()) {
        attended.value = false
      }
    })
  }, { signal })
}
