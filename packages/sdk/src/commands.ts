/**
 * The core's records across its conversations: the commands its agents ran,
 * as the core's command history keeps them, and every conversation's history
 * as the core's search index holds it.
 */
import {
  commandHistorySchema,
  historyHitsSchema,
  type CommandHistory,
  type HistoryHits,
  type SearchKind,
} from '@jush-agent/protocol'
import { query, type CoreRoutes } from './routes'

/** Where a read of the command history starts, and how many rows it takes. */
export interface CommandHistoryOptions {
  /** After the row a page's `next` named. */
  after?: string
  /** At the first row that ended at or after this moment; not with `after`. */
  from?: Date
  /** From 1 to 1,000 rows, 100 by default. */
  limit?: number
}

export interface CoreCommands {
  /**
   * The core's command history in the order its rows were written, which is
   * the order their commands ended: each command its agents' `shell` calls
   * started, once it ended, and each such call that returned an error before
   * its command started. Pass a page's `next` as `after` to read on.
   */
  history(options?: CommandHistoryOptions): Promise<CommandHistory>
}

/** What a search of every conversation's history keeps, and where its page starts. */
export interface HistorySearchOptions {
  /** A text that holds any of the words, rather than every one. */
  any?: boolean
  /** The kinds of text searched; all three by default. */
  kinds?: SearchKind[]
  /** The texts written at or after this moment. */
  from?: Date
  /** The texts written before this moment. */
  to?: Date
  /** After the place a page's `next` named; give the same text and options as that page's. */
  after?: string
  /** From 1 to 200 hits, 50 by default. */
  limit?: number
}

export interface CoreHistory {
  /**
   * The texts of every conversation's agents, its subagents' and its archived
   * conversations' included, that hold every word of `text`, newest first: a
   * message or steer of the user or of the agent's parent, a text of the
   * agent, or the title of a command it ran. Each hit names its conversation,
   * its agent by number and by the node `history.transcript` reads, its
   * block and its time. Pass a page's `next` as `after`, with the same
   * text and options, to read on.
   */
  search(text: string, options?: HistorySearchOptions): Promise<HistoryHits>
}

export function history(routes: CoreRoutes): CoreHistory {
  return {
    search: (text, options = {}) => routes.json(
      'GET',
      `/history/search${query({
        q: text,
        any: options.any ? 'true' : undefined,
        kinds: options.kinds?.join(','),
        from: options.from?.toISOString(),
        to: options.to?.toISOString(),
        after: options.after,
        limit: options.limit,
      })}`,
      historyHitsSchema,
    ),
  }
}

export function commands(routes: CoreRoutes): CoreCommands {
  return {
    history: (options = {}) => routes.json(
      'GET',
      `/commands/history${query({ after: options.after, from: options.from?.toISOString(), limit: options.limit })}`,
      commandHistorySchema,
    ),
  }
}
