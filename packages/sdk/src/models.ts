/** The models a core's entries offer, and what their calls used. */
import { modelSearchResultsSchema, usageTotalsSchema, type ModelMatch, type UsageGroup } from '@jush-agent/protocol'
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

export interface Usage {
  /** The core's usage, totalled by provider and model. */
  totals(): Promise<UsageGroup[]>
}

export function usage(routes: CoreRoutes): Usage {
  return {
    totals: async () => (await routes.json('GET', '/usage', usageTotalsSchema)).totals,
  }
}
