---
status: accepted
---

# A user clears a conversation's context

## Problem

Alex has worked in one conversation all day: a release, then a bug in the
importer. The conversation was compacted three times. Its summary and the
messages kept beside it still describe the release, so every request carries
them, and the model keeps steering toward a release that is done. Alex wants
the model to start over, but in this conversation: its pinned documents, its
project, device and workflow, and its history on the page are what Alex
wants to keep. Today the only way to drop the old context is a new
conversation, which keeps none of them.

## Decision

The context card has a Clear action. After it, the model reads the
conversation as a new one, with the conversation's pinned documents and a
line that says the context was cleared; the page keeps every message.

```text
  ┌──────────────────────────────────────────┐
  │ Context                         41% ▸    │
  │ …                                        │
  │ 64K until compaction   [Clear] [Compact] │
  └──────────────────────────────────────────┘

  … the earlier messages, as they were …
  ────────────── Context cleared ──────────────
  Let's look at the importer.
```

* **The action.** Clear stands beside Compact in the context section of the
  main agent's card, and acts at once, with no confirmation, as Compact does.
  It is disabled, with its reason, while a turn runs, and when nothing was
  sent since the last clear. A subagent's card has none.
* **The page.** Every block stays. The clear shows as a divider, *Context
  cleared*, where compaction shows *Context compacted*.
* **What the model reads.** The system prompt, then from the clear on:

  ```text
  The user cleared the context here: what came before is left out.
  `jush conversation history` reads it.
  ```

  Then the next message, and with it what the context sources answer that
  the blocks since the clear lack, as after a compaction: the execution
  context, and every pinned document. Nothing before the clear is replayed:
  no summary, no kept message, no earlier block, and no side question asked
  about a reply before it.
* **Running work.** A command or subagent still running is not listed. Its
  report reaches the model as usual, and the line tells the model where to
  read what started it.
* **An unfinished turn before the clear** is given up: Continue and Retry
  reach only turns after the last clear, and the conversation list shows the
  conversation as finished. Editing a message from before the clear cuts the
  clear with every block after the message, as it cuts a compaction.
* **A side question** about a reply before the clear reads the context that
  reply was read in, up to the clear.
* **The next compaction** summarizes from the clear on and keeps no message
  from before it.
* **Claude Code.** Its session file is written from where replay starts, so
  the next process resumes a new file that begins at the line above.
* **Context usage** drops at once to the system prompt and the line above;
  what the context sources give back counts from the next request.
* **The cost card** counts no cache miss for the first request after a
  clear, which begins a new prompt, as the first after a compaction does.

The design:

```rust
/// Where the user cleared the model's context: replay starts here.
pub struct ClearBlock {
    pub id: BlockId,
    pub created_at: Timestamp,
    pub request: Option<BlockId>,
    pub model: ModelSelection,
}

pub enum Block {
    // …
    Clear(ClearBlock), // "clear"
}
```

* `replay_start` is the last `compaction_boundary` or `clear`. The store's
  `Kind::CompactionBoundary`, documented as *where a window starts*, becomes
  `Kind::WindowStart` and names both types, so loading a window, releasing
  the blocks before it and the database's `last_of` agree with replay.
  `answered_ends` and `kept_messages` stop at a clear as they stop at a
  compaction.
* The lane takes a `clear` frame (`ClientFrame::Clear`); the session admits
  it as `ActionKind::Clear` under the rules of Compact without its
  threshold. The action appends the block and saves it; it calls no model.
* The block is a new `type` in the stored rows and the web contract. Older
  data holds none, so nothing migrates.

## Spike

None.

## Prior art

* **aider**'s `/clear` empties the chat history and keeps the files added to
  the chat; `/reset` drops both (`aider/commands.py`). Its kept files are what
  pinned documents are here.
* **Gemini CLI**'s `/clear`, also named `/new`, resets the chat, clears the
  screen and starts a session with a new id
  (`packages/cli/src/ui/commands/clearCommand.ts`).
* **Codex** starts a new thread with `/new`, and with `/clear` also clears
  the terminal (`codex-rs/tui/src/slash_command.rs`).
* **Claude Code**'s `/clear` starts a new session.

Each of them leaves the old conversation. Here the conversation stays, with
its page, pins and settings, and the model is told that earlier context
exists and where to read it.

## Alternatives

* **A compaction boundary with an empty summary.** The model would read the
  compaction note over nothing, and every rule about compactions, such as
  the page's divider and idle compaction, would have to tell the two apart: one
  name for two concepts.
* **A New Conversation action.** It loses the pins, the history on the page
  and the conversation's place in the sidebar, which are why Alex stays.
* **No line for the model.** The model could not know that earlier context
  exists, and would read a report of a command it never started as noise.

## Open questions

None.
