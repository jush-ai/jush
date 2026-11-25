<script setup lang="ts">
import { computed, ref, useHost } from 'vue'
import SearchDialog from '@jush-agent/web-ui/search/SearchDialog.vue'
import type { MessageRow, MessageSource } from '@jush-agent/web-ui/search/search'
import { appOverlayStore } from '@jush-agent/web-ui/overlay/appOverlay'
import SessionStatus from '@jush-agent/web-ui/agent/SessionStatus.vue'
import ChatPage from '../conversation/ChatPage.vue'
import { useConversationNavigation } from '../conversation/navigation'
import { provideShownConversation } from '../conversation/shown'
import { searchConversation } from '../api/search'
import PageHosts from './PageHosts.vue'
import { elementsPage } from './page'

/**
 * `<jush-conversation src>`: the web app's conversation view, its
 * transcript, composer and cards, its work's windows and taskbar, with Find
 * in Conversation, and what the web app's window gives it around
 * (`PageHosts.vue`), of the conversation the program's server answers below
 * `src`, with the controls of what the page may do.
 */
const props = defineProps<{
  /** Where the program's server answers the page, such as `/chats/42/jush`. */
  src: string
}>()

const page = elementsPage(props.src, useHost()!)
provideShownConversation(page.shown)

const { openAt } = useConversationNavigation()
/** Find in Conversation's window, which searches the conversation alone. */
const searchOpen = ref(false)
const searchIn = computed<MessageSource | null>(() => {
  const id = page.conversation.value
  return id === null ? null : (query, signal) => searchConversation(id, query, signal)
})
function openFoundMessage(row: MessageRow): void {
  searchOpen.value = false
  if (page.conversation.value !== null) {
    openAt(page.conversation.value, null, row.blockId)
  }
}
</script>

<template>
  <div class="jush-conversation flex h-full min-h-0 flex-col">
    <ChatPage v-if="page.conversation.value !== null" :id="page.conversation.value" @find="searchOpen = true" />
    <SessionStatus v-else-if="page.failed.value" kind="missing" />
  </div>
  <SearchDialog
    :is-open="searchOpen"
    :overlay-store="appOverlayStore"
    :recent="[]"
    :search-in="searchIn"
    @close="searchOpen = false"
    @open-message="openFoundMessage"
  />
  <PageHosts :page="page" />
</template>
