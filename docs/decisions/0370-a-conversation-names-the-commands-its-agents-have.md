---
status: proposed
---

# A conversation names the commands its agents have

Supersedes: 0324 (A conversation speaks as its program's product, with the
plugins it names), the list of the program's plugins, which this list
replaces.

## Problem

A program's conversation has every command Jush has. A help desk whose
answers are read in a team's chat has agents that run `jush notify`, whose
notices nobody receives; `jush conversation new`, whose conversation nobody
sees; `jush conversation history` of another team's conversation;
`jush device pair`; and `jush credentials`, whose permission nobody can
grant. The program can deny each permission request, but the agent still
reads the commands in its prompt and tries them.

## Decision

A conversation names the commands its agents have, Jush's and its
plugins' alike:

```ts
await core.conversations.create({
  commands: ['browser', 'schedule', 'conversation list', 'acme'],
})
```

- `commands` is a list of command paths without `jush`: a group, such as
  `browser`, names every command under it, and a leaf, such as
  `conversation list`, names that one. A program's plugin's command is
  named by its own path, the name of its group, such as `acme` (0364), not
  by the plugin's id; one placed at the root under the name of a group of
  Jush's, such as `notify`, is named together with it. Null names every
  command, as a conversation of the web app does.
- A command the list does not name is not in the agent's prompt, its index
  or `--help`, and running it answers as for a command that does not exist.
  A group the list names in part says in the index the commands it kept;
  the guide's and the execution context's sentences that tell of a command,
  such as `jush attachment upload`'s and `jush device list`'s, go with it.
- `jush shell`, which reads what the shell tool cut short, stays whatever
  the list says, as the shell tool's own; so does `jush agent cwd` where the
  list names a command of `agent`, since a child's working directory block
  points at it.
- A plugin none of whose commands the list names gives the conversation no
  context block either. A plugin of no command is not the list's, and its
  context answers each conversation as its program decides.
- A command must also be on for the core, as Jush's plugins follow the
  user's switches today.
- A change reaches each live agent as 0324's list did: its commands from its
  next command, its prompt from its next request.
- The conversation's toolset is narrowed to the list, and the runtime's
  `agent` and `shell` groups, which the agent server grafts on it, are
  narrowed again, so the prompt's index, `--help` and the commands that run
  are one set. The toolset's revision names the list, so two conversations
  of two lists never share one.
- The record keeps the list in 0324's column, renamed `commands`. A
  migration turns an old list of plugin ids into the top groups of Jush's
  commands then and those ids, each taken for the name of its plugin's
  group, as the plugins of the SDK's examples name theirs. A plugin whose
  group is named otherwise, and a group Jush adds later, are the program's
  to name again.

## Spike

A program named `['shell', 'conversation history']` for a conversation, a
group under `conversation`:

- Its prompt's index listed `jush conversation: history` and `jush shell`
  alone, and its guide told of neither `jush attachment upload` nor
  `jush device move`.
- `jush conversation history --help` answered, and `jush conversation
  list`, `jush notify send` and `jush agent list` answered as unknown
  commands.
- With the list cleared, `jush notify --help` answered from the next
  command.
- A path such as `Browser Tabs!` was refused.
- A core of 0.2.6332 opened with its lists turned: `['acme']` became Jush's
  nineteen groups and `acme`, `[]` Jush's groups alone, and no list stayed
  none. The step's turn of a pending fork's metadata was run on its own on
  rows with `plugins` of one id, of none and without the key: each became
  `commands` as a JSON array, or stayed.

## Prior art

- Gemini CLI's `tools.core` lists the tools its agent has, down to a shell
  command, which its guide for the enterprise recommends over its deny list
  `tools.exclude`, deprecated for its policy engine
  ([Gemini CLI for the enterprise](https://geminicli.com/docs/cli/enterprise/)).
- The Claude Agent SDK takes `allowedTools` and `disallowedTools`, which
  name the tools an agent has, down to a tool's arguments
  ([Agent SDK permissions](https://docs.claude.com/en/docs/agent-sdk/permissions)).
- Jush departs in taking an allow list alone, so a command Jush adds later
  does not reach a program's conversations until the program names it, and
  in keeping the shell tool's own reader.

## Alternatives

- A deny list: every command Jush adds would reach the program's agents
  unannounced.
- Two lists, Jush's commands and the program's plugins, as 0324 had one for
  plugins: two ways to say one thing.

## Open questions

None.
