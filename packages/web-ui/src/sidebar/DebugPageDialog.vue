<script setup lang="ts">
import { computed } from 'vue'
import CopyLabel from '../ui/CopyLabel.vue'
import type { OverlayStore } from '../overlay/overlayStore'
import Button from '../ui/Button.vue'
import Dialog from '../ui/Dialog.vue'
import IndeterminateSpinner from '../ui/IndeterminateSpinner.vue'
import InlineError from '../ui/InlineError.vue'
import TextInput from '../ui/TextInput.vue'
import { shownError } from '../infra/errors'
import { t } from './locales/text'
import type { Action, Operation } from '../ui/operations'
import type { DebugPhase } from './debug-page'

/**
 * Debug This Page: what an agent given the link can do, until when, the link
 * in a read-only field, Copy Link, and Stop Debugging at the leading edge,
 * since it ends the link, as the share dialog lays them out.
 *
 * - While the link is made, Creating link… stands in the field, with a spinner
 *   at its end, and Copy Link waits.
 * - A failure says why under the field, and Try Again takes Copy Link's place.
 * - Copy Link is the default button and turns to Copied after a copy, keeping
 *   its width.
 *
 * Closing the dialog leaves the page debugged; the account's row says so.
 */
const props = defineProps<{
  isOpen: boolean
  overlayStore: OverlayStore
  phase: DebugPhase
  /** The link was just copied. */
  copied: boolean
  /** Ends the link; the button shows its progress. */
  onStop?: Action
}>()

const emit = defineEmits<{
  close: []
  copy: []
  retry: []
}>()

const url = computed(() => (props.phase.status === 'on' ? props.phase.url : ''))
const until = computed(() =>
  props.phase.status === 'on'
    ? new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(props.phase.expiresAt)
    : null,
)
const failure = computed(() => (props.phase.status === 'failed' ? shownError(props.phase.error) : null))
const stop: Operation = { action: 'stop', target: 'debug:page', group: 'debug' }
</script>

<template>
  <Dialog :is-open="isOpen" :overlay-store="overlayStore" :label="t('debug.title')" @close="emit('close')">
    <div class="flex flex-col gap-3 px-5 pt-5">
      <h3 class="select-none pr-8 text-[15px] font-medium text-fg-emphasis">{{ t('debug.title') }}</h3>
      <p class="text-explanation leading-5 text-fg-body">{{ t('debug.who') }}</p>
      <!-- The line stands before the link exists, so the dialog keeps its height. -->
      <p class="text-explanation leading-5 text-fg-subtle" :class="until ? '' : 'invisible'">
        {{ t('debug.until', { time: until ?? '—' }) }}
      </p>
      <TextInput
        :model-value="url"
        :placeholder="phase.status === 'starting' ? t('debug.creating') : undefined"
        :aria-label="t('debug.field')"
        :aria-busy="phase.status === 'starting' || undefined"
        :invalid="phase.status === 'failed'"
        readonly
        literal
        autocomplete="off"
        @focus="($event.target as HTMLInputElement).select()"
      >
        <!-- At the field's end, so the text stands where the link will. -->
        <template v-if="phase.status === 'starting'" #suffix>
          <IndeterminateSpinner :size="14" />
        </template>
      </TextInput>
      <InlineError v-if="failure" :message="failure.message" :request-id="failure.requestId" />
    </div>
    <template #footer-leading>
      <Button variant="danger" :operation="stop" :disabled="phase.status !== 'on'" @click="onStop?.()">{{ t('debug.stop') }}</Button>
    </template>
    <template #footer>
      <Button v-if="phase.status === 'failed'" variant="primary" @click="emit('retry')">{{ t('debug.tryAgain') }}</Button>
      <Button v-else variant="primary" :disabled="phase.status !== 'on'" @click="emit('copy')">
        <CopyLabel :label="t('debug.copyLink')" :copied="copied" />
      </Button>
    </template>
  </Dialog>
</template>
