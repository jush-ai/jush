<script setup lang="ts">
import { computed } from 'vue'
import { RegionStatus } from '@jush-ai/plugin-sdk'
import type { MobileDevicesController } from './controller'
import { t } from '../locales/text'

/** The content of a simulator's tab the user opens until the work holds it: what Jush does, booting or opening it. */
const props = defineProps<{
  controller: MobileDevicesController
  /** The opening's id. */
  opening: number
}>()

const shown = computed(() => props.controller.openings.value.find((each) => each.id === props.opening) ?? null)
</script>

<template>
  <RegionStatus
    v-if="shown"
    class="absolute inset-0 z-10 bg-surface"
    status="loading"
    :label="shown.boots ? t('open.booting', { name: shown.name }) : t('open.opening', { name: shown.name })"
  />
</template>
