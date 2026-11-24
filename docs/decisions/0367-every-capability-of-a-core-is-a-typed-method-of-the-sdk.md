---
status: proposed
---

# Every capability of a core is a typed method of the SDK

Supersedes: 0301 (A TypeScript SDK runs Jush's cores in a program), the
table of calls, which covered what a first program needed and left every
other capability to `core.fetch`.

## Problem

A program that forks a conversation, reads a subagent's transcript, searches
a core, reads its usage or switches a plugin finds no method for it. It
builds a request to a route with `core.fetch`, such as
`POST /conversations/{id}/fork`, and writes the answer's schema itself:

```ts
const response = await core.fetch(new Request(`http://local/api/conversations/${id}/fork`, {
  method: 'POST',
  body: JSON.stringify({ id: crypto.randomUUID(), blockId }),
}))
const forked = myForkAnswerSchema.parse(await response.json())
```

Of a core's 111 routes, the SDK calls about 20. The routes are the web app's
internal contract, shaped for its screens: a program that calls them breaks
when a screen changes, and no type tells it what they take.

## Decision

The SDK's entry comment (`packages/sdk/src/index.ts`) states the rule:

- The routes and lanes are how the SDK reaches its core; they are no API.
- Every capability of a core a program can use is a method of the SDK, whose
  input and answer are typed and exported from `@jush-agent/sdk`.
- A route that serves only the web app's own screens has no method.

Methods hang on the object they act on and are named as JavaScript names
them: `list`, `get`, `create`, `update`, `remove`. They take and answer the
protocol's types, as the SDK's methods do today, so a type has one source.

### The methods

| Object | Methods | Routes |
| --- | --- | --- |
| `core.conversations` | `list({ archived })`, `get(id)`, `create`, `open`, `remove(id)`, `updateMany(items)` | `/conversations`, `/conversations/{id}`, `/conversations/batch` |
| `conversation` | `get()`, `update`, `remove()`, `fork({ through })`, `reloadPlugins()`, `search(text)`, `cost(window)`, `workflows()`, `unpin(pin)`, `detach(device)` | `/conversations/{id}/…` |
| `conversation.history` | `transcript(agent, page)`, `block(agent, id)`, `turn(id)`, `commandOrigin(id)`, `commandOutput(id)` | `agents/{node}/transcript`, `agents/{node}/blocks/{block}`, `turns/{turn}`, `commands/{command}`, `commands/{command}/output` |
| `conversation.sideQuestions` | `ask`, `followUp`, `retry`, `stop`, `fold`, `remove` | `side-questions/…` |
| `conversation.files` | `list`, `lists`, `read`, `readBytes`, `write`, `makeDirectory`, `remove`, `search` | `fs/…`, `devices/{device}/fs` |
| `conversation.changes` | `status`, `file`, `committed`, `branches`, `repositories`, `log`, `show(commit)` | `changes/…` |
| `conversation.plugins` | `state(plugin)`, `call(plugin, method, input)` | `plugins/{plugin}/state`, `plugins/{plugin}/calls/{method}` |
| `core` | `search(text)`, `blob(sha256)`, `upload` | `/search`, `/blobs/{sha256}`, `/attachments` |
| `core.usage` | `totals()` | `/usage` |
| `core.models` | `search(text)`, `refresh()` | `/models/…` |
| `core.providers` | today's, and `catalog()`, `order(ids)`, `status(id)`, `quota(id)`, `test(id)`, `cli(id)`, `installCli(id)` | `/providers/…` |
| `core.credentials` | `list`, `create`, `update`, `remove`, `reveal(id, field)` | `/credentials/…` |
| `core.instructions` | `get()`, `set(text)` | `/settings/instructions`, `/instructions` |
| `core.subagents` | `get()`, `update(patch)`, `profiles.create`, `profiles.update`, `profiles.remove` | `/settings/subagents`, `/subagents/…` |
| `core.plugins` | `enable(plugin, on)`, `enableFeature(plugin, feature, on)`, `call(plugin, method, input)` | `/plugins/{plugin}/…` |
| `core.workspaces` | `list`, `create`, `update`, `remove`, `order`, `move` | `/workspaces/…` |
| `core.projects` | `list`, `create`, `update`, `remove` | `/projects/…` |
| `core.devices`, `device` | today's, and `core.devices.list()`; `device.update(patch)`, `elevate()`, `workflows(directory)` | `/devices/…` |
| `device.files` | `list(path)`, `makeDirectory(path)` | `/devices/{id}/fs` |
| `core.portForwards` | `list`, `create`, `update`, `remove`, `retry` | `/port-forwards/…` |

0372 adds `core.usage.report`, and 0374 a fork before a turn.
`core.state.preferences` and `core.updatePreferences` remain the
preferences' reader and writer.

The protocol's `CommandCall`, where the shell call that started a command
lies, becomes `CommandOrigin`: the SDK already names a plugin command's
call `CommandCall`, and one name names one concept.

The routes that serve only the web app's screens keep no method: the sidebar's
order (`/sidebar/reorder`), unread marks (`/conversations/{id}/read`), the
composer's draft and saved messages (`/draft`, `/saved/…`), the suggested
replies' use, whether an attachment's file is there for the Files window
(`/attachments/{attachment}/file`; its bytes are `core.blob`'s), a page's
rendering of an HTML file (`/fs/html`, `/html/{token}`),
a reply block's document and facts, a plugin page's files, the layout of a
conversation's windows (`/work/changes`, which the core never reads), and a
project's saved-message scope. A plugin's sign-in return is 0369's. The
lanes' live file watches and command output serve the conversation's views,
and come with 0368's source.

`core.fetch` and `core.accept` go (0368).

## Spike

A program called every method against a core with a device, a git working
tree and a scripted model: a conversation's run, its transcript, block,
turn, command origin and output, side question, fork and search; files read,
written, listed, searched and deleted; changes, branches, log and commit;
credentials, instructions, subagent profiles, workspaces, projects,
devices, providers' catalog, status, order and test; and a blob. Each
answered as its route does. A conversation's plugin state was not reached:
the core it ran had no plugin that keeps one.

## Prior art

- The OpenAI and Anthropic TypeScript SDKs give every endpoint of their APIs
  a typed method on the resource it acts on, such as
  `client.files.create` and `client.beta.files.list`, and export the types
  ([openai-node](https://github.com/openai/openai-node),
  [anthropic-sdk-typescript](https://github.com/anthropics/anthropic-sdk-typescript)).
- Jush departs in leaving out the routes of its own screens, which no
  program has a use for.

## Alternatives

- Methods generated from the routes, as Stainless generates the SDKs above
  from an OpenAPI document: the routes are named for the web app's screens,
  so generated names would carry that shape into the program.
- A method only where a program asked for one, as the SDK did: every other
  capability stayed a route the program called blind.

## Open questions

None.
