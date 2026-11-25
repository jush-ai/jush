<script setup lang="ts">
import { nextTick, ref, useTemplateRef, watch } from 'vue'
import { Pin, X } from '@lucide/vue'
import { appOverlayStore } from '../overlay/appOverlay'
import Dropdown from '../ui/Dropdown.vue'
import IconButton from '../ui/IconButton.vue'
import Menu from '../ui/Menu.vue'
import MenuItem from '../ui/MenuItem.vue'
import Tooltip from '../ui/Tooltip.vue'
import { ICON_PX } from '../ui/icon-metrics'
import { t } from './locales/text'
import { may } from './page-operations'

/** A document the conversation's main agent pinned, a file of a device. */
export interface PinnedDocumentView {
  /** What unpins it. */
  id: number
  /** Its absolute path on the device. */
  path: string
  /** What the user calls the device. */
  device: string
  deviceId: string
}

/**
 * The documents the agent pinned, as the dock's chips row shows them: one
 * chip however many there are, with the pin's mark, the first one's name and
 * how many more, *+2*, so the row keeps its room. The chip opens a menu of
 * every one: its name, the name of the folder it is in, its path and device
 * as its tip, and a ghost × that unpins it. Choosing one opens the file where
 * the page opens one of the conversation's device; one it cannot open is a
 * row that says why.
 */
const props = defineProps<{
  pins: readonly PinnedDocumentView[]
  /** The device whose files the page opens; none when it opens none. */
  opensOn: string | null
}>()
const emit = defineEmits<{
  open: [pin: PinnedDocumentView]
  unpin: [id: number]
}>()

const dropdown = useTemplateRef<InstanceType<typeof Dropdown>>('dropdown')
/**
 * Whether the chip is there. The last unpin, the user's or the agent's,
 * closes the menu before the chip goes, so the menu gives the focus back to
 * where it was, as a menu closed by Escape does.
 */
const shown = ref(props.pins.length > 0)
watch(() => props.pins.length > 0, (any) => {
  if (any) {
    shown.value = true
    return
  }
  dropdown.value?.close()
  void nextTick(() => (shown.value = props.pins.length > 0))
}, { flush: 'sync' })

const name = (pin: PinnedDocumentView) => pin.path.split(/[\\/]/).pop() || pin.path
/** The folder it is in, by its name: the start of a long path tells nothing, and the tip holds the whole. */
const folder = (pin: PinnedDocumentView) => pin.path.split(/[\\/]/).slice(0, -1).filter(Boolean).pop() ?? ''
const place = (pin: PinnedDocumentView) => `${pin.path} · ${pin.device}`
/** Why the page does not open it, after where it is; none when it does. */
function unopenable(pin: PinnedDocumentView): string | undefined {
  if (pin.deviceId === props.opensOn) {
    return undefined
  }
  return props.opensOn === null ? place(pin) : `${pin.path} · ${t('dock.pinnedElsewhere', { device: pin.device })}`
}

function open(pin: PinnedDocumentView, close: () => void): void {
  emit('open', pin)
  close()
}
</script>

<template>
  <Dropdown v-if="shown" ref="dropdown" :overlay-store="appOverlayStore" side="top" :offset="6">
    <template #trigger="{ isOpen }">
      <button
        type="button"
        data-pinned-documents
        class="btn-solid inline-flex h-chip min-w-0 select-none items-center gap-1.5 rounded-full px-2.5 text-chrome text-fg-body hover:text-fg-emphasis"
        :data-pressed="isOpen"
        :aria-label="t('dock.pinned', { count: pins.length })"
        @mousedown.prevent
      >
        <Pin :size="ICON_PX.in24" class="shrink-0 text-fg-subtle" aria-hidden="true" />
        <span v-if="pins.length > 0" class="min-w-0 truncate">{{ name(pins[0]!) }}</span>
        <span v-if="pins.length > 1" class="shrink-0 tabular-nums text-fg-subtle">+{{ pins.length - 1 }}</span>
      </button>
    </template>
    <template #content="{ close }">
      <Menu class="w-80">
        <MenuItem
          v-for="pin in pins"
          :key="pin.id"
          :icon="Pin"
          :label="name(pin)"
          :detail="folder(pin)"
          :hint="place(pin)"
          :disabled="unopenable(pin) !== undefined"
          :disabled-reason="unopenable(pin)"
          @select="open(pin, close)"
        >
          <template v-if="may('pins')" #actions>
            <Tooltip :content="t('dock.unpin')" class="inline-flex">
              <IconButton
                :icon="X"
                size="xs"
                variant="ghost"
                :aria-label="t('dock.unpin')"
                @mousedown.prevent
                @click.stop="emit('unpin', pin.id)"
              />
            </Tooltip>
          </template>
        </MenuItem>
      </Menu>
    </template>
  </Dropdown>
</template>
