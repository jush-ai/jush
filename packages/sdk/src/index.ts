// `@jush-agent/sdk`: Jush's agents in a program. It starts `jush-core`, the
// process a program's cores run in, and speaks HTTP/2 to it on its standard
// input and output, or connects to one that runs on its own; it gives the
// program its cores, their conversations, providers, devices and product
// state, and the program's own plugins, with the clients the web app uses.
//
// ```text
// public API    Jush, Core, Conversation, definePlugin        this package
// clients       PageConnection, ConversationClient,           shared with
//               the product state's withPart, the schemas     the web app
// wire          HTTP/2 on the process's stdio, or over TCP    process.ts
// ```
//
// ```ts
// const jush = await Jush.start({ plugins: [chat] })
// const core = await jush.open('/srv/bot/team-a')
// const provider = await core.providers.create({ source: 'vendor', vendorId: 'anthropic', apiKey })
// const device = await core.devices.create()
// await device.spawn()
// const conversation = await core.conversations.create({
//   model: { providerId: provider.id, modelId: 'claude-sonnet-4-5' },
//   target: { kind: 'device', deviceId: device.id, path: '/srv/bot/work' },
// })
// const turn = await conversation.run('Draft the notes for this week')
// ```
//
// The SDK writes no client of its own: what a page and a program both need
// is in `@jush-agent/protocol` and `@jush-agent/conversation-client`.

export { Jush, type JushConnection, type JushOptions } from './jush'
export {
  Core,
  Conversations,
  Device,
  Devices,
  Providers,
  type AcceptedSocket,
  type ConversationOptions,
  type ExecOptions,
  type ExecResult,
  type Notified,
  type ProviderAccounts,
  type SignIn,
} from './core'
export { Conversation, type ConversationEvent, type Input, type Permission, type Turn } from './conversation'
export {
  definePlugin,
  leaf,
  PortFailure,
  type CommandCall,
  type GroupDefinition,
  type LeafDefinition,
  type Plugin,
  type PluginDefinition,
  type PluginPort,
} from './plugin'
export { JushError } from './routes'
