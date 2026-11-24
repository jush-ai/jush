/**
 * A core's routes, as a page has them under `/api`, at `/cores/{id}/api` of
 * the process, and its page connection; and the error a route answers.
 */
import type { PageConnection } from '@jush-agent/conversation-client'
import { errorBodySchema, schemaRefusal, type ErrorBody } from '@jush-agent/protocol'
import type { z } from 'zod'
import type { Process } from './process'

/** A route refused the call: its status, and the core's error body. */
export class JushError extends Error {
  readonly code: ErrorBody['code'] | null
  readonly requestId: string | null

  constructor(readonly status: number, readonly body: ErrorBody | null, fallback: string) {
    super(body?.message ?? fallback)
    this.name = 'JushError'
    this.code = body?.code ?? null
    this.requestId = body?.requestId ?? null
  }
}

/** The base of a process's URLs; the process reads the path alone. */
export const PROCESS_ORIGIN = 'http://jush-core'

/**
 * `path` below `base`, such as `/conversations` of `/jush/api/conversations`
 * below `/jush/api`; `path` itself when it is not below it, as
 * `/jush/apiary` is not.
 */
export function below(path: string, base: string): string {
  const root = base.endsWith('/') ? base.slice(0, -1) : base
  return path === root || path.startsWith(`${root}/`) ? path.slice(root.length) : path
}

/** The refusal `response`, an answer other than 2xx to `what`, carries. */
export async function refusal(response: Response, what: string): Promise<JushError> {
  const text = await response.text()
  let body: ErrorBody | null = null
  try {
    const parsed = errorBodySchema.safeParse(JSON.parse(text))
    body = parsed.success ? parsed.data : null
  } catch {
    // A body that is no error body is said as it is.
  }
  return new JushError(response.status, body, `${what} answered ${response.status}: ${text}`)
}

export class CoreRoutes {
  constructor(readonly process: Process, readonly id: string, readonly connection: PageConnection) {}

  /** The process's path of the core's route `path`, such as `/conversations`. */
  path(path: string): string {
    return `/cores/${segment(this.id)}/api${path}`
  }

  /**
   * Calls the route `path` with `body`, a `Blob` as its bytes of its type
   * and anything else as JSON; throws `JushError` for an answer other than
   * 2xx. A `Blob`'s length is declared, so a route refuses one over its
   * limit before it reads a byte of it.
   */
  async request(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<Response> {
    const sent: RequestInit = body === undefined
      ? {}
      : body instanceof Blob
        ? { body, headers: { 'content-type': body.type || 'application/octet-stream', 'content-length': String(body.size) } }
        : { body: JSON.stringify(body), headers: { 'content-type': 'application/json' } }
    const response = await this.process.fetch(new Request(PROCESS_ORIGIN + this.path(path), {
      method,
      ...sent,
      ...(signal === undefined ? {} : { signal }),
    }))
    if (!response.ok) {
      throw await refusal(response, `${method} ${path}`)
    }
    return response
  }

  /** The answer of the route `path`, read with `schema`. */
  async json<T>(method: string, path: string, schema: z.ZodType<T>, body?: unknown, signal?: AbortSignal): Promise<T> {
    const response = await this.request(method, path, body, signal)
    const parsed = schema.safeParse(await response.json())
    if (!parsed.success) {
      throw new Error(schemaRefusal(parsed.error))
    }
    return parsed.data
  }

  /** Calls the route `path`, and drops its answer. */
  async send(method: string, path: string, body?: unknown): Promise<void> {
    const response = await this.request(method, path, body)
    await response.body?.cancel()
  }

  /** The bytes the route `path` answers. */
  async bytes(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<Uint8Array> {
    const response = await this.request(method, path, body, signal)
    return new Uint8Array(await response.arrayBuffer())
  }

  /** Each JSON value the route `path` answers, one a line, read with `schema` as it comes. */
  async *lines<T>(method: string, path: string, schema: z.ZodType<T>, body?: unknown, signal?: AbortSignal): AsyncGenerator<T> {
    const response = await this.request(method, path, body, signal)
    if (response.body === null) {
      return
    }
    const decoder = new TextDecoder()
    let held = ''
    for await (const chunk of response.body) {
      held += decoder.decode(chunk, { stream: true })
      let end = held.indexOf('\n')
      while (end >= 0) {
        const line = held.slice(0, end).trim()
        held = held.slice(end + 1)
        if (line !== '') {
          yield parsed(schema, line)
        }
        end = held.indexOf('\n')
      }
    }
    const last = (held + decoder.decode()).trim()
    if (last !== '') {
      yield parsed(schema, last)
    }
  }
}

/** `line`, one JSON value, read with `schema`. */
function parsed<T>(schema: z.ZodType<T>, line: string): T {
  const read = schema.safeParse(JSON.parse(line))
  if (!read.success) {
    throw new Error(schemaRefusal(read.error))
  }
  return read.data
}

/** The query of `values`, `?a=1&b=x`, leaving out each one undefined; empty for none. */
export function query(values: Record<string, string | number | boolean | undefined>): string {
  const search = new URLSearchParams()
  for (const [name, value] of Object.entries(values)) {
    if (value !== undefined) {
      search.set(name, String(value))
    }
  }
  const text = search.toString()
  return text === '' ? '' : `?${text}`
}

/** `segment` as one segment of a route's path. */
export const segment = encodeURIComponent
