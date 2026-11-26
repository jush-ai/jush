---
status: proposed
---

# A program serves its own addresses

Supersedes: 0304 (A program opens cores in a process of its own and drives
them with the web API), the process's `/hooks/{id}`, its sign-in return, and
the public URL the SDK gives it; 0301 (A TypeScript SDK runs Jush's cores in
a program), `publicUrl` and `jush.fetch`; 0327 (A core process runs on its
own, and several programs drive it), `JUSH_PUBLIC_URL`.

## Problem

A program that starts Jush with `publicUrl: 'https://bot.example.com/jush'`
forwards two kinds of call to `jush.fetch`:

- An outside system calls a schedule's URL,
  `https://bot.example.com/jush/hooks/{id}`.
- A user signs in to a plugin's vendor, which sends the browser back to
  `https://bot.example.com/jush/cores/{core}/api/plugins/{plugin}/oauth/callback`.

Jush answers both itself: 401 for an unknown address and for a wrong secret,
429 and 413 for the limits, and the sign-in's result as a page in Jush's
words. The program cannot log which schedule was called or why a call was
refused, answer in its own format, or show the sign-in's result on its own
page. The paths are Jush's, below a base the program must give over to
them, and in a process on its own the base is the operator's
`JUSH_PUBLIC_URL`, not the program's. Over stdio, whatever else the program
forwards to `jush.fetch` reaches the core with the program's own authority.

## Decision

The program says where it serves each kind of call, receives the call at
its own route, and hands it to a typed method; it writes the HTTP answer:

```ts
const jush = await Jush.start({
  plugins: [chat],
  addresses: {
    schedule: 'https://bot.example.com/hooks/{id}',
    signInReturn: 'https://bot.example.com/sign-in/{core}/{plugin}',
  },
})

// The program's server.
async function answer(request: Request): Promise<Response> {
  const { pathname, searchParams } = new URL(request.url)
  const hook = pathname.match(/^\/hooks\/([^/]+)$/)
  if (hook !== null && request.method === 'POST') {
    const called = await jush.schedules.call(hook[1], request)
    if (called.type === 'taken') {
      return new Response(null, { status: 202 })
    }
    log.info('schedule call refused', { id: hook[1], reason: called.reason })
    return new Response(null, { status: called.reason === 'tooMany' ? 429 : called.reason === 'tooLarge' ? 413 : 404 })
  }
  const signIn = pathname.match(/^\/sign-in\/([^/]+)\/([^/]+)$/)
  if (signIn !== null) {
    const done = await jush.plugins.completeSignIn({ core: signIn[1], plugin: signIn[2], query: searchParams })
    return page(done.type === 'signedIn' ? `Signed in to ${done.vendor}.` : 'This sign-in did not finish.')
  }
  return new Response(null, { status: 404 })
}
```

### Addresses

`addresses` are URI templates (RFC 6570), in its simplest form: `{name}`
stands for a value, percent-encoded. Jush fills them where it writes an
address today:

| Address | Values | Where Jush writes it |
| --- | --- | --- |
| `schedule` | `{id}`, the schedule's id | `jush schedule create --url` and `show` print it |
| `signInReturn` | `{core}`, the core's id (`core.id`); `{plugin}`, the plugin's id | a plugin's OAuth sign-in sends it as its `redirect_uri` |

- A template is filled each time Jush writes the address; nothing stores
  the address. A program that changes `schedule` changes the address of
  every URL schedule, whose id and secret stay.
- `schedule` holds `{id}`: no other part of a call says which schedule it
  is for. `signInReturn` may leave out `{core}` and `{plugin}`, for a
  program that knows them from its own session, as the server knows its
  user from the browser's. A template that holds another name, that holds
  a fragment, which no call carries, or that filled is no `http` or
  `https` URL, is refused, and `Jush.start` or `Jush.connect` fails with
  the refusal.
- A core registers its OAuth client with its vendor itself (RFC 7591), for
  the return address it sends, and registers again when the address
  changes: the program registers nothing.
- Each connection's greeting declares the addresses, before its plugins
  and cores, as it declares its plugins: the last declaration stands, and
  a connection that names no `addresses` leaves those another made. A
  process holds them while it runs; after it restarted, a program's
  greeting declares them again before it opens its cores.
- Without `schedule`, a URL schedule prints no address; without
  `signInReturn`, a plugin's sign-in is refused, saying that no address
  takes its return.

### A schedule's call

`jush.schedules.call(id, request)` takes the standard `Request` the program
received. The process finds the open core that holds the schedule, and
checks the call as the server's `/hooks/{id}` does. It answers
`{ type: 'taken' }` or `{ type: 'refused', reason }`:

| `reason` | When | The server's answer |
| --- | --- | --- |
| `unknown` | No open core holds a URL schedule of the id. | 401 |
| `secret` | The call carries the secret neither as a bearer token nor in GitHub's signature. | 401 |
| `tooMany` | The schedule's calls in a minute are over 30. | 429 |
| `tooLarge` | The call's body is over 1 MiB, or its headers over 64 KiB. | 413 for the body; hyper's limit holds the headers |

- `taken` says that the call is kept, not that a run started: calls in a
  burst make one run 30 seconds after the last, 2 minutes after the first
  at most; a delivery ID seen in the last hour is answered as the first;
  a paused schedule keeps the call. The runs are in
  `conversation.schedules.runs(id)` and in `core.state`.
- The program's route decides which methods it takes: the server takes
  only `POST` at its `/hooks/{id}`, and a program's call is checked
  whatever its method.
- A core that closes as a call comes is open no longer: the call is
  `unknown`, as a sign-in's return is `expired`.
- A call that ends with no answer of the core's fails, as reading its
  request would: `call` throws the body's own error when the body breaks
  off, as when its sender goes away, and a `JushError` with the code
  `transfer_stalled` when nothing of it comes for a minute.
- The server answers `unknown` and `secret` alike, so a caller cannot tell
  which addresses exist. The README advises a program to do the same.
- The server limits the calls of unknown addresses by the caller's network
  address. The process does not see it: the program's server, which does,
  limits them, as it limits any other route of its own. The limit of each
  schedule's calls stays in the core, whatever address they come through:
  it bounds the calls the core keeps and the runs they start.
- The SDK passes the request's method, headers and body to the process as
  one HTTP message (`message/http`, RFC 9112 § 10.1), the body streamed
  as it arrives, so its bytes reach the core as they came, and a GitHub
  signature over them verifies.

### A sign-in's return

`jush.plugins.completeSignIn({ core, plugin, query })` takes the query the
vendor sent the browser back with, as `URLSearchParams`, a string or a
record. It answers `{ type: 'signedIn', vendor }` or
`{ type: 'refused', reason }`:

| `reason` | When |
| --- | --- |
| `unknownPlugin` | The core has no plugin of the id that signs in, or has it off. |
| `expired` | No open core of the id started this sign-in, or it started it over 10 minutes ago, or the sign-in ended already. |
| `declined` | The vendor sent the browser back with an error, such as the user's refusal. |
| `vendorFailed` | The vendor issued no tokens for the code. |

The program shows the result on its own page.

### What goes

- `publicUrl`, `JUSH_PUBLIC_URL` and `jush.fetch`.
- The process's `/hooks/{id}`, and the routes it answered without its
  token: a process on its own answers only its programs and its runners.
- The core routes' sign-in return, which rendered its result as a page of
  Jush's: the server alone serves it now, beside its `/hooks/{id}`.
- A device's start command, which was `jush-runner run --backend` with the
  public URL: a program starts its runners itself (`device.spawn`), or
  names the address they dial (0327).
- The server's name a borrowed provider entry gives its lender, which was
  the public URL's host: it is the machine's name, as on a server whose
  URL is not known yet.

The server keeps its `/hooks/{id}`, its sign-in return and its public URL.

### The process's routes and the starter

| Route | Body | Answer |
| --- | --- | --- |
| `PUT /addresses` | `ProgramAddresses { schedule, signInReturn }` | 204; 400 for a template that breaks a rule |
| `POST /schedules/calls?id=` | the call, `message/http` | 200 `ScheduleCallAnswer` |
| `POST /sign-ins` | `CompleteSignIn { core, plugin, query }` | 200 `SignInAnswer` |

The schedule's id is in the query, where no `.` or `..` is a step of the
path. `Starter::public_url` gives way to `schedule_url(id)` and
`sign_in_return(plugin)`. The server's are `{public URL}/hooks/{id}` and
`{public URL}/api/plugins/{plugin}/oauth/callback`, as they are today; a
program's core fills its program's templates.

### What older ones meet

- A program built for an earlier SDK that passes `publicUrl` gets no
  address until it passes `addresses`, and its `jush.fetch` call fails.
- A core keeps its URL schedules, their ids and their secrets; their
  callers call the program's new address.
- An SDK that declares addresses to an earlier `jush-core serve` is
  refused, 404, and `Jush.connect` fails: the process updates with the SDK
  whose package carries it.
- A `JUSH_PUBLIC_URL` left in a process's environment is not read.

## Spike

- Whether a GitHub-signed call verifies after the SDK carries it: a probe
  sent calls signed over a body with CRLF line ends and bytes that are no
  UTF-8 through a `Bun.serve` and a Node server to `jush.schedules.call`,
  and each was `taken`; a signature over other bytes was `secret`.
- Whether a refusal before the body is read reaches the program at once:
  bodies of 1 MiB and 1 byte, 5 MiB and 20 MiB, sent whole and streamed in
  64 KiB chunks, were `tooLarge` within 10 ms on Bun and Node. On Node, the
  streamed ones waited forever until the SDK stopped writing a body once
  the process reset its stream.

## Prior art

- Stripe's and GitHub's webhook libraries check a call the application
  received at its own route, from its headers and its raw body
  ([`stripe.webhooks.constructEvent`](https://docs.stripe.com/webhooks),
  [`@octokit/webhooks`](https://github.com/octokit/webhooks.js)'s
  `verifyAndReceive`), and the application writes the answer. Jush also
  keeps the call, since the run it starts is Jush's.
- [openid-client](https://github.com/panva/openid-client)'s
  `authorizationCodeGrant(config, currentUrl, checks)` completes a sign-in
  at the application's own callback route from the URL the browser came
  back to. Jush keeps the state and the PKCE verifier, so the program passes
  only the query.
- GitHub's REST API answers URI templates such as
  `"following_url": "https://api.github.com/users/octocat/following{/other_user}"`
  for its clients to fill.
- Google's batch requests carry each request inside another as
  `application/http`, the media type RFC 9112 defines beside
  `message/http`.

## Alternatives

- Addresses as functions, `schedule: (id) => ...`: the core process writes
  the address, as an agent's `jush schedule create --url` runs, and cannot
  call into the program, which may be away from a process on its own.
- A base URL with Jush's paths below it, today's `publicUrl`: the program
  gives its paths over to Jush, the problem this record solves.
- A sign-in return that the process finds by asking every open core for
  its state: the program knows the core, from the address or its session,
  and names it.
- The call's headers in a JSON header beside its body, or the call as JSON
  with its body in base64: the first is bound by the 16 KiB a header list
  may take, and the second carries the body's bytes as text.

## Open questions

None.
