import type { Block } from '@jush-agent/protocol'
import { demoModel, inRequests } from './blocks'

/**
 * A clear as the transcript shows it: right after the user cleared, or with
 * the next message, which the model read as a new conversation's first.
 */
export type ClearCase = 'cleared' | 'next'

const createdAt = '2026-09-24T12:00:00.000Z'

function user(id: string, text: string): Block {
  return { type: 'user', id, turnId: id, createdAt, model: demoModel, content: [{ type: 'text', text }], preamble: null }
}

function answer(id: string, text: string): Block[] {
  return [
    { type: 'text', id, createdAt, endedAt: createdAt, model: demoModel, text, forkable: true },
    {
      type: 'response',
      id: `${id}-usage`,
      createdAt, sentAt: createdAt, firstSentAt: null, firstOutputAt: createdAt,
      model: demoModel,
      usage: { inputTokens: 84_000, outputTokens: 600, cacheReadTokens: 0, cacheWriteTokens: 0 },
    },
  ]
}

/**
 * The release is done and the user clears before an unrelated task. The
 * messages before the divider stay on the page; the model reads only what
 * comes after it.
 */
export function clearTranscript(state: ClearCase): Block[] {
  const blocks: Block[] = [
    user('clear-u1', 'Tag the 2.4 release once the checklist passes.'),
    ...answer('clear-a1', 'Every step of the checklist passed, and `v2.4.0` is tagged.'),
    { type: 'clear', id: 'clear-divider', createdAt, model: demoModel },
  ]
  if (state === 'next') {
    blocks.push(
      user('clear-u2', 'Why does the importer skip empty rows?'),
      ...answer('clear-a2', '`parseRows` drops a row whose cells are all blank before it validates the header.'),
    )
  }
  return inRequests(blocks)
}
