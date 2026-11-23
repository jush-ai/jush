// The `device-file` kind of the file-browser plugin's page: a file of another
// device the conversation reaches, as a `device:<id>/path` link in a reply
// names it, in a tab of the Files window beside the File view, which shows
// the conversation's own device.
import { defineComponent, h, type PropType } from 'vue'
import { z } from 'zod'
import { FileIcon, ICON_PX, baseName, type IntentPayloads, type TabKind } from '@jush-ai/plugin-sdk'
import DeviceFileTab from './DeviceFileTab.vue'
import { filesWindow } from './data'

/** The file a tab shows: the device's id, its absolute path there, and the line it was opened at. */
export const deviceFileDataSchema = z.object({
  device: z.string(),
  path: z.string(),
  line: z.number().int().positive().optional(),
})
export type DeviceFileData = z.infer<typeof deviceFileDataSchema>

/** The tab's data for the `deviceFile` intent. */
export function openDeviceFile(payload: IntentPayloads['deviceFile']): DeviceFileData {
  return { device: payload.device, path: payload.path, ...(payload.line === undefined ? {} : { line: payload.line }) }
}

/** The file's own icon. */
const DeviceFileMark = defineComponent({
  props: { data: { type: Object as PropType<DeviceFileData>, required: true } },
  setup: (props) => () => h(FileIcon, { name: props.data.path, isDirectory: false, size: ICON_PX.markIn28 }),
})

/**
 * The `device-file` kind: each file of another device gets a tab of its own,
 * named by the file and tagged with its device, as a browser opens a
 * document in a tab; a second link to it selects its tab again.
 */
export const deviceFileKind: TabKind<DeviceFileData> = {
  kind: 'device-file',
  schema: deviceFileDataSchema,
  window: filesWindow,
  title: (data) => baseName(data.path),
  key: (data) => `${data.device}\n${data.path}`,
  deviceOf: (data) => data.device,
  mark: DeviceFileMark,
  content: DeviceFileTab,
  intents: {
    deviceFile: (payload) => openDeviceFile(payload),
  },
}
