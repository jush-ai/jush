---
status: proposed
---

# An Antigravity subscription runs through Jush's copy of Google's agy

## Problem

Alex pays for Google AI Pro and wants Jush's agents to run on Gemini 4 Neon
with it. Jush reaches Gemini only through a Gemini API key, which Google
bills apart from the subscription, so Alex pays twice or does not use
Gemini at all. Claude, ChatGPT and Grok subscriptions already serve Jush's
agents (Claude Code, Codex, Grok Build); Google's does not.

Google serves a personal subscription only to its own programs. Since
2026-06-18 Gemini CLI no longer takes one; the Antigravity app and its CLI,
`agy`, do. The Antigravity SDK, the one embeddable runtime Google
publishes, takes an API key, Vertex or an OpenAI-compatible endpoint, and no
subscription.

## Decision

Settings has an Antigravity page beside Claude Code's, with the same
account list, Add Account, Rotation and Remove. Add Account first says, in
a dialog with Cancel and Continue, that Antigravity's terms prohibit using
it through third-party software such as Jush and that an account used this
way may be suspended; it says so each time an account is added and nowhere
else. Continue shows the link `agy` prints and a field for the code
Google's page gives back. The account shows its plan and the usage `agy`
reports. The model picker lists what the account's plan offers, each model
with the effort levels Google gives it:

```text
Antigravity · alex@example.com (Google AI Pro)
  Gemini 4 Neon        Low · Medium · High · XHigh
  Gemini 4 Hydrogen    Low · Medium · High · XHigh
  Gemini 4 Argon       Low · Medium · High
  Gemini 3.8 Flash     Low · Medium · High
  Claude Opus 5.5      Low · Medium · High
  Claude Sonnet 5.5    Low · Medium · High
  gpt-oss 120B         Medium
```

The agent, its system prompt, its `shell` tool and its transcript are
Jush's, as with Claude Code. `agy` is the channel to Google and nothing
else.

### The rule this provider never breaks

Every request of the subscription is `agy`'s own, made by an unmodified
`agy` with its default endpoints. Jush never reads, copies or uses the
account's OAuth tokens, never calls a Google endpoint with them, and never
starts `agy` with a flag or an environment variable its documentation does
not list. Jush reads one
field of the sign-in, the account's email, to name the account: once
`agy`'s login succeeds, Jush runs `sed` in the sign-in's directory, which
prints the email alone, so the sign-in's bytes never leave the machine.

### The pieces

- **Family** `antigravity`, named *Antigravity*. Its entries do not connect
  through a device (0269): `agy` makes the requests, on the machine that
  runs it, so that machine's network must be one Google serves, as Claude
  Code's must be one Anthropic serves. The page says nothing more about it.
- **Install.** `agy` is a CLI of the Subscriptions machine (0353): the
  provider declares it (`Cli { name: "Antigravity", process_parent:
  "processes" }`), and `jush.subscription-cli` installs Jush's own copy of
  it as it installs Claude Code's. Its release reader follows the newest
  version of Google's per-platform manifest
  (`manifests/<platform>.json`: version, URL, SHA-512), reads each
  platform's archive size from its URL's `Content-Length`, and maps
  Google's platform names onto Jush's, such as `linux_amd64_musl` onto
  `linux-x64-musl`. Jush runs only `agy acp`, which starts no updater, so
  Jush alone changes the version.
- **Account directory**, one per account. The sign-in lives there, in
  `.gemini/antigravity-cli/antigravity-oauth-token`, and nowhere else: each
  process reaches it through a link, and `agy` renews it through the link,
  writing the account's file (measured). Signing in runs `agy acp --login`
  with the new directory as its `HOME`, which
  becomes the account's when it succeeds, as Claude Code's login does: the
  sign-in moves from the Claude Code provider to `provider-subscription-cli`,
  and each CLI says only how it starts its login, how it refuses a code,
  where it says why it failed and how to read the account it signed in.
  `agy` ends on a code it refuses, saying why on its standard output.
- **Process**, one per session, kept between turns and closed when idle by
  the rules Claude Code's follow, SIGTERM first. Its own directory is its
  `HOME` and its working directory, so everything `agy` keeps of the
  process, its conversation database, summaries, logs and caches, goes
  with the directory. The placement makes the directory's link to the
  account's sign-in before it starts `agy`, as it writes a start's files,
  so `agy` is started directly and the placement sees its end as it is,
  such as an executable a runner removed:

  ```text
  agy acp --agent jush --dangerously-skip-permissions --disable-telemetry
    HOME = cwd = <the process's own directory>
      .gemini/antigravity-cli/antigravity-oauth-token -> the account's
      .agents/agents/jush.md     the request's system prompt
  ```

  `jush.md` is a custom agent, a documented workspace customization:
  `mainAgent: true`, `subagent: false`, `tools: []` (none of `agy`'s own
  tools), `mcpServers: [{name: jush}]`, `excludeDefaultComponents: true`,
  and the request's system prompt as its body. The agent's body takes the
  place of `agy`'s own prompt, but `agy` still adds parts of its own to
  every agent's: the workspace and app-data paths of the process's
  directory, its artifact and scratch-file instructions, its messaging
  system, its MCP notes, its guidelines and a rule to link files with
  `file://` paths, about 2,100 tokens a request, which name `agy`'s ways
  and contradict Jush's prompt. `excludeDefaultComponents`, which `agy`'s
  release notes list, leaves them all out. What stays is the local time
  and model note `agy` adds to each message, and three of its tools
  (below). A request whose system prompt differs from the process's starts
  a new process; a different model or effort is `session/set_model` on the
  same one, with the model's id at its level, such as `gemini-4-neon-high`.
- **Tools.** The process directory's `.gemini/config/mcp_config.json` names
  one server, `jush`, whose `shell` tool is eager, so the model sees it with
  its schema as `mcp_jush_shell`, and whose `timeoutSeconds` is the most it
  holds: Jush returns every call itself, and a call that waits for the user
  has no end. The server's command is `sh` and `cat`
  alone, joined to two named pipes in the process's directory. The
  placement starts a companion process beside `agy`, in the same
  directory, which joins the pipes to its own standard streams, and Jush's
  MCP server, the one Claude Code's tools use, reads and writes those:

  ```text
  agy --stdio-- sh of mcp_config.json      companion sh --stdio-- Jush's
                  cat > mcp-in  --------->  cat mcp-in            MCP server
                  cat mcp-out  <---------  cat > mcp-out
  ```

  The channel needs no program of Jush's on the machine, and runs on the
  processes' own streams, which a relay to another user's shard (0353)
  carries as it carries `agy`'s. A `tools/call` names no call of the
  model's, so the server gives each an id of its own; it waits for the
  agent's result, as Claude Code's do.
- **History.** A session with no history, a conversation's first turn,
  starts in its turn's process. One with history starts in two processes:

  ```text
  1. agy acp: session/new, no prompt    agy makes conversations/<id>.db,
     then its input closes              fixing the agent and its prompt,
                                        and exits
  2. provider reads the db, fills its   the placement keeps the first
     steps from the conversation's      process's directory until the
     blocks                             provider lets it go
  3. agy acp: session/load <id>         the turn's process, whose files
                                        hold the filled db
  ```

  A session takes its agent and system prompt from when it was made, and a
  process does not read steps written after it opened the session, so
  neither process can be skipped. A session made in one directory loads in
  another, with the agent and prompt it was made with (measured). The
  database is read from the first process's directory and written as a
  file of the second, which a relay carries as well. The steps
  are written into `steps` alone: a user message as a user step, the
  model's text as a response step, a `shell` call in its response step with
  its result as an MCP tool step. Reasoning is left out: ACP gives its text
  without Google's signature. The format is `agy`'s own and undocumented;
  `agy` carries the full schema of its steps as protobuf descriptors, which
  the scenario suite reads to check a new version against what the provider
  writes. The database goes with its process's directory.
- **Turns.** A turn is `session/prompt`, of the user messages the
  transcript ends with. `agent_message_chunk` is text and
  `agent_thought_chunk` reasoning. A call of the model's ends a run and
  keeps the process, whose turn waits for the result the next run delivers.
  `agy` takes input only between turns, so a message that comes beside a
  result, such as a steer, is answered by a new process, whose history holds
  the result. `agy` takes no prompt without a message, so a transcript that
  ends with a result its process no longer waits for, as after the backend
  restarted while a tool ran, fails and asks the user to send a message.
  Giving a run up closes its process.
- **Usage.** `agy` reports a turn's tokens only at its end, summed over the
  turn's calls, and after each call the tokens the session holds
  (`usage_update`). A run reports the latter as its input: the context the
  next request carries, which the agent compacts by. The usage ledger's
  price of a call is therefore an estimate.
- **Failures.** `agy` reports a model error as message text,
  `⚠ <STATUS> (code <n>): <message>`. `RESOURCE_EXHAUSTED` marks the
  account limited, as Claude Code's usage limit does, unless a usage read
  that began after the request was sent shows quota left in every limit of
  the request's model, its group's. Google refuses a request as it
  arrives, so such a read sees the account at least as used as Google did.
  Two requests refused together, such as a turn and its title, share one
  read, and a request sent after that read began reads once more. Google
  gives the same answer to a request whose system prompt it refuses and
  to a short throttle, whatever quota is left (see Prior art). That
  request fails with Google's answer and the quota left, and
  with no error code, so nothing sends it again, rotates away from the
  account or rests it. A read that fails counts as no quota left.
  `FAILED_PRECONDITION` "User location is not supported" fails the request
  with that cause and is never retried. An authentication error marks the
  account signed out.
- **Quota.** The plan comes from `session/new`
  (`_meta.antigravity.tier`), and the windows from a usage read: a process
  of the account runs `agy --print /usage --output-format json`, the
  documented print mode and command, which makes no model request and
  prints each group of models that share limits (Gemini, Claude and GPT,
  Gemini 4 Argon) with each limit's share left and reset. Each limit is a
  window scoped to its group, and the scope names the group's models by
  their catalog ids: the ones its description lists by the names
  `session/new` gives them, or, for a group that lists none, the model of
  the group's name. Rotation then counts a window only for its group's
  models. A read that fails, such as one Google refuses where `agy` runs,
  fails the probe: the page shows why, and the next probe waits as after
  any failed read. Every `agy` runs with `AGY_CLI_DISABLE_AUTO_UPDATE=true`,
  the switch Google's troubleshooting guide documents: print mode
  otherwise replaces its own executable, Jush's verified copy, with
  Google's newest release.
- **Catalog.** A process of the account answers `session/new` and closes:
  its `models.availableModels` lists each model with each effort level,
  such as `gemini-4-neon-high` "Gemini 4 Neon (High)", which the catalog
  groups into a model and its levels, the levels being those its
  `thought_level` option names. Context sizes and prices come from
  the listing models.dev's search gives each model, its maker's where the
  maker lists it, such as `google`'s for Gemini and `anthropic`'s for
  Claude; the catalog carries each price, so the usage of a call is priced
  as any listed model's is. A model no listing knows, such as `agy`'s
  orchestrators, which no vendor sells, is left out: it has no context
  size and no price to show.

### The runner installs what Google publishes

Google publishes `agy` as a `tar.gz` archive with its SHA-512, and the
runner's artifact cache takes one executable or a zip or `.tar.zst`
archive, each checked by its SHA-256. The cache learns both, so that it
checks the bytes Google hashed, and nothing in between downloads or hashes
them again:

```rust
// crates/command-protocol: written as today's `"sha256": "<hex>"` or as
// `"sha512": "<hex>"` beside the artifact's other fields
pub enum ArtifactHash {
    Sha256(String),
    Sha512(String),
}
pub struct ArtifactInstall { /* … */ #[serde(flatten)] pub hash: ArtifactHash, /* … */ }
pub struct InstalledArtifact { /* … */ #[serde(flatten)] pub hash: ArtifactHash, /* … */ }
```

- **What stays SHA-256.** A package's executable (`PackageArtifact`), the
  runner's own release and everything else Jush builds keep their SHA-256,
  and the backend locates only a package's executable, by its SHA-256.
- **Release record.** An artifact of `jush.subscription-cli`'s record
  carries its hash the same way and its form as an install names it: a
  file, or an archive and its entry (`antigravity` in `agy`'s).
- **Every install names its URL.** The package names each artifact's
  vendor URL in its install, as Chrome's install does, instead of the
  backend attaching the URLs to the stream. Attached artifacts had no other
  user and go, and with them an install without a URL, which the backend
  could locate only through them: `ArtifactInstall.url` becomes required.
- **Cache.** An entry is named by its hash: a SHA-256 entry by its hex, as
  today, so no entry and no image copy moves, and a SHA-512 one by
  `sha512-<hex>`. An archive whose first bytes are gzip's is unpacked as a
  `tar.gz`.
- **Older runners.** A runner of an earlier release cannot read an install
  by SHA-512: the request fails to parse, which breaks the resident's
  artifacts stream, so its later installs fail too. The Subscriptions
  machine's runner is always its server's release, and no other artifact
  changes its hash.

### What changes for the model

- A response's calls run one after another: `agy` hands Jush a call only
  when the one before it has its result. The transcript records each call
  as its own step, and the `shell` description's "Calls in one response run
  in parallel" does not hold on this provider.
- `agy` wraps each user message in `<USER_REQUEST>`, adds the local time,
  notes a model change in the message after it, and puts `Created At` and
  `Completed At` lines before each tool result. The model reads those
  words, which Jush did not send.
- A `/` or `@` in Jush's text reaches the model as written: `agy` runs no
  slash command and reads no file for it.

## Spike

Measured with `agy` 1.5.13 on macOS arm64, on a Google AI Pro account
signed in through `agy`, and on a Gemini API key for the same checks:

| Point | Result |
| --- | --- |
| A custom agent replaces the system prompt | Its body takes the place of `agy`'s prompt: a turn's base input fell from about 8,900 tokens to about 2,500. `agy` still adds its own parts: asked to quote everything before its message, the model quoted the body and then `agy`'s workspace, MCP, messaging, artifact, guideline and link-style blocks, the same on the subscription and on an API key; a one-line prompt with one tool took about 2,830 input tokens. |
| `excludeDefaultComponents: true` | Those blocks are gone: the same request took 739 input tokens. The local time, the model note and the three tools below remain. |
| `tools: []` removes `agy`'s tools | 3 remain: `list_resources`, `read_resource` and `manage_task`. No documented setting removes them. |
| An eager MCP tool | Offered as `mcp_<server>_<tool>` with its schema. A lazy one, the default, is called through `call_mcp_tool` after the model reads a schema file, and the model guessed its argument names 10 times before reading it. |
| An MCP server given in ACP's `session/new` | Reaches a custom agent only by name and only lazily, so the server goes in `mcp_config.json`. |
| A long call | `agy` gives up on a call after 180 s by default and the model calls again; with `timeoutSeconds` at 2,147,483,647, a call answered after 240 s reached the model once. |
| `agy`'s arguments to Jush's tool | Only the schema's: `agy` asks the model for `toolSummary` and removes it before the call. |
| A session written from scratch | Six steps, built by hand from the schema (user, response, response with a call, MCP result, response), loaded with `session/load`: the model repeated the conversation's facts and the call's output, then called the tool again. |
| Making the session's database | `session/new` makes it with no model request. A database built entirely by hand loads, but its session runs `agy`'s default agent, whose prompt says "I am Antigravity". A changed agent file does not reach a session made before it, and a load in the process that made the session ignores steps written since. `session/new` takes about 0.1 s from the process's start, `session/load` 0.2 s, and SIGTERM ends a process in under 10 ms. |
| Updates | `agy -p` started its background updater on every run, and one run of `agy -p /usage` replaced its own executable, 1.5.13 with 1.5.14; `agy acp` started none in 17 runs. |
| `agy -p /usage --output-format json` | Printed the account's groups and limits as JSON in about 11 s, with no model request (`num_turns` 0). ACP offers no `/usage` command. |
| Workspace agent | `.agents/agents/jush.md` in the process's directory is found by `--agent jush`. |
| MCP server's environment | It inherits `agy`'s: a variable set on `agy` reached the server. |
| `/` and `@` | `/goal Say hi.` reached the model as text, and `@/tmp/notes.txt` did not put the file's content in the request. |
| Linux | The same agent, eager tool and inherited variable work on Ubuntu 26.04 x86_64. |
| Parallel calls | The model asked for 3 in one response; `agy` sent them one at a time, each after the previous result. |
| Reasoning, images, cancel | Reasoning text streams; an image in the prompt is read; `session/cancel` returns `cancelled` within 10 ms. |
| Location | Through one US exit, 5 of 17 processes met "User location is not supported" on Google's daily backend, some only on `agy`'s own title request, while others on the same exit succeeded seconds apart. The Gemini API through the same exit passed 16 of 16. |

Not measured: several processes of one account renewing its sign-in at the
same moment, and Linux on arm64 or musl.

## Prior art

- Editors drive `agy acp` as an ACP agent with `agy`'s own tools: Zed,
  JetBrains IDEs, Xcode
  ([Other editors (ACP)](https://antigravity.google/docs/ide/extensions/acp/)).
  They send no system prompt. Jush instead keeps the agent and hands `agy`
  none of its tools.
- [T3 Code](https://github.com/pingdotgg/t3code) hosts `agy acp` the way
  editors do, with `agy`'s own tools, permissions, skills and subagents,
  and appends one short instruction through
  `_meta.antigravity.systemPrompt`. It reads the account's quota by calling
  Google's backend with the token `agy` keeps, which Jush never does.
- magpie's [`agy-cli`](https://github.com/magpie-community/plugins)
  provider bridges another agent's tools to `agy acp` over MCP, as Jush
  does. By default it keeps `agy`'s own prompt and puts the agent's in the
  first user message; `excludeDefaultComponents` is an option it leaves
  off, as requests without `agy`'s prompt no longer look like `agy`'s own.
  It reads the quota with `agy -p /usage`. It found that Google answers
  `429 RESOURCE_EXHAUSTED`, whatever quota is left, to a request whose
  system instruction holds one of a growing list of other agents' identity
  lines, and rewords those lines to pass; Jush rewords nothing to pass a
  check. magpie withdrew its earlier provider, which called Google's
  backend with the account's tokens, because Google suspends accounts used
  that way.
- oh-my-pi appended its whole prompt through
  `_meta.antigravity.systemPrompt`, met that 429, and a proposal there
  moves the prompt out of the system instruction.
- [opencode-antigravity-auth](https://github.com/NoeFabris/opencode-antigravity-auth)
  calls Google's backend with the account's OAuth tokens. Jush never does;
  that is the account's ban risk.
- [Ki #202](https://github.com/arteeh/ki/issues/202) plans the same shape
  (Ki's tools over MCP, Ki's prompt, Ki's history) and is parked on
  Google's terms.
- Jush's Claude Code provider is the shape this follows: Jush's copy of
  the vendor's CLI, one directory per account, a kept process, the session
  written from Jush's blocks.

## Alternatives

- **The Antigravity SDK** (`localharness`): every part is public and
  better (all built-in tools off, Jush's tools over its own connection,
  history through `initial_trajectory`), but it takes no subscription.
- **`agy -p` with stream-json**: one process per session too, but its
  input schema is undocumented (it wants an `event` field), where ACP is.
- **Calling Google's backend with the account's tokens**: refused, see the
  rule above.

## Release

Google's FAQ forbids running `agy` from a third-party application on a
consumer plan, and section 6 of Antigravity's terms calls using it through
third-party software a breach that may suspend the account. Accounts that
called Google's backend with their tokens have been suspended in numbers,
with `403 PERMISSION_DENIED` "This service has been disabled in this
account for violation of Terms of Service"; for accounts used through an
unmodified `agy acp`, public reports disagree. One user of an agent with
`excludeDefaultComponents: true`, whose prompt also carried another
agent's editing instructions, reports a suspension; what caused it is not
known. The provider stays on its branch, unmerged, until the user decides
to release it.

## Open questions

None.
