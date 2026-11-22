---
status: proposed
---

# Antigravity lets its model's read of a missing file fail, and the turn goes on

Supersedes: 0354 (Antigravity's model reads the tool results agy saves),
the denial of every read outside the saved results and the workspace.

## Problem

Alex's conversation runs on an Antigravity subscription, and its commands
run on Alex's MacBook. An hour into a turn, after `git status` lists
`src/cart.ts` as changed, the model calls `agy`'s own `read_file` on
`/Users/alex/Projects/storefront/src/cart.ts`. That tool reads the
Subscriptions machine, not the MacBook, so `agy` asks Jush first, and Jush
denies it, as it denies every read outside the saved results and the
workspace. `agy` then ends the turn with no answer, and the run fails with
"Antigravity ended the turn because Jush did not let its model read a file
outside the tool results it saved." The hour's files stay, but the turn
stops in the middle, and Alex has to send a message for it to go on.

The agent's sentence about `read_file` makes such a call rare, but not
gone: deep in a long turn the model reaches for `read_file` on a path a
command printed. The denial is what turns a wrong call into a stopped turn.

## Decision

When `agy` asks before a `read_file` call, Jush looks on the Subscriptions
machine at every path the request names. When nothing is at any of them,
not a file, a directory nor a link, Jush allows the call. `agy` then
answers the model `file does not exist`, and the model goes on, such as
with `shell` on the device where the file is. Every other request is
denied as before, and a denial still ends the turn:

| `agy` asks | Jush answers |
|---|---|
| `read_file`, nothing at any path it names | allows: the model reads `file does not exist` |
| `read_file`, something at a path it names | denies |
| `read_file`, the check fails, or the request names no path | denies |
| any other call | denies |

- **The paths.** A request names the path the model gave, `rawInput.Path`,
  and the path `agy` resolved, each `locations[].path`: the file it reads,
  with `..` and every link resolved, `/proc/self/cwd` among them. Jush
  checks both, denies a request with no resolved path, and needs every path
  absolute; `agy` itself refuses a relative path before it asks.
- **The check.** A new `ConfigDir::exists(path)` asks the process's machine
  whether anything is at the absolute path `path`, a link that points
  nowhere included: the runner's `fs.exists`, by `lstat`. A relayed
  process's directory carries it as a new `DirOrder::Exists`, as it
  carries a read.
- A missing path holds nothing to read. What the model learns is whether
  something is at a path of the machine: the turn goes on where nothing is,
  and ends where something is. It can learn that at paths of its choosing,
  such as another account's directory, whose name is a random id; it reads
  no file's content.
- The agent's sentence about `read_file`, and the failure's text, stay.

A file made at the path in the moment between the check and the read is
read. Only a process of the Subscriptions machine could make one, and the
processes there are the subscriptions' CLIs; the model's commands run on
the conversation's device, never on that machine.

## Spike

`agy` 1.5.14 in `agy acp`, `gemini-3.8-flash-low`, on a Gemini API key,
with Jush's agent, on macOS and on Linux:

| Case | What happened |
|---|---|
| Allow `read_file` of a missing absolute path | `locations` named the path itself. The tool failed with `file does not exist`; the model quoted it and the turn ended with `end_turn` and the answer. |
| A relative path | `agy` asked nothing and failed the call: `must be an absolute path`. |
| A link to a missing file | `agy` asked nothing and failed the call: `file does not exist`. |
| A link to an existing file | `agy` asked, naming the link in `rawInput.Path` and its target in `locations`. |
| `/nope/../etc/hostname` | `agy` asked, naming `/etc/hostname` in `locations`, and read it once allowed. |
| `/proc/self/cwd/` or `/proc/self/root/` and a link to a file elsewhere | `agy` asked, naming the link's target in `locations`. |
| Deny | The turn ended with no answer, as 0354 measured. |

Every request `agy` asked named one resolved path in `locations`.

## Prior art

Claude Code's permission callback denies with a message the model reads as
the tool's result, and the turn goes on, unless the denial asks to stop
([`canUseTool`](https://docs.anthropic.com/en/docs/claude-code/sdk/sdk-permissions)).
`agy` gives a denial no such message and ends the turn, so Jush avoids the
denial where nothing can leak, instead of recovering after it.

## Alternatives

- **Send a new prompt after a denial**, telling the model to read with
  `shell`. It works for every path, but Jush would write a message the user
  did not, and the turn would cost another request. Not now.
- **Allow every read.** The machine holds other accounts' sign-ins.
- **A stronger sentence in the prompt.** The current one already fails deep
  in a long turn.

## Open questions

None.
