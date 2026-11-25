<script setup lang="ts">
import ContextMenu from '@jush-agent/web-ui/ui/ContextMenu.vue'
import ToastHost from '@jush-agent/web-ui/ui/ToastHost.vue'
import SelectionFieldHost from '@jush-agent/web-ui/agent/comments/SelectionFieldHost.vue'
import MediaViewer from '@jush-agent/web-ui/files/MediaViewer.vue'
import TargetDialog from '../targets/TargetDialog.vue'
import type { ElementsPage } from './page'

/**
 * What the web app's window has once around its views, and a program's page
 * has once too, in its element: the context menu, the toasts, the media
 * viewer, the place dialog and the selection's field.
 */
defineProps<{ page: ElementsPage }>()
</script>

<template>
  <TargetDialog />
  <ContextMenu :open-in-browser-window="page.shown.openInBrowserWindow.value" />
  <SelectionFieldHost />
  <ToastHost :below="0" />
  <MediaViewer :viewer="page.shown.mediaViewer" />
</template>
