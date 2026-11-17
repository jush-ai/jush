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
 * - A step joins its run once the agent no longer waits for it. What the
 *   agent waits for, a call being written or waiting for its result, or
 *   thinking being written, is a row of its own under the run, so at the end
 *   of a running turn the run is what has happened and the row under it what
 *   happens now. A call whose command runs on after it returned, as one the
 *   agent left running or stopped waiting for, is no longer waited for: it
 *   joins its run, which says how many of its commands still run.
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
import type { HeldBlock, TextArrival } from '@jush-agent/conversation-client'
import type { MessageListBlock } from './pending-steers'
import type { ShownBlock } from './whole-blocks'
import { upperFirst } from '@jush-agent/utils'
import { formatSpan } from '../ui/dates'
import { t } from './locales/text'
import { pageLanguage } from '../i18n/text'
import { parseToolCallInput } from './block-helpers'
import type { LiveCallLookup } from './live-calls'
import { isTerminalRunning, shellRowRunning, shellRowTime } from './terminals'
import { reportMark, reportShown } from './command-reports'
import { receiptRow } from './agent-receipts'
import { shellStatusLooks, toolRenderKind } from './tool-rendering'
import { searchKind } from './opaque-steps'

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
 * The steps of a run the agent no longer waits for, which the transcript
 * shows as one row: a lone step as its own row, more as what they did, which
 * opens to every step. A step the agent waits for is a row of its own under
 * it.
 */
export interface WorkGroupBlock {
  type: 'work_group'
  id: string
  steps: WorkStep[]
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
 * - A step joins its run's row once the agent no longer waits for it: a call
 *   once it returned, whether or not its command runs on, thinking once it is
 *   written. A step the agent waits for is a row of its own after the run's,
 *   as the step's own block: a call waiting for its result wherever its run
 *   stands, and at the end of a running turn a call being written or thinking
 *   being written.
 * - A run that holds only thinking shows no row while a step of it runs, until
 *   a step that says something joins it.
 * - A run's row is named after the run the backend stored (`runRowId`), so it
 *   stays one row whichever of its steps ends first, as parallel calls return
 *   in any order. The row a lone step ran in becomes the run's row by taking
 *   its place (`succeedKeys`).
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
      : { blocks: runBlocks, live, rows: runRows(runBlocks, live, before?.rows) }
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
 * The rows of one run, `blocks`: the row of the steps the agent no longer
 * waits for, then each step it waits for. The row is the one in `last`, the
 * run's rows before, while it holds the same steps, so what the row derives
 * from them is not derived again for each piece a step that runs streams.
 */
function runRows(
  blocks: readonly MessageListBlock[],
  live: boolean,
  last: readonly MessageListBlock[] = [],
): MessageListBlock[] {
  const rows: MessageListBlock[] = []
  const steps = joinThinking(blocks.filter((step, at, run): step is WorkStep =>
    isWorkStep(step) && !(isEmptyThinking(step) && !(live && at === run.length - 1))))
  const runs = (step: WorkStep, at: number) => stepRuns(step, live, at === steps.length - 1)
  const ended = steps.filter((step, at) => !runs(step, at))
  const runningSteps = steps.filter(runs)
  // Thinking alone is one step once joined, so every ended run that shows
  // anything is a row.
  if (ended.length > 0 && !((live || runningSteps.length > 0) && ended.every((step) => step.type === 'thinking'))) {
    const id = runRowId(runOf(ended[0]!) ?? stepKey(ended[0]!))
    const before = last.find((row): row is WorkGroupBlock => row.type === 'work_group' && row.id === id)
    rows.push(before && sameSteps(before.steps, ended) ? before : { type: 'work_group', id, steps: ended })
  }
  rows.push(...runningSteps)
  return rows
}

/**
 * Whether `next` are the steps `previous` were: the same blocks, a thinking
 * step joined anew of the same parts (`joinThinking`) included.
 */
function sameSteps(previous: readonly WorkStep[], next: readonly WorkStep[]): boolean {
  return previous.length === next.length && previous.every((step, at) => {
    const other = next[at]!
    if (step === other)
      return true
    const parts = 'parts' in step ? step.parts : undefined
    const otherParts = 'parts' in other ? other.parts : undefined
    return Array.isArray(parts) && Array.isArray(otherParts) && parts.length === otherParts.length
      && parts.every((part, index) => part === otherParts[index])
  })
}

/**
 * The transcript block whose state `row` follows, as whether it streams: a
 * thinking step joined of blocks next to each other follows its last part,
 * which streams while the step does; any other row its own block.
 */
export function followedBlockId(row: MessageListBlock): string {
  return row.type === 'thinking' && 'parts' in row && Array.isArray(row.parts) ? row.parts.at(-1)!.id : row.id
}

/**
 * Whether `step` still runs by its block: a call waiting for its result,
 * wherever it stands, and in a run at the end of a running turn, `live`, a
 * call being written or thinking, `newest`, that is being written.
 */
function stepRuns(step: WorkStep, live: boolean, newest: boolean): boolean {
  switch (step.type) {
    case 'tool_call':
      return step.status === 'executing'
    case 'pending_call':
      return live
    case 'thinking':
      return live && newest
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
 * The run's commands that still run after their calls returned, by their live
 * frames: `running`, and `stopping`, those whose stop waits for their runner
 * to say that they ended.
 */
export function stepsRunning(group: WorkGroupBlock, liveCalls: LiveCallLookup = () => undefined): { running: number, stopping: number } {
  let running = 0
  let stopping = 0
  for (const step of group.steps) {
    if (step.type !== 'tool_call')
      continue
    const command = liveCalls(step.toolUseId)
    if (command?.stopping && isTerminalRunning(command.phase))
      stopping += 1
    else if (shellRowRunning(step.status, command))
      running += 1
  }
  return { running, stopping }
}

/**
 * What a run of more than one step says: what it did, then its total time in
 * the same sentence, Ran 3 commands for 1m 32s, as Thought for 5s says it; a
 * run under a second says no time, as Thought briefly names none. While
 * commands of it still run, it says how many in place of the time, Ran 3
 * commands, 1 still running, whose own tag counts the time.
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
 * The name of the row of the run `run`, the id of its first block, as the
 * backend stored it.
 */
export function runRowId(run: string): string {
  return `run:${run}`
}

/**
 * The name of the row `first` shows in as a step of its own: while it runs, and
 * as an ended run's lone step before any other step.
 */
export function workRowId(first: WorkStep): string {
  return `work:${stepKey(first)}`
}

/**
 * What a call did, as a group's row counts it: a `shell` call whose script
 * only runs `jush shell status` checks the commands it names; any other
 * `shell` call, or one still being written, runs a command of its own; a call
 * of another tool uses that tool; a vendor's step of a kind Jush does not
 * know uses that built-in tool.
 */
type CallKind =
  | { kind: 'run' }
  | { kind: 'check', commands: string[] }
  | { kind: 'generic' | 'builtin', tool: string }
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
 * built-in tool.
 */
function opaqueKind(step: Extract<WorkStep, { type: 'opaque' }>): CallKind {
  switch (step.view.kind) {
    case 'search':
      return { kind: searchKind(step.view.action) }
    case 'other':
      return { kind: 'builtin', tool: step.view.name }
    case 'reasoning':
      return { kind: 'generic', tool: '' }
  }
}

/**
 * A run's calls by what they did, in the order the run first did each, with the
 * commands each kind touched, or the built-in tools it used.
 */
interface CallGroup {
  kind: CallKind['kind']
  tool: string
  touched: Set<string>
}

/**
 * The run's calls by what they did, in the order the run first did each:
 * a run counts a command per call, a check the commands its calls looked
 * at, each once, the built-in tools their names, each once, and another
 * tool its calls. A failed call counts as any other.
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
    else if (call.kind === 'builtin')
      group.touched.add(call.tool)
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
  builtin: (count: number, tools: string) => t('transcript.work.usedBuiltin', { tools }, count),
} satisfies Record<CallKind['kind'], (count: number, tool: string) => string>

/**
 * Tools' names listed as the language lists things. A Chinese conjunction
 * between them, 和, gets the spaces Chinese text puts between its own words
 * and Latin ones, which Intl leaves out: read_file 和 grep_search.
 */
function listNames(names: Iterable<string>): string {
  return new Intl.ListFormat(pageLanguage(), { type: 'conjunction' })
    .formatToParts(names)
    .map((part) => part.type === 'literal' && /^\p{Script=Han}+$/u.test(part.value) ? ` ${part.value} ` : part.value)
    .join('')
}

/** The phrases of a row joined, as the language lists clauses. */
function joinPhrases(parts: readonly string[]): string {
  return parts.reduce((joined, part) => t('transcript.work.joined', { first: joined, next: part }))
}

/**
 * Each kind of call the run made, once, in the order it first made it, joined
 * as one phrase.
 */
function describeCalls(steps: readonly WorkStep[]): string {
  const parts = callGroups(steps).map((group) =>
    WORDS[group.kind](group.touched.size, group.kind === 'builtin' ? listNames(group.touched) : group.tool))
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
