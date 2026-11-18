<script setup lang="ts">
import { computed } from 'vue'
import type { ActivityKind } from '../activity-slot'
import { useElapsedTime } from '../../composables/useElapsedTime'
import { activityLabel } from '../thinking-label'

/**
 * What a wait says, wherever it shows: in the tail row, or in the face of the
 * run it follows.
 */
const props = defineProps<{
  kind: ActivityKind
  /**
   * When the server began the wait, as `Date.now()`; Requesting shows how
   * long it has waited since. Without it, as while the page's own action has
   * no answer, the row counts nothing.
   */
  since?: number | null
}>()

const requestingElapsed = useElapsedTime(
  () => props.since ?? Number.NaN,
  () => props.kind === 'requesting' && props.since != null,
)

const label = computed(() => activityLabel(props.kind, requestingElapsed.value ?? 0))
</script>

<template>
  <span class="min-w-0 truncate">{{ label }}</span>
</template>
