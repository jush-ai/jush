<script setup lang="ts">
import ThinkingFace from './ThinkingFace.vue'
import ToolCallFace from './ToolCallFace.vue'
import OpaqueSearchFace from './OpaqueSearchFace.vue'
import OpaqueOtherFace from './OpaqueOtherFace.vue'
import type { WorkStep } from '../work-groups'

/**
 * What the step a run does now says on the run's line: the words and tags of
 * the step's own row (`stepNow`), so the run reads as the step while it runs.
 */
defineProps<{ step: WorkStep }>()
</script>

<template>
  <ThinkingFace
    v-if="step.type === 'thinking'"
    streaming
    :created-at="step.createdAt"
    :ended-at="step.endedAt"
  />
  <ToolCallFace v-else-if="step.type === 'tool_call'" :block="step" />
  <span v-else-if="step.type === 'pending_call'" class="min-w-0 truncate">{{ step.call.description }}</span>
  <template v-else-if="step.type === 'opaque'">
    <OpaqueSearchFace v-if="step.view.kind === 'search'" :block="step" :view="step.view" />
    <OpaqueOtherFace v-else-if="step.view.kind === 'other'" :block="step" :view="step.view" />
  </template>
</template>
