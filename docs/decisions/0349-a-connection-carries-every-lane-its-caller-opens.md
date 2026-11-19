---
status: proposed
---

# A connection carries every lane its caller opens

Supersedes: 0120 (A page reaches the backend over one socket), the rule that
a connection holds at most 64 lanes.

## Problem

A program follows each conversation it runs on a lane of its one connection
to the core. A program that starts a turn in 100 conversations at once has
about 40 of its sends refused with 429 `too_many_lanes`, before any message
is sent: the synchronization channel and the program's other lanes leave
about 62 of the connection's 64 lanes to conversations. A program that
answers in a team's many group chats follows hundreds at once.

The limit was set for a page, which holds 18 lanes at most, "so that a page's
open lanes and what they hold are bounded". It bounds no caller: a caller
opens as many connections as it likes, each with 64 lanes more.

## Decision

A connection carries every lane its caller opens, and refuses none for their
number. `MAX_LANES` and the error code `too_many_lanes` go, with its message
in every language.

What a lane holds stays bounded by the rules that bound it now: at most
`LANE_CREDIT` of the caller's messages that its owner has not taken, each
within the message size limit, and an owner's message waits until the
connection writes it. A conversation's lane keeps the conversation's tree
open while it follows it, as a page's lane does; the tree of a conversation
that runs is open anyway. A program that never closes the conversations it
ran keeps each one's tree open, without a refusal to tell it: it closes a
conversation it stops following (`conversation.close()`), and its lane and
tree go.

The connection writes a piece of each lane with a message in turn, so lanes
that send at once share its rate. A lane whose conversation sends faster
than its caller reads is closed as lagging, as a page's is now, and the SDK
opens it again and reads the conversation afresh.

An older page or SDK meets no change but that an open past 64 lanes now
succeeds.

## Spike

Whether one connection carries hundreds of conversations at once was run on
the built change: one program connected to `jush-core serve`, conversations
of one core, a scripted model.

| Case | What it showed |
| --- | --- |
| 100 conversations each run a turn at once, before the change | 41 runs refused before their message was sent |
| 300 at once, after it | all 300 turns ended; the core held 327 MB |
| 60 conversations never opened before, opened at once | 1.5 s in a release build, 8 s in a development build; their 60 turns then took 0.2 s |
| 300 followed, the connection drops for 3 seconds | each lane opened again once, none closed as lagging, and 300 turns ended 7.4 s after the connection returned, in a development build |

Most of the time goes to opening a conversation the first time: the core
opens its tree and answers its reads one at a time. That cost is a
conversation's, not a lane's, and the limit never bounded it.

## Prior art

- [Phoenix](https://github.com/phoenixframework/phoenix)
  (`lib/phoenix/socket.ex`) refuses a channel's join past
  `max_channels_per_transport`, 100 by default, which an application raises
  in its socket's options. Jush has no such setting, and the number a program
  needs has no bound Jush knows.
- HTTP/2 ([RFC 9113](https://www.rfc-editor.org/rfc/rfc9113) §6.5.2) sets no
  limit on a connection's concurrent streams until the receiver sets one, and
  recommends no fewer than 100.
- [gRPC](https://grpc.io/docs/guides/performance/) queues a client's calls
  past a connection's stream limit, and advises an application with high
  load to spread them over a pool of channels, a workaround its library means
  to make needless ([grpc/grpc#21386](https://github.com/grpc/grpc/issues/21386)).

## Alternatives

- **The SDK opens another connection when one is full**, as a gRPC pool does.
  The server keeps a limit that bounds no caller, and the SDK reconnects and
  routes lanes over several connections.
- **No limit for a program's connection alone.** One connection protocol
  would have two rules.
- **A higher limit, such as 1,000.** A larger program reaches it again, and no
  number follows from what Jush needs.
