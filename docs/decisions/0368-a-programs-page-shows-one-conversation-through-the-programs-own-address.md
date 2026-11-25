---
status: proposed
---

# A program's page shows one conversation, through the program's own address

Supersedes: 0302 (A program's page shows Jush's views), the element that
reaches a core's routes through the program's forward, and the elements of
a page that share one core; 0301 (A TypeScript SDK runs Jush's cores in a
program), `core.fetch` and `core.accept`.

## Problem

`<jush-conversation api="/jush/api" conversation="...">` reaches every
route of the core below `api`, where the program's server forwards them
whole with `core.fetch` and `core.accept`. A page that shows one
conversation to a team can read every conversation of the core, browse each
device's files, and change the core's providers and credentials. The
program cannot narrow it: the routes are the internal channel of the web
app's screens (0367), so the program cannot tell which ones a page needs,
and they change with the screens.

## Decision

The program's server binds a page to one conversation and names what the
page may do. It answers the page below an address of its own by handing
each request to the SDK, as a ChatKit server hands each request to
`ChatKitServer.process`:

```html
<jush-conversation src="/chats/42/jush"></jush-conversation>
```

```ts
// The program's server, with its own sign-in and its own rules.
async function answer(request: Request): Promise<Response> {
  const chat = await signedInChat(request) // which chat, and who asks
  const page = core.conversations.open(chat.conversation).page({
    base: `/chats/${chat.id}/jush`,
    allow: ['send', 'steer', 'stop', 'files', 'browser'],
  })
  if (request.headers.get('upgrade') === 'websocket') {
    const { socket, response } = Deno.upgradeWebSocket(request)
    page.accept(socket, request.url)
    return response
  }
  return page.fetch(request)
}
```

```text
browser                       the program's server            jush-core
<jush-conversation src=/chats/42/jush>
  GET  /chats/42/jush/... ---> its sign-in, its chat --------> the page's
  WS   /chats/42/jush/connect  page.fetch / page.accept        caller: one
                                                               conversation,
                                                               its `allow`
```

### The page

- `src` is the address below which the program's server answers the
  element, as an `iframe`'s or a `video`'s `src`. The element sends every
  request there and opens its connection there, with the page's own
  cookies. Its requests' paths below `src` are the element's and the SDK's
  internal channel, which the program passes on without reading.
- The element learns its conversation and its operations from the first
  state it reads, and shows the controls of its operations alone: a page
  without `send` shows no composer, one without `files` no Files window.
- A page shows one conversation, as the web app's window shows one at a
  time: the views' stores and their one connection are the page's. The
  element reads `src` once; another `src`, or a second
  `<jush-conversation>` on the page, is refused with an error. Another
  conversation is another page, and two side by side are two frames. An
  element that takes the place of one the page removed, as a framework
  draws it again, shows the same conversation.
- What the views change of their own look, such as the file tree's width,
  stays on the page: the core's preferences are the program's.
- A link out of the conversation, to another conversation, such as a fork,
  or to settings, stays a `navigate` event with the address the web app
  would show (0302), which the program handles: it binds the other
  conversation's page itself, or does nothing.

### The program's server

```ts
class Conversation {
  /** This conversation's page, answered below `options.base`. */
  page(options: PageOptions): ConversationPage
}

interface PageOptions {
  /** The element's `src`, where the program's server answers the page. */
  base: string
  /** What the page may do beyond reading its conversation. */
  allow: readonly PageOperation[]
}

interface ConversationPage {
  /** Answers one of the page's requests, below `base`. */
  fetch(request: Request): Promise<Response>
  /** Carries the page's connection, opened at `url`, both ways. */
  accept(socket: AcceptedSocket, url: string | URL): void
}
```

- `allow` is an allow list, as a conversation's commands are (0370): an
  operation a later Jush adds is on no page until the program names it. An
  empty list shows the conversation and its updates, with no control.
- A page object holds nothing: the program makes one for each request, so
  each viewer's page can name other operations. A connection keeps the
  operations it opened with; other ones take effect as the page connects
  again.
- A file the Files window previews loads in a frame that sends no cookie,
  below `{base}/html/`, with the token the page was given in its path. The
  program hands those requests to the page without its own sign-in.
- `core.fetch` and `core.accept` leave the SDK. The web app's own server
  keeps its routes and session.

### What every page reads

Its conversation as the web app shows it: the transcript and its updates,
its turns, blocks and versions, its agents' transcripts, its commands and
their output, the attachments and media its blocks name and the
Attachments window that shows them, the documents and facts of the reply
blocks it names, the plugins' icons, its recent cost, its draft, its saved
messages and the list of its workflows, the states of its plugins that are
no window, and Find in Conversation. Reading it marks it read.

### The operations

`PageOperation` names what a user does on the page. Each one is a control
of the views and the requests behind it, which the core checks:

| Operation | The page may | Requests behind it |
| --- | --- | --- |
| `send` | Send a message, queue it, take it back from the queue, keep a draft and saved messages | `Send`, `SendSaved`, `SendQueuedMessage`, `DequeueMessage`, `ClearMessageQueue`; writes of the draft and the saved messages, suggested replies used |
| `attach` | Attach files to a message | `POST /attachments` |
| `steer` | Steer a running turn | `Steer`, `SteerNow`, `CancelPendingSteer` |
| `edit` | Edit and send again, regenerate a reply, switch its version | `EditAndSend`, `Regenerate`, `SwitchVersion` |
| `stop` | Stop the turn, its subagents, a command | `Abort`, `AbortSubagents`, `AbortSubagent`, `ShellAbort` |
| `retry` | Retry a failed turn, resume a stopped one | `Retry`, `Resume` |
| `compact` | Compact the context | `Compact` |
| `clear` | Clear the context | `Clear` |
| `ask` | Ask side questions | `side-questions` and below |
| `messageAgents` | Message a subagent | `agents/{node}/messages` |
| `decide` | Answer a permission request | `permissions/requests/{request}` |
| `fork` | Fork the conversation | `fork` |
| `delete` | Delete the conversation | `DELETE` |
| `rename` | Change its title | `PATCH` `title` |
| `chooseModel` | Choose its model, reasoning effort and service tier | `PATCH` `model`, `thinkingEffort`, `serviceTierId`; model search and refresh |
| `chooseDevice` | Choose where it runs, detach a device | `PATCH` `target`; `devices/{device}`; the folders a device offers and a new one, `/devices/{id}/fs`, and their watch |
| `chooseWorkflow` | Choose what its subagents spawn with | `PATCH` `workflow` |
| `pins` | Unpin a pinned document | `pins/{pin}` |
| `schedules` | Create, change, run and delete its schedules | `schedules` and below |
| `files` | Open the Files window: browse the conversation's devices, search, preview | `fs` reads, `fs/html` and `/html`, `devices/{device}/fs` reads, the file watch lane |
| `writeFiles` | Upload, make folders and remove in the Files window | `fs` writes, `fs/raw` `PUT`, `devices/{device}/fs` `POST` |
| `changes` | Open the Changes window | `changes` and below |
| `browser` | Open the Browser window: watch and use its live view, open and close its tabs | the browser plugin's calls and states, its tabs in the conversation's work, its user streams |
| `simulators` | Open the Simulators window, as `browser` | the mobile plugin's calls and states, its tabs in the work, its user streams |
| `plugins` | Use the other plugins' controls in the conversation, such as a card's buttons, and reload its plugins | the other plugins' calls, `reload` |

- The window operations are the windows' own ids (`WindowKind.id`): a window
  whose id names an operation shows only with it. Opening, moving and
  closing a window's tab (`work/changes`) needs its window's operation.
- A control the page may not use is not there: no composer without `send`,
  and in it no Steer without `steer`, no Stop without `stop`, no attach
  button without `attach`. A control that also shows a value, such as the
  model menu or where the conversation runs, stays as the value and does
  not open.
- A saved message sent as a steer needs `send` and `steer`, and a close of
  the lane, which ends every turn, needs `stop`.
- A page never changes the program's settings of the conversation,
  `systemPrompt`, `commands` and `browserProfile`, nor reads them, and never
  changes the sidebar's, `archived` and `pinned`.

### The page's caller

The core answers a page as a third caller beside the server's session and
the program (`crates/core-api/src/caller.rs`):

- The SDK sends the page's requests on the process's channel below
  `/cores/{core}/pages/{conversation}/`, with the page's operations, which
  only a holder of the process's token reaches.
- One table beside the routes names each route's operation, or none for a
  route no page reaches. A request whose route is in no row, or whose
  conversation is another, is answered 404; one of an operation the page
  was not given, 403.
- The page opens the sync lane and its conversation's lane, and with their
  operations its file watch, its plugins' user streams and the folder watch
  of `chooseDevice`; never another conversation's lane, a device's
  signaling, the starter lane or a plugin lane. A frame of the conversation
  lane is refused by its operation as a request is.
- An HTML file's frame sends no cookie, so its token is its only key: the
  core granted it to a page with `files`, and answers it to the page that
  carries it.
- A blob or a reply block reaches the page when its conversation names it,
  as a share's does, in its transcript, its draft or its saved messages,
  and a blob the page uploaded while the core runs. A page sends the bytes
  of every file it attaches: knowing a blob's SHA-256 is not holding it.
- Its state holds its conversation, its workspace, its project and the
  devices it runs on; the models and devices to choose from only with
  `chooseModel` and `chooseDevice`, without a provider's address, accounts,
  quota or tool install; the page state of the Browser's, the Simulators'
  and the Changes' plugins only with their windows, and no other plugin's,
  which is its settings'; and the preferences its views follow. Never
  another conversation, the other projects, credentials, sign-ins, port
  forwards, instructions or the Cloud.
- A secret's value, which a credential card reveals, never reaches a page.
- A page has no direct channel to a device: everything it reads goes
  through the program's server.

## Spike

- **What the views call.** The conversation view and its windows use 23
  frames of the conversation lane, 61 methods of the 47
  `/conversations/{id}/...` routes, and the sync, file watch, signaling and
  user stream lanes, besides blobs, attachments, HTML views, plugin
  documents and model search. The table above files each of them.
- **The views need addresses.** An HTML file previews in a frame from
  `/html/{token}/{place}`, so its relative URLs reach the files around it;
  a plugin's reply block is a document in a frame; media play from URLs
  that answer byte ranges. A source the page implements as functions
  cannot give a frame its URL, so the page names an address instead.
- **The direct channel has no gate per conversation.** A runner trusts the
  introduction alone, and a peer then reaches any of the owner's
  conversations on the device (`crates/runner-direct/src/lib.rs`, Trust).
  A page therefore gets no direct channel, and its live views relay
  through the program's server: their bytes cross the program and the core
  on their way.
- **The connection passes as it is.** The lanes' frames are JSON text and
  a user stream's bytes binary messages, so `accept` carries them without
  reading them.

## Prior art

- OpenAI's ChatKit, in its advanced integration, has the application run
  one endpoint whose handler, after its own authentication, passes the
  request to the SDK's `ChatKitServer.process(request, context)`. The
  client names that endpoint's `url` and can replace `fetch`; requests are
  typed by `type`, such as `threads.add_user_message`
  ([chatkit-python](https://github.com/openai/chatkit-python),
  [chatkit-js](https://github.com/openai/chatkit-js)). ChatKit's page shows
  messages alone; Jush's shows frames and live views, which need an
  address below the endpoint, not one path.
- ChatKit's client options decide which controls show, such as
  `composer.attachments`, and the developer's server enforces its own
  rules apart. Jush takes both from the one `allow` the program gives.
- Apache Guacamole has the application implement its tunnel endpoint:
  `GuacamoleHTTPTunnelServlet.doConnect` and
  `GuacamoleWebSocketTunnelEndpoint.createTunnel`, where it authenticates
  and picks the connection, and the library carries the remote screen
  ([guacamole-common](https://guacamole.apache.org/doc/gug/guacamole-common.html)).
- The AI SDK's `useChat` talks to a route the application writes, through a
  transport it can replace
  ([AI SDK UI](https://ai-sdk.dev/docs/ai-sdk-ui/chatbot)).

## Alternatives

- A source the page implements, one optional method for each action: about
  60 methods on the page and as many endpoints on the program's server for
  every window, more with each window's next feature, and no URL for a
  frame or a medium.
- One request protocol on one path, as ChatKit's: frames, media and HTML
  previews need addresses below it.
- The program inspecting each request itself: the paths are internal, and
  a lane's frames go inside its connection. The operations are the words
  it decides in.
- A gated direct channel for pages: the runner would check each operation
  against a grant the introduction carries; a later decision, for a page
  whose live views must not cross the program.

## Open questions

None.
