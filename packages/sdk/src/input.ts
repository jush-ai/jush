/** What a program sends a conversation: text, or content blocks. */
import type { ClientContent } from '@jush-agent/protocol'

/** What a program sends: text, or the content blocks a page sends. */
export type Input = string | ClientContent[]

/** Who sent a message or a steer. */
export interface MessageOptions {
  /**
   * Your own id of whoever sent it, such as a person of a team's chat, 1 to
   * 255 characters, which the message keeps: every call its agent makes
   * after reading it, until the next message or the next turn the end of
   * other work opens, is that sender's in `core.usage.report`, and so are
   * the calls of the subagents it starts meanwhile, and of the turn the end
   * of its work opens. The message's id names the assignment those calls
   * belong to, as `assignment` groups them. The model does not read it.
   */
  senderId?: string
}

/** `input` as content blocks. */
export function content(input: Input): ClientContent[] {
  return typeof input === 'string' ? [{ type: 'text', text: input }] : input
}
