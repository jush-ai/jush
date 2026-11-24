/** The ports a core's devices forward. */
import {
  portForwardAnswerSchema,
  portForwardListSchema,
  type PortForwardDto,
  type PortForwardPatch,
} from '@jush-agent/protocol'
import { segment, type CoreRoutes } from './routes'

/** A device's port that another device, or the core's server, reaches. */
export interface PortForwards {
  list(): Promise<PortForwardDto[]>
  create(forward: PortForwardPatch): Promise<PortForwardDto>
  update(id: string, patch: PortForwardPatch): Promise<PortForwardDto>
  remove(id: string): Promise<void>
  /** Tries a forward that failed again. */
  retry(id: string): Promise<void>
}

export function portForwards(routes: CoreRoutes): PortForwards {
  const one = (id: string) => `/port-forwards/${segment(id)}`
  return {
    list: async () => (await routes.json('GET', '/port-forwards', portForwardListSchema)).forwards,
    create: async (forward) => (await routes.json('POST', '/port-forwards', portForwardAnswerSchema, forward)).forward,
    update: async (id, patch) => (await routes.json('PATCH', one(id), portForwardAnswerSchema, patch)).forward,
    remove: (id) => routes.send('DELETE', one(id)),
    retry: (id) => routes.send('POST', `${one(id)}/retry`),
  }
}
