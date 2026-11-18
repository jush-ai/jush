import type { Component } from 'vue'
import type { AgentMessage } from '@jush-ai/protocol'
import type { OpaqueView } from '@jush-ai/protocol'
import { AtSign, Bell, Bot, Brain, Cloud, Clapperboard, Globe, Image, MoveRight, ShieldCheck, SquareTerminal, Zap } from '@lucide/vue'
import { searchKind } from '../opaque-steps'
import { toolRenderKind } from '../tool-rendering'

export const THINKING_ICON: Component = Brain

/** The icon of a command's notice row. */
export const NOTICE_ICON: Component = Bell

/**
 * The icon of a step its vendor ran on its side: a search of the web or of X
 * by what it searched, an image or a video looked at by what it is, any other
 * a cloud.
 */
export function opaqueIcon(view: OpaqueView): Component {
  if (view.kind !== 'search')
    return Cloud
  switch (view.action.type) {
    case 'image':
      return Image
    case 'x_video':
      return Clapperboard
    default:
      return searchKind(view.action) === 'x' ? AtSign : Globe
  }
}

/**
 * The icon of a tool's row: a `shell` call, or a tool the runtime does not
 * have. A row that stands in for the call, such as the call being written or
 * the tail row it rolls into, shows the same.
 */
export function toolRowIcon(toolName: string): Component {
  switch (toolRenderKind(toolName)) {
    case 'shell':
      return SquareTerminal
    case 'generic':
      return Zap
  }
}

/**
 * The icon of a receipt's row: the user's permission decision, a move, or
 * another agent.
 */
export function receiptIcon(message: AgentMessage): Component {
  switch (message.event.type) {
    case 'permission':
      return ShieldCheck
    case 'moved':
    case 'move_failed':
      return MoveRight
    default:
      return Bot
  }
}
