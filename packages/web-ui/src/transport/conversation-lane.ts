import { createConversationClient as onConnection, type ConversationClient, type ConversationClientOptions } from '@jush-ai/conversation-client'
import { pageConnection } from './page-connection'

export { ConversationLaneError } from '@jush-ai/conversation-client'

/**
 * The client of the conversation `conversation`, over its lane of the page
 * connection (`@jush-ai/conversation-client`'s `createConversationClient`).
 */
export function createConversationClient(
  conversation: string,
  options: ConversationClientOptions = {},
  before: () => Promise<unknown> | undefined = () => undefined,
): ConversationClient {
  return onConnection(pageConnection, conversation, options, before)
}
