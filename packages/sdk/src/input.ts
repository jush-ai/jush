/** What a program sends a conversation: text, or content blocks. */
import type { ClientContent } from '@jush-agent/protocol'

export type { MessageOptions } from '@jush-agent/conversation-client'

/** What a program sends: text, or the content blocks a page sends. */
export type Input = string | ClientContent[]

/** `input` as content blocks. */
export function content(input: Input): ClientContent[] {
  return typeof input === 'string' ? [{ type: 'text', text: input }] : input
}
