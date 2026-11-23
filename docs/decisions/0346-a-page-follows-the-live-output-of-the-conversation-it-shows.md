---
status: proposed, superseded in part by 0359
---

# A page follows the live output of the conversation it shows

Supersedes: 0090 (A running conversation stays connected), the part that
has every running conversation's transcript stream to the page while the
page does not show it.

## Problem

Alex has five conversations working and reads one of them on a phone, then
locks the phone. The page keeps every running conversation's lane open, as
0090 decided, and each lane follows its root's transcript, so every piece
the five models write reaches the phone: each patch, each change of the
request's output count, each command's output and cache chart. Locked, the
phone still receives all of it.

Measured on a page in the background with five conversations running, over
two minutes:

| What arrived | Messages a second | Bytes a second |
| --- | --- | --- |
| Everything | 6.9 | 16.6 KB, about 60 MB an hour |
| The five conversations' lanes | 5.7 | 11.3 KB |
| Of those, what is not live output | 0.43 | 5.1 KB |

Each message wakes the radio and the page. The page in the background did
nothing with any of it but keep its copy current.

Most of what remains is whole values sent again for a change of one field.
A conversation's metadata, 163 KB in a long conversation because it lists
every command that ended, is sent whole when any of its fields changes,
three times in those two minutes.

## Decision

A page follows the live output of the conversations it shows, and only
while it shows them. Of every other conversation it holds, it follows the
status: whether it runs, waits for the user, or ended. Going back to a
conversation brings it current in one step, before its newest part shows.

### What a page follows

| The conversation | Its lane follows |
| --- | --- |
| Shown, on a page in front | The agents, the metadata and its lists, and the transcript of each node the page shows |
| Held but not shown, or shown on a page in the background | The agents, the metadata and its lists |

A page is in the background while `document.visibilityState` is `hidden`.
A conversation with a message still on its way follows its transcripts
whether it is shown or not: the page learns that the message was written
from the root's transcript. A message that failed to send is not on its way.

While a conversation's transcripts are not followed, its status in the
sidebar is the one the sidebar's list sends, which the backend derives: the
page derives it from the transcript, which it does not hold current then.

What a conversation's lane sends while the page follows no transcript of it:

- The agents' entries: each node's phase, its pending calls and its wait,
  which the sidebar, the title's count, the taskbar and notifications read.
- The metadata, the saved messages, the workflows and the shares.

What it no longer sends: transcript patches, request output, shell output
and cache charts. The server sends these to the connections that follow
the node already (`Tree::emit_to`), so no server change is needed for them.

### Going back to a conversation

The page follows each transcript it holds again with the version it holds,
as it does when its lane opens. The server answers with the patches since,
from the node's recent patches, or `stale`, after which the page reads the
latest page again. The page applies everything that came in that answer
before it draws the conversation's newest part, as it does when it catches
up today, and shows the catching-up mark until then.

Today a follow whose version is current is answered with nothing, so the
page cannot tell when the answer has all come. The server now ends the
answer to each follow with a frame that says so, after the frames that
bring the page current, and after the refusal of a follow it refused. The
answers come in the order of the follows, so the page counts the follows of
each node not answered yet, and the last one's answer is the resume's:

```rust
/// The answer to a `follow` of `value` has all come: the page holds it current, or was told it is stale.
ServerFrame::Followed { value: Followed }
```

A node keeps its last 1,000 operations or 1 MiB of patches. A conversation
that works as fast as the busiest of the five measured comes back by
patches for about 7 minutes, and by a read of its latest page after that.

Today a node frees its recent patches when it stops running, so every
conversation whose turn ended while the page did not follow it would come
back by a read. The root now keeps its recent patches after its turn ends,
until its tree is dropped: a tree holds at most a mebibyte more. A
subagent or a side question still frees them when it stops, and is read
again when the page shows it, which is rare.

### The end of a turn

When a conversation the page does not show ends its turn, the page brings
its transcript current once, by the same follow, writes its copy on disk,
and stops following again. So the copy holds the end of each turn, as 0090
requires, and opening the conversation later shows it whole with no read.
A page in the background does this when it comes to the front, for each
conversation whose turn ended meanwhile.

The page knows a turn ran by the root's agent entry: any change of it that
the lane tells while the page does not follow the transcripts, the
reopened lane's included, marks the transcript behind, until the page holds
it current again. A turn that ends is a root that is behind and idle. No
edge of the phase is watched, since a whole turn can run while the
connection is down.

### The metadata, by the fields that changed

The metadata is sent as the top-level fields that changed, with the entity
tag before and after. A list that changed goes as one splice, the items
between what it starts and ends with as before: a command that ends is put
first in the list of ended commands, so the splice carries that one
command.

```rust
/// A change of the metadata the page holds at `base`, which makes it the
/// metadata at `etag`.
ConversationFrame::MetadataChanged {
    base: String,
    etag: String,
    fields: BTreeMap<String, FieldChange>,
}

enum FieldChange {
    Set { value: serde_json::Value },
    Splice { at: u32, remove: u32, insert: Vec<serde_json::Value> },
}
```

The lane keeps a digest of each field of the metadata the page holds, and
of each item of a list, which it compares the next metadata with: a few
kilobytes per lane, not the metadata. A field that comes or goes sends the
metadata whole.

The page applies the change to the metadata it holds when that is the one
the lane or a read brought with the tag `base`, and checks the result as
it checks the whole metadata. Otherwise, as when a copy on disk or an
action's answer left the metadata it shows, it follows the metadata again
with no tag, which sends it whole.

The lists, the saved messages, the workflows and the shares, stay whole:
each is a few kilobytes at most, and none was measured large.

### The client

`ConversationClient` gains, for all the transcripts of its conversation:

```ts
/** Stops following every transcript it holds, and each it reads later, keeping the versions it holds. */
pauseTranscripts(): void

/** Follows them again from the versions held; reads a node's latest page when the server answers stale. */
resumeTranscripts(): Promise<void>
```

`store.ts` decides which conversations are shown, as it does for the
eight it keeps (`KEPT_CONVERSATIONS`), and pauses or resumes their
transcripts when the shown conversation or `visibilityState` changes.

An older page follows every transcript, as today. It checks every frame
against its schemas and drops the connection on one it does not know, so
the server sends `MetadataChanged` and `Followed` only on a lane whose
`open` says it understands them (`understands: ["followed",
"metadata_changed"]`),
and the whole value and no answer otherwise. A page newer than its server
is not served: the server serves the page.

## Spike

1. **Whether the live output stops without a server change.** Read
   `agent-server`: transcript patches, request output, shell output and
   cache charts go through `Tree::emit_to`, to the connections that follow
   the node; the agents' entries through `emit_to_agents`. Unfollowing the
   root's transcript stops all four.
2. **What a page in the background receives.** Counted every lane message
   on a page in the background with five conversations running, for two
   minutes: the table under Problem. Status alone would be 0.43 messages a
   second of the conversations' 5.7.
3. **How far recent patches reach.** In the same two minutes the busiest
   conversation sent 299 patch operations, 118 KB; the others 35 to 175.
   At those rates a node's 1,000 operations last 7 to 60 minutes, and its
   1 MiB longer. A conversation shown again within that comes back by its
   patches.
4. **What fills the whole values.** Took one metadata message apart by
   field: 165 KB, of which the list of ended commands was 163 KB and every
   other field under 1 KB.
5. **What changes in the metadata.** Compared each metadata message with
   the one before it, on the same page, over eight minutes: 65 messages,
   6 MB. The field that changed was most often the latest attachment's
   number (21 times), the schedules (16), the tabs' states (12) and the
   plugins' revisions (10). The list of ended commands changed twice,
   each time by one command put first. As the changed fields, the same
   changes are about 14 KB.

## Prior art

- [Matrix's sliding sync](https://github.com/matrix-org/matrix-spec-proposals/blob/main/proposals/4186-simplified-sliding-sync.md),
  which Element X uses, subscribes to the timelines of the rooms on screen,
  and sends the room list with a timeline of one event for the others. A
  room comes into view by a subscription that sends what it missed. Jush
  does the same with its follows and their versions.
- [Open WebUI](https://github.com/open-webui/open-webui) joins only the
  shown chat's room (`src/lib/components/chat/Chat.svelte`), and reads a
  chat again when it is shown. Jush instead resumes from the version held,
  so going back costs the patches since, not a read.
- Kubernetes' watch resumes from a `resourceVersion` within the window of
  recent events the server keeps, and lists again outside it, which is the
  follow's `stale` already.
- [JSON Merge Patch (RFC 7396)](https://www.rfc-editor.org/rfc/rfc7396)
  sends an object's changed members. `MetadataChanged` is that for the
  top-level fields only, with the tags around it, and a list's splice,
  which JSON Merge Patch replaces whole.

## Alternatives

- **Send live output at most every 100 ms.** Measured on the same traffic,
  this would leave 2.0 of 5.7 messages a second and 9.6 of 11.3 KB. A page
  in the background has no use for any of it, and the shown conversation
  is the one that must stay live.
- **Close the lanes of conversations not shown.** Status would then come
  only from the sidebar's channel, which would have to carry what the
  agents' entries carry, a second source of the same facts.
- **Leave a page in the background as it is, and change only what is not
  shown.** A locked phone is a page in the background, and the problem
  this record solves.

## Open questions

None. Settled while this record was proposed:

- Going back to a conversation that ran meanwhile waits for one answer
  from the server before its newest part shows, about one round trip,
  under the catching-up mark. 0090 removed a wait for a read of the
  transcript; this one is a follow's answer, which carries only what
  changed.
- The list of ended commands still grows with each command that ended.
  With `MetadataChanged` it travels only when it changes, by the command
  that ended; whether the dock
  should keep fewer of them is a question of what the dock shows, not of
  this record.
- The sidebar's channel sends a conversation's entry again each time its
  revision rises while it runs. That is the sidebar's channel, and its
  own record.
