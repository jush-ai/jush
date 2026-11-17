<script setup lang="ts">
import { computed } from 'vue'
import { ChevronRight } from '@lucide/vue'
import { useIconPx } from '../ui/icon-metrics'
import { pressOnKey } from '../ui/button-events'
import StatusDot from '../ui/StatusDot.vue'
import Tooltip from '../ui/Tooltip.vue'
import { t } from './locales/text'
import type { UnreadEntry } from './unread-entry'

/**
 * The unread entry, a rounded button below the sidebar's entries, framed as
 * the workspace switcher is: the count of the workspace's conversations with
 * news, beside the dot of the most urgent, or else the count of those that
 * run, beside the accent, breathing while a model works, with a chevron that
 * says a click goes on; with neither, Nothing Running, faint, which does
 * nothing. While the list is not there, it shows its frame alone. It keeps
 * its size whatever it says, so nothing below it moves.
 *
 * Its frame is as wide as a lit row's background (`AppSidebar`), and its dot
 * and words start on the line the rows' icons and text keep.
 *
 * A click goes to the next conversation, which the host shows without
 * reading it (`unread-entry.ts`).
 */
const props = defineProps<{ entry: UnreadEntry }>()

const emit = defineEmits<{ next: [] }>()

const icons = useIconPx()
/** It does nothing: nothing runs, or the list is not there to say. */
const idle = computed(() => props.entry.kind === 'idle' || props.entry.kind === 'unknown')

const label = computed(() => {
  switch (props.entry.kind) {
    case 'unread':
      return t('unreadEntry.unread', { count: props.entry.count })
    case 'running':
      return t('unreadEntry.running', { count: props.entry.count })
    case 'idle':
      return t('unreadEntry.idle')
    case 'unknown':
      return ''
  }
})
const hint = computed(() =>
  props.entry.kind === 'unread'
    ? t('unreadEntry.nextUnread')
    : props.entry.kind === 'running' ? t('unreadEntry.nextRunning') : undefined,
)
</script>

<template>
  <Tooltip
    :content="hint"
    :disabled="!hint"
    tag="div"
    :open-delay-ms="80"
  >
    <!-- Marked, so that its click reads no conversation (`state/attention.ts`). -->
    <div
      data-unread-entry
      role="button"
      :aria-label="label || undefined"
      :aria-hidden="entry.kind === 'unknown' || undefined"
      :aria-disabled="idle || undefined"
      :tabindex="idle ? undefined : 0"
      class="flex h-8 w-full cursor-default select-none items-center gap-2 rounded-lg border border-line-subtle pl-[9px] pr-[7px] text-chrome transition-colors duration-200 ease-out phone:h-11 phone:gap-3 phone:pl-[11px] phone:text-[16px]"
      :class="idle ? 'text-fg-faint' : 'bg-hover text-fg hover:bg-active'"
      @click="!idle && emit('next')"
      @keydown="pressOnKey"
      @keyup="pressOnKey"
    >
      <span
        class="flex shrink-0 items-center justify-center"
        :style="{ width: `${icons.row}px`, height: `${icons.row}px` }"
      >
        <StatusDot
          v-if="entry.kind === 'unread'"
          :tone="entry.tone"
        />
        <StatusDot
          v-else-if="entry.kind === 'running'"
          tone="accent"
          :breathing="entry.working"
        />
        <StatusDot
          v-else-if="entry.kind === 'idle'"
          tone="muted"
          ring
        />
      </span>
      <span class="min-w-0 flex-1 truncate">{{ label }}</span>
      <ChevronRight
        v-if="!idle"
        :size="icons.row"
        class="shrink-0 text-fg-faint"
      />
    </div>
  </Tooltip>
</template>
