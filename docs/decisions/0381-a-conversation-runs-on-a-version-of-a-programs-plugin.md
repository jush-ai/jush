---
status: proposed
---

# A conversation runs on a version of a program's plugin

## Problem

A team runs its program as several nodes on one core process: a production
node and canary nodes, each a build of the same program with the same
plugin, `acme`, whose command `acme ticket create` its agents and its
scripts call by that name. Each chat group belongs to one node at a time,
and moves from production to a canary and back while its conversation
goes on. A command of a group's conversation must run in the process of
the node the group belongs to, so a canary runs the new code.

A core sends every request of a plugin to the plugin's newest lane,
whichever program opened it, and a plugin's last declaration stands
(0327). Root command names are the core's (0364). So a canary that
declares `acme` replaces production's for every conversation, and one
that declares it under another name changes the name the agents call.

## Decision

A plugin may be declared in several versions at once, and a conversation
names the version it runs on:

```ts
// The canary node: its plugins are the version `canary-1126`.
const jush = await Jush.connect({ url, token, plugins: [acme], pluginVersion: 'canary-1126' })

// A group moves to the canary, and back.
await conversation.update({ pluginVersions: { acme: 'canary-1126' } })
await conversation.update({ pluginVersions: {} })
```

- A declaration without a version is the plugin's current one, as today:
  the last stands. `PUT /plugins/{id}/versions/{version}` declares that
  version, the last of it standing, beside the current one and the other
  versions; `DELETE` of the same path removes that version alone, and
  `DELETE /plugins/{id}` the current one alone. A version is 1 to 64
  letters, digits, `.`, `_` and `-`. A process of an earlier release has
  no such route and refuses the declaration, so a canary never takes the
  current declaration's place there.
- A program's connection declares its plugins in its `pluginVersion`, the
  current one without it, and opens their lanes as that version's:
  `{ kind: "plugin", plugin, version }`.
- A conversation's `pluginVersions` names a version by plugin id, at its
  creation or by `update`, which replaces the whole map; `{}` returns it to
  the current versions. A plugin it names none of runs on the current
  version. A fork and a conversation its agent starts with `jush
  conversation new` keep its versions, as they keep the commands it names.
- A version gives a conversation the work of its agents: their command
  calls, the context of each of their requests, and whether the
  conversation runs. Its agents' commands, their `--help` and the index of
  its system prompt are that version's manifest's. Everything else of the
  plugin is its current declaration's, which every request of it goes to:
  what it keeps of a conversation as it is forked or deleted, a project's
  search, a page's calls and state, tabs, command packages, user streams,
  reply blocks and sign-ins. A plugin declared only in versions has none
  of these until a current declaration comes. A core's notifications go on
  the newest starter lane of a connection of no `pluginVersion`, and on the
  newest of any while none is open.
- A conversation's request goes to the version its record names at that
  moment, which the core reads at each request. A change of its versions
  gives its live agents their commands at once, as a change of its
  `commands` does (0370), and its system prompt's index from their next
  request. A call already sent goes on where it is.
- A version is checked alone as it is declared. A conversation's versions
  are checked with the other plugins as it names them: a plugin of the
  release's own has none, and two declarations that take one command name
  are refused, with the name. A declaration since that takes a name
  another version takes leaves the conversation without a toolset, with
  the name, until the declarations or its versions change.
- A version no connection declares now, such as one whose node has not
  connected since the process started, leaves the conversation without
  that plugin's commands, as a plugin no program declared (0327). While no
  lane of a version is open, its requests wait as long as a device whose
  connection dropped counts as online, then fail, and a context request
  does not wait, as for a plugin whose program is unreachable.
- What a plugin keeps is the plugin's, whichever version reads it: its
  store, its secrets, its sign-ins, the user's switch of it and of its
  features. A feature only a version declares takes the default it
  declares.
- A conversation's versions are a column of its record, `plugin_versions`,
  a JSON object. A conversation of an earlier release names none.

## Spike

A process ran on its own; a production connection declared `acme` and a
canary connection declared it as `canary-1126`, with a `ticket close` of
its own and its own index sentence, each answering with its node's name,
over a scripted model, on Bun; a Node connection pinned and read back a
conversation's versions:

- A conversation pinned to `canary-1126` filed its ticket on the canary,
  its context came from the canary, `acme ticket close` ran there, and its
  `--help` and index named `close`. An unpinned one filed on production,
  got production's context, and did not know `close`.
- Unpinned while a slow call ran on the canary, the call answered from the
  canary; the next call ran on production.
- Pinned again after the canary disconnected, a call waited 10 seconds and
  failed with the version named; the unpinned conversation went on.
- A pin to a version no connection declared left `acme` unknown to the
  conversation's shell. A version with a space was refused, in a pin and
  in a connection's `pluginVersion`.
- Each notification of the core reached the production connection, though
  the canary's starter lane opened after it.
- A fork of a pinned conversation ran its calls on the canary, and a
  pinned conversation deleted and made again under its id with no
  versions ran on production: each request reads the record, which a copy
  kept in memory would not.
- Two plugins whose canaries both added the root command `tix` were
  declared; pinning both was refused with the name. A version of `web`, a
  plugin of the release's own, was refused.
- A process of 0.2.6342 refused a connection with `pluginVersion`: it has
  no route `PUT /plugins/acme/versions/c1`.
- Cores made by 0.2.6252, 0.2.6254 and 0.2.6342 open, their conversations
  naming no version.

## Prior art

- Temporal's Worker Versioning lets workers of several deployment versions
  poll one task queue. A workflow without a pinned version runs on the
  current version; `update-options` pins one workflow to a version, also
  while it runs; an activity in flight finishes where it is
  ([Worker Versioning](https://docs.temporal.io/production-deployment/worker-deployments/worker-versioning)).
- Jush pins a conversation, as Temporal pins a workflow, and keeps a
  plugin's data shared by its versions, as Temporal's versions share their
  namespace.

## Alternatives

- Root command names unique only within a conversation, and the canary a
  plugin of another id that names the same command: the two plugins share
  no store, secrets or sign-ins, the conversation's `commands` needs a way
  to tell two of one name apart, and the plugin settings list both.
- A request to the lane of the connection that last sent the
  conversation's message: the core does not know which program a message
  came from, and a schedule's run or a background command's end comes
  from none.
- The canary's plugin under a new command name: every script and
  procedure that names the command changes with each canary.

## Open questions

None.
