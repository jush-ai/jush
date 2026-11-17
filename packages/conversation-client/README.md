# @jush-agent/conversation-client

The client of a Jush core's page connection, which the web app and
[`@jush-agent/sdk`](../sdk/README.md) both use. It exports four things:

- `PageConnection` keeps the page connection open and carries its lanes; a
  host it is given opens the socket and hears how the connection stands.
- `ConversationClient` opens a conversation, sends messages, edits, steers and
  aborts, and keeps the transcript current from the core's patches;
  `createConversationClient` makes one on a conversation's lane.
- `createWebSocketTransport` carries its frames over anything shaped as a
  WebSocket, such as a conversation's lane of the page connection.
- `applyTranscriptPatches` applies a patch list to a transcript.

The client validates every frame the backend sends against the schemas of
[`@jush-agent/protocol`](../protocol/README.md) before it acts on the frame. It
refuses a frame over the backend's message limit locally, as the backend would
refuse it.

The following example opens a conversation and sends a message:

```ts
import { createConversationClient } from '@jush-agent/conversation-client'
import { pageConnection } from '@jush-agent/web-ui/transport/page-connection'

// The client opens the conversation's lane of the page connection.
const client = createConversationClient(pageConnection, id)
client.subscribe((event) => {
  if (event.type === 'transcript_patch') {
    render(event.blocks)
  }
})
await client.open()
await client.submit([{ type: 'text', text: 'Fix the failing test.' }])
```

The client drives a conversation a core runs. It does not run Jush's agent
itself: a program runs cores with `@jush-agent/sdk`
([ARCHITECTURE.md](../../ARCHITECTURE.md)).

Part of [Jush](../../README.md). The package is under the Apache License 2.0.
For the text, see [LICENSE](../../LICENSE).
