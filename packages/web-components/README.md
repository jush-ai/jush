# @jush-agent/web-components

Jush's conversation view as a custom element, the web app's own views, for a
program's page in any framework, or none. It shows the one conversation the
program's server answers below its `src`, with
[`@jush-agent/sdk`](../sdk/README.md)'s `conversation.page()`.

The following page shows a conversation, with its work in windows over and
beside it and a taskbar under them, as the web app does:

```html
<link rel="stylesheet" href="/assets/jush-web-components.css" />
<script type="module" src="/assets/jush-web-components.js"></script>
<jush-conversation src="/chats/42/jush"></jush-conversation>
```

The program's server decides which conversation that is and what the page
may do, and hands each request below `src` to the SDK:

```ts
const page = core.conversations.open(chat.conversation).page({
  base: '/chats/42/jush',
  allow: ['send', 'stop', 'files'],
})
if (request.headers.get('upgrade') === 'websocket') {
  const { socket, response } = Deno.upgradeWebSocket(request)
  page.accept(socket, request.url)
  return response
}
return page.fetch(request)
```

- The element shows the controls of what `allow` names alone: without
  `send` there is no composer, without `files` no Files window.
- A page shows one conversation: the element reads `src` once, and a page
  holds one `<jush-conversation>`. Show another conversation on another
  page, or two side by side in frames.
- A link to another conversation, such as a fork, or to settings is a
  `navigate` event with the address the web app would show, such as
  `/chat/0193f1c2-...`, which the program handles.
- The element renders in the page's own tree, without a shadow root, and
  its stylesheet also styles the page's own elements.

Part of [Jush](../../README.md). The package is under the Apache License 2.0.
