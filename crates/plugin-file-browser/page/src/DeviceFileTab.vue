<script setup lang="ts">
import { computed } from 'vue'
import { FileView, parentPath, treeLayout, usePage, type TabUpdate } from '@jush-ai/plugin-sdk'
import { fileView } from './data'
import type { DeviceFileData } from './device-file'

/**
 * A file of another device the conversation reaches, beside that device's
 * tree from the file's folder: a file picked there, or a link in a Markdown
 * preview, replaces it, on the same device.
 */
const props = defineProps<{
  conversation: string
  tabId: string
  data: DeviceFileData
  shown: boolean
}>()
const emit = defineEmits<{ update: [data: DeviceFileData, how?: TabUpdate] }>()

const page = usePage()
const source = computed(() => page.files(props.conversation).deviceSource(props.data.device))

function open(path: string, line?: number): void {
  emit('update', { device: props.data.device, path, ...(line === undefined ? {} : { line }) })
}
</script>

<template>
  <FileView
    v-model:tree="treeLayout.open.value"
    v-model:tree-width="treeLayout.width.value"
    v-model:mode="fileView"
    :source="source"
    :root="parentPath(data.path)"
    :path="data.path"
    :line="data.line ?? null"
    @open="open"
  />
</template>
