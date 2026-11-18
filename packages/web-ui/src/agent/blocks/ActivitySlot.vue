<script setup lang="ts">
import ActivityMark from '@jush-ai/web-ui/ui/ActivityMark.vue'
import type { ActivityKind } from '../activity-slot'
import ActivityLabel from './ActivityLabel.vue'
import FunctionalBlock from './FunctionalBlock.vue'

/**
 * The transcript's tail row while it waits: a FunctionalBlock face with the
 * wait mark and why. A step or a Stop that follows the wait takes the row's
 * place and rolls over it (`RowRoll`); the answer or a failure appears in it.
 * After a run, the run's row says the wait instead (`useMessageRows`).
 */
defineProps<{
  kind: ActivityKind
  /**
   * When the server began the wait, as `Date.now()`; Requesting shows how
   * long it has waited since. Without it, as while the page's own action has
   * no answer, the row counts nothing.
   */
  since?: number | null
}>()
</script>

<template>
  <!--
    data-transcript-wait: the product checks tell a turn on its way by it, in
    any language (`browse.md`).
  -->
  <div class="px-[var(--agent-pad-x,2rem)]" :data-transcript-wait="kind">
    <!-- The wait mark stays while only the reason changes. -->
    <FunctionalBlock
      loading
      :roll-key="kind"
      icon-key="wait"
    >
      <template #icon>
        <ActivityMark />
      </template>
      <ActivityLabel :kind="kind" :since="since" />
    </FunctionalBlock>
  </div>
</template>
