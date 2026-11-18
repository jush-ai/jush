/**
 * Work groups: an agent often takes many steps in a row, and listed one per row
 * a run would push the answer off the screen, so the transcript shows each run
 * of consecutive steps as one row.
 *
 * The backend decides which blocks make a run, once. Each block of a run
 * carries `run`, the id of the run's first block, set as the block is stored,
 * so the page groups by it and judges nothing. A run's start depends only on
 * the blocks before it, so `run` never changes.
 *
 * What joins and ends a run:
 *
 * - Every step joins its run as it appears, also while it still runs: a call
 *   being written or waiting for its result, a call whose command runs on
 *   after it returned, thinking being written. A run is one row from its first
 *   step to its last, so nothing under it comes and goes as its steps start
 *   and end.
 * - What ends a run is what the reader reads: the agent's text and the user's
 *   own words and decisions, never folded.
 * - A `resume` block ends none, as it shows nothing: the work after a Continue
 *   or a Resume joins the run before it.
 *
 * Thinking that streams never opens by itself, since folding itself when it
 * ends would shrink the list with nothing the reader did. Thinking blocks next
 * to each other show as one step, while each stays its own block with its
 * signature for replay: only the page joins them.
 *
 * A run's time is the union of its steps' spans, each from its start to its
 * end, so time that passed counts once however many steps ran in it. A call
 * that started a command ends where the time its row shows for the command
 * ends, also when the command went on in the background after the call
 * returned; another call ends where it returned (`endedAt`), and a thinking
 * block where its last piece came (`endedAt`). The wait for the model between
 * steps is not counted.
 */
import type { HeldBlock, TextArrival } from '@jush-ai/conversation-client'
import type { MessageListBlock } from './pending-steers'
import type { ShownBlock } from './whole-blocks'
import { upperFirst } from '@jush-ai/utils'
import { formatSpan } from '../ui/dates'
import { t } from './locales/text'
import { parseToolCallInput } from './block-helpers'
import type { LiveCallLookup } from './live-calls'
import { isTerminalRunning, shellRowRunning, shellRowTime } from './terminals'
import { reportMark, reportShown } from './command-reports'
import { receiptRow } from './agent-receipts'
import { shellStatusLooks, toolRenderKind } from './tool-rendering'
import { searchKind } from './opaque-steps'
import type { ActivitySlotState } from './useActivitySlot'

/**
 * A step of the agent's work: a thinking block, a tool call, a call the model
 * is still writing, a step its vendor ran, a command's notice (a `wakeup`
 * that shows), or an agent's receipt: an update or a child's end. The receipts of the user's own
 * decisions are not steps: like the user's words, they end a run.
 */
export type WorkStep = Extract<MessageListBlock, { type: 'thinking' | 'tool_call' | 'pending_call' | 'opaque' | 'wakeup' | 'agent_message' }>

/**
 * A step that tells of work done elsewhere: a command's notice or another
 * agent's receipt.
 */
export type NoticeStep = Extract<WorkStep, { type: 'wakeup' | 'agent_message' }>

/** A call: one written whole, or one the model is still writing. */
type CallStep = Extract<WorkStep, { type: 'tool_call' | 'pending_call' }>

/**
 * The steps of a run, which the transcript shows as one row: a lone step as
 * its own row, unless the row says the turn's wait after it; more as what
 * they did, or at the end of a running turn as what the turn does now, which
 * opens to every step.
 */
export interface WorkGroupBlock {
  type: 'work_group'
  id: string
  steps: WorkStep[]
  /** The run is the work at the end of a running turn. */
  live: boolean
  /**
   * The turn's wait for the model after the run's steps, which the run's row
   * says in place of a tail row of its own (`useMessageRows`).
   */
  wait?: ActivitySlotState
}

/**
 * Whether a block the transcript shows is a step of the work: one the
 * backend stored in a run, or a call the model is still writing, which joins
 * the run at the transcript's end.
 */
export function isWorkStep(block: MessageListBlock): block is WorkStep {
  return block.type === 'pending_call' || ('run' in block && block.run !== undefined)
}

/**
 * The run a step belongs to; none for a call being written, which joins
 * whichever run it follows.
 */
function runOf(step: WorkStep): string | undefined {
  return step.type === 'pending_call' ? undefined : step.run
}

export function isNoticeStep(step: WorkStep): step is NoticeStep {
  return step.type === 'wakeup' || step.type === 'agent_message'
}

/**
 * Whether `step` shows the block `id`: it is that block, or thinking that joins
 * it.
 */
export function stepHolds(step: WorkStep & Pick<ShownBlock, 'parts'>, id: string): boolean {
  return step.id === id || (step.parts?.some((part) => part.id === id) ?? false)
}

/**
 * What names a step across its forms: a call keeps its key from being written
 * to its block, so its row never rolls again.
 */
export function stepKey(step: WorkStep): string {
  switch (step.type) {
    case 'tool_call':
      return step.toolUseId
    case 'pending_call':
      return step.call.toolUseId
    default:
      return step.id
  }
}

/**
 * A thinking without text. One a page holds light comes without its text
 * and tells how long it is.
 */
function isEmptyThinking(block: MessageListBlock): boolean {
  if (block.type !== 'thinking') {
    return false
  }
  return 'light' in block && block.light ? block.textLength === 0 : block.text.trim() === ''
}

/**
 * The list's rows with the work grouped: the steps of one run, as the backend
 * stored each step's run, and the calls being written after them.
 *
 * - A thinking block without text shows only while it is the newest block of a
 *   running turn, as what the agent is doing. Once anything follows it or the
 *   turn ends, it has nothing to show and goes.
 * - Thinking blocks next to each other are one step.
 * - Every step of a run is in its run's row, the steps that still run with
 *   those that ended, so the row says what the run does now while it runs
 *   (`stepNow`).
 * - A run's row is named after its first block (`runRowId`), so it stays one
 *   row under one name whichever of its steps ends first, as parallel calls
 *   return in any order, and whatever form its first block takes.
 */
export function groupWork(
  blocks: readonly MessageListBlock[],
  running: boolean,
): MessageListBlock[] {
  return groupRuns(blocks, running, new Map(), new Map())
}

/**
 * `groupWork` for a transcript that changes a little at a time, as a turn that
 * streams changes its last blocks many times a second.
 *
 * Each call groups the blocks it is given. A run whose blocks and whether it
 * is live are the ones the last call grouped keeps the rows it had then, the
 * same objects. A change at the end then regroups only the run
 * it is in, and the list redraws none of the rows above it. A block that
 * changes is replaced, never changed in place (`applyTranscriptPatches`), so
 * the same block is the same content.
 */
export function workGrouping(): (blocks: readonly MessageListBlock[], running: boolean) => MessageListBlock[] {
  let last = new Map<MessageListBlock, GroupedRun>()
  return (blocks, running) => {
    const next = new Map<MessageListBlock, GroupedRun>()
    const rows = groupRuns(blocks, running, last, next)
    last = next
    return rows
  }
}

/**
 * The rows a run of steps was grouped into, and what they were grouped from.
 */
interface GroupedRun {
  blocks: readonly MessageListBlock[]
  live: boolean
  rows: readonly MessageListBlock[]
}

/**
 * `blocks` grouped, each run's rows taken from `last`, by the run's first
 * block, when it was grouped from the same blocks, and kept in `kept`.
 */
function groupRuns(
  blocks: readonly MessageListBlock[],
  running: boolean,
  last: ReadonlyMap<MessageListBlock, GroupedRun>,
  kept: Map<MessageListBlock, GroupedRun>,
): MessageListBlock[] {
  const rows: MessageListBlock[] = []
  let index = 0
  while (index < blocks.length) {
    const block = blocks[index]!
    if (!isWorkStep(block)) {
      rows.push(block)
      index += 1
      continue
    }
    const run = runOf(block)
    let end = index + 1
    while (end < blocks.length) {
      const next = blocks[end]!
      if (!isWorkStep(next) || (next.type !== 'pending_call' && runOf(next) !== run))
        break
      end += 1
    }
    const live = running && end === blocks.length
    const runBlocks = blocks.slice(index, end)
    const before = last.get(block)
    const grouped = before && sameRun(before, runBlocks, live)
      ? before
      : { blocks: runBlocks, live, rows: runRows(runBlocks, live) }
    kept.set(block, grouped)
    rows.push(...grouped.rows)
    index = end
  }
  return rows
}

/** Whether `grouped` was grouped from the run `blocks`. */
function sameRun(
  grouped: GroupedRun,
  blocks: readonly MessageListBlock[],
  live: boolean,
): boolean {
  if (grouped.live !== live || grouped.blocks.length !== blocks.length)
    return false
  return grouped.blocks.every((block, at) => block === blocks[at])
}

/**
 * The row of one run, `blocks`, with every step it shows; none when it shows
 * none, as a run of thinking without text that ended.
 */
function runRows(
  blocks: readonly MessageListBlock[],
  live: boolean,
): MessageListBlock[] {
  const steps = joinThinking(blocks.filter((step, at, run): step is WorkStep =>
    isWorkStep(step) && !(isEmptyThinking(step) && !(live && at === run.length - 1))))
  return steps.length === 0
    ? []
    : [{ type: 'work_group', id: runRowId(blocks[0] as WorkStep), steps, live }]
}

/**
 * What the run at the end of a running turn does now: its newest step that
 * runs, a call being written or waiting for its result, the newest step's
 * thinking being written, or a vendor's step, as the newest of calls made
 * together may return before an older one; null for any other run, and while
 * the run's steps have all ended.
 */
export function stepNow(group: WorkGroupBlock): WorkStep | null {
  if (!group.live)
    return null
  const steps = group.steps
  for (let at = steps.length - 1; at >= 0; at -= 1) {
    if (stepRuns(steps[at]!, at === steps.length - 1))
      return steps[at]!
  }
  return null
}

/**
 * The wait for the model a run's row says: the turn's wait the server told
 * (`wait`), or, at the end of a running turn whose steps have all ended, the
 * wait the server is about to tell or has just ended before the model's next
 * block arrives, as the result of a call and the wait after it, or the wait's
 * end and the block that ends it, arrive apart. It counts no time.
 */
export function runWait(group: WorkGroupBlock): ActivitySlotState | null {
  return group.wait ?? (group.live && stepNow(group) === null ? BETWEEN_STEPS : null)
}

const BETWEEN_STEPS: ActivitySlotState = { kind: 'requesting', since: null }

/**
 * The step a run of one step shows as its own row; null for a run of more,
 * and for one whose row says the wait after its step (`runWait`).
 */
export function loneStep(group: WorkGroupBlock): WorkStep | null {
  return group.steps.length === 1 && runWait(group) === null ? group.steps[0]! : null
}

/**
 * Whether `step` of a run at the end of a running turn still runs by its
 * block: a call being written or waiting for its result, a vendor's step, or
 * thinking, `newest`, that is being written.
 */
function stepRuns(step: WorkStep, newest: boolean): boolean {
  switch (step.type) {
    case 'tool_call':
      return step.status === 'executing'
    case 'pending_call':
      return true
    case 'thinking':
      return newest
    case 'opaque':
      return step.status === 'running'
    default:
      return false
  }
}

type ThinkingStep = Extract<WorkStep, { type: 'thinking' }>
type HeldThinking = Extract<HeldBlock, { type: 'thinking' }>

/**
 * Thinking blocks next to each other, with no other step between them, as
 * the one step the page shows them as: named and started as the first, so
 * its row stays the row it was when the next one streams in and it ends
 * where the last ends, which makes its time theirs together; its text
 * theirs in order, a paragraph apart; light while any of its `parts` is,
 * which an opened row reads whole. Each stays its own block in the
 * transcript.
 */
function joinThinking(steps: readonly WorkStep[]): WorkStep[] {
  const joined: WorkStep[] = []
  let parts: HeldThinking[] = []
  const flush = () => {
    if (parts.length === 1)
      joined.push(parts[0]!)
    else if (parts.length > 1)
      joined.push(joinedThinking(parts))
    parts = []
  }
  for (const step of steps) {
    if (step.type === 'thinking') {
      parts.push(step)
      continue
    }
    flush()
    joined.push(step)
  }
  flush()
  return joined
}

/** The step `parts`, thinking blocks next to each other, show as. */
function joinedThinking(parts: readonly HeldThinking[]): HeldThinking & { parts: readonly HeldThinking[] } {
  let text = ''
  // Each piece of a part's text arrived where it now stands in the joined
  // text; what was there before the part, the separator included, had
  // arrived already, as the text of a part read whole has.
  const arrivals: TextArrival[] = []
  for (const part of parts) {
    if (part.text.trim() === '')
      continue
    if (text !== '')
      text += '\n\n'
    const offset = text.length
    arrivals.push({ end: offset, at: Number.NEGATIVE_INFINITY })
    for (const arrival of part.arrivals ?? [])
      arrivals.push({ end: offset + arrival.end, at: arrival.at })
    text += part.text
  }
  const { light: _light, arrivals: _arrivals, ...first } = parts[0]!
  return {
    ...first,
    text,
    endedAt: parts.at(-1)!.endedAt,
    textLength: parts.reduce((sum, part) => sum + (part.textLength ?? 0), 0),
    ...(parts.some((part) => part.light) ? { light: true } : {}),
    ...(parts.some((part) => part.arrivals) ? { arrivals } : {}),
    parts,
  }
}

/**
 * The time `group`'s steps took: the union of their spans, so commands that
 * run at once, or a command that runs on while the next steps work, count the
 * time that passed once, never more than passed between the run's first start
 * and its last end. A step's span runs from its start to its end. A call that
 * started a command ends where the time its shell row shows for that command
 * (`shellRowTime`) ends, so a command that went on in the background after the
 * call returned counts until it ended, and the run says the number the row
 * says; any other call ends where it returned, and a thinking where its last
 * piece came. The time the turn waited for the model between steps is no
 * step's. A notice or a receipt takes none.
 */
export function runTime(group: WorkGroupBlock, liveCalls: LiveCallLookup = () => undefined): number {
  const spans: [number, number][] = []
  group.steps.forEach((step) => {
    if (!('createdAt' in step))
      return
    const start = Date.parse(step.createdAt)
    const command = step.type === 'tool_call' ? shellRowTime(step, liveCalls(step.toolUseId)) : null
    const end = command?.kind === 'ran'
      ? start + command.ms
      : step.type === 'tool_call' || step.type === 'thinking' || step.type === 'opaque' ? parseEnd(step.endedAt) : null
    if (end !== null && end > start)
      spans.push([start, end])
  })
  let total = 0
  let reached = -Infinity
  for (const [start, end] of spans.toSorted((a, b) => a[0] - b[0])) {
    total += Math.max(0, end - Math.max(start, reached))
    reached = Math.max(reached, end)
  }
  return total
}

/** The time of an end, null where the step has none. */
function parseEnd(end: string | null): number | null {
  return end === null ? null : Date.parse(end)
}

/**
 * The run's calls that still run, by their block or, for a command that runs
 * on after its call returned, by its live frames: `running`, and `stopping`,
 * those whose stop waits for their runner to say that they ended.
 */
export function stepsRunning(group: WorkGroupBlock, liveCalls: LiveCallLookup = () => undefined): { running: number; stopping: number } {
  let running = 0
  let stopping = 0
  for (const step of group.steps) {
    if (step.type === 'tool_call') {
      const command = liveCalls(step.toolUseId)
      if (command?.stopping && isTerminalRunning(command.phase))
        stopping += 1
      else if (shellRowRunning(step.status, command))
        running += 1
    } else if (step.type === 'opaque' && step.status === 'running') {
      running += 1
    }
  }
  return { running, stopping }
}

/**
 * What a run of more than one step says: what it did, then its total time in
 * the same sentence, Ran 3 commands for 1m 32s, as Thought for 5s says it; a
 * run under a second says no time, as Thought briefly names none. While steps
 * of it still run, it says how many in place of the time, Ran 3 commands, 1
 * still running, whose own tag counts the time.
 */
export function runLabel(group: WorkGroupBlock, liveCalls: LiveCallLookup = () => undefined): string {
  const what = workSummary(group.steps)
  const { running, stopping } = stepsRunning(group, liveCalls)
  if (running > 0 || stopping > 0) {
    const still = [
      ...(running > 0 ? [t('transcript.work.stillRunning', { count: running }, running)] : []),
      ...(stopping > 0 ? [t('transcript.work.stillStopping', { count: stopping }, stopping)] : []),
    ]
    return joinPhrases([what, ...still])
  }
  const time = runTime(group, liveCalls)
  return time < 1000 ? what : t('transcript.work.took', { what, span: formatSpan(time) })
}

/**
 * The name of the row of the run that starts with `first`, by the name the
 * block keeps across its forms (`stepKey`): a call's from being written to its
 * block, and thinking's whether or not it shows.
 */
export function runRowId(first: WorkStep): string {
  return `run:${stepKey(first)}`
}

/**
 * What a call did, as a group's row counts it: a `shell` call whose script
 * only runs `jush shell status` checks the commands it names; any other
 * `shell` call, or one still being written, runs a command of its own; a call
 * of another tool uses that tool.
 */
type CallKind =
  | { kind: 'run' }
  | { kind: 'check', commands: string[] }
  | { kind: 'generic', tool: string }
  | { kind: 'web' | 'page' | 'x' | 'media' }

function callKind(step: CallStep): CallKind {
  const tool = step.type === 'tool_call' ? step.toolName : step.call.toolName
  if (toolRenderKind(tool) === 'generic')
    return { kind: 'generic', tool }
  const script = step.type === 'tool_call' ? parseToolCallInput(step).script : undefined
  const looked = typeof script === 'string' ? shellStatusLooks(script) : null
  return looked === null ? { kind: 'run' } : { kind: 'check', commands: looked }
}

/**
 * What a vendor's step did, as a group's row counts it: a search of the web,
 * a page read, a search of X, or, of a kind Jush does not know, a use of that
 * tool at its vendor.
 */
function opaqueKind(step: Extract<WorkStep, { type: 'opaque' }>): CallKind {
  switch (step.view.kind) {
    case 'search':
      return { kind: searchKind(step.view.action) }
    case 'other':
      return { kind: 'generic', tool: step.view.name }
    case 'reasoning':
      return { kind: 'generic', tool: '' }
  }
}

/**
 * A run's calls by what they did, in the order the run first did each, with the
 * commands each kind touched.
 */
interface CallGroup {
  kind: CallKind['kind']
  tool: string
  touched: Set<string>
}

/**
 * The run's calls by what they did, in the order the run first did each:
 * a run counts a command per call, a check the commands its calls looked
 * at, each once, and another tool its calls. A failed call counts as any
 * other.
 */
function callGroups(steps: readonly WorkStep[]): CallGroup[] {
  const groups = new Map<string, CallGroup>()
  for (const step of steps) {
    if (step.type === 'thinking' || isNoticeStep(step))
      continue
    const call = step.type === 'opaque' ? opaqueKind(step) : callKind(step)
    const key = call.kind === 'generic' ? `tool:${call.tool}` : call.kind
    const group = groups.get(key) ?? { kind: call.kind, tool: call.kind === 'generic' ? call.tool : '', touched: new Set<string>() }
    if (call.kind === 'check')
      call.commands.forEach((command) => group.touched.add(command))
    else
      group.touched.add(stepKey(step))
    groups.set(key, group)
  }
  return [...groups.values()]
}

/** How a row words each kind of call the run made. */
const WORDS = {
  run: (count: number) => t('transcript.work.ran', { count }, count),
  check: (count: number) => t('transcript.work.checked', { count }, count),
  generic: (_count: number, tool: string) => t('transcript.work.used', { tool }),
  web: (count: number) => t('transcript.work.searchedWeb', { count }, count),
  page: (count: number) => t('transcript.work.openedPages', { count }, count),
  x: (count: number) => t('transcript.work.searchedX', { count }, count),
  media: (count: number) => t('transcript.work.viewedMedia', { count }, count),
} satisfies Record<CallKind['kind'], (count: number, tool: string) => string>

/** The phrases of a row joined, as the language lists clauses. */
function joinPhrases(parts: readonly string[]): string {
  return parts.reduce((joined, part) => t('transcript.work.joined', { first: joined, next: part }))
}

/**
 * Each kind of call the run made, once, in the order it first made it, joined
 * as one phrase.
 */
function describeCalls(steps: readonly WorkStep[]): string {
  const parts = callGroups(steps).map((group) => WORDS[group.kind](group.touched.size, group.tool))
  return parts.length === 0 ? '' : upperFirst(joinPhrases(parts))
}

/**
 * What a notice or a receipt told: a command's end or an agent's end, each
 * by how it ended, as its own row says it, or an agent's update.
 */
type ToldEvent =
  | { subject: 'command', ending: 'finished' | 'failed' | 'stopped' | 'stopping' | 'lost' }
  | { subject: 'agent', ending: 'finished' | 'failed' | 'stopped' }
  | { subject: 'update' }

type Told = ToldEvent & {
  /** The commands or the agents it named, each once. */
  names: Set<string>
}

/**
 * The words of what the run was told: what it named, counted, and how it ended.
 */
function toldWords(told: Told): string {
  const count = told.names.size
  switch (told.subject) {
    case 'update':
      return t('transcript.work.heard', { count }, count)
    case 'command':
      return t(`transcript.work.command.${told.ending}`, { count }, count)
    case 'agent':
      return t(`transcript.work.agent.${told.ending}`, { count }, count)
  }
}

/**
 * What the run's notices and receipts told, each kind once, by how its
 * command or agent ended, in the order the run first had it, counting the
 * commands and the agents they named, each once: a run without a call reads
 * them, as nothing else says what happened.
 */
function describeNotices(steps: readonly WorkStep[]): string {
  const told = new Map<string, Told>()
  const add = (event: ToldEvent, name: string) => {
    const key = event.subject === 'update' ? event.subject : `${event.subject} ${event.ending}`
    const entry: Told = told.get(key) ?? { ...event, names: new Set<string>() }
    entry.names.add(name)
    told.set(key, entry)
  }
  for (const step of steps) {
    if (step.type === 'wakeup') {
      for (const report of step.reports.filter(reportShown))
        add({ subject: 'command', ending: reportMark(report)?.kind ?? 'finished' }, report.commandId)
    }
    else if (step.type === 'agent_message') {
      const name = step.message.sender?.id ?? step.id
      // Another conversation's end is news of it, not a subagent's end.
      if (step.message.event.type === 'completion' && !step.message.sender?.conversation)
        add({ subject: 'agent', ending: receiptRow(step.message).tag ?? 'finished' }, name)
      else
        add({ subject: 'update' }, name)
    }
  }
  const parts = [...told.values()].map(toldWords)
  return parts.length === 0 ? '' : upperFirst(joinPhrases(parts))
}

/**
 * What an ended group says it did: each kind of call once, in the order the
 * run first made it; a run without a call, what its notices and receipts told.
 */
export function workSummary(steps: readonly WorkStep[]): string {
  return describeCalls(steps) || describeNotices(steps)
}
