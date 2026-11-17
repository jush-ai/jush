<script setup lang="ts">
import { ref } from 'vue'
import { appOverlayStore } from '@jush-agent/web-ui/overlay/appOverlay'
import DebugPageDialog from '@jush-agent/web-ui/sidebar/DebugPageDialog.vue'
import type { DebugPhase } from '@jush-agent/web-ui/sidebar/debug-page'
import GalleryDialogFrame from './GalleryDialogFrame.vue'

/**
 * Debug This Page as the account's menu opens it, in one of four states:
 *
 * - `on`: on a page an agent may debug, with its link.
 * - `starting`: pinned while its link is made, until Open makes it again.
 * - `new`: from Open, which stands for the menu item, making the link with
 *   Creating link… and then showing it.
 * - `failing`: on one whose making fails first, with Try Again.
 *
 * Copy Link copies. Stop Debugging works a beat and closes the dialog, and Open
 * makes a new link, as the product's backend would.
 */
const props = defineProps<{ state: 'on' | 'starting' | 'new' | 'failing' }>()

/** How long the specimen's backend takes to answer, so the waits show. */
const BEAT_MS = 900
const LINK = 'https://jush.example.com/api/debug/k3VqP0dZr8yWm2hLx7NcTa'

const linked = (): DebugPhase => ({ status: 'on', url: LINK, expiresAt: new Date(Date.now() + 24 * 60 * 60 * 1000) })
const phase = ref<DebugPhase>(props.state === 'on' ? linked() : { status: 'starting' })
const copied = ref(false)
let failNext = props.state === 'failing'

function start(): void {
  copied.value = false
  phase.value = { status: 'starting' }
  setTimeout(() => {
    if (failNext) {
      failNext = false
      phase.value = { status: 'failed', error: new Error('The page could not reach the backend.') }
    } else {
      phase.value = linked()
    }
  }, BEAT_MS)
}

async function copy(): Promise<void> {
  if (phase.value.status === 'on') {
    await navigator.clipboard.writeText(phase.value.url).catch(() => {})
    copied.value = true
  }
}

async function stop(close: () => void): Promise<void> {
  await new Promise((done) => setTimeout(done, BEAT_MS))
  close()
}
</script>

<template>
  <GalleryDialogFrame v-slot="{ open, close }" :starts-closed="state === 'new' || state === 'failing'" @reopen="start">
    <DebugPageDialog
      :is-open="open"
      :overlay-store="appOverlayStore"
      :phase="phase"
      :copied="copied"
      :on-stop="() => stop(close)"
      @copy="copy"
      @retry="start"
      @close="close()"
    />
  </GalleryDialogFrame>
</template>
