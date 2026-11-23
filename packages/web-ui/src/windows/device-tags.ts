/**
 * Which device a window, a tab or a command is of, said as a tag of the
 * device's name where things of several devices show together: the taskbar's
 * buttons, and one strip's tabs.
 *
 * - Something of a device other than the conversation's primary device always
 *   says its device, alone or not.
 * - Something of the primary device says it only while something of another
 *   device shows in the same place: with the primary device alone, nothing
 *   changes from a conversation of one device.
 * - Something of no one device, such as Files, never says one.
 */

/** A device the conversation reaches, as its windows name it. */
export interface SessionDevice {
  id: string
  /** What the user calls it. */
  name: string
  /** It is the conversation's primary device. */
  primary: boolean
  /** Its runner is not connected: its windows say so over their tabs. */
  offline: boolean
}

/**
 * The tag of each of `of`, the devices of the things shown in one place in
 * their order (null for a thing of no one device), among the conversation's
 * `devices`: the device's name, or null for no tag.
 */
export function deviceTags(of: readonly (string | null)[], devices: readonly SessionDevice[]): (string | null)[] {
  const primary = devices.find((device) => device.primary)?.id ?? null
  const mixed = of.some((device) => device !== null && device !== primary)
  return of.map((device) => {
    if (device === null || (device === primary && !mixed)) {
      return null
    }
    // A device the conversation no longer reaches is still named by its id.
    return devices.find((each) => each.id === device)?.name ?? device
  })
}
