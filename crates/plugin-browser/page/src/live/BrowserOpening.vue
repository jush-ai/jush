<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { AddressBar, RegionStatus, downloadProgress } from '@jush-agent/plugin-sdk'
import { t } from '../locales/text'
import { BLANK_URL, startingLabel, type BrowserTabsController } from './tabs'
import { BLANK_PAGE, panelReport } from './view'

/**
 * The content of a tab the user opens until the work holds it: the tab as
 * it will look, an address bar on what it opens and below it what Jush
 * does, or a blank page for a blank tab the browser opens now, so the tab
 * that comes moves nothing. An address typed meanwhile is where the tab
 * goes once it opens.
 *
 * Shown, it keeps the page's view open watching nothing and tells it the
 * panel's size, as a tab's content before the browser has its tab does, so
 * the device opens the tab at the size its picture shows at.
 */
const props = defineProps<{
  session: BrowserTabsController
  /** The opening's id. */
  opening: number
  /** Whether its window is on screen. */
  shown: boolean
}>()

const shown = computed(() => props.session.opening(props.opening))
const phase = computed(() => props.session.openingPhase(props.opening))

/** The opening's name among the contents the view follows. */
const content = `opening:${props.opening}`
const area = ref<HTMLElement | null>(null)
let observer: ResizeObserver | null = null

function measure(): void {
  const bounds = area.value?.getBoundingClientRect()
  if (props.shown && bounds && bounds.width >= 1 && bounds.height >= 1) {
    props.session.resize(panelReport(bounds.width, bounds.height))
  }
}

watch(() => props.shown, (shown, was) => {
  if (shown) {
    props.session.show(content, null)
    measure()
  } else if (was) {
    props.session.hide(content)
  }
}, { immediate: true, flush: 'post' })

onMounted(() => {
  observer = new ResizeObserver(() => measure())
  if (area.value) {
    observer.observe(area.value)
  }
})

onBeforeUnmount(() => {
  observer?.disconnect()
  props.session.hide(content)
})
</script>

<template>
  <div v-if="shown" class="absolute inset-0 z-10 flex flex-col bg-surface">
    <AddressBar
      :address="shown.url ?? BLANK_URL"
      :back-reason="t('address.noBack')"
      :forward-reason="t('address.noForward')"
      :can-reload="false"
      @submit="(url) => session.goOpening(opening, url)"
    />
    <div ref="area" class="relative flex min-h-0 flex-1 flex-col border-t border-line">
      <RegionStatus
        v-if="phase !== null"
        class="min-h-0 flex-1"
        status="loading"
        :label="startingLabel(phase)"
        :progress="downloadProgress(session.api.downloading())"
      />
      <div v-else class="min-h-0 flex-1" :class="BLANK_PAGE" />
    </div>
  </div>
</template>
