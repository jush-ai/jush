<script setup lang="ts">
import RunEndTag from './RunEndTag.vue'
import type { OpaqueBlock } from '../block-types'
import { t } from '../locales/text'

/**
 * The line of a vendor's step of a kind Jush does not know: Built-in tool and
 * the tool's name, with no tense, since it reads the same while the step
 * runs and once it ended; marked Failed when it failed.
 */
defineProps<{
  block: OpaqueBlock
  view: Extract<OpaqueBlock['view'], { kind: 'other' }>
}>()
</script>

<template>
  <span class="shrink-0">{{ t('transcript.opaque.builtin') }}</span>
  <span class="min-w-0 truncate text-fg-muted group-hover:text-fg-body">{{ view.name }}</span>
  <RunEndTag v-if="block.status === 'failed'" kind="failed" />
</template>
