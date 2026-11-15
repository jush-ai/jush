import { computed, nextTick, watch } from 'vue'
import { sentMessage } from './message-delivery'
import type { MessageListBlock } from './pending-steers'

/**
 * A message the reader just sent scrolls the transcript to the bottom, even
 * when they had scrolled up to read: the list follows what they did, the way a
 * chat does. The reply that follows is tracked only while they stay at the
 * bottom, which is the scroller's own rule. Every transcript scroller applies
 * this one, so a send reads the same in the product and the gallery.
 *
 * A message is not just sent in these cases:
 *
 * - a message read from a page of history, in its light form, wherever the page
 *   puts it, or whole, when it arrives above the rows the list held: the page
 *   before the window, which the reader asked for by scrolling up. The window
 *   of a long run of work may hold no message, so a message read above it is
 *   told by the window's first row of content, not by its messages; the row
 *   that shows a page coming is no content and stays at the top.
 * - a message, or a row of the turn's tail, that comes in while the list
 *   shows or has just stopped showing a window before the latest: the page
 *   after the window, read as the reader scrolls down, and the tail that
 *   shows again once that page reaches the end.
 * - a message not delivered, which shows where it was sent once the window that
 *   holds that place is read, and was on its way, a row already, when it was
 *   sent
 *
 * A message is any row that shows one at any stage of its delivery
 * (`sentMessage`), named by `keyOf` across its stages and forms, so the server
 * confirming or starting a message sent, or a page reading it whole, is not
 * another send.
 */
export function useFollowSentMessages(
  blocks: () => readonly MessageListBlock[],
  keyOf: (block: MessageListBlock) => string,
  latest: () => boolean,
  scrollToBottom: () => void,
): void {
  const rows = computed(() => blocks().map((block) => ({
    key: keyOf(block),
    content: block.type !== 'history_edge',
    message: sentMessage(block) !== undefined && !('light' in block && block.light) && !undelivered(block),
  })))
  watch(() => [rows.value, latest()] as const, ([now, shownLatest], [before, shownLatestBefore]) => {
    if (!shownLatest || !shownLatestBefore) {
      return
    }
    const held = new Set(before.filter((row) => row.content).map((row) => row.key))
    const heldMessages = new Set(before.filter((row) => row.message).map((row) => row.key))
    // A message sent comes in below the messages the list held and below its
    // first row, which a page read above the window leaves in place.
    const below = Math.max(
      now.findIndex((row) => held.has(row.key)),
      now.findLastIndex((row) => heldMessages.has(row.key)),
    )
    if (now.some((row, index) => row.message && index > below && !held.has(row.key))) {
      void nextTick(scrollToBottom)
    }
  }, { flush: 'post' })
}

/** A message on its way whose delivery failed. */
function undelivered(block: MessageListBlock): boolean {
  return block.type === 'pending_submission' && block.submission.error !== null
}
