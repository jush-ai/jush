<script setup lang="ts">
import { computed } from 'vue'
import { t } from './locales/text'
import { useRelativeTime } from '../composables/useRelativeTime'
import QuotaWindowRow from './QuotaWindowRow.vue'
import type { SettingsQuotaWindow } from './types'

/**
 * A window only a response reports, as its row with when that response came
 * under it, "Updated 2 hours ago": the vendor's usage query leaves it out, so
 * a refresh does not read it again (`ProviderQuota`).
 */
const props = defineProps<{ window: SettingsQuotaWindow & { readAt: string } }>()
const ago = useRelativeTime(() => props.window.readAt)
const updated = computed(() => t('quota.updatedAt', { when: ago.value }))
</script>

<template>
  <div class="flex flex-col gap-2.5">
    <QuotaWindowRow :window="window" />
    <p>{{ updated }}</p>
  </div>
</template>
