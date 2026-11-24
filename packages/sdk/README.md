# @jush-agent/sdk

Jush's agents in your program. The SDK starts `jush-core`, the process your
program's cores run in, and gives your program the cores, their
conversations, providers and devices, and plugins of your own whose commands
the agents run, under `jush` or by names of their own.

A core is one Jush: its conversations, provider entries, devices and
settings, kept in a directory you choose. A team bot, for example, opens one
core for each team it serves.

## Install

```sh
npm install @jush-agent/sdk zod
```

The package installs the executables of your machine's platform, macOS,
Linux or Windows on x86-64 or ARM64, from its optional platform packages,
such as `@jush-agent/sdk-linux-x64`. It runs on Node.js 22 or later, Bun and
Deno. `zod` defines your plugins' command arguments.

## Quick start

The following program opens a core, gives it an Anthropic API key and a
device on your machine, and runs one turn:

```ts
import { Jush } from '@jush-agent/sdk'

const jush = await Jush.start()
const core = await jush.open('./data/team-a')

const provider = await core.providers.create({
  source: 'vendor',
  vendorId: 'anthropic',
  apiKey: process.env.ANTHROPIC_API_KEY,
})
const device = await core.devices.create()
await device.spawn()

const conversation = await core.conversations.create({
  title: 'Release notes',
  model: { providerId: provider.id, modelId: 'claude-sonnet-4-5' },
  target: { kind: 'device', deviceId: device.id, path: '/srv/work' },
})
const turn = await conversation.run('Draft the notes for this week from the git log')
console.log(turn.outcome, turn.text)

await jush.close()
```

`turn.outcome` is `finished`, `failed` or `stopped`; `turn.text` is the text
the agent wrote after your message, and `turn.blocks` everything it did.
Blocks read back as a page, as after a dropped connection, come in the light
form the app's list holds: a tool call without its output's text, and with
only the description of a long input, and thinking without its text. The
answer's text is whole.

## Cores

`Jush.start()` starts one `jush-core` process for your program, and
`jush.open(dir)` opens a core in it, creating the core when `dir` is absent
or empty. A process holds any number of cores. Open the same directory again
later, and the core has everything it had: conversations, provider entries,
devices and settings.

A core is open in one process at a time; opening it in a second fails with a
`JushError` whose `code` is `core_open`.

`core.close()` closes one core, and `jush.close()` closes every core and ends
the process. A turn still running when its core closes stops there, and Jush
takes it up again when the core opens again.

`jush-core` and the runners the SDK starts write their logs to your
program's standard error.

## A process of its own

`Jush.start()` ties the process to your program: when your program stops,
for example to deploy a new version, every core closes, and a running turn
stops until the core opens again. To keep the cores running while your
program restarts, or to share them between several services, run
`jush-core` on its own, as a server:

```sh
export JUSH_CORE_TOKEN=7f3c9e2a51d84b06
export JUSH_RELEASE=node_modules/@jush-agent/sdk/release:node_modules/@jush-agent/sdk-linux-x64/release
node_modules/@jush-agent/sdk-linux-x64/bin/jush-core serve --listen 127.0.0.1:4100
```

`jush-core` is in your platform's package. `JUSH_RELEASE` lists the
`release` directories of the SDK and of each platform package, as `PATH`
does, so the cores' devices run Jush's commands. Each of your services
connects with the same token:

```ts
const jush = await Jush.connect({ url: 'http://127.0.0.1:4100', token, plugins: [chat] })
const core = await jush.open('/srv/bot/data/team-a')
```

- Opening a directory the process holds already answers the same core, with
  its conversations and turns as they went on while your program was away.
- `jush.close()` disconnects and closes nothing; `core.close()` closes the
  core for every service.
- Each connection declares its plugins again, and the last declaration of a
  plugin's id stands: a new version of your program changes the commands
  of every conversation at once.
- `SIGTERM` or Ctrl-C stops the process, which closes every core. Your
  program connects again by itself once the process is back, declares its
  plugins and opens its cores again, and goes on.

Set `JUSH_PUBLIC_URL` to where the process is reached from outside, such as
`https://bot.example.com/jush`, for schedules' URLs and plugins' sign-ins,
which the process then answers itself.

A runner reaches a process of its own as it reaches a Jush server: start it
anywhere with `jush-runner run --backend http://bot.example.com:4100`. It
prints a pairing code and waits; pair it to a core with
`core.devices.claim(code)`. The runner keeps its token, and connects again
by itself after it or the process restarts.

## Providers

A provider entry is what a core's agents call models with. Create one with
an API key of a vendor, such as `anthropic`, `openai` or `google`:

```ts
const openai = await core.providers.create({ source: 'vendor', vendorId: 'openai', apiKey: process.env.OPENAI_API_KEY })
```

Or with an endpoint that speaks a vendor's protocol, such as a gateway or a
model server of your own; `providerType` is the protocol, `anthropic` when
absent:

```ts
const gateway = await core.providers.create({
  source: 'compatible',
  providerType: 'openai',
  label: 'Team Gateway',
  baseUrl: 'https://gateway.example.com/v1',
  apiKey: process.env.GATEWAY_KEY,
})
```

The models each entry offers are in the core's state, as
`core.state.models.providers`, each with its `providerId` and `models`.
`core.providers` also lists, updates and removes entries.

### Subscriptions

A subscription entry signs in with an account instead of a key: `codex` for
ChatGPT, `claude-code` for Claude, or `grok-build` for Grok. The user
confirms the sign-in in a browser. Its stages are in `core.state.logins`:

```ts
const signIn = await core.providers.signIn({ providerType: 'codex' })
const stop = core.on('state', (state) => {
  const login = state.logins.find((each) => each.id === signIn.id)?.login
  if (login?.status === 'pending' && login.verificationUrl !== null) {
    showToUser(`Open ${login.verificationUrl} and enter ${login.userCode}`)
  } else if (login !== undefined && login.status !== 'pending') {
    stop()
  }
})
```

When a login's `needsCode` is true, the user signs in at the address
instead, and you hand the code the page shows to `signIn.submitCode(code)`.
`signIn.cancel()` stops it.

An entry can hold several accounts: `core.providers.accounts(id)` signs in
another one, and lists, activates, orders and removes them. Once an entry
holds two or more, its `rotation` says which account each agent uses:
`manual` the active one, `in_order` the first in their order that is not used
up, and `smart` first the one whose quota resets soonest, since what is left
at a reset is lost:

```ts
await core.providers.update(entry.id, { rotation: 'smart' })
```

## Devices

A device is a machine the agents run commands on, served by a
`jush-runner`. `device.spawn()` starts the package's own runner on your
program's machine. Give it a command to start a runner elsewhere, such as in
a container; each device's runner keeps its state in a directory of its own,
`--home`:

```ts
const box = await core.devices.create()
await box.spawn(['docker', 'exec', '-i', 'box-7', 'jush-runner', 'run', '--stdio', '--home', `/var/lib/jush/${box.id}`])
```

`spawn()` resolves once the core counts the device online, so a conversation
can be created on it, and answers the runner's child process. It fails when
the command exits before, or when no runner comes on its channel within 30
seconds, and then ends the command. The core counts a device online for a
short while after its runner drops, and after the core opens, if its runner
was connected when the core closed, waiting for the runner to come back;
`spawn()` of such a device resolves at once, so watch the child process for a
runner that exits.

A runner that reaches the core some other way, such as over SSH or a socket,
connects with `device.connect({ readable, writable })`, the standard output
and input of a runner already started. It resolves as `spawn()` does, with
`ended`, which resolves once either end closed.

A core keeps its devices, but not their runners: when your program starts
again, start a runner for each device the core lists:

```ts
for (const listed of core.state.devices) {
  await core.devices.of(listed).spawn()
}
```

### The language and time zone of the agents' work

Commands run in the time zone and languages of the browser that last
showed the core's pages, and in `en-US` and UTC before one did. A
conversation's browser starts in them: a site shows its text in that
language, and a page its times in that zone. A new schedule counts in that
zone too, and each turn tells the model the time in it. Set them for the
core's work with `commandLocale`, which no page changes:

```ts
await core.updatePreferences({ commandLocale: { timeZone: 'Asia/Shanghai', languages: ['zh-CN'] } })
// Back to what the pages' browser reports:
await core.updatePreferences({ commandLocale: null })
```

A conversation's browser takes them as it starts, as it takes the color
scheme: one already open keeps its own until it starts again, after its
last tab closed.

### Scripts outside a conversation

`device.exec(script, options)` runs a script on a paired device as the agent's
shell runs a command, for work that is no conversation's, such as a filter a
chat message goes through before the agent hears of it. No transcript holds
it, and no model reads it:

```ts
const { exitCode, stdout } = await device.exec('bun filter.js', {
  cwd: '/work/filters/release-notes',
  env: { CHAT_TOKEN: token },
  stdin: JSON.stringify(event),
  timeout: 10_000,
})
```

- It starts in `cwd`, the home directory of the device's runner by default,
  with `env` above the device's environment; `env` reaches the script's
  process and no file on the device.
- It resolves once the script exited, whatever its status, with its
  `exitCode`, `stdout` and `stderr` read as UTF-8. A device keeps 16 MiB of a
  script's output and leaves out the middle of more, and a read of it that
  failed keeps only its first and newest bytes; `leftOutBytes` counts what is
  missing.
- Each run is a job of its own, so runs on one device go side by side.
- The request, the script and `stdin` with it, is JSON of at most 1 MiB.
- A device whose runner drops keeps running the script, and the result comes
  once the runner is back. `timeout`, in milliseconds, bounds the wait; 0, the
  default, sets no limit. A script past its time gets `SIGTERM`, and
  `SIGKILL` 5 seconds later if it is still running.
- A runner that ends, or whose connection breaks, after the core sent it the
  whole of `stdin` but before the script read all of it, loses the rest: the
  script reads the input's end there, as a program whose `ssh` client went
  does.
- `signal` stops the script, and the promise rejects with its reason, as a
  `fetch`'s does. The end of your program's connection to the core stops it
  too.

It rejects with a `JushError` only when the script has no exit status:

- `device_offline`: the runner was not connected, and the script never ran.
- `command_timed_out`: it ran past `timeout` and was stopped; what it printed
  is in `error.body.output`.
- `device_lost`: the device lost it; its runner was away when it was to be
  stopped; or the runner's connection broke before the core sent it the
  whole of `stdin`.
- `backend_closing`: the core closed while it ran. The device stops the
  script when it connects to the core again.

`device.revoke()` revokes a device, as the app's Revoke does: its token opens
nothing from then on, the commands running on it are lost, and the commands of
a conversation whose target it is fail until `update({ target })` names
another. A runner connected to it exits and removes its `--home`, or only its
own files there when `--home` is the home directory, holds it, or is a
filesystem's root, such as a volume's mount point.

### Sandboxes that expire

A sandbox that lives for a while and then goes, such as a container your
platform keeps for 24 hours, is one device for each sandbox. Make a device
when the sandbox starts, and revoke it when the sandbox ends; a device is
never reused for the next sandbox, since its runner's state went with the
sandbox. One sandbox can serve one conversation, or every conversation of its
day.

When a sandbox ends before a conversation does, start a new sandbox, move the
conversation onto its device, and revoke the old device:

```ts
const next = await core.devices.create()
await next.spawn(['sandbox', 'exec', 'sbx-0412', 'jush-runner', 'run', '--stdio', '--home', `/var/lib/jush/${next.id}`])
for (const conversation of onOldSandbox) {
  await conversation.update({ target: { kind: 'device', deviceId: next.id, path: '/workspace' } })
}
await old.revoke()
```

The conversation keeps its transcript, and its agent is told on its next turn
that no file moved: what the old sandbox held, its files and the browser
profiles on it among them, went with it. What has to outlive a sandbox lives
on a volume your platform keeps across sandboxes.

## Conversations

`core.conversations.create()` takes the conversation's `title`, `model` and
`target`, the device and directory its commands run in. A conversation
without a target runs on the Cloud, which only Jush's server runs, so in a
program it is refused with `cloud_unavailable`, as one on a device that does
not exist is with `device_not_found`. `core.conversations.open(id)` takes a
conversation created earlier, and `conversation.update(patch)` changes the
same fields later.

Each conversation's browser has a profile of its own, so a sign-in made in
one conversation is not seen in another. Conversations that name the same
`browserProfile` on one device share its browser instead: a site one of them
signs in to is signed in for the others, and each keeps its own tabs. The
profile is the directory `~/.jush/browser-profiles/<name>` on the device,
made at its first use; list, copy or remove it as any directory.

```ts
const triage = await core.conversations.create({ title: 'Triage', model, target, browserProfile: 'support' })
const refunds = await core.conversations.create({ title: 'Refunds', model, target, browserProfile: 'support' })
// Back to a profile of its own, from its next browser command on:
await refunds.update({ browserProfile: null })
```

Every agent of a conversation, its subagents included, reads Jush's system
prompt unless the conversation names a `systemPrompt`, your whole text in
its place. After it, Jush writes the index of the conversation's commands
and the line naming the model that serves it, and a subagent's profile's
instructions after those. A change made with `update` reaches each agent
from its next request, and `systemPrompt: null` gives Jush's back.

`commands` lists the commands the conversation's agents have, Jush's and
your plugins' alike, each by its path without `jush`: a group, such as
`browser`, names every command under it, and a leaf, such as
`conversation list`, names that one. Your plugin's commands are named by
their group's name, not by the plugin's id. A command the list does not name
is not in the agent's prompt, and running it answers as for a command that
does not exist; a plugin the list names no command of gives no context
either. `jush shell`, which reads what the shell tool cut short, stays
whatever the list says. Without a list the agents have every command; with
one, a command Jush adds later reaches the conversation only once its list
names it. A command still follows the core's own switches. A change of the
list reaches each agent from its next command.

```ts
const desk = await core.conversations.create({
  model,
  target,
  systemPrompt: "You are the help desk's assistant. You answer the team's questions about their accounts, in the team's chat: plain text, no links.",
  commands: ['browser', 'schedule', 'tickets'],
})
// Try the next version of the commands in this conversation alone:
await desk.update({ commands: ['browser', 'schedule', 'tickets-next'] })
```

A core an earlier release made kept a list of plugin ids, `plugins`, which
opens as `commands`: Jush's groups of then and those ids, each taken for its
plugin's group name. Name again a plugin whose group is named otherwise. A
conversation's `identity`, which opened Jush's system prompt, opens as the
`systemPrompt` its agents read then: the identity, and Jush's rules for
running commands and replying after it.

`conversation.run(input, options?)` sends a message and resolves once the
turn it starts ended, with that turn's blocks and outcome, also when your
program's connection to the process dropped while the turn ended. It fails
when the message leaves the queue without running, as an abort that clears
the queue or gives the message back makes it, or when the conversation
closes first. `options.senderId` names who sent the message, by your own
id, such as a person of a team's chat: the message's `user` block keeps it
as `senderId`, so a page can show who wrote what, the model does not read
it, and [Usage](#usage) says what it counts. The other actions are the ones
the app has:

| Method | What it does |
| --- | --- |
| `send(input, messageId?, options?)` | Sends a message, queued behind a running turn; resolves once the conversation holds it; `options.senderId` as `run` takes it |
| `steer(input, steerId?, options?)` | Hands a message to the running turn; resolves once the turn took it; `options.senderId` as `run` takes it |
| `abort()` | Stops the running turn |
| `retry()` | Runs the latest turn again from your message |
| `resume()` | Finishes a turn that did not finish |
| `compact()` | Summarizes the conversation so far, to free its context |

`send` and `steer` take an id after the input. Give one when you may send
the same message again, such as after a lost answer: while the conversation
holds input under an id, queued, waiting or in its history, anything sent
under it again, as a message or a steer, is acknowledged and not taken a
second time. Input that leaves it, such as a message taken out of the queue,
frees its id. Make each id your own, such as a ticket's number with a
prefix: a steer under the id of one of the conversation's blocks is refused.

```ts
await conversation.steer('Use the staging database instead.', 'steer-ticket-42')
```

`conversation.fork` starts a new conversation with this one's history,
under `id` when you name one, which a retry repeats. `before` takes a turn
by its message's id, `turn.id` as `run` answers it or the id you sent it
under, and keeps everything before that message, the turn before it whole,
its commands' output and its reply among it. `through` keeps everything
through a completed text of the main agent, by its block's id.

```ts
const turn = await conversation.run('Draft the reply to the customer.')
const retry = await conversation.fork({ before: turn.id })
await retry.run('Draft the reply, in a warmer tone.')
```

A message is text, or content blocks, such as a file on one of the core's
devices:

```ts
await conversation.run([
  { type: 'text', text: 'Summarize this report' },
  { type: 'remote_file', deviceId: device.id, path: '/srv/work/report.md' },
])
```

A file your program holds, such as a photo a user sent you, goes up with
`core.upload`. The upload answers a block, which sends the file in a
message, a steer or a run:

```ts
const photo = await core.upload({ name: 'receipt.png', bytes, mediaType: 'image/png' })
await conversation.run([{ type: 'text', text: 'What does this receipt total?' }, photo])
```

- A file holds from 1 byte to 25 MiB. `bytes` is a `Uint8Array`, an
  `ArrayBuffer` or a `Blob`; a `Blob`'s own type serves when `mediaType` is
  absent.
- The agent sees the file as an attachment of the conversation, such as
  `<attachment id="a1" name="receipt.png" … />`. Nothing is written to the
  device: the agent's commands fetch the file with `jush attachment
  download`.
- The core keeps an upload while a message holds it, so the same block can
  go in more messages. An upload no message holds may be removed once a day
  has passed since it was made, and a message that sends it after that
  tells the model `[attachment … is not available]`: send an upload within
  the day, or upload the file again.

The model is shown the file itself, as an image, a PDF or another medium,
when the model's `acceptedExtensions` list the file's type and the
provider's API carries that medium. A vendor entry's types come from
models.dev. A compatible entry's are yours to list among its `models`:

```ts
const gateway = await core.providers.create({
  source: 'compatible',
  providerType: 'openai',
  label: 'Team Gateway',
  baseUrl: 'https://gateway.example.com/v1',
  apiKey: process.env.GATEWAY_KEY,
  models: [{
    id: 'vision-large',
    displayName: 'Vision Large',
    contextWindow: 128_000,
    outputLimit: null,
    thinkingEfforts: [],
    acceptedExtensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'],
    fastTier: null,
  }],
})
```

`acceptedExtensions: null` means the types are unknown, and the model is
told only that a file came; `[]` means the model reads none.

`conversation.blocks` is what the conversation holds, oldest first.
`conversation.events()` yields everything that happens in it, as the app
shows it live; a `transcript_patch` event says the blocks changed, such as
while the agent writes:

```ts
for await (const event of conversation.events()) {
  if (event.type === 'transcript_patch') {
    render(conversation.blocks)
  }
}
```

The loop ends when you leave it or the conversation closes.
`conversation.close()` stops following the conversation; its agents go on in
the core, and the next action follows it again.

`conversation.agents` lists the conversation's agents: the root, whose id is
the conversation's, and each subagent, with its role, phase and queue. The
`agents` and `agent` events say they changed.

`conversation.sendToAgent(node, text, messageId?, options?)` sends a message
to one subagent, named by its id in `conversation.agents`. A subagent at work
takes it as a steer, an idle one starts a turn with it, and one that ended is
resumed with it. Give the message an id when you may send it again, such as
after a lost answer: the subagent takes a message once, however often it
arrives. `options.senderId` names who sent it, as `run` takes it.

```ts
const tester = conversation.agents.find((agent) => agent.role.kind === 'subagent' && agent.role.profile === 'tester')
if (tester) {
  await conversation.sendToAgent(tester.id, 'The build finished.', 'reminder-7')
}
```

## Schedules

A schedule wakes its conversation with a message: at set times, when a
command's output changes, when its URL is called, or when another
conversation finishes. The conversation's agent makes one with
`jush schedule`; your program makes, changes, runs and removes any
conversation's schedules with `conversation.schedules`, which takes the same
fields:

```ts
const { schedule } = await conversation.schedules.create({
  name: 'Daily report',
  trigger: { kind: 'time', timing: { kind: 'cron', expression: '0 10 * * 1-5' } },
  timeZone: 'America/New_York',
  prompt: 'Post the daily report.',
})
await conversation.schedules.update(schedule.id, { paused: true })
await conversation.schedules.run(schedule.id)
const runs = await conversation.schedules.runs(schedule.id)
await conversation.schedules.remove(schedule.id)
```

A new schedule starts from the defaults of `jush schedule create`, every day
at 09:00, and is on unless the patch says `paused: true`. `create` and
`update` answer the schedule with `secret`, a URL trigger's secret when the
call made one, and null otherwise: it is the only time the secret is shown.

`runs(id, before?)` answers 50 runs at most, newest first; pass the last
one's id as `before` for the 50 before it. Each run says what fired it
(`cause`) and the turn its message started (`turn`), whose id is the
message's: null until the message goes, such as in the answer of `run`,
and for a run skipped before it went. A schedule's `createdIn` and
`editedIn` name the agent's turn that made it and that last changed what it
does; both are null when your program did.

Each conversation in `core.state` lists its schedules with their next time
and last run, and `core.on('state')` hears of each change and each run.

## Permission requests

A command that acts on Jush itself, such as `jush credentials`, which hands
the agent a stored secret, or one of your plugin's commands that declares a
permission, waits for the user's permission in the conversation. `events()` yields a `permission`
event for each request, and `conversation.permissions` lists those waiting:

```ts
for await (const event of conversation.events()) {
  if (event.type === 'permission') {
    await event.permission.allow()
  }
}
```

`deny()` refuses it. A request for a credential takes the values it asks
for: `allow({ values: { token: 'xoxb-example' } })`.

## Plugins

A plugin gives the agents commands that your program runs. The following
plugin adds `jush chat send`, which posts to the chat a conversation answers
in:

```ts
import { Jush, definePlugin, leaf } from '@jush-agent/sdk'
import { z } from 'zod'

const chat = definePlugin({
  id: 'chat',
  name: 'Chat Commands',
  description: 'Gives the agent `jush chat send`, which posts to the chat.',
  commands: {
    chat: {
      summary: 'The chat this conversation answers in',
      index: 'Posts to the chat this conversation answers in.',
      commands: {
        send: leaf({
          summary: 'Posts a message to the chat',
          input: z.object({ text: z.string().describe('The message, in Markdown') }),
          positionals: ['text'],
          output: 'the id of the message posted',
          async run({ text }, call) {
            const id = await postToChat(call.invocation.context.conversation, text)
            await call.stdout(`${id}\n`)
          },
        }),
      },
    },
  },
})

const jush = await Jush.start({ plugins: [chat] })
```

Every core of the process gives its agents the plugin's commands. The agent
learns them as it learns Jush's own: `index` is the group's entry in the
index of capabilities the agent reads, which says what the group is for, and
each command's `summary`, input and `output` are its `--help`.

- Each field of `input` is an option of the same name, such as `--text`, and
  its `describe` text is the option's help; `positionals` lists the fields
  given by their place, in order. Arguments that do not fit the schema fail
  before `run` is called.
- `run(input, call)` writes the command's output with `call.stdout()` and
  `call.stderr()`, and reads its standard input with `call.readStdin()` when
  the leaf names a `stdin` field. What it returns is the exit status, 0 when
  it returns nothing; a failure writes why to standard error and returns a
  status other than 0, as a shell command does.
- `call.signal` aborts when the agent stops the command.
- `jsonOutput` declares what the command's `--json` writes.
- A group's `permissions` declares categories, and a leaf's `permission`
  names the one it needs: the conversation then asks before the command
  runs.

A group is under `jush` unless it says `placement: 'root'`, which makes it a
command of the shell, run by its own name: with the plugin above at the
root, the agent and every process its commands start run `chat send`, and
`chat --help` shows it, as `jush` shows its own. Name it after your program:
the core refuses `jush`, a name another plugin takes, and the names of shell
words and common tools, such as `ls`, `git` or `node`; a device leaves out a
command whose name its own shell answers, such as `history`, and keeps the
rest.

```ts
commands: {
  acme: {
    placement: 'root',
    summary: "Acme's tickets",
    index: 'Files and reads Acme tickets.',
    commands: { ticket: { summary: 'Tickets', commands: { create: leaf({ /* … */ }) } } },
  },
},
```

A plugin can also add a context block before each request (`context`),
search a conversation's project (`projectSearch`), and follow conversations
being deleted or forked.

A command your program runs outlives your program's restart. If your
program stops while `run` is under way, the command waits, and once your
program serves the plugin again, `run` is called again for the same call,
with `call.resent` true. `call.request` is the call's number, which no other
call ever takes: use it to do the work once, for example when the command
posts a message. What the command already wrote stays in its output, and
what `run` writes now follows it; input the earlier call read is not read
again. While no program serves the plugin, a new command waits 10 seconds
for one, and then fails, telling the agent that the plugin's program is
unreachable. A `context` call does not wait: when your program stops
during one, the agent goes on without its block.

## Product state and notifications

`core.state` is the core's product state: its workspaces, projects, provider
entries and their models, devices, conversations and sign-ins, as the app
shows them. `core.on('state', listener)` calls the listener at each change,
and answers a function that stops it.

`core.on('notify', listener)` receives what the app would notify the user
of: a turn that finished or failed, a permission request, or a notification
the agent sent with `jush notify send`. A program has one such listener for
each core, and a new one replaces it; when several services listen to one
core, the one that listened last receives them. Answer how you delivered
it; the agent's `jush notify send` reports it:

```ts
core.on('notify', async (message) => {
  if (message.kind === 'agent') {
    await postToChat(message.conversationId, `${message.title}\n${message.body}`)
    return { type: 'sending', to: 1 }
  }
  return { type: 'nowhere' }
})
```

`{ type: 'shown' }` says every place that would show it shows its
conversation already.

## Usage

`core.usage.report` sums the calls of a period by hour or by day of a time
zone, grouped by any of `conversation`, `sender`, `model`, `provider`,
`account`, `turn` and `purpose`:

```ts
const report = await core.usage.report({
  from: new Date('2026-11-01T00:00:00+08:00'),
  to: new Date('2026-12-01T00:00:00+08:00'),
  bucket: 'day',
  timeZone: 'Asia/Shanghai',
  groupBy: ['conversation', 'sender', 'model'],
})
for (const bucket of report.buckets) {
  for (const group of bucket.groups) {
    console.log(bucket.start, group.sender, group.model, group.calls, group.cost.usd, group.apiPrice)
  }
}
```

A group counts its calls and their tokens, `cost` is what the vendors said
the calls cost, by currency and never converted, and `apiPrice` what their
tokens would cost on the vendors' APIs in dollars, leaving out the
`unpricedCalls` whose model has no price. A bucket holds only the groups that
made calls, and a period without calls has no bucket; the first and the last
bucket start and end with the period, so two reports of adjoining periods
never hold the same bucket.

A call's sender is that of the latest message that handed its agent work,
by the `senderId` your program sent it with: a message, a steer, a side
question, or another agent's message; or that of the work whose end opened
its turn.

- Alice's message starts a turn: the turn's calls are Alice's, and so are
  those of the turn that continues once a command it started ends.
- A subagent the agent starts while working for Alice works for Alice,
  and its calls and those of the commands it runs are hers. So is the work
  of a subagent or a conversation the agent sends a message to then.
- Bob steers the running turn: the calls after the agent read his steer
  are Bob's.
- A turn that work for Alice wakes once it ends is Alice's, even after
  Bob's message: a command started in her turn, a subagent working for
  her, or another conversation her turn sent a message to with
  `--wake-when-done`. What ends while Bob's turn runs joins that turn,
  which stays Bob's.

After a message that names no sender, as one from the app or from a
schedule, the calls have a null sender, and so does a turn that the end of
work it started opens. So do the calls outside an agent's work, such as a
title's or the suggested replies after a turn.

## Everything else a core does

Every capability of a core has a method, on the object it acts on, and its
input and answer types are exported from `@jush-agent/sdk`:

```ts
// The root agent's transcript, whose id is the conversation's.
const page = await conversation.history.transcript(conversation.id, { latest: true })
const changed = await conversation.changes.status()
const token = await core.credentials.create({
  name: 'Deploy Token',
  fields: [{ name: 'token', label: 'Token', kind: 'secret', value: deployToken }],
  keep: 'always',
})
```

| Object | Methods |
| --- | --- |
| `core.conversations` | `list({ archived })`, `get(id)`, `create`, `open`, `remove(id)`, `updateMany(items)` |
| `conversation` | `get()`, `update`, `remove()`, `fork`, `reloadPlugins()`, `search(text)`, `cost('hour' \| 'day')`, `workflows()`, `unpin(pin)`, `detach(device)` |
| `conversation.history` | `transcript(agent, page)`, `block(agent, id)`, `turn(id)`, `commandOrigin(id)`, `commandOutput(id)` |
| `conversation.sideQuestions` | `ask`, `followUp`, `retry`, `stop`, `fold`, `remove` |
| `conversation.files` | `list`, `lists`, `read`, `readBytes`, `write`, `makeDirectory`, `remove`, `search` |
| `conversation.changes` | `status`, `file`, `committed`, `branches`, `repositories`, `log`, `show(commit)` |
| `conversation.plugins` | `state(plugin)`, `call(plugin, method, input)` |
| `core` | `search(text)`, `blob(sha256)`, `upload` |
| `core.usage` | `totals()`, `report(options)` |
| `core.models` | `search(text)`, `refresh()` |
| `core.providers` | `list`, `create`, `update`, `remove`, `catalog`, `order`, `status`, `quota`, `test`, `cli`, `installCli`, `accounts(id)`, `signIn` |
| `core.credentials` | `list`, `create`, `update`, `remove`, `reveal(id, field)` |
| `core.instructions` | `get()`, `set(text)` |
| `core.subagents` | `get()`, `update(patch)`, `profiles.create`, `profiles.update`, `profiles.remove` |
| `core.plugins` | `enable(plugin, on)`, `enableFeature(plugin, feature, on)`, `call(plugin, method, input)` |
| `core.workspaces` | `list`, `create`, `update`, `remove`, `order`, `move` |
| `core.projects` | `list`, `create`, `update`, `remove` |
| `core.devices` | `list`, `create`, `claim`, `of(listed)` |
| `device` | `connect`, `spawn`, `exec`, `revoke`, `update`, `elevate`, `workflows(directory)` |
| `device.files` | `list(path)`, `makeDirectory(path)` |
| `core.portForwards` | `list`, `create`, `update`, `remove`, `retry` |

## Showing conversations on your pages

[`@jush-agent/web-components`](../web-components/README.md) shows a
conversation on your own pages with the app's own views: its transcript,
its composer, and its work in windows with a taskbar.
Your server forwards the pages' requests with `core.fetch(request, base)`
and their connection with `core.accept(socket, url)`, which takes a
WebSocket of the standard interface, as Deno's `upgradeWebSocket` and the
`ws` package give.

Jush can reach your program from outside when you tell it where your server
forwards its routes: `Jush.start({ publicUrl: 'https://bot.example.com/jush' })`.
A schedule's URL is then `/hooks/{id}` below it, which your server answers
with `jush.fetch(request, '/jush')`, and a plugin's sign-in returns there.

## Errors

A request the core refuses fails with a `JushError`: `status` is the HTTP
status, `code` the refusal's code, such as `core_open`, and `message` says
why in words. `requestId` names the request in the core's log.

Part of [Jush](../../README.md). The package is under the Apache License 2.0.
