<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { Layers } from '@lucide/vue'
import { ICON_PX } from '@jush-ai/web-ui/ui/icon-metrics'
import ActivityMark from '@jush-ai/web-ui/ui/ActivityMark.vue'
import Fold from '@jush-ai/web-ui/ui/Fold.vue'
import { CHROME_ENTER_MS, chromeEntrance } from '@jush-ai/web-ui/ui/chrome-enter'
import FunctionalBlock from './FunctionalBlock.vue'
import StepRow from './StepRow.vue'
import StepFace from './StepFace.vue'
import ActivityLabel from './ActivityLabel.vue'
import CommandTimeTag from './CommandTimeTag.vue'
import FileChangePills from './FileChangePills.vue'
import GoneMediaLines from './GoneMediaLines.vue'
import { stepIcon } from './tool-row-icon'
import { useLiveCalls } from '../live-calls'
import { loneStep, runLabel, runWait, stepKey, stepNow, stepsRunning, type WorkGroupBlock, type WorkStep } from '../work-groups'
import { pillAction, useEditSelection, useFilePills, useTranscript } from '../edit-selection'
import { useMessageFiles } from '../../markdown/message-files'
import { pillSelection, runFiles, runPills } from '../../files/request-changes'
import { foldedMedia, splitMedia } from '../tool-media'

/**
 * The steps of a run as one row, those that still run with those that ended.
 *
 * - A lone step is that step's own row, which opens to the step, also while
 *   it runs.
 * - At the end of a running turn more steps, or a lone step the turn waits for
 *   the model after, say what the turn does now, as the step's own row would
 *   say it: the step that runs (`stepNow`), or the wait for the model, rolling
 *   from one to the next; the row opens to every step.
 * - Otherwise more steps read what they did, and open to every step, each as
 *   its own row. While steps of it still run, as a command the agent left
 *   running, the row says how many, shimmers and wears the time of the one
 *   that has run longest.
 * - A step that joins never changes whether the row is open. One that joins
 *   while it is open enters as a row joining the transcript does, and a lone
 *   step's row that turns into the stack is open if the reader opened the
 *   step. A stack that turns back into its lone step, as when the answer ends
 *   the wait after it, folds: open, it listed the step, while the step's own
 *   row opens to the step's body, which the reader did not open.
 * - Folded, the row wears the pictures its steps returned, and the files they
 *   changed show under it. Open, each step has its own.
 */
const props = defineProps<{ group: WorkGroupBlock }>()
/** The row is open: the transcript's (`useOpenRuns`). */
const isOpen = defineModel<boolean>('open', { default: false })

const lone = computed(() => loneStep(props.group))
const wait = computed(() => runWait(props.group))
watch(lone, (step, before) => {
  if (step && !before && isOpen.value)
    isOpen.value = false
})
const now = computed(() => stepNow(props.group))
/** What the run did and the time its steps took. */
const liveCalls = useLiveCalls()
const label = computed(() => runLabel(props.group, liveCalls))
/**
 * `next`, or `previous` when it holds the same items: what derives from the
 * steps is not derived again for each piece a step streams.
 */
function kept<T>(previous: readonly T[] | undefined, next: readonly T[]): readonly T[] {
  return previous !== undefined && previous.length === next.length && next.every((item, at) => item === previous[at]) ? previous : next
}
/** The steps that made something, the calls and the notices, the same while only thinking or a call being written streams. */
const madeSteps = computed<readonly WorkStep[]>((previous) => kept(previous, props.group.steps.filter((step) => step.type === 'tool_call' || step.type === 'wakeup')))
const calls = computed(() => madeSteps.value.filter((step) => step.type === 'tool_call'))
const icon = computed(() => wait.value ? null : now.value ? stepIcon(now.value) : { icon: Layers, key: 'run' })
const rollKey = computed(() => {
  if (wait.value)
    return `wait:${wait.value.kind}`
  return now.value ? `step:${stepKey(now.value)}` : 'run'
})
const loading = computed(() => {
  if (wait.value || now.value)
    return true
  const { running, stopping } = stepsRunning(props.group, liveCalls)
  return running + stopping > 0
})
const select = useEditSelection()
const transcript = useTranscript()
const messageFiles = useMessageFiles()
const pillsShown = useFilePills()
// Folded, each file the run changed shows once, newest first, with its lines
// from before the run to after its latest call, as that call stored them. A
// file whose ends were not kept is named alone. A conversation outside a
// project shows none.
const changed = computed(() => runFiles(calls.value))
const files = computed(() => pillsShown() ? runPills(calls.value) : [])
const media = computed(() => splitMedia(foldedMedia(madeSteps.value)))

// A step enters once, as it joins, the way a row joining the transcript does:
// one that joins while the row is open slides in. Opening the row shows the
// steps that were there, still.
const entering = ref(new Set<string>())
const timers = new Set<ReturnType<typeof setTimeout>>()
const keys = computed<readonly string[]>((previous) => kept(previous, props.group.steps.map(stepKey)))
watch(keys, (now, before) => {
  const had = new Set(before)
  const joined = new Set(now.filter((key) => !had.has(key)))
  if (joined.size === 0)
    return
  entering.value = new Set([...entering.value, ...joined])
  const timer = setTimeout(() => {
    timers.delete(timer)
    entering.value = new Set([...entering.value].filter((key) => !joined.has(key)))
  }, CHROME_ENTER_MS)
  timers.add(timer)
})
onBeforeUnmount(() => {
  for (const timer of timers)
    clearTimeout(timer)
  timers.clear()
})

/**
 * A summed pill opens its file at the first of the run's calls that
 * changed it, under the name the file has in the request now.
 */
function selection(path: string) {
  const first = changed.value.find((file) => file.path === path)?.edits[0]
  const call = calls.value.at(-1)
  return transcript && first && call
    ? pillSelection(transcript.node, transcript.requests(), call, first.call, first.path, transcript.directory(call.id))
    : null
}
/** What a pill's click does: its edit, or a binary file as it is now (`pillAction`). */
function action(path: string): (() => void) | null {
  const picked = selection(path)
  return picked && transcript ? pillAction(select(), transcript.requests(), messageFiles(), picked) : null
}
function selectable(path: string): boolean {
  return action(path) !== null
}
function pick(path: string): void {
  action(path)?.()
}
</script>

<template>
  <!-- A lone step is its own row, in the list's column already. -->
  <div v-if="lone" class="[--agent-pad-x:0px]">
    <StepRow v-model:open="isOpen" :step="lone" :streaming="lone === now" />
  </div>
  <template v-else>
    <FunctionalBlock
      v-model:open="isOpen"
      expandable
      flow
      :loading="loading"
      :roll-key="rollKey"
      :icon-key="icon?.key ?? 'wait'"
      :pictures="isOpen ? undefined : media.pictures"
    >
      <template #icon>
        <ActivityMark v-if="!icon" />
        <component :is="icon.icon" v-else :size="ICON_PX.in28" />
      </template>
      <!--
        data-transcript-wait: the product checks tell a turn on its way by it, in
        any language (`browse.md`).
      -->
      <ActivityLabel
        v-if="wait"
        :kind="wait.kind"
        :since="wait.since"
        :data-transcript-wait="wait.kind"
      />
      <StepFace v-else-if="now" :step="now" />
      <template v-else>
        <span class="min-w-0 truncate">{{ label }}</span>
        <CommandTimeTag :calls="calls" />
      </template>
      <template #body>
        <div class="[--agent-pad-x:0px]">
          <!--
            Open, each step's row is named, so a block brought into view inside
            the run marks its own row; the row stands in the column already, and
            its icon gives the mark room at its start.
          -->
          <div
            v-for="(step, index) in group.steps"
            :key="stepKey(step)"
            class="[--found-inset-x:0px]"
            :data-step-id="isOpen ? step.id : undefined"
            v-bind="chromeEntrance(entering.has(stepKey(step)))"
          >
            <StepRow :step="step" :streaming="step === now" />
          </div>
        </div>
      </template>
    </FunctionalBlock>
    <!--
      The lines of gone media fold away as the row opens to show each under its
      own call, so none shows twice.
    -->
    <Fold :open="!isOpen && media.gone.length > 0">
      <GoneMediaLines :lines="media.gone" />
    </Fold>
    <!--
      The summed pills fold in as the first file changes, and away as the row
      opens to show each call's own.
    -->
    <Fold :open="!isOpen && files.length > 0">
      <FileChangePills
        v-if="files.length > 0"
        class="py-1"
        :style="{ paddingLeft: `${ICON_PX.in28 + 8}px` }"
        :files="files"
        :selectable="selectable"
        @select="pick"
      />
    </Fold>
  </template>
</template>
