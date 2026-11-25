<script setup lang="ts">
import { ref } from 'vue'
import { AppWindow } from '@lucide/vue'
import { appOverlayStore } from '@jush-agent/web-ui/overlay/appOverlay'
import { allowOperations, type PageOperation } from '@jush-agent/web-ui/agent/page-operations'
import Dropdown from '@jush-agent/web-ui/ui/Dropdown.vue'
import IconButton from '@jush-agent/web-ui/ui/IconButton.vue'
import Menu from '@jush-agent/web-ui/ui/Menu.vue'
import MenuDivider from '@jush-agent/web-ui/ui/MenuDivider.vue'
import MenuItem from '@jush-agent/web-ui/ui/MenuItem.vue'
import { t } from '../locales/text'

/**
 * Where the specimens' views run: the web app, where every control shows, or
 * a program's page of one conversation, which shows the controls of the
 * operations its program gave it alone (`page-operations.ts`).
 */
type Page = 'webApp' | 'programNone' | 'programSome'

const OPERATIONS: Record<Page, readonly PageOperation[] | null> = {
  webApp: null,
  programNone: [],
  programSome: ['send', 'stop', 'files'],
}

const page = ref<Page>('webApp')

function choose(chosen: Page, close: () => void): void {
  page.value = chosen
  allowOperations(OPERATIONS[chosen])
  close()
}
</script>

<template>
  <Dropdown :overlay-store="appOverlayStore">
    <template #trigger="{ isOpen }">
      <IconButton :icon="AppWindow" variant="ghost" :pressed="isOpen" :aria-label="t('chrome.page')" />
    </template>
    <template #content="{ close }">
      <Menu>
        <MenuItem :label="t('chrome.webApp')" choice :is-selected="page === 'webApp'" @select="choose('webApp', close)" />
        <MenuDivider />
        <MenuItem :label="t('chrome.programNone')" choice :is-selected="page === 'programNone'" @select="choose('programNone', close)" />
        <MenuItem :label="t('chrome.programSome')" choice :is-selected="page === 'programSome'" @select="choose('programSome', close)" />
      </Menu>
    </template>
  </Dropdown>
</template>
