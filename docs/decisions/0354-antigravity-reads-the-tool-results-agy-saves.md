---
status: proposed
---

# Antigravity's model reads the tool results agy saves, and its turns report their output

Supersedes: 0352 (An Antigravity subscription runs through Jush's copy of
Google's agy), these parts: `tools: []`, `--dangerously-skip-permissions`,
the session's working directory being the process's directory, the agent's
prompt being the request's system prompt alone, Jush refusing every request
`agy` makes, and **Usage**.

## Problem

Alex asks Jush's agent, running on Gemini 4 Neon through Antigravity, what
a screenshot of the login page shows. The agent runs `shell`, the result
carries the 60 KB PNG, and the model answers that it cannot see any
picture. Later it reads a 30 KB build log and reports the build as clean;
the error it missed was in the middle of the log.

`agy` gives the model an MCP tool's result only up to 10,240 bytes. A
larger image is saved to a file, and the model reads only a line naming
it:

```text
[Resource offloaded to file:///<process dir>/.gemini/antigravity-cli/brain/<session>/.system_generated/steps/2/media_0.jpg]
```

Longer text keeps its first and last parts around this notice, and the full
text goes to `output.txt` beside it:

```text
The tool's output was truncated due to being too long. You can find the full output in a file at file:///…/steps/2/output.txt.
<truncated 23262 of 33104 bytes from the middle of the output>
```

Jush's agent has none of `agy`'s tools, so it can open neither file. Every
image over 10 KB is invisible to the model, and the middle of every long
output is lost without the model knowing it. The usage of these
conversations is wrong as well: a turn records no output, no thinking and
no cache, only the input of each request.

## Decision

The model reads the saved files with `agy`'s own file tool, and Jush lets
that tool read nothing else. Alex's screenshot then reaches the model in a
second step: the model calls the file tool, and `agy` runs it without Jush.

```text
mcp_jush_shell  -> [Resource offloaded to file:///…/steps/2/media_0.jpg]
read_file       -> the picture
answer          -> "The login page shows the email field and a Sign In button…"
```

### The agent and its tool

- `jush.md` lists `tools: [view_file]`. `agy` offers it to the model as
  `read_file`, which reads a file of the machine `agy` runs on, an image as
  the image.
- The agent's body is the request's system prompt and, after it, one
  sentence of the provider's, since the model otherwise takes `read_file`
  for a way to read the user's files:

  ```text
  Your read_file tool reads only the files Antigravity saves on its own machine when a tool result is too long for it; read anything else with shell.
  ```

  The sentence is the same for every request, so it changes nothing that
  decides whether a kept process serves a request.
- Its calls are `agy`'s own steps, which reach Jush only as
  `session/update`s (`tool_call`, then `tool_call_update` with its status).
  The run reports each as a vendor's step of a kind Jush does not know, as
  it reports any such step, and the conversation shows it as a step after
  the `shell` step whose result it reads, marked Failed when it failed:

  ```text
  ▸ Ran jush file view login.png
  ▸ read_file at Antigravity
    The login page shows the email field and a Sign In button…
  ```

  `agy`'s other tools, which no setting removes (0352), show the same way
  when the model calls them; they ask nothing and reach no file (see Spike).
  The step's entries are the provider's and no request carries them: a
  history written into a new process's session holds the `shell` result as
  0352 writes it, which needs no step to read it back.

### Jush answers agy's permission requests

The Subscriptions machine runs the processes of every user's accounts, so a
model that reads any file there could read another account's sign-in.
`agy` starts without `--dangerously-skip-permissions`, and asks Jush,
through ACP's `session/request_permission`, before each call it does not
allow by itself. Jush allows a call of its own MCP server, named
`mcp_jush_<tool>`, and denies every other request. `agy` never asks to read
inside its own data directory, `.gemini/antigravity-cli`, where it saves
the results, nor inside the session's workspace; it asks for anything else,
after following symbolic links. So:

| The model reads | `agy` | Jush |
| --- | --- | --- |
| A saved result, `.gemini/antigravity-cli/brain/…` | reads it | is not asked |
| The account's sign-in, a link in `.gemini/antigravity-cli` | asks, naming the link's target | denies |
| The MCP channel's pipes or `.gemini/config` in the process's directory | asks | denies |
| Any other file of the machine | asks | denies |

- **Workspace.** The session's working directory, which `agy` treats as
  its workspace, is `workspace` in the process's directory, which nothing
  creates, so the process's own files are outside it and no file is in it.
  `agy` itself keeps the process's directory as its `HOME` and working
  directory, where it still finds Jush's agent.
- **A denied read ends the turn.** `agy` ends the turn after any denial,
  without the model's answer. Jush then fails the run with "Antigravity
  ended the turn because Jush did not let its model read a file outside the
  tool results it saved." The user sends a message to go on. With the
  sentence above, the model did not try such a read (see Spike).
- `agy`'s own permission rules in `settings.json` do not apply to `agy
  acp`, so Jush's answers are the only rules.

### Usage

`agy` reports the tokens of each model request when it ends
(`usage_update`'s `used`: the request's input, cache and output together),
and at the turn's end its totals: uncached input, cache read, cache write
and output, whose output counts the thinking too. It reports no split of a
single request. The agent takes the sum of a response's tokens for the
context, so each run's tokens add up to the `used` of its last request:

- **Every run** reports that `used` as its input, as today.
- **The turn's last run** reports the turn's output as its output, and the
  rest of `used` as its input, divided into uncached input, cache read and
  cache write in the proportions of the turn's totals.

The turn's output and its speed, which the agent sums over the turn's
requests, are then exact. A turn's input in the usage ledger stays an
estimate: it counts each run's last request, not the extra requests `agy`
makes inside a run, such as the one that reads a saved file; it counts the
earlier runs' input as uncached; and the last run's is short by the turn's
output. Counting more would put more than one request's context in one
response, which the agent would take for a larger context and compact by.

```rust
// crates/provider-antigravity/src/live.rs
/// What `agy` reports of a turn when it ends.
pub(crate) struct TurnUsage {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// Thinking included.
    pub output: u64,
}
```

### What an older process meets

A process kept from before the upgrade runs with its old agent and flags
until it closes. The next process of the session starts with the new ones,
and makes its session anew with the new agent, as every new process with
history does (0352), so no session of the old agent is loaded.

## Spike

Measured with `agy` 1.5.14 on macOS arm64, on a Gemini API key through ACP
`authenticate`, with Jush's agent file and an MCP server like Jush's:

| Point | Result |
| --- | --- |
| The size limit | An image of 10,200 bytes reached the model and one of 10,300 bytes did not. A 33 KB text kept about 10 KB around the notice. Image content, annotated images, embedded resources and resource links are all limited the same way. |
| Where the limit comes from | `agy`'s default configuration sets the MCP tool limit to 10,240. No documented setting changes it: not the agent file's 20 keys, `mcp_config.json`, `settings.json`, `user_config.json` or ACP's session options. A server-side experiment flag can change it, which only Google sets. `agy`'s local flag-override call takes on-or-off values only and refused this one. The Antigravity SDK sets it, but takes no subscription. |
| An MCP server given in `session/new` | The same limit. |
| `read_file` of a saved result | 2 of 2 runs described the 4 MB image's colors right; 2 of 2 quoted a line from the middle of a 33 KB text right. |
| `--dangerously-skip-permissions` | The model read a file in `/tmp` and one in the process's directory, also when `settings.json` denied both. |
| Without it | `agy` asked through `session/request_permission` before each MCP call and before reading `/tmp`, naming `/private/tmp/…`, the link's target. It did not ask before reading a saved result, nor a file in `.gemini/antigravity-cli`, nor in the workspace, with the workspace the process's directory or an empty directory in it. A link in `.gemini/antigravity-cli` to a file in `/tmp` was asked about and denied. `settings.json` rules, even `mcp(*)` allowed, changed nothing. |
| A denial | The turn ended with `end_turn` and no answer after the denied call; answering with an error instead did the same. |
| `agy`'s other tools | `list_resources`, `read_resource` and `manage_task` ran without asking; `read_resource` reads only an MCP server's resources, and refused `file:///etc/hosts` because Jush's server has none. |
| The data directory | In a signed-in `agy`'s `.gemini/antigravity-cli`, only the sign-in holds a token; its logs, caches, state and databases hold none. |
| A separate workspace | With `session/new`'s `cwd` an empty directory, the agent of the process's directory still ran: its body's made-up name was the model's answer. A `cwd` that does not exist works the same, and `agy` does not create it. A session made that way, copied to another process's directory and loaded there with that directory's `workspace`, kept its agent and history: the model answered with its name and a word the earlier turn gave it. |
| The sentence in the prompt | Asked for the first line of `/etc/hosts` on the user's computer, the model called `read_file` in 3 of 3 runs without it, each denied, which ended the turn; with it, it called `shell` in 3 of 3. |
| Usage | `usage_update` came after each model request. Its `used` summed over a turn to the turn's input, cache read and output: 919 + 1,373 + 2,617 = 4,909; 737 + 2,033 + 3,329 + 4,606 = 10,705. A one-number answer after long thinking reported 578 output and 569 thinking tokens, so output counts the thinking. |

Not measured: Linux.

## Prior art

- Claude Code saves an MCP result over its limit (25,000 tokens by default,
  or 50,000 characters of text) to a file of the session and tells the
  model to read it in parts with its Read tool
  ([MCP output too large](https://www.morphllm.com/mcp-output-too-large)).
  `agy` does the same at 10,240 bytes; Jush's `shell` results stay under
  Claude Code's limits.
- Zed, JetBrains IDEs
  ([Other editors (ACP)](https://antigravity.google/docs/ide/extensions/acp/))
  and [T3 Code](https://github.com/pingdotgg/t3code) run `agy acp` with all
  of `agy`'s tools, so its `read_file` reaches the saved files, and any
  other. They run on one user's own machine; Jush allows only what `agy`
  reads without asking.
- ACP's `session/request_permission` is how every ACP client answers an
  agent's request, such as Zed's prompt; Jush answers by a fixed rule
  instead of the user.

## Alternatives

- **Send the image in the next prompt.** A `PreInvocation` hook stops the
  turn after the call, and Jush sends the image as the next message: the
  model sees it, but not in the tool's result, and long text is still cut.
- **Make each image smaller than 10 KB.** Nothing is left to read at that
  size.
- **Change `agy`'s executable or its configuration in memory.** The rule of
  0352 forbids it.
- **Keep `--dangerously-skip-permissions` with deny rules.** `agy` ignores
  the rules under the flag, and a deny rule cannot leave one directory
  readable, since a deny overrides every allow.
- **Cut every `shell` result under 10 KB on this provider.** A result is
  cut once, by one rule for every model, so that replay sends it unchanged;
  and an image would still be limited.

## Open questions

None.
