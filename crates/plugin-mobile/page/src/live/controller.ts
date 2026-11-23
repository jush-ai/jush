/**
 * A conversation's simulators on one of its devices on the page (`crates/plugin-mobile/page/src/index.ts`),
 * which the plugin's store keeps while the plugin is on (`./store.ts`): each
 * open device's live view, on a stream of its own, kept until the device
 * closes, and the
 * window's controls, each the page method of the command that does the same;
 * and the simulators the user opens from the taskbar's Open, until their
 * tabs are there.
 */
import { ref, shallowRef, watch, type Ref } from 'vue'
import { clientPlatform } from '@jush-ai/utils'
import { pageVisibility, picturesSupported, type HeadlineText, type PluginCache } from '@jush-ai/plugin-sdk'
import { LIVE_NO_FOCUSED_FIELD, type FoldResult, type ControlReason, type Simulator } from '../generated/plugin'
import { MobileError, type MobileApi } from '../devices'
import { DeviceSession } from './session'
import type { MobileTabData } from './kind'
import type { DeviceCall } from './controls'
import { t } from '../locales/text'

/**
 * A view notice's words: the reason's own, quoting the viewer's text the
 * notice carries where the reason is about it; never the runner's error text.
 */
export function noticeText(reason: ControlReason, message: string): string {
  return reason === LIVE_NO_FOCUSED_FIELD ? t(`reasons.${reason}`, { text: message }) : t(`reasons.${reason}`)
}

/** How often the view checks that its stream still speaks. */
const TICK_MS = 250

export interface MobileErrors {
  report(message: HeadlineText, error: unknown): void
  defect(message: string, error: unknown): void
}

/**
 * A simulator the user opens from the taskbar's Open, which stands in the
 * window's strip from the click until the work holds its tab (`done`), and is
 * kept until the work no longer holds it, which places that tab in the
 * window; one refused leaves.
 */
export interface Opening {
  /** Which opening it is, numbered from the first of the page. */
  id: number
  nativeId: string
  name: string
  /** It was shut down, so it boots first, which can take a minute. */
  boots: boolean
  /** Its handle once it is open, before the plugin adds its tab. */
  device: string | null
  /** The user closed it before the work held its tab, which the window then closes. */
  cancelled: boolean
  /** The work has held its tab: it opens no more. */
  done: boolean
}

/**
 * The id of the work tab the plugin adds for the simulator `device` on the
 * conversation's device `on`, as the plugin names it
 * (`crates/plugin-mobile/plugin/src/tabs.rs`).
 */
export function addedWorkTab(on: string | null, device: string): string {
  return `mobile-${on ?? ''}-${device}`
}

export interface MobileControllerOptions {
  /** Whether this web browser shows the pictures; the page asks its decoder unless the gallery says. */
  pictures?: () => Promise<boolean>
  /** The stream's platform, the client's own unless the gallery says. */
  platform?: 'mac' | 'windows' | 'linux' | 'other'
  /** The plugin's disk cache, which keeps the devices' frames; none keeps none. */
  cache?: PluginCache
}

export class MobileDevicesController {
  /** Whether the pictures can show: unknown until the decoder answered. */
  readonly pictures: Ref<'unknown' | 'supported' | 'unsupported'> = ref('unknown')
  /** The simulators the user opens, in the order they were chosen (`Opening`); shallow, so each is the object `open` made. */
  readonly openings: Ref<readonly Opening[]> = shallowRef([])
  /** The id of the latest opening. */
  private opened = 0
  /** Each open device's live view, by device, made as a view of it first shows. */
  private readonly devices = new Map<string, DeviceSession>()
  private readonly platform: 'mac' | 'windows' | 'linux' | 'other'
  private readonly cache: PluginCache | undefined
  /** Whether the page is visible. */
  private visible = true
  private readonly ticking: ReturnType<typeof setInterval>

  constructor(
    readonly api: MobileApi,
    /** The conversation's device whose simulators these are; null for its primary device's, whichever that is. */
    private readonly on: string | null,
    private readonly errors: MobileErrors,
    /** Selects the tab `id` of the conversation's work, once it is there. */
    private readonly select: (id: string) => void,
    /** The devices of the conversation's work's tabs, read reactively. */
    private readonly held: () => readonly Pick<MobileTabData, 'device' | 'nativeId'>[],
    options: MobileControllerOptions = {},
  ) {
    this.platform = options.platform ?? clientPlatform(navigator)
    this.cache = options.cache
    const supported = options.pictures ?? (() => picturesSupported((message, error) => errors.defect(message, error)))
    void supported().then((yes) => {
      this.pictures.value = yes ? 'supported' : 'unsupported'
      if (yes) {
        for (const session of this.devices.values()) {
          session.enable()
        }
      }
    })
    this.ticking = setInterval(() => {
      for (const session of this.devices.values()) {
        session.tick()
      }
    }, TICK_MS)
    // A hidden page looks at no device, so a device nobody looks at gets its
    // hour; the watch ends with the effect scope its store made it in.
    watch(pageVisibility, (visibility) => {
      this.visible = visibility !== 'hidden'
      for (const session of this.devices.values()) {
        session.visibility(this.visible)
      }
    }, { immediate: true })
  }

  /** The live view of the device `data` names, its tab's, made the first time. */
  device(data: MobileTabData): DeviceSession {
    const kept = this.devices.get(data.device)
    if (kept) {
      return kept
    }
    const session: DeviceSession = new DeviceSession({
      open: this.api.stream,
      device: data.device,
      model: data.model ?? null,
      nativeId: data.nativeId ?? null,
      cache: this.cache,
      platform: this.platform,
      onNotice: (reason: ControlReason, message: string) =>
        this.errors.report(t('failures.control'), new MobileError(reason, null, noticeText(reason, message))),
      // A device that closed takes its view along; its tab goes with it.
      gone: () => {
        if (this.devices.get(data.device) === session) {
          this.devices.delete(data.device)
        }
      },
      defect: (message, error) => this.errors.defect(message, error),
    })
    this.devices.set(data.device, session)
    session.visibility(this.visible)
    if (this.pictures.value === 'supported') {
      session.enable()
    }
    return session
  }

  /**
   * The id of the work tab the opening becomes: its device's, once the
   * device opened it or the work holds a tab of its simulator.
   */
  openingTab(opening: Opening): string | null {
    const device = opening.device ?? this.held().find((data) => data.nativeId === opening.nativeId)?.device
    return device === undefined || device === null ? null : addedWorkTab(this.on, device)
  }

  /**
   * Opens `simulator` as the user chose it in the taskbar's Open, an opening
   * from the click, and selects its tab once the plugin adds it; one opening
   * already is left to finish. One the conversation has open already only
   * has its tab selected, with nothing to wait for. A refusal is a toast.
   */
  async open(simulator: Simulator): Promise<void> {
    if (this.openings.value.some((each) => each.nativeId === simulator.nativeId && !each.cancelled && !each.done)) {
      return
    }
    this.opened += 1
    const id = this.opened
    if (simulator.open === undefined) {
      const opening: Opening = { id, nativeId: simulator.nativeId, name: simulator.name, boots: simulator.state === 'shut-down', device: null, cancelled: false, done: false }
      this.openings.value = [...this.openings.value, opening]
    }
    try {
      const opened = await this.api.open(simulator.nativeId)
      const now = this.openings.value.find((each) => each.id === id)
      this.openings.value = this.openings.value.map((each) => each === now ? { ...each, device: opened.device } : each)
      if (!now?.cancelled) {
        this.select(opened.tab)
      }
    } catch (error) {
      this.openings.value = this.openings.value.filter((each) => each.id !== id)
      this.errors.report(t('failures.open'), error)
    }
  }

  /** The user closed the opening `id`: the window closes its tab once the work holds it. */
  cancel(id: number): void {
    this.openings.value = this.openings.value.map((each) => each.id === id ? { ...each, cancelled: true } : each)
  }

  /**
   * The work's tabs changed: an opening whose tab it holds is done, and one
   * done whose tab it no longer holds is over, as its tab closed.
   */
  settle(): void {
    const held = new Set(this.held().map((data) => addedWorkTab(this.on, data.device)))
    let changed = false
    const next = this.openings.value.flatMap((opening) => {
      const tab = this.openingTab(opening)
      const holds = tab !== null && held.has(tab)
      if (opening.done && !holds) {
        changed = true
        return []
      }
      if (holds && !opening.done) {
        changed = true
        return [{ ...opening, done: true }]
      }
      return [opening]
    })
    if (changed) {
      this.openings.value = next
    }
  }

  /** The store released it, as when the plugin is turned off: every device's view ends and nothing reopens it. */
  dispose(): void {
    clearInterval(this.ticking)
    for (const session of this.devices.values()) {
      session.close()
    }
    this.devices.clear()
  }

  /**
   * The video of a recording this window started that ended without a press of
   * Stop, at its limit: the file on the runner's device that the content saves on the
   * user's computer. None when the runner holds none, as after the device
   * reconnected.
   */
  async collect(device: string): Promise<string | null> {
    try {
      return (await this.api.record(device, 'stop')).path ?? null
    } catch (error) {
      if (MobileError.of(error).reason !== 'recording_not_found') {
        this.errors.report(t('failures.control'), error)
      }
      return null
    }
  }

  /**
   * Folds `device` to `to`, as `jush mobile fold` does: answers how it is
   * folded once it got there, or none when it refused, which is a toast.
   */
  async fold(device: string, to: string): Promise<FoldResult | null> {
    try {
      return await this.api.fold(device, to)
    } catch (error) {
      this.errors.report(t('failures.control'), error)
      return null
    }
  }

  /**
   * Runs a toolbar control's call; answers the file on the runner's device that
   * Screenshot or a stopped Record left, which the content saves on the
   * user's computer. A refusal is a toast. Whether a device records is the
   * device's to say, in the view's state, whoever started it.
   */
  async run(call: DeviceCall): Promise<string | null> {
    try {
      switch (call.method) {
        case 'rotate': {
          // The shown device turns at once, and back if the turn fails.
          const { device, turn } = call.params
          const session = this.devices.get(device)
          const press = session?.turning(turn) ?? null
          let answered
          try {
            answered = await this.api.rotate(device, turn)
          } catch (error) {
            session?.turnAnswered(press, null)
            throw error
          }
          session?.turnAnswered(press, answered.held)
          return null
        }
        case 'fold':
          await this.fold(call.params.device, call.params.to)
          return null
        case 'toggleAppearance':
          await this.api.toggleAppearance(call.params.device)
          return null
        case 'press':
          await this.api.press(call.params.device, call.params.button)
          return null
        case 'screenshot':
          return (await this.api.screenshot(call.params.device)).path
        case 'record':
          return (await this.api.record(call.params.device, call.params.action)).path ?? null
      }
    } catch (error) {
      this.errors.report(t('failures.control'), error)
      return null
    }
  }
}
