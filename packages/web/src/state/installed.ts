import { deviceDownloading, deviceInstalled, type DeviceArtifact, type DeviceDownload } from '@jush-agent/web-ui/devices/installed'
import type { PageState } from '@jush-agent/protocol'

/** What `deviceId` holds of `packages`, a plugin's (`crates/runner-command-packages/src/cache.rs`). */
export function packageInstalled(
  state: PageState | null,
  deviceId: string | null,
  packages: readonly string[],
): DeviceArtifact[] {
  return deviceId === null ? [] : deviceInstalled(state?.devices ?? [], deviceId, packages)
}

/** What `deviceId` downloads of `packages`, a plugin's, now. */
export function packageDownloading(
  state: PageState | null,
  deviceId: string | null,
  packages: readonly string[],
): DeviceDownload[] {
  return deviceId === null ? [] : deviceDownloading(state?.devices ?? [], deviceId, packages)
}
