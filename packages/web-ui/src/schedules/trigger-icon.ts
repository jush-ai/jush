import { Binoculars, Calendar, MessagesSquare, Timer, Webhook } from '@lucide/vue'
import type { Component } from 'vue'
import type { Trigger } from './types'

/**
 * The icon of a trigger's kind, never of how often it wakes the conversation:
 *
 * - a time trigger that repeats, whether by a cycle, every so many minutes or
 *   cron: a calendar;
 * - a time trigger that wakes once: a timer;
 * - a command's output that changes: binoculars, which watch;
 * - a URL: the webhook;
 * - another conversation's finish: speech bubbles, a conversation's.
 */
export function triggerIcon(trigger: Trigger): Component {
  if (trigger.kind === 'change') {
    return Binoculars
  }
  if (trigger.kind === 'url') {
    return Webhook
  }
  if (trigger.kind === 'done') {
    return MessagesSquare
  }
  return trigger.timing.kind === 'once' ? Timer : Calendar
}
