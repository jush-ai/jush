# @jush-ai/web-components

Jush's conversation view as a custom element, the web app's own views, for a
program's page in any framework, or none. It talks to a core through the
path where the program's server forwards it with
[`@jush-ai/sdk`](../sdk/README.md).

The following page shows a conversation, with its work in windows over and
beside it and a taskbar under them, as the web app does:

```html
<link rel="stylesheet" href="/assets/jush-web-components.css" />
<script type="module" src="/assets/jush-web-components.js"></script>
<jush-conversation api="/jush/api" conversation="0193f1c2-..."></jush-conversation>
```

The program's server forwards what the element asks:

```ts
if (url.pathname === '/jush/api/connect') {
  const { socket, response } = Deno.upgradeWebSocket(request)
  core.accept(socket, request.url)
  return response
}
if (url.pathname.startsWith('/jush/api/')) {
  return core.fetch(request, '/jush/api')
}
```

- A link to another conversation or to settings is a `navigate` event with
  the address the web app would show, such as `/chat/0193f1c2-...`; setting
  an element's `conversation` shows another conversation.
- The element renders in the page's own tree, without a shadow root, and
  its stylesheet also styles the page's own elements.

Part of [Jush](../../README.md). The package is under the Apache License 2.0.
