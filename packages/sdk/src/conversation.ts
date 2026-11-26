/**
 * A conversation of a core, driven as the web app drives one: a
 * `ConversationClient` on the conversation's lane, which reads with the
 * core's routes, holds the root's transcript with the one patch applier, and
 * sends what the program asks.
 *
 * `run(input)` sends a message and waits for the turn it starts to end, as
 * the Codex SDK's `thread.run`; `events()` is everything the conversation's
 * lane tells, as an async iterator, with a `permission` event for each
 * request the conversation's metadata adds.
 */
import {
  addPage,
  applyTranscriptPatches,
  ConversationClient,
  createConversationClient,
  EMPTY_TRANSCRIPT,
  heldBlocks,
  reconnectWait,
  resetTranscript,
  windowOf,
  type ClientSessionEvent,
  type HeldTranscript,
  type TranscriptWindow,
} from '@jush-agent/conversation-client'
import {
  conversationDetailSchema,
  conversationFrameSchema,
  conversationUpdateSchema,
  conversationWorkflowListSchema,
  forkAnswerSchema,
  messageResultsSchema,
  pluginStateAnswerSchema,
  recentCostSchema,
  scheduleAnswerSchema,
  scheduleDtoSchema,
  scheduleRunDtoSchema,
  scheduleRunsSchema,
  subagentMessageAnswerSchema,
  transcriptPageSchema,
  turnStateSchema,
  type AbortResult,
  type AgentEntry,
  type Block,
  type ConversationDetail,
  type ConversationMetadata,
  type ConversationPatch,
  type ConversationUpdate,
  type ConversationWorkflowList,
  type CostWindow,
  type MessageResults,
  type PluginStateAnswer,
  type RecentCost,
  type PermissionRequest,
  type RequestedValues,
  type ScheduleAnswer,
  type ScheduleDto,
  type SchedulePatch,
  type ScheduleRunDto,
  type TurnOutcome,
  type TurnState,
} from '@jush-agent/protocol'
import { randomUUID } from 'node:crypto'
import { z } from 'zod'
import { conversationChanges, conversationFiles, type ConversationChanges, type ConversationFiles } from './files'
import { conversationHistory, type ConversationHistory } from './history'
import { content, type Input, type MessageOptions } from './input'
import { ConversationPage, type PageOptions } from './page'
import { JushError, query, segment, type CoreRoutes } from './routes'
import { sideQuestions, type SideQuestions } from './side-questions'

/** A plugin's part of one conversation: its state, and its methods about it. */
export interface ConversationPlugins {
  state(plugin: string): Promise<PluginStateAnswer>
  /** Calls the plugin's method `method` about the conversation, of its attached `device` when named. */
  call(plugin: string, method: string, input?: unknown, options?: { device?: string }): Promise<unknown>
}

/** The block that ended a turn, and how. */
interface TurnEnd {
  end: string
  outcome: TurnOutcome
}

/** A turn that ended: how, the blocks it wrote after its message, and their text. */
export interface Turn {
  /** The turn's id, its message's, which `fork({ before })` names it by. */
  id: string
  outcome: TurnOutcome
  blocks: Block[]
  text: string
}

/** A permission request of the conversation, which the program may decide. */
export interface Permission {
  readonly request: PermissionRequest
  /** Grants it; a credential's request takes the values asked for. */
  allow(values?: RequestedValues): Promise<void>
  deny(): Promise<void>
}

/** What `events()` yields: the lane's events, and each new permission request. */
export type ConversationEvent = ClientSessionEvent | { type: 'permission'; permission: Permission }

/**
 * Where a fork's history ends, one of the two: `through` a completed text of
 * the main agent, by its block's id, or `before` a turn of it, by the id of
 * the message that started it. `id` is the new conversation's.
 */
export type ForkOptions =
  | { through: string; before?: undefined; id?: string }
  | { before: string; through?: undefined; id?: string }

/**
 * A conversation's schedules, as `jush schedule` acts on them; each call is
 * one of the core's routes, and opens no lane.
 */
export interface ConversationSchedules {
  /** Makes one from the patch over the defaults `jush schedule create` takes; it is on unless the patch pauses it. */
  create(patch: SchedulePatch): Promise<ScheduleAnswer>
  get(id: string): Promise<ScheduleDto>
  /** Changes what the patch names. */
  update(id: string, patch: SchedulePatch): Promise<ScheduleAnswer>
  /** Removes it and its runs' records. */
  remove(id: string): Promise<void>
  /** Runs it now, of the repeat's task `task`; resolves with the run as it started, before its message names a turn. */
  run(id: string, task?: string): Promise<ScheduleRunDto>
  /** Its runs before the run `before`, newest first, 50 at most; the latest without it. */
  runs(id: string, before?: string): Promise<ScheduleRunDto[]>
}

export class Conversation {
  private client: ConversationClient | null = null
  private opening: Promise<ConversationClient> | null = null
  /** The root's transcript, as the lane keeps it. */
  private transcript: HeldTranscript = EMPTY_TRANSCRIPT
  private metadata: ConversationMetadata | null = null
  /** The permission requests told already. */
  private readonly told = new Set<string>()
  private readonly listeners = new Set<(event: ConversationEvent) => void>()
  /** What waits for a change of the transcript or the metadata, and fails once the conversation closes. */
  private readonly waiters = new Set<{ changed: () => void; closed: () => void }>()
  /** Whether the lane is being opened again after it dropped. */
  private reopening = false
  /** Whether the lane dropped again while it was being opened again. */
  private droppedAgain = false

  readonly schedules: ConversationSchedules
  /** Any agent's transcript by pages, and the turns and commands it names. */
  readonly history: ConversationHistory
  readonly sideQuestions: SideQuestions
  /** The files of the conversation's devices. */
  readonly files: ConversationFiles
  /** The working tree's changes. */
  readonly changes: ConversationChanges
  readonly plugins: ConversationPlugins

  constructor(
    readonly id: string,
    private readonly routes: CoreRoutes,
    /** The core's conversations, which give one for each id and forget a deleted one. */
    private readonly registry: { open(id: string): Conversation; removed(id: string): void },
  ) {
    this.history = conversationHistory(routes, id)
    this.sideQuestions = sideQuestions(routes, id)
    this.files = conversationFiles(routes, id)
    this.changes = conversationChanges(routes, id)
    const plugin = (name: string) => `/conversations/${segment(id)}/plugins/${segment(name)}`
    this.plugins = {
      state: (name) => routes.json('GET', `${plugin(name)}/state`, pluginStateAnswerSchema),
      call: (name, method, input = {}, options = {}) => routes.json(
        'POST',
        `${plugin(name)}/calls/${segment(method)}${query({ device: options.device })}`,
        z.unknown(),
        input,
      ),
    }
    const path = `/conversations/${segment(id)}/schedules`
    const one = (schedule: string) => `${path}/${segment(schedule)}`
    this.schedules = {
      create: (patch) => routes.json('POST', path, scheduleAnswerSchema, patch),
      get: (schedule) => routes.json('GET', one(schedule), scheduleDtoSchema),
      update: (schedule, patch) => routes.json('PATCH', one(schedule), scheduleAnswerSchema, patch),
      remove: (schedule) => routes.send('DELETE', one(schedule)),
      run: (schedule, task) => routes.json('POST', `${one(schedule)}/runs`, scheduleRunDtoSchema, task === undefined ? {} : { task }),
      runs: async (schedule, before) => {
        return (await routes.json('GET', `${one(schedule)}/runs${query({ before })}`, scheduleRunsSchema)).runs
      },
    }
  }

  /** Opens the conversation's lane, once; each action opens it first. */
  open(): Promise<ConversationClient> {
    this.opening ??= this.connect().catch((error: unknown) => {
      // The next action opens it anew.
      this.opening = null
      throw error
    })
    return this.opening
  }

  private async connect(): Promise<ConversationClient> {
    const path = `/conversations/${segment(this.id)}`
    const client = createConversationClient(this.routes.connection, this.id, {
      reader: {
        conversation: async (signal) => {
          const detail = await this.routes.json('GET', path, conversationDetailSchema, undefined, signal)
          this.takeMetadata(detail.metadata)
          return { agents: detail.agents, entries: detail.entries, tags: { metadata: detail.metadataTag } }
        },
        transcript: async (node, signal) => {
          const page = await this.routes.json('GET', `${path}/agents/${segment(node)}/transcript${query({ latest: true })}`, transcriptPageSchema, undefined, signal)
          if (!page.version) {
            throw new Error('The latest page names no version of its transcript')
          }
          return { ...page, version: page.version }
        },
      },
      // The program follows the root and its agents as a page that shows
      // the conversation does: a read again after a stale version starts
      // at once.
      shows: (node) => node === null || node === this.id,
      conversationFrame: (frame) => {
        const parsed = conversationFrameSchema.safeParse(frame)
        if (!parsed.success) {
          return null
        }
        const data = parsed.data
        if (data.type === 'conversation') {
          return { apply: () => this.takeMetadata(data.metadata), tagged: { value: 'metadata', tag: data.etag } }
        }
        return { apply: () => {} }
      },
    })
    client.subscribe((event) => this.receive(event))
    this.client = client
    try {
      await client.open({ root: this.id })
    } catch (error) {
      client.dispose()
      if (this.client === client) {
        this.client = null
      }
      throw error
    }
    return client
  }

  private receive(event: ClientSessionEvent): void {
    switch (event.type) {
      // The lane dropped, as when the program's connection to the process
      // was made again, or another client closed the tree: it opens again,
      // as a page's view does, and the agents ran on meanwhile.
      case 'closed':
      case 'disconnected':
        this.reopen()
        break
      case 'transcript_reset':
        if (event.node === this.id) {
          this.transcript = resetTranscript(this.transcript, event, event.asked)
        }
        break
      case 'transcript_patch':
        if (event.node === this.id) {
          this.transcript = applyTranscriptPatches(this.transcript, event.patches, performance.now(), event.version)
        }
        break
    }
    this.emit(event)
    this.changed()
  }

  /** Opens the lane of the client again until it opens, unless the program closes the conversation first. */
  private reopen(): void {
    const client = this.client
    if (client === null) {
      return
    }
    if (this.reopening) {
      // The lane dropped again while it opened: the opening goes round once
      // more, since what it read meanwhile settles without failing.
      this.droppedAgain = true
      return
    }
    this.reopening = true
    const opening = (async () => {
      try {
        for (let failures = 0; ; failures++) {
          try {
            this.droppedAgain = false
            await client.open({ root: this.id })
            // What the core restored since is read before the next action.
            await client.reopened()
            if (!this.droppedAgain) {
              return client
            }
          } catch (error) {
            if (this.client !== client) {
              throw error
            }
            await new Promise((resolve) => setTimeout(resolve, reconnectWait(failures)))
          }
        }
      } finally {
        this.reopening = false
        // What waits judges the lane as it now holds the conversation.
        this.changed()
      }
    })()
    this.opening = opening
    opening.catch(() => {})
  }

  private takeMetadata(metadata: ConversationMetadata): void {
    this.metadata = metadata
    for (const request of metadata.permissions.requests) {
      if (!this.told.has(request.id)) {
        this.told.add(request.id)
        this.emit({ type: 'permission', permission: this.permission(request) })
      }
    }
    this.changed()
  }

  private changed(): void {
    for (const waiter of [...this.waiters]) {
      waiter.changed()
    }
  }

  private permission(request: PermissionRequest): Permission {
    const decide = (body: unknown) => this.routes.send('POST', `/conversations/${segment(this.id)}/permissions/requests/${segment(request.id)}`, body)
    return {
      request,
      allow: (values) => decide({ decision: 'allow', ...(values === undefined ? {} : { credential: values }) }),
      deny: () => decide({ decision: 'deny' }),
    }
  }

  private emit(event: ConversationEvent): void {
    for (const listener of [...this.listeners]) {
      listener(event)
    }
  }

  /** The permission requests waiting for a decision. */
  get permissions(): Permission[] {
    return (this.metadata?.permissions.requests ?? []).map((request) => this.permission(request))
  }

  /** The root's blocks the conversation holds, oldest first. */
  get blocks(): Block[] {
    return heldBlocks(this.transcript)
  }

  /**
   * The conversation's agents as its lane holds them: the root's entry,
   * whose id is the conversation's, each subagent's, whose id `sendToAgent`
   * takes, and each side question's. Empty until the lane opens.
   */
  get agents(): AgentEntry[] {
    return this.client?.agentEntries() ?? []
  }

  /**
   * Sends a message; resolves once it is in the transcript or the queue.
   * The conversation takes it once however often it is sent under
   * `messageId`, which messages and steers share.
   */
  async send(input: Input, messageId: string = randomUUID(), options: MessageOptions = {}): Promise<void> {
    const client = await this.open()
    await client.submit(content(input), messageId, [], options)
  }

  /**
   * Changes what `patch` names of the conversation, as the app's settings
   * change it, and resolves with how each field went: one field can be
   * refused while the others apply.
   */
  async update(patch: ConversationPatch): Promise<ConversationUpdate> {
    return this.routes.json('PATCH', `/conversations/${segment(this.id)}`, conversationUpdateSchema, patch)
  }

  /** The conversation as the list shows it, with its metadata and its agents. */
  get(): Promise<ConversationDetail> {
    return this.routes.json('GET', `/conversations/${segment(this.id)}`, conversationDetailSchema)
  }

  /** Deletes the conversation, with its history and its files. */
  async remove(): Promise<void> {
    await this.routes.send('DELETE', `/conversations/${segment(this.id)}`)
    this.close()
    this.registry.removed(this.id)
  }

  /**
   * The conversation's page for `<jush-conversation src>`, which the
   * program's server answers below `options.base` with `fetch` and `accept`,
   * doing what `options.allow` names; make one for each request.
   */
  page(options: PageOptions): ConversationPage {
    return new ConversationPage(this.routes, this.id, options)
  }

  /**
   * A new conversation holding this one's history through the completed text
   * `through`, or everything before the turn `before`, which the message of
   * that id started; under `id`, made unless named, which a retry repeats.
   */
  async fork(options: ForkOptions): Promise<Conversation> {
    const answer = await this.routes.json('POST', `/conversations/${segment(this.id)}/fork`, forkAnswerSchema, {
      id: options.id ?? randomUUID(),
      blockId: options.through,
      beforeTurn: options.before,
    })
    return this.registry.open(answer.conversation.id)
  }

  /** Opens the conversation's agents again with the core's plugins as they are now, as after `core.plugins.enable`. */
  reloadPlugins(): Promise<void> {
    return this.routes.send('POST', `/conversations/${segment(this.id)}/reload`)
  }

  /** The conversation's messages that match `text`. */
  search(text: string): Promise<MessageResults> {
    return this.routes.json('GET', `/conversations/${segment(this.id)}/search${query({ q: text })}`, messageResultsSchema)
  }

  /** What the calls of the last hour or day cost, by agent. */
  cost(window: CostWindow): Promise<RecentCost> {
    return this.routes.json('GET', `/conversations/${segment(this.id)}/cost/recent${query({ window })}`, recentCostSchema)
  }

  /** The workflows the conversation's agents can start, as its directory keeps them. */
  workflows(): Promise<ConversationWorkflowList> {
    return this.routes.json('GET', `/conversations/${segment(this.id)}/workflows`, conversationWorkflowListSchema)
  }

  /** Takes the pinned document `pin`, one of the metadata's `pins`, back from the conversation's agents. */
  unpin(pin: number): Promise<void> {
    return this.routes.send('DELETE', `/conversations/${segment(this.id)}/pins/${pin}`)
  }

  /** Detaches the device `device` the conversation brought in. */
  detach(device: string): Promise<void> {
    return this.routes.send('DELETE', `/conversations/${segment(this.id)}/devices/${segment(device)}`)
  }

  /**
   * Sends `text` to the subagent `node`, the id of one of `agents` whose
   * role is a subagent's, which takes it once however often it is sent
   * under `messageId`: a live one as a steer, and an ended one is resumed
   * with it. Resolves with the message's id, the one made for it when
   * `messageId` is absent.
   */
  async sendToAgent(node: string, text: string, messageId?: string, options: MessageOptions = {}): Promise<string> {
    const path = `/conversations/${segment(this.id)}/agents/${segment(node)}/messages`
    const { id } = await this.routes.json('POST', path, subagentMessageAnswerSchema, {
      id: messageId,
      text,
      senderId: options.senderId,
      assignmentId: options.assignmentId,
    })
    return id
  }

  /**
   * Steers the running turn with `input`; resolves once the turn took it.
   * The conversation takes it once however often it is sent under
   * `steerId`, which messages and steers share.
   */
  async steer(input: Input, steerId: string = randomUUID(), options: MessageOptions = {}): Promise<void> {
    const client = await this.open()
    await client.steer(content(input), steerId, [], options)
  }

  async abort(): Promise<AbortResult> {
    return (await this.open()).abort()
  }

  async retry(): Promise<void> {
    return (await this.open()).retry()
  }

  async resume(): Promise<void> {
    return (await this.open()).resume()
  }

  async compact(): Promise<void> {
    return (await this.open()).compact()
  }

  /**
   * Sends `input`, and answers the turn it starts once the turn ended: once
   * the conversation's last ended turn is its own, as a turn the root
   * resumes ends anew. When the lane's view cannot tell, as after its
   * connection dropped while the turn ended, it asks the conversation where
   * the turn stands, again at each view that cannot tell. It reads the
   * transcript back to the message when the turn ended past what the lane
   * holds. It fails when the message left the queue without running, as an
   * abort's clearing of the queue removes it.
   */
  async run(input: Input, options: MessageOptions = {}): Promise<Turn> {
    const messageId = randomUUID()
    let sent = false
    // How the turn ended, as the conversation last answered when asked.
    let answered: TurnEnd | null = null
    // The view the conversation was asked about last: each view asks once.
    let asked: string | null = null
    // Whether a page before the turn's end is being read.
    let reading = false
    // The reads in a row that failed, whose backoff the next one waits.
    let failures = 0
    let waiter = { changed: () => {}, closed: () => {} }
    const ended = new Promise<Turn>((resolve, reject) => {
      // What a read brings is taken while the run still waits.
      const again = () => {
        if (this.waiters.has(waiter)) {
          waiter.changed()
        }
      }
      // A read that failed, as across a drop, is made again after the
      // backoff, since an idle conversation may change no more.
      const later = () => {
        failures += 1
        setTimeout(again, reconnectWait(failures))
      }
      const ask = (view: string) => {
        asked = view
        this.turnState(messageId).then(
          (state) => {
            // A later view asked again, and its answer counts.
            if (asked !== view) {
              return
            }
            if (state === null) {
              this.waiters.delete(waiter)
              reject(new Error('The message left the queue without running'))
            } else if (state.state === 'ended') {
              failures = 0
              answered = { end: state.end, outcome: state.outcome }
              again()
            } else {
              // The view, idle without the message, and the answer disagree
              // for a moment, as while a restored root takes its queue back:
              // the same view asks again after the backoff.
              asked = null
              later()
            }
          },
          () => {
            if (asked === view) {
              asked = null
              later()
            }
          },
        )
      }
      const readBefore = (window: TranscriptWindow) => {
        reading = true
        this.readBefore(window).then(
          () => {
            reading = false
            failures = 0
            again()
          },
          () => {
            reading = false
            later()
          },
        )
      }
      waiter = {
        changed: () => {
          const last = this.metadata?.lastTurn
          const end = last?.turnId === messageId ? { end: last.id, outcome: last.outcome } : answered
          if (end) {
            const turn = this.turnOf(messageId, end)
            if (turn) {
              this.waiters.delete(waiter)
              resolve(turn)
              return
            }
            const short = this.shortOf(messageId, end)
            if (short && !reading) {
              readBefore(short)
            }
          }
          const view = sent ? this.unaccounted(messageId) : null
          if (view !== null && view !== asked) {
            ask(view)
          }
        },
        closed: () => reject(new Error('The conversation was closed before its turn ended')),
      }
    })
    // Its failure is the caller's once the message is sent, and nobody's
    // when the send fails, as a close while it sends fails both.
    ended.catch(() => {})
    this.waiters.add(waiter)
    try {
      await this.send(input, messageId, options)
    } catch (error) {
      // A message never sent starts no turn to wait for.
      this.waiters.delete(waiter)
      throw error
    }
    sent = true
    waiter.changed()
    return ended
  }

  /**
   * The lane's view of the conversation when it leaves the turn of the sent
   * message `messageId` unaccounted for: the root idle, and the message
   * neither queued, nor starting, nor the last ended turn's, as when the
   * turn ended unseen or the message left the queue. Null when the view
   * accounts for it, or cannot tell yet: while the lane opens again, or
   * before it holds both the agents and the root's transcript current, as
   * after a drop.
   */
  private unaccounted(messageId: string): string | null {
    const client = this.client
    const agents = client?.heldAgents()
    if (this.reopening || !client || !agents || !client.holdsCurrent(this.id)) {
      return null
    }
    const root = this.agents.find((agent) => agent.role.kind === 'root')
    const phase = root?.phase ?? 'idle'
    const last = this.metadata?.lastTurn ?? null
    const accounted = (phase !== 'idle' && phase !== 'compacting_idle')
      || root?.queue.some((message) => message.id === messageId)
      || root?.starting?.id === messageId
      || last?.turnId === messageId
    return accounted ? null : JSON.stringify([agents, client.transcriptVersion(this.id), last?.id ?? null])
  }

  /** Where the turn `messageId` stands, as the conversation answers; null when it holds no turn of it. */
  private async turnState(messageId: string): Promise<TurnState | null> {
    const path = `/conversations/${segment(this.id)}/turns/${segment(messageId)}`
    try {
      return await this.routes.json('GET', path, turnStateSchema)
    } catch (error) {
      if (error instanceof JushError && error.code === 'turn_not_found') {
        return null
      }
      throw error
    }
  }

  /** The turn `messageId` started, which ended at `ending`; null until one window of the transcript holds both ends. */
  private turnOf(messageId: string, ending: TurnEnd): Turn | null {
    const blocks = windowOf(this.transcript, ending.end)?.blocks ?? []
    const at = blocks.findIndex((block) => block.type === 'user' && block.turnId === messageId)
    const end = blocks.findIndex((block) => block.id === ending.end)
    if (at < 0 || end <= at) {
      return null
    }
    const after = blocks.slice(at + 1, end + 1)
    const text = after.flatMap((block) => (block.type === 'text' ? [block.text] : [])).join('\n\n')
    return { id: messageId, outcome: ending.outcome, blocks: after, text }
  }

  /**
   * The window of the root's transcript that holds the turn's end `ending`
   * but not the message `messageId` that started it, when the transcript
   * goes on before it; null otherwise.
   */
  private shortOf(messageId: string, ending: TurnEnd): TranscriptWindow | null {
    const window = windowOf(this.transcript, ending.end)
    const started = window?.blocks.some((block) => block.type === 'user' && block.turnId === messageId)
    return window && window.start > 0 && !started ? window : null
  }

  /** Reads the page of the root's transcript before `window` into what the conversation holds. */
  private async readBefore(window: TranscriptWindow): Promise<void> {
    const root = segment(this.id)
    const edge = segment(window.blocks[0]!.id)
    const path = `/conversations/${root}/agents/${root}/transcript?before=${window.start}&edge=${edge}`
    const page = await this.routes.json('GET', path, transcriptPageSchema)
    this.transcript = addPage(this.transcript, page, true)
  }

  /**
   * Everything the conversation's lane tells from now on, until the loop
   * that reads it stops or the conversation closes.
   */
  async *events(): AsyncGenerator<ConversationEvent, void, undefined> {
    const waiting: ConversationEvent[] = []
    let wake: (() => void) | null = null
    let closed = false
    const listener = (event: ConversationEvent) => {
      waiting.push(event)
      wake?.()
    }
    const waiter = {
      changed: () => {},
      closed: () => {
        closed = true
        wake?.()
      },
    }
    this.listeners.add(listener)
    this.waiters.add(waiter)
    try {
      await this.open()
      for (;;) {
        const event = waiting.shift()
        if (event !== undefined) {
          yield event
          continue
        }
        if (closed) {
          return
        }
        await new Promise<void>((resolve) => (wake = resolve))
        wake = null
      }
    } finally {
      this.listeners.delete(listener)
      this.waiters.delete(waiter)
    }
  }

  /**
   * Stops following the conversation: its lane closes, and a `run` that
   * waits fails. Its agents run on in the core; the next action opens the
   * lane again.
   */
  close(): void {
    const client = this.client
    this.client = null
    this.opening = null
    client?.dispose()
    for (const waiter of [...this.waiters]) {
      this.waiters.delete(waiter)
      waiter.closed()
    }
  }
}
