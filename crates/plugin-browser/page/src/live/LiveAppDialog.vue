<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import type { AppAnswer, AppRequest } from '../generated/plugin'
import { Button, CopyButton } from '@jush-agent/plugin-sdk'
import { t } from '../locales/text'

/**
 * A page's request to open a link in an app, as a browser asks before it
 * starts one. It shows over the picture without blocking the page, and asks
 * where to open the link: on the device the browser runs on, as the device's
 * own browser would, or here, on the computer or phone the user looks from.
 *
 * The link shows on one line, cut where it is longer, with Copy beside it,
 * so the user can take it elsewhere. Cancel is the default, as in Chrome's
 * own dialog: the page chooses when to ask, so a key meant for the page
 * opens nothing. Enter and Escape cancel.
 */
const props = defineProps<{
  request: AppRequest
  /** The host of the page that asked; empty for a page without one, such as `about:blank`. */
  host: string
  /** The device the browser runs on, as the conversation names it. */
  device: string
}>()
const emit = defineEmits<{ answer: [AppAnswer] }>()

const panel = ref<HTMLElement | null>(null)

const scheme = computed(() => props.request.url.slice(0, props.request.url.indexOf(':') + 1))
const onDevice = computed(() => props.request.app !== null)
const title = computed(() => (props.request.app ? t('app.open', { app: props.request.app }) : t('app.openLink')))

onMounted(() => panel.value?.focus({ preventScroll: true }))

function key(event: KeyboardEvent): void {
  // The dialog's keys are the dialog's, never the page's under it.
  event.stopPropagation()
  if (event.key === 'Enter' || event.key === 'Escape') {
    event.preventDefault()
    emit('answer', 'cancel')
  }
}
</script>

<template>
  <div class="absolute inset-x-0 top-0 flex justify-center p-3">
    <div
      ref="panel"
      role="alertdialog"
      tabindex="-1"
      :aria-label="title"
      class="overlay-shell pointer-events-auto w-full max-w-lg rounded-lg p-3 outline-none"
      @keydown="key"
    >
      <p class="text-explanation font-medium text-fg">{{ title }}</p>
      <p class="mt-1 text-explanation text-fg-muted">
        {{ host ? t('app.wants', { host }) : t('app.thisPageWants') }}
      </p>
      <div class="mt-2 flex min-w-0 items-center gap-1 rounded-md border border-line bg-surface pl-2">
        <p class="min-w-0 flex-1 select-text truncate font-mono text-detail text-fg-body" :title="request.url">
          {{ request.url }}
        </p>
        <CopyButton :source="request.url" :label="t('app.copyLink')" size="xs" />
      </div>
      <p v-if="!onDevice" class="mt-1 text-detail text-fg-subtle">{{ t('app.noApp', { device, scheme }) }}</p>
      <div class="mt-3 flex justify-end gap-2">
        <Button variant="primary" size="sm" @click="emit('answer', 'cancel')">{{ t('dialog.cancel') }}</Button>
        <Button variant="default" size="sm" @click="emit('answer', 'here')">
          {{ t('app.here') }}
        </Button>
        <Button variant="default" size="sm" :disabled="!onDevice" @click="emit('answer', 'device')">
          {{ t('app.onDevice', { device }) }}
        </Button>
      </div>
    </div>
  </div>
</template>
