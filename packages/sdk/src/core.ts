/**
 * A core a program opened: its product state, followed on the
 * synchronization lane; its conversations, providers and devices, through its
 * routes; its notifications, on the starter lane; the program's plugins, on
 * their lanes; and its routes and page connection for the program's own
 * server to forward a page's to.
 */
import { spawn, type ChildProcess } from 'node:child_process'
import { join } from 'node:path'
import type { Readable, Writable } from 'node:stream'
import { PageConnection, reconnectWait, SILENCE_MS, type ConnectionHost, type Lane } from '@jush-agent/conversation-client'
import {
  accountsSchema,
  activeAccountSchema,
  attachmentAnswerSchema,
  batchAnswerSchema,
  clientContentSchema,
  codeTakenSchema,
  conversationDetailSchema,
  conversationsSchema,
  coreStateSchema,
  createdConversationSchema,
  deviceAnswerSchema,
  deviceElevationSchema,
  deviceExecResultSchema,
  devicesSchema,
  keptWorkflowListSchema,
  loginStartedSchema,
  providerAnswerSchema,
  providerCliSchema,
  providerDetailsSchema,
  providersSchema,
  quotaAnswerSchema,
  removedAccountSchema,
  schemaRefusal,
  searchResultsSchema,
  starterRequestSchema,
  syncEventSchema,
  testResultSchema,
  userPreferencesSchema,
  vendorCatalogSchema,
  withPart,
  type Accounts,
  type BatchItem,
  type BatchResult,
  type ChangeDevice,
  type ClientContent,
  type ConversationDetail,
  type ConversationSummary,
  type CoreState,
  type CreateConversation,
  type CreateProvider,
  type DeviceDto,
  type DeviceExec,
  type ElevationOutcome,
  type KeptWorkflowList,
  type NewestVersion,
  type Preferences,
  type PreferencesPatch,
  type ProviderDetails,
  type ProviderDto,
  type ProviderPatch,
  type PushMessage,
  type QuotaSnapshot,
  type SearchResult,
  type StarterAnswer,
  type SubscriptionLogin,
  type TestResult,
  type VendorCatalog,
} from '@jush-agent/protocol'
import { randomUUID } from 'node:crypto'
import { Conversation } from './conversation'
import { deviceFiles, type DeviceFiles } from './files'
import { models, usage, type Models, type Usage } from './models'
import { commands, type CoreCommands } from './commands'
import { projects, workspaces, type Projects, type Workspaces } from './organize'
import { servePlugin, type Plugin } from './plugin'
import { portForwards, type PortForwards } from './port-forwards'
import type { Process } from './process'
import { CoreRoutes, query, segment } from './routes'
import {
  corePlugins,
  credentials,
  instructions,
  subagents,
  type CorePlugins,
  type Credentials,
  type Instructions,
  type Subagents,
} from './settings'
import { StreamSocket } from './socket'

/** How a notification went, as the program answers the core: `id` aside. */
export type Notified = StarterAnswer extends infer Answer ? (Answer extends { id: number } ? Omit<Answer, 'id'> : never) : never

/** A file the program holds, as `core.upload` takes it. */
export interface FileBytes {
  /** The file's name, without a path, such as `receipt.png`. */
  name: string
  bytes: Uint8Array | ArrayBuffer | Blob
  /**
   * Its media type, else a `Blob`'s own; an image, a video, a recording or a
   * PDF is recorded as its bytes show it whatever this says.
   */
  mediaType?: string
}

/** What a core is opened with. */
export interface CoreOpening {
  process: Process
  id: string
  dir: string
  plugins: readonly Plugin[]
  /** The package's own runner, which `device.spawn()` starts with no command. */
  runner: () => string
}

export class Core {
  readonly id: string
  /** The directory the core's data is in. */
  readonly dir: string
  readonly conversations: Conversations
  readonly providers: Providers
  readonly devices: Devices
  readonly credentials: Credentials
  readonly instructions: Instructions
  readonly subagents: Subagents
  readonly plugins: CorePlugins
  readonly models: Models
  readonly usage: Usage
  readonly commands: CoreCommands
  readonly workspaces: Workspaces
  readonly projects: Projects
  readonly portForwards: PortForwards
  private readonly routes: CoreRoutes
  private readonly connection: PageConnection
  // Read by `follow`, which opening the core awaits before it hands the core over.
  private stateRead!: CoreState
  private readonly stateListeners = new Set<(state: CoreState) => void>()
  private notify: ((message: PushMessage) => Promise<Notified> | Notified) | null = null
  private starter: Lane | null = null
  private sync: Lane | null = null
  private closed = false

  constructor(opening: CoreOpening, private readonly closing: () => Promise<void>) {
    this.id = opening.id
    this.dir = opening.dir
    this.connection = new PageConnection(processConnection(opening.process, `/cores/${segment(opening.id)}/api/connect`))
    this.connection.start()
    this.routes = new CoreRoutes(opening.process, opening.id, this.connection)
    this.conversations = new Conversations(this.routes, () => this.stateRead)
    this.providers = new Providers(this.routes)
    this.devices = new Devices(this.routes, opening.process, opening.runner, opening.dir, (id, ended) => this.online(id, ended))
    this.credentials = credentials(this.routes)
    this.instructions = instructions(this.routes)
    this.subagents = subagents(this.routes)
    this.plugins = corePlugins(this.routes)
    this.models = models(this.routes)
    this.usage = usage(this.routes)
    this.commands = commands(this.routes)
    this.workspaces = workspaces(this.routes)
    this.projects = projects(this.routes)
    this.portForwards = portForwards(this.routes)
    for (const plugin of opening.plugins) {
      this.servePlugin(plugin)
    }
  }

  /** Reads the core's product state and follows it from then on. */
  async follow(): Promise<void> {
    this.stateRead = await this.routes.json('GET', '/state', coreStateSchema)
    this.openSync()
  }

  /** The core's product state, as the synchronization lane last left it. */
  get state(): CoreState {
    return this.stateRead
  }

  /**
   * Calls `listener` with the product state at each change of it, or with
   * each notification the core sends, whose answer says how it went;
   * answers what stops it. Notifications reach the program only while it
   * listens for them.
   */
  on(event: 'state', listener: (state: CoreState) => void): () => void
  on(event: 'notify', listener: (message: PushMessage) => Promise<Notified> | Notified): () => void
  on(event: 'state' | 'notify', listener: ((state: CoreState) => void) | ((message: PushMessage) => Promise<Notified> | Notified)): () => void {
    if (event === 'state') {
      const each = listener as (state: CoreState) => void
      this.stateListeners.add(each)
      return () => this.stateListeners.delete(each)
    }
    const notify = listener as (message: PushMessage) => Promise<Notified> | Notified
    this.notify = notify
    this.openStarter()
    return () => {
      // A listener that replaced this one goes on.
      if (this.notify !== notify) {
        return
      }
      this.notify = null
      this.starter?.close()
      this.starter = null
    }
  }

  /**
   * Keeps `file` as an upload of the core's, as the web app's composer does,
   * and answers the block a message, a steer or a run sends it in.
   */
  async upload(file: FileBytes): Promise<Extract<ClientContent, { type: 'upload' }>> {
    // A name the block refuses, such as one with a path, fails before the bytes go.
    const named = clientContentSchema.safeParse({ type: 'upload', ref: 'unsent', fileName: file.name })
    if (!named.success) {
      throw new Error(`${JSON.stringify(file.name)} is not a file's name: ${schemaRefusal(named.error)}`)
    }
    const type = file.mediaType || (file.bytes instanceof Blob ? file.bytes.type : '')
    const query = new URLSearchParams({ name: file.name })
    const { attachment } = await this.routes.json('POST', `/attachments?${query}`, attachmentAnswerSchema, new Blob([file.bytes], { type }))
    return { type: 'upload', ref: attachment.id, fileName: file.name }
  }

  /** The conversations whose titles and messages match `text`, of the workspace `workspace` alone when named. */
  async search(text: string, options: { workspace?: string } = {}): Promise<SearchResult[]> {
    return (await this.routes.json('GET', `/search${query({ q: text, workspace: options.workspace })}`, searchResultsSchema)).results
  }

  /** The bytes of the blob `sha256`, such as a medium a block names. */
  blob(sha256: string): Promise<Uint8Array> {
    return this.routes.bytes('GET', `/blobs/${segment(sha256)}`)
  }

  /**
   * Changes what `patch` names of the core's preferences, the web app's
   * Settings, and answers them, such as `commandLocale`, the time zone and
   * languages its commands and browsers run in.
   */
  async updatePreferences(patch: PreferencesPatch): Promise<Preferences> {
    return (await this.routes.json('PATCH', '/settings/preferences', userPreferencesSchema, patch)).preferences
  }

  /** Closes the core, for every program: its lanes end, and the process closes it. */
  async close(): Promise<void> {
    if (this.closed) {
      return
    }
    this.disconnect()
    await this.closing()
  }

  /** Ends the program's lanes to the core, which stays open. */
  disconnect(): void {
    this.closed = true
    this.conversations.closeAll()
    this.connection.end()
  }

  private openSync(): void {
    if (this.closed) {
      return
    }
    const lane = this.connection.openLane({ kind: 'sync' }, 'synchronization channel')
    this.sync = lane
    lane.addEventListener('open', () => {
      const version = this.stateRead.version
      lane.send(JSON.stringify({ type: 'open', follows: version ? [{ value: 'state', holds: { kind: 'version', version } }] : [] }))
    })
    lane.addEventListener('message', (message) => {
      const parsed = syncEventSchema.safeParse(JSON.parse(message.data as string))
      if (!parsed.success) {
        process.emitWarning(schemaRefusal(parsed.error))
        return
      }
      const event = parsed.data
      switch (event.type) {
        case 'synced':
          this.setState({ ...this.stateRead, version: event.version })
          return
        case 'stale':
          // The core stopped following the state as it said so: the lane
          // follows it again from the state read, as a page does. A read
          // that fails leaves the state as it was, and the lane opens again,
          // whose open is told again that it is stale, unless the core
          // closed, as a close fails the read.
          this.routes.json('GET', '/state', coreStateSchema).then(
            (state) => {
              this.setState(state)
              if (this.sync === lane && state.version) {
                lane.send(JSON.stringify({ type: 'follow', value: 'state', holds: { kind: 'version', version: state.version } }))
              }
            },
            () => lane.close(),
          )
          return
        case 'update':
          return
      }
      this.setState(withPart(this.stateRead, event))
    })
    lane.addEventListener('close', () => {
      if (this.sync === lane && !this.closed && lane.refusal === null) {
        this.openSync()
      }
    })
  }

  private setState(state: CoreState): void {
    this.stateRead = state
    for (const listener of [...this.stateListeners]) {
      listener(state)
    }
  }

  /**
   * Resolves once the product state shows the device `id` online, its
   * runner connected and the core holding it; fails once `ended` comes
   * first, as a runner that exits before it connects ends its channel.
   */
  private online(id: string, ended: Promise<void>): Promise<void> {
    const isOnline = (state: CoreState) => state.devices.some((device) => device.id === id && device.state === 'online')
    if (isOnline(this.stateRead)) {
      return Promise.resolve()
    }
    return new Promise<void>((resolve, reject) => {
      const listener = (state: CoreState) => {
        if (isOnline(state)) {
          this.stateListeners.delete(listener)
          resolve()
        }
      }
      this.stateListeners.add(listener)
      void ended.then(() => {
        this.stateListeners.delete(listener)
        reject(new Error(`The channel of the device ${id} ended before the device came online`))
      })
    })
  }

  private openStarter(): void {
    if (this.closed || this.starter !== null) {
      return
    }
    const lane = this.connection.openLane({ kind: 'starter' }, 'starter lane')
    this.starter = lane
    lane.addEventListener('message', (message) => {
      const parsed = starterRequestSchema.safeParse(JSON.parse(message.data as string))
      if (!parsed.success) {
        process.emitWarning(schemaRefusal(parsed.error))
        return
      }
      const { id, message: notification } = parsed.data
      const notify = this.notify
      void Promise.resolve(notify === null ? ({ type: 'nowhere' } as Notified) : notify(notification))
        .catch((): Notified => ({ type: 'nowhere' }))
        .then((notified) => lane.send(JSON.stringify({ ...notified, id } satisfies StarterAnswer)))
    })
    lane.addEventListener('close', () => {
      if (this.starter === lane) {
        this.starter = null
        if (!this.closed && this.notify !== null && lane.refusal === null) {
          this.openStarter()
        }
      }
    })
  }

  private servePlugin(plugin: Plugin): void {
    if (this.closed) {
      return
    }
    const lane = this.connection.openLane({ kind: 'plugin', plugin: plugin.manifest.id }, `plugin lane of ${plugin.manifest.id}`)
    servePlugin(plugin, lane, (error) => process.emitWarning(error))
    lane.addEventListener('close', () => {
      if (lane.refusal !== null) {
        process.emitWarning(`The core refused the lane of the plugin ${plugin.manifest.id}: ${lane.refusal.error.message}`)
      } else if (!this.closed) {
        this.servePlugin(plugin)
      }
    })
  }
}

/** A socket a program's server accepted, as a WebSocket of the platform's. */
export interface AcceptedSocket {
  binaryType: string
  send(data: string | ArrayBuffer | Uint8Array): void
  close(code?: number, reason?: string): void
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void
  addEventListener(type: 'close', listener: () => void): void
}

/** The page connection's host for a core of the process. */
function processConnection(process: Process, path: string): ConnectionHost {
  return {
    open: () => new StreamSocket(process.connect(path, 'websocket', { 'sec-websocket-version': '13' })),
    watch: (broken) => {
      let timer = setTimeout(broken, SILENCE_MS)
      return {
        heard: () => {
          clearTimeout(timer)
          timer = setTimeout(broken, SILENCE_MS)
        },
        ponged: () => {},
        stop: () => clearTimeout(timer),
      }
    },
    wait: (failures, _restarting, connect) => {
      const timer = setTimeout(connect, reconnectWait(failures))
      return { cancel: () => clearTimeout(timer) }
    },
    reached: () => {},
    failedToOpen: () => {},
    changed: () => {},
    log: { info: () => {}, warn: () => {} },
  }
}

/** What a new conversation is created with: the web API's; without an id, a new one, and without a workspace, the core's first. */
export type ConversationOptions = Partial<CreateConversation>

export class Conversations {
  /** Each conversation the program drives, one for each id. */
  private readonly opened = new Map<string, Conversation>()

  constructor(private readonly routes: CoreRoutes, private readonly state: () => CoreState) {}

  /** Creates a conversation; its workspace is the core's first unless named. */
  async create(options: ConversationOptions = {}): Promise<Conversation> {
    const workspaceId = options.workspaceId ?? this.state().workspaces?.[0]?.id
    if (workspaceId === undefined) {
      throw new Error('The core has no workspace to create the conversation in')
    }
    const body: CreateConversation = { ...options, id: options.id ?? randomUUID(), workspaceId }
    const created = await this.routes.json('POST', '/conversations', createdConversationSchema, body)
    return this.open(created.conversation.id)
  }

  /** The core's conversations that are not archived, or with `archived` the ones that are. */
  async list(options: { archived?: boolean } = {}): Promise<ConversationSummary[]> {
    return (await this.routes.json('GET', `/conversations${query({ archived: options.archived })}`, conversationsSchema)).conversations
  }

  /** The conversation `id` as the list shows it, with its metadata and its agents. */
  get(id: string): Promise<ConversationDetail> {
    return this.open(id).get()
  }

  /** Deletes the conversation `id`, with its history and its files. */
  remove(id: string): Promise<void> {
    return this.open(id).remove()
  }

  /** Changes several conversations at once, such as archiving them; each item answers apart. */
  async updateMany(items: BatchItem[]): Promise<BatchResult[]> {
    return (await this.routes.json('POST', '/conversations/batch', batchAnswerSchema, { items })).results
  }

  /** The conversation `id`, whose lane opens at its first use. */
  open(id: string): Conversation {
    let conversation = this.opened.get(id)
    if (conversation === undefined) {
      conversation = new Conversation(id, this.routes, {
        open: (other) => this.open(other),
        removed: (gone) => this.opened.delete(gone),
      })
      this.opened.set(id, conversation)
    }
    return conversation
  }

  /** Closes every conversation, as the core closes. */
  closeAll(): void {
    for (const conversation of this.opened.values()) {
      conversation.close()
    }
    this.opened.clear()
  }
}

/** A sign-in to a subscription, which finishes as its user confirms it; its stages are the product state's `logins`. */
export interface SignIn {
  readonly id: string
  /** Hands the sign-in the code its user pasted. */
  submitCode(code: string): Promise<void>
  cancel(): Promise<void>
}

/** The accounts of a provider entry. */
export interface ProviderAccounts {
  list(): Promise<Accounts>
  /** Makes `credentialId` the active account. */
  activate(credentialId: string): Promise<void>
  /** Orders the accounts, each once. */
  order(credentialIds: string[]): Promise<void>
  remove(credentialId: string): Promise<void>
  /** Signs in another account. */
  signIn(): Promise<SignIn>
}

export class Providers {
  constructor(private readonly routes: CoreRoutes) {}

  async list(): Promise<ProviderDto[]> {
    return (await this.routes.json('GET', '/providers', providersSchema)).providers
  }

  async create(provider: CreateProvider): Promise<ProviderDto> {
    return (await this.routes.json('POST', '/providers', providerAnswerSchema, provider)).provider
  }

  async update(id: string, patch: ProviderPatch): Promise<ProviderDto> {
    return (await this.routes.json('PATCH', `/providers/${segment(id)}`, providerAnswerSchema, patch)).provider
  }

  async remove(id: string): Promise<void> {
    await this.routes.send('DELETE', `/providers/${segment(id)}`)
  }

  /** The vendors and subscriptions an entry can be made of, and the shapes of a compatible endpoint. */
  catalog(): Promise<VendorCatalog> {
    return this.routes.json('GET', '/providers/catalog', vendorCatalogSchema)
  }

  /** Orders the entries, each once. */
  order(providerIds: string[]): Promise<void> {
    return this.routes.send('PUT', '/providers/order', { providerIds })
  }

  /** The entry `id` as Settings shows it: its models, accounts and state. */
  status(id: string): Promise<ProviderDetails> {
    return this.routes.json('GET', `/providers/${segment(id)}/status`, providerDetailsSchema)
  }

  /** The quota of the entry's subscription, its active account's or `credentialId`'s, read now. */
  async quota(id: string, options: { credentialId?: string } = {}): Promise<QuotaSnapshot | null> {
    return (await this.routes.json('POST', `/providers/${segment(id)}/quota`, quotaAnswerSchema, options)).quota
  }

  /** Calls the entry's model, or `modelId`, once, with its active account or `credentialId`. */
  test(id: string, options: { modelId?: string; credentialId?: string } = {}): Promise<TestResult> {
    return this.routes.json('POST', `/providers/${segment(id)}/test`, testResultSchema, options)
  }

  /** The newest version of the command-line program the entry's subscription runs, read again with `refresh`. */
  async cli(id: string, options: { refresh?: boolean } = {}): Promise<NewestVersion> {
    return (await this.routes.json('GET', `/providers/${segment(id)}/cli${query({ refresh: options.refresh })}`, providerCliSchema)).newest
  }

  /** Starts installing that program's newest version; the entry's status says when it is in place. */
  installCli(id: string): Promise<void> {
    return this.routes.send('POST', `/providers/${segment(id)}/cli/install`)
  }

  accounts(id: string): ProviderAccounts {
    const path = `/providers/${segment(id)}/accounts`
    return {
      list: () => this.routes.json('GET', path, accountsSchema),
      activate: async (credentialId) => void (await this.routes.json('PUT', `${path}/active`, activeAccountSchema, { credentialId })),
      order: (credentialIds) => this.routes.send('PUT', `${path}/order`, { credentialIds }),
      remove: async (credentialId) => void (await this.routes.json('DELETE', `${path}/${segment(credentialId)}`, removedAccountSchema)),
      signIn: async () => this.signInOf((await this.routes.json('POST', `${path}/login`, loginStartedSchema)).login.id),
    }
  }

  /** Signs in to a subscription, which makes its entry. */
  async signIn(login: SubscriptionLogin): Promise<SignIn> {
    return this.signInOf((await this.routes.json('POST', '/providers/subscription-login', loginStartedSchema, login)).login.id)
  }

  private signInOf(id: string): SignIn {
    const path = `/providers/subscription-login/${segment(id)}`
    return {
      id,
      submitCode: async (code) => void (await this.routes.json('POST', `${path}/code`, codeTakenSchema, { code })),
      cancel: () => this.routes.send('DELETE', path),
    }
  }
}

/** Where and how `device.exec` runs a script. */
export interface ExecOptions {
  /** The directory it starts in; the home directory the device's runner reported by default. */
  cwd?: string
  /** The variables it starts with, above the device's own environment. */
  env?: Record<string, string>
  /** What it reads on its standard input, which then ends. */
  stdin?: string
  /** How many milliseconds it may run before it is stopped on the device; 0, the default, or `Infinity` for no limit. */
  timeout?: number
  /** Stops it; the run rejects with its reason, as a `fetch` does. */
  signal?: AbortSignal
}

/** How a script `device.exec` ran exited, and what it printed, read as UTF-8. */
export interface ExecResult {
  /** Its exit status, as the agent's shell reports one. */
  exitCode: number
  stdout: string
  stderr: string
  /** The bytes missing from `stdout` and `stderr`: a device keeps 16 MiB of a script's output and leaves out the middle of more, and a read of it that failed keeps only its first and newest bytes. */
  leftOutBytes: number
}

/** A device of the core, which a runner serves: one that dialed the process, or over a channel the program gives it. */
export class Device {
  constructor(
    private readonly listed: DeviceDto,
    private readonly routes: CoreRoutes,
    private readonly process: Process,
    private readonly runner: () => string,
    private readonly dir: string,
    private readonly online: (id: string, ended: Promise<void>) => Promise<void>,
  ) {}

  get id(): string {
    return this.listed.id
  }

  /**
   * Carries the device's channel on `stream`, a runner's standard output and
   * input, both ways; resolves once the device is online, so a conversation
   * can run on it, and `ended` once either end closed. It fails when the
   * channel ends before the device came online.
   */
  async connect(stream: { readable: Readable; writable: Writable }): Promise<{ ended: Promise<void> }> {
    const channel = await this.process.connect(this.routes.path(`/devices/${segment(this.id)}/runner`), 'h2c')
    stream.readable.pipe(channel)
    channel.pipe(stream.writable)
    const ended = new Promise<void>((resolve) => {
      channel.once('close', resolve)
      stream.readable.once('close', () => {
        channel.close()
        resolve()
      })
    })
    try {
      await this.online(this.id, ended)
    } catch (error) {
      channel.close()
      throw error
    }
    return { ended }
  }

  /**
   * Starts `command`, a `jush-runner run --stdio --home <dir>` and whatever
   * reaches it, such as `docker exec -i box jush-runner run --stdio --home
   * /var/lib/jush/<device>`, and connects its standard output and input; the
   * runner of each device keeps its state in a directory of its own. With no
   * command, the package's own runner runs on the program's machine, its
   * state in the core's directory. Resolves once the device is online, as
   * `connect` does; a command that exits before is stopped and fails it.
   */
  async spawn(command?: readonly string[]): Promise<ChildProcess> {
    const [executable, ...args] = command ?? [this.runner(), 'run', '--stdio', '--home', join(this.dir, 'devices', this.id)]
    if (executable === undefined) {
      throw new Error('The command is empty')
    }
    const child = spawn(executable, args, { stdio: ['pipe', 'pipe', 'inherit'] })
    // A command that cannot start, as one that is missing, fails the spawn;
    // a later error is the program's, as Node's own `spawn` leaves it.
    await new Promise<void>((resolve, reject) => {
      child.once('spawn', () => {
        child.off('error', reject)
        resolve()
      })
      child.once('error', reject)
    })
    let connected: { ended: Promise<void> }
    try {
      connected = await this.connect({ readable: child.stdout!, writable: child.stdin! })
    } catch (error) {
      child.kill()
      throw error
    }
    // The runner ends with its channel.
    void connected.ended.then(() => child.stdin?.end())
    return child
  }

  /**
   * Runs `script` on the device, outside any conversation, as the agent's
   * shell runs a command, and resolves once it exited, whatever its status.
   * Rejects with a `JushError` when it has no status: `device_offline`, when
   * it never ran; `command_timed_out`, when it ran past `timeout` and was
   * stopped (`SIGTERM`, then `SIGKILL` 5 seconds later), with what it printed
   * in `body.output`; `device_lost`, when the device lost it or its input was
   * cut; `backend_closing`, when the core closed while it ran, whose device
   * stops it when it connects again.
   */
  async exec(script: string, options: ExecOptions = {}): Promise<ExecResult> {
    const { timeout, signal, ...where } = options
    // 0 is no limit, as Node's `child_process` reads it, and so is any
    // number that is not finite. The backend takes whole milliseconds, so a
    // fraction rounds up.
    const limited = timeout !== undefined && timeout !== 0 && Number.isFinite(timeout)
    const body: DeviceExec = { script, ...where, ...(limited ? { timeoutMs: Math.ceil(timeout) } : {}) }
    const ran = await this.routes.json('POST', `/devices/${segment(this.id)}/exec`, deviceExecResultSchema, body, signal)
    return { exitCode: ran.exitCode, ...ran.output }
  }

  /**
   * Revokes the device, as the app's Revoke does: its token opens nothing
   * from then on, the commands running on it are lost, and the commands of
   * a conversation whose target it is fail until `update({ target })` names
   * another. A runner connected to it exits and removes its `--home`, or
   * only its own files there when `--home` is the home directory, holds it,
   * or is a filesystem's root, such as a volume's mount point.
   */
  async revoke(): Promise<void> {
    await this.routes.send('DELETE', `/devices/${segment(this.id)}`)
  }

  /** Renames the device, or changes how the core reaches it. */
  async update(patch: ChangeDevice): Promise<DeviceDto> {
    return (await this.routes.json('PATCH', `/devices/${segment(this.id)}`, deviceAnswerSchema, patch)).device
  }

  /** Asks the device's runner to run as an administrator, as the user allows on the device. */
  async elevate(): Promise<ElevationOutcome> {
    return (await this.routes.json('POST', `/devices/${segment(this.id)}/elevation`, deviceElevationSchema)).outcome
  }

  /** The device's files, by absolute paths. */
  get files(): DeviceFiles {
    return deviceFiles(this.routes, this.id)
  }

  /** The workflows a new conversation working in the absolute `directory` on the device would offer. */
  workflows(directory: string): Promise<KeptWorkflowList> {
    return this.routes.json('GET', `/devices/${segment(this.id)}/workflows${query({ directory })}`, keptWorkflowListSchema)
  }
}

export class Devices {
  constructor(
    private readonly routes: CoreRoutes,
    private readonly process: Process,
    private readonly runner: () => string,
    private readonly dir: string,
    private readonly online: (id: string, ended: Promise<void>) => Promise<void>,
  ) {}

  /** The core's devices, as the product state lists them; `of` gives the one a listing names. */
  async list(): Promise<DeviceDto[]> {
    return (await this.routes.json('GET', '/devices', devicesSchema)).devices
  }

  /** Creates a device, which serves the core once a runner connects to it. */
  async create(): Promise<Device> {
    const { device } = await this.routes.json('POST', '/devices', deviceAnswerSchema)
    return this.of(device)
  }

  /**
   * Pairs the runner that dialed a process on its own and waits under
   * `code`, the pairing code it printed: the device is made in this core,
   * and the runner keeps its token.
   */
  async claim(code: string): Promise<Device> {
    const { device } = await this.routes.json('POST', '/devices/claim', deviceAnswerSchema, { code })
    return this.of(device)
  }

  /** The device `listed` is, as the product state lists it. */
  of(listed: DeviceDto): Device {
    return new Device(listed, this.routes, this.process, this.runner, this.dir, this.online)
  }
}
