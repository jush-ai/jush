/**
 * A program's Jush: the process its cores run in, which `Jush.start` starts
 * or `Jush.connect` connects to, with the program's plugins and addresses,
 * the cores it opens, and the calls from outside that the program's server
 * takes at its addresses and hands on.
 */
import { resolve } from 'node:path'
import {
  openedCoreSchema,
  scheduleCallAnswerSchema,
  signInAnswerSchema,
  type ProgramAddresses,
  type ScheduleCallAnswer,
  type SignInAnswer,
} from '@jush-agent/protocol'
import { binaries } from './binaries'
import { Core } from './core'
import type { Plugin } from './plugin'
import { Process } from './process'
import { PROCESS_ORIGIN, refusal, segment } from './routes'

/** What Jush starts with. */
export interface JushOptions {
  /** The program's plugins, which its cores' agents use besides Jush's own. */
  plugins?: readonly Plugin[]
  /**
   * Where the program's server takes the calls from outside, as URI
   * templates whose `{name}`s Jush fills: a schedule's URL, and a
   * plugin's sign-in return. Without one, a URL schedule has no address,
   * and a plugin's sign-in is refused.
   */
  addresses?: ProgramAddresses
}

/** How Jush connects to a process that runs on its own, `jush-core serve`. */
export interface JushConnection {
  /** Where the process listens, such as `http://127.0.0.1:4100`. */
  url: string
  /** The token the process was started with, `JUSH_CORE_TOKEN`. */
  token: string
  /**
   * The program's plugins, which the process's cores' agents use besides
   * Jush's own; each replaces the one of its id declared before, as the
   * program's earlier run declared it.
   */
  plugins?: readonly Plugin[]
  /**
   * Where the program's server takes the calls from outside, in place of
   * the addresses declared before; without them, those another
   * connection declared stay.
   */
  addresses?: ProgramAddresses
}

/** A schedule's URL, as the program's server takes its calls. */
export interface JushSchedules {
  /**
   * Hands on `request`, a call of the URL of the schedule `id` that the
   * program's server took at its `schedule` address, and answers what
   * became of it: `taken`, kept for a run, or `refused` and why. The
   * server writes the HTTP answer.
   */
  call(id: string, request: Request): Promise<ScheduleCallAnswer>
}

/** The return of a plugin's sign-in, which the vendor sends the browser to. */
export interface SignInReturn {
  /** The core that started the sign-in, `core.id`. */
  core: string
  /** The plugin's id. */
  plugin: string
  /** The query of the address the browser came back to. */
  query: URLSearchParams | string | Record<string, string>
}

/** The plugins' sign-ins, as the program's server takes their returns. */
export interface JushPlugins {
  /**
   * Completes the sign-in the browser came back from to the program's
   * `signInReturn` address, and answers how it ended: `signedIn` to the
   * vendor, or `refused` and why. The server shows the browser the end.
   */
  completeSignIn(signIn: SignInReturn): Promise<SignInAnswer>
}

export class Jush {
  /** A schedule's URL, as the program's server takes its calls. */
  readonly schedules: JushSchedules
  /** The plugins' sign-ins, as the program's server takes their returns. */
  readonly plugins: JushPlugins

  private readonly cores = new Map<string, Core>()

  private constructor(
    private readonly process: Process,
    private readonly declared: readonly Plugin[],
    private readonly addresses: ProgramAddresses | undefined,
    /** The package's own runner, which a device spawns with no command. */
    private readonly runner: () => string,
    /** Whether the process is the program's own, which ends with it. */
    private readonly owned: boolean,
  ) {
    this.schedules = { call: (id, request) => this.callSchedule(id, request) }
    this.plugins = { completeSignIn: (signIn) => this.completeSignIn(signIn) }
  }

  /** Starts a process of the program's own, with the program's plugins and addresses. */
  static async start(options: JushOptions = {}): Promise<Jush> {
    const found = binaries()
    const process = Process.start({ executable: found.core, release: found.release })
    return Jush.greeted(new Jush(process, options.plugins ?? [], options.addresses, () => found.runner, true))
  }

  /**
   * Connects to a process that runs on its own, and declares the program's
   * addresses and plugins: the cores it holds, their conversations and
   * their turns went on while the program was away. A connection made
   * again, as after the process restarted, declares them again and opens
   * the program's cores again before anything else.
   */
  static async connect(connection: JushConnection): Promise<Jush> {
    const process = Process.connect({ url: connection.url, token: connection.token })
    // A program that spawns no device needs no executable of its own.
    return Jush.greeted(new Jush(process, connection.plugins ?? [], connection.addresses, () => binaries().runner, false))
  }

  /**
   * `jush` once it greeted its process; a greeting the process refused,
   * such as an address that is no template, ends the process the program
   * started, or the connection to one on its own.
   */
  private static async greeted(jush: Jush): Promise<Jush> {
    try {
      await jush.process.greet((fetch) => jush.greet(fetch))
    } catch (error) {
      await jush.process.close()
      throw error
    }
    return jush
  }

  /**
   * Opens the core in `dir`, creating it when `dir` is absent or empty, and
   * reads its product state, which it follows from then on. A core the
   * process holds already, as one the program's earlier run opened, is the
   * same core.
   */
  async open(dir: string): Promise<Core> {
    const path = resolve(dir)
    const response = await this.call('POST', '/cores', { dir: path })
    const { id } = openedCoreSchema.parse(await response.json())
    const opened = this.cores.get(id)
    if (opened !== undefined) {
      return opened
    }
    const core = new Core(
      { process: this.process, id, dir: path, plugins: this.declared, runner: this.runner },
      async () => {
        this.cores.delete(id)
        await (await this.call('DELETE', `/cores/${segment(id)}`)).body?.cancel()
      },
    )
    this.cores.set(id, core)
    await core.follow()
    return core
  }

  /**
   * Ends the program's use of Jush: a process the program started closes
   * every core and exits; one on its own keeps its cores, which the
   * program's next run opens again.
   */
  async close(): Promise<void> {
    const cores = [...this.cores.values()]
    this.cores.clear()
    if (this.owned) {
      await Promise.all(cores.map((core) => core.close()))
    } else {
      for (const core of cores) {
        core.disconnect()
      }
    }
    await this.process.close()
  }

  /**
   * The call `request` as one HTTP message (`message/http`, RFC 9112
   * § 10.1): its head, then its body's bytes as they arrive, which a
   * signature over them verifies.
   */
  private async callSchedule(id: string, request: Request): Promise<ScheduleCallAnswer> {
    const url = new URL(request.url)
    const lines = [`${request.method} ${url.pathname}${url.search} HTTP/1.1`]
    request.headers.forEach((value, name) => lines.push(`${name}: ${value}`))
    // A header's value is bytes, one to a character.
    const head = Buffer.from(`${lines.join('\r\n')}\r\n\r\n`, 'latin1')
    const path = `/schedules/calls?${new URLSearchParams({ id })}`
    const sent: RequestInit & { duplex: 'half' } = {
      method: 'POST',
      body: request.body === null ? head : headed(head, request.body),
      headers: { 'content-type': 'message/http' },
      duplex: 'half',
    }
    const response = await this.process.fetch(new Request(PROCESS_ORIGIN + path, sent))
    if (!response.ok) {
      throw await refusal(response, `POST ${path}`)
    }
    return scheduleCallAnswerSchema.parse(await response.json())
  }

  private async completeSignIn(signIn: SignInReturn): Promise<SignInAnswer> {
    const query = new URLSearchParams(signIn.query).toString()
    const response = await this.call('POST', '/sign-ins', { core: signIn.core, plugin: signIn.plugin, query })
    return signInAnswerSchema.parse(await response.json())
  }

  /**
   * What each connection to the process begins with: the program's
   * addresses and plugins, then its cores, which a process that
   * restarted holds no longer.
   */
  private async greet(fetch: (request: Request) => Promise<Response>): Promise<void> {
    if (this.addresses !== undefined) {
      const response = await this.call('PUT', '/addresses', this.addresses, fetch)
      await response.body?.cancel()
    }
    for (const plugin of this.declared) {
      const response = await this.call('PUT', `/plugins/${segment(plugin.manifest.id)}`, plugin.manifest, fetch)
      await response.body?.cancel()
    }
    for (const core of this.cores.values()) {
      const response = await this.call('POST', '/cores', { dir: core.dir }, fetch)
      await response.body?.cancel()
    }
  }

  private async call(
    method: string,
    path: string,
    body?: unknown,
    fetch = (request: Request) => this.process.fetch(request),
  ): Promise<Response> {
    const response = await fetch(new Request(PROCESS_ORIGIN + path, {
      method,
      ...(body === undefined ? {} : { body: JSON.stringify(body), headers: { 'content-type': 'application/json' } }),
    }))
    if (!response.ok) {
      throw await refusal(response, `${method} ${path}`)
    }
    return response
  }
}

/**
 * `head`, then `body`'s bytes as they arrive; cancelling it, as the process
 * does once it answered, cancels `body`.
 */
function headed(head: Uint8Array, body: ReadableStream<Uint8Array>): ReadableStream<Uint8Array> {
  const reader = body.getReader()
  let sent = false
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      if (!sent) {
        sent = true
        controller.enqueue(head)
        return
      }
      const read = await reader.read()
      if (read.done) {
        controller.close()
      } else {
        controller.enqueue(read.value)
      }
    },
    cancel: (reason) => reader.cancel(reason),
  })
}
