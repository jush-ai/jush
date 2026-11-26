/** The models a core's entries offer, and what their calls used. */
import {
  modelSearchResultsSchema,
  usageReportSchema,
  usageTotalsSchema,
  type ModelMatch,
  type UsageBucketSize,
  type UsageDimension,
  type UsageGroup,
  type UsageReport,
} from '@jush-agent/protocol'
import { query, type CoreRoutes } from './routes'

export interface Models {
  /** The models whose names match `text`, the vendor of the provider entry `provider` filling them in. */
  search(text: string, options?: { provider?: string }): Promise<ModelMatch[]>
  /** Downloads the catalog of models again. */
  refresh(): Promise<void>
}

export function models(routes: CoreRoutes): Models {
  return {
    search: async (text, options = {}) => (await routes.json('GET', `/models/search${query({ q: text, entry: options.provider })}`, modelSearchResultsSchema)).results,
    refresh: () => routes.send('POST', '/models/refresh'),
  }
}

/** A report of the calls that ended from `from` until `to`. */
export interface UsageReportOptions {
  from: Date
  to: Date
  /** How long a bucket is. */
  bucket: UsageBucketSize
  /** The IANA zone a bucket's minute, hour or day is counted in; UTC when absent. */
  timeZone?: string
  /**
   * What each bucket's calls are grouped by: a dimension, or one of the
   * labels messages carry (`MessageOptions.labels`) as `label:<key>`, which
   * each group answers in `labels`; none sums each bucket whole.
   */
  groupBy?: Array<UsageDimension | `label:${string}`>
  /** The conversations whose calls it counts; every one's when absent, none when empty. */
  conversations?: string[]
  /**
   * The numbers of the agents of `conversations` whose calls it counts: 0
   * for a main agent, a subagent's `number` in `conversation.agents`;
   * every call of them when absent, none when empty.
   */
  agents?: number[]
}

export interface Usage {
  /** The core's usage, totalled by provider and model. */
  totals(): Promise<UsageGroup[]>
  /**
   * The calls of a period, in buckets of a minute, an hour or a day,
   * grouped by what `groupBy` names.
   */
  report(options: UsageReportOptions): Promise<UsageReport>
}

export function usage(routes: CoreRoutes): Usage {
  return {
    totals: async () => (await routes.json('GET', '/usage', usageTotalsSchema)).totals,
    report: async (options) => routes.json(
      'GET',
      `/usage/report${query({
        from: options.from.toISOString(),
        to: options.to.toISOString(),
        bucket: options.bucket,
        timeZone: options.timeZone,
        groupBy: options.groupBy !== undefined && options.groupBy.length > 0 ? options.groupBy.join(',') : undefined,
        conversations: options.conversations?.join(','),
        agents: options.agents?.join(','),
      })}`,
      usageReportSchema,
    ),
  }
}
