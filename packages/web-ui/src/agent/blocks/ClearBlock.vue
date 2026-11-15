<script setup lang="ts">
import { t } from '../locales/text'

/**
 * Where the user cleared the model's context: a rule that says so, as a
 * compaction's does. The messages above it stay, and the model reads what
 * comes after as a new conversation.
 */
</script>

<template>
  <div class="flex items-center gap-3 px-[var(--agent-pad-x,2rem)]">
    <div class="h-px flex-1 bg-line-subtle" />
    <span class="text-row text-fg-row">{{ t('transcript.cleared') }}</span>
    <div class="h-px flex-1 bg-line-subtle" />
  </div>
</template>
