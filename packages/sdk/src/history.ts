/**
 * What a conversation holds beyond its lane's window: any agent's
 * transcript by pages, a whole block, a turn, and a command and its output.
 */
import {
  commandOriginSchema,
  endedOutputSchema,
  transcriptPageSchema,
  turnStateSchema,
  wholeBlockSchema,
  type CommandOrigin,
  type EndedOutput,
  type TranscriptPage,
  type TurnState,
  type WholeBlock,
} from '@jush-agent/protocol'
import { query, segment, type CoreRoutes } from './routes'

/** Which page of an agent's transcript to read. */
export type TranscriptPageOptions =
  /** The latest page, or its last `blocks` blocks. */
  | { latest: true; blocks?: number }
  /** The page that ends before the block at position `before`, which is `edge`. */
  | { before: number; edge: string }
  /** The page that starts after the block at position `after`, which is `edge`. */
  | { after: number; edge: string }
  /** The page around the block `around`. */
  | { around: string }

export interface ConversationHistory {
  /** A page of the transcript of `agent`: the conversation's id for the root, a subagent's node id for one of its. */
  transcript(agent: string, page: TranscriptPageOptions): Promise<TranscriptPage>
  /** The block `block` of `agent`'s transcript, whole. */
  block(agent: string, block: string): Promise<WholeBlock>
  /** How the turn `turn` stands. */
  turn(turn: string): Promise<TurnState>
  /** Where the shell call that started the command `command` lies: its agent and its block. */
  commandOrigin(command: string): Promise<CommandOrigin>
  /** What the ended command `command` wrote. */
  commandOutput(command: string): Promise<EndedOutput>
}

export function conversationHistory(routes: CoreRoutes, id: string): ConversationHistory {
  const base = `/conversations/${segment(id)}`
  return {
    transcript: (agent, page) => {
      const values = 'latest' in page
        ? { latest: true, blocks: page.blocks }
        : 'around' in page
          ? { around: page.around }
          : 'before' in page
            ? { before: page.before, edge: page.edge }
            : { after: page.after, edge: page.edge }
      return routes.json('GET', `${base}/agents/${segment(agent)}/transcript${query(values)}`, transcriptPageSchema)
    },
    block: (agent, block) => routes.json('GET', `${base}/agents/${segment(agent)}/blocks/${segment(block)}`, wholeBlockSchema),
    turn: (turn) => routes.json('GET', `${base}/turns/${segment(turn)}`, turnStateSchema),
    commandOrigin: (command) => routes.json('GET', `${base}/commands/${segment(command)}`, commandOriginSchema),
    commandOutput: (command) => routes.json('GET', `${base}/commands/${segment(command)}/output`, endedOutputSchema),
  }
}
