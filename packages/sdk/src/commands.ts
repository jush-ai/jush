/** The commands a core's agents ran, as the core's command history keeps them. */
import { commandHistorySchema, type CommandHistory } from '@jush-agent/protocol'
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

export function commands(routes: CoreRoutes): CoreCommands {
  return {
    history: (options = {}) => routes.json(
      'GET',
      `/commands/history${query({ after: options.after, from: options.from?.toISOString(), limit: options.limit })}`,
      commandHistorySchema,
    ),
  }
}
