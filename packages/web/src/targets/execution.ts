import type { MoveLock } from '@jush-agent/web-ui/targets/move-question'
import type { SessionDevice } from '@jush-agent/web-ui/windows/device-tags'
import { isSubagentRunning } from '@jush-agent/web-ui/agent/subagents'
import type { DeviceOption, MenuDevice } from '@jush-agent/web-ui/targets/types'
import type { Conversation, Device } from '../state/types'
import { useProduct } from '../state/product'
import { directPath, lastDirectPath } from '../direct'
import { cloudDeviceIn, deviceMarkOf, targetKindOf, deviceLabel } from '../state/devices'

/** Resolve display metadata from the canonical target and account snapshot. */
export function executionFor(conversation: Pick<Conversation, 'target' | 'cwd'>) {
  const snapshot = useProduct().snapshot
  const target = conversation.target
  const project =
    target.kind === 'project'
      ? snapshot?.projects?.find((project) => project.id === target.projectId)
      : null
  const deviceId =
    target.kind === 'device'
      ? target.deviceId
      : (project?.deviceId
        ?? cloudDeviceIn(snapshot?.devices)?.id
        ?? null)
  const device = snapshot?.devices?.find((device) => device.id === deviceId)
  const kind = target.kind === 'cloud' ? ('cloud' as const) : targetKindOf(device)
  return {
    deviceId,
    kind,
    name: deviceLabel(kind, device),
    /** Where the work runs: the backend's resolved directory, for every kind of target. */
    path: conversation.cwd,
    /** The directory the reader chose; null while the Cloud runs in its own session directory. */
    directory: target.kind === 'device' ? target.path : (project?.path ?? null),
    projectName: project?.name ?? null,
    /** Whether the device's runner serves it: a paired device's, or a running Cloud's. */
    state: device?.state ?? ('offline' as const),
    /** How much of its next runner the device has downloaded, while that download moves. */
    downloadPercent: device?.downloadPercent ?? null,
    /** The device's mark; null for the Cloud and a device the account no longer lists. */
    mark: deviceMarkOf(device, snapshot?.devices ?? []),
  }
}

/**
 * The devices the conversation reaches as its windows name them, as its
 * device access admits them: its primary device first, then the attached ones
 * in the order they were attached, then those a running subagent works on; none
 * of the primary device while the conversation has no device yet, as a Cloud
 * never started. A Cloud that is stopped or starting is not offline, primary
 * or attached, as a move away from it leaves it: its windows say what starts.
 */
export function sessionDevices(
  conversation: Pick<Conversation, 'target' | 'cwd' | 'attachedDevices' | 'subagents'>,
): SessionDevice[] {
  const execution = executionFor(conversation)
  const devices = useProduct().snapshot?.devices ?? []
  const cloud = cloudDeviceIn(devices)?.id
  const listed: SessionDevice[] = execution.deviceId === null
    ? []
    : [{ id: execution.deviceId, name: execution.name, primary: true, offline: execution.kind === 'device' && execution.state === 'offline' }]
  for (const attached of conversation.attachedDevices.shown.value ?? []) {
    if (listed.some((device) => device.id === attached.deviceId)) {
      continue
    }
    const device = devices.find((each) => each.id === attached.deviceId)
    const offline = attached.deviceId !== cloud && (device?.state ?? 'offline') === 'offline'
    listed.push({ id: attached.deviceId, name: attached.name, primary: false, offline })
  }
  for (const agent of conversation.subagents) {
    if (agent.device === null || !isSubagentRunning(agent.phase) || listed.some((device) => device.id === agent.device)) {
      continue
    }
    const device = devices.find((each) => each.id === agent.device)
    const offline = agent.device !== cloud && (device?.state ?? 'offline') === 'offline'
    listed.push({ id: agent.device, name: device?.name ?? agent.device, primary: false, offline })
  }
  return listed
}

/**
 * Why the conversation's move controls are unavailable, if they are: it is
 * archived, or a change of it is under way. Work that runs does not lock them;
 * moving asks to stop it first.
 */
export function moveLock(
  conversation: Pick<Conversation, 'id' | 'archived'>,
  pendingChanges: readonly string[],
): MoveLock | undefined {
  if (conversation.archived) {
    return 'archived'
  }
  return pendingChanges.includes(conversation.id) ? 'moving' : undefined
}

/** The Device a conversation runs on, as Run On checks it. */
export function primaryDeviceOf(execution: ReturnType<typeof executionFor>): MenuDevice {
  return {
    id: execution.deviceId ?? 'cloud',
    name: execution.name,
    kind: execution.kind,
    state: execution.state,
    downloadPercent: execution.downloadPercent,
    mark: execution.mark,
  }
}

/**
 * The user's paired devices as Run On lists them, each with the path this
 * page reaches it by, none for one it has not used this session, and the
 * path this browser found last time.
 */
export function deviceOptions(devices: readonly Device[]): DeviceOption[] {
  return devices.map((device) => ({
    id: device.id,
    name: device.name,
    state: device.state,
    downloadPercent: device.downloadPercent,
    mark: device.mark,
    path: directPath(device.id),
    lastPath: lastDirectPath(device.id),
  }))
}
