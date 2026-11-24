/**
 * A core's settings a program manages: credentials, the instructions every
 * agent reads, subagents and their profiles, and plugins.
 */
import {
  credentialAnswerSchema,
  credentialListSchema,
  instructionsAnswerSchema,
  profileAnswerSchema,
  revealedValueSchema,
  subagentSettingsSchema,
  type CredentialDto,
  type CredentialPatch,
  type NewCredential,
  type NewProfile,
  type ProfilePatch,
  type SubagentProfile,
  type SubagentSettings,
  type SubagentsPatch,
} from '@jush-agent/protocol'
import { z } from 'zod'
import { segment, type CoreRoutes } from './routes'

export interface Credentials {
  list(): Promise<CredentialDto[]>
  create(credential: NewCredential): Promise<CredentialDto>
  update(id: string, patch: CredentialPatch): Promise<CredentialDto>
  remove(id: string): Promise<void>
  /** The value of the credential's field `field`. */
  reveal(id: string, field: string): Promise<string>
}

export function credentials(routes: CoreRoutes): Credentials {
  const one = (id: string) => `/credentials/${segment(id)}`
  return {
    list: async () => (await routes.json('GET', '/credentials', credentialListSchema)).credentials,
    create: async (credential) => (await routes.json('POST', '/credentials', credentialAnswerSchema, credential)).credential,
    update: async (id, patch) => (await routes.json('PATCH', one(id), credentialAnswerSchema, patch)).credential,
    remove: (id) => routes.send('DELETE', one(id)),
    reveal: async (id, field) => (await routes.json('POST', `${one(id)}/fields/${segment(field)}/reveal`, revealedValueSchema)).value,
  }
}

/** The instructions every agent of the core reads, as Settings › Instructions keeps them. */
export interface Instructions {
  get(): Promise<string>
  set(text: string): Promise<void>
}

export function instructions(routes: CoreRoutes): Instructions {
  return {
    get: async () => (await routes.json('GET', '/settings/instructions', instructionsAnswerSchema)).instructions,
    set: (text) => routes.send('PUT', '/instructions', { text }),
  }
}

export interface Subagents {
  /** Whether agents start subagents, their limits and their profiles. */
  get(): Promise<SubagentSettings>
  update(patch: SubagentsPatch): Promise<void>
  readonly profiles: {
    create(profile: NewProfile): Promise<SubagentProfile>
    update(id: string, patch: ProfilePatch): Promise<SubagentProfile>
    remove(id: string): Promise<void>
  }
}

export function subagents(routes: CoreRoutes): Subagents {
  const one = (id: string) => `/subagents/profiles/${segment(id)}`
  return {
    get: () => routes.json('GET', '/settings/subagents', subagentSettingsSchema),
    update: (patch) => routes.send('PUT', '/subagents', patch),
    profiles: {
      create: async (profile) => (await routes.json('POST', '/subagents/profiles', profileAnswerSchema, profile)).profile,
      update: async (id, patch) => (await routes.json('PATCH', one(id), profileAnswerSchema, patch)).profile,
      remove: (id) => routes.send('DELETE', one(id)),
    },
  }
}

/** The plugins of the core, Jush's and the program's: their switches and their methods. */
export interface CorePlugins {
  enable(plugin: string, enabled: boolean): Promise<void>
  enableFeature(plugin: string, feature: string, enabled: boolean): Promise<void>
  /** Calls the plugin's method `method` with `input`, as its page does, and answers what it answers. */
  call(plugin: string, method: string, input?: unknown): Promise<unknown>
}

export function corePlugins(routes: CoreRoutes): CorePlugins {
  const one = (plugin: string) => `/plugins/${segment(plugin)}`
  return {
    enable: (plugin, enabled) => routes.send('PUT', one(plugin), { enabled }),
    enableFeature: (plugin, feature, enabled) => routes.send('PUT', `${one(plugin)}/features/${segment(feature)}`, { enabled }),
    call: (plugin, method, input = {}) => routes.json('POST', `${one(plugin)}/calls/${segment(method)}`, z.unknown(), input),
  }
}
