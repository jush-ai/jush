---
status: proposed
---

# A program attaches a file's bytes to a message

## Problem

A program passes on what its users send, such as a photo a person posted in
a team's chat, to a conversation. The SDK names a file in a message only as
one on a device (`remote_file`); a file the program holds as bytes has no
documented way in. The web app uploads it (`POST /attachments`) and sends
an `upload` block that names the upload, but the SDK has no call for the
upload, and a program that finds the route reads that a block of the
message's own files, `attachment` or `media`, belongs to an edit alone.

A program that sends the image still finds the model told only that a file
came, when the entry's model does not say which types it reads: a
compatible entry's `acceptedExtensions` is null until the program lists
them, and the SDK's README says neither.

## Decision

`core.upload(file)` keeps a file's bytes as an upload of the core's, and
answers the block a message, a steer or a run sends it in:

```ts
const photo = await core.upload({ name: 'receipt.png', bytes, mediaType: 'image/png' })
await conversation.run([{ type: 'text', text: 'What does this receipt total?' }, photo])
await conversation.steer([photo], 'steer-ticket-42')
```

```ts
/** A file the program holds, as `core.upload` takes it. */
interface FileBytes {
  /** The file's name, without a path. */
  name: string
  bytes: Uint8Array | ArrayBuffer | Blob
  /**
   * Its media type, else a `Blob`'s own; an image, a video, a recording or
   * a PDF is recorded as its bytes show it.
   */
  mediaType?: string
}
// Answers { type: 'upload', ref: <the upload's id>, fileName: name }.
```

- It is the web app's upload, `POST /attachments?name=`, with the bytes as
  the body and their length declared, from 1 byte to the route's 25 MiB, and
  the block the web app's composer sends. A name the block refuses, such as
  one with a path, fails before the request is made.
- The upload is the core's record of the file's blob, as the web app's is.
  It stays while a message holds it, so one upload serves any number of
  messages; one no message holds may be removed once a day has passed since
  it was made, and a message that sends it after that tells the model the
  attachment is not available.
- A message that sends it gives the model the file's medium, then the line
  that names the conversation's attachment, `<attachment id="a1" … />`.
  Nothing is written to the device: the agent's commands fetch the file with
  `jush attachment download`.
- The model reads the medium when its `acceptedExtensions` list the file's
  type and the provider's API carries it: a vendor entry's types come from
  models.dev, and a compatible entry's are the program's to list in its
  `models`, such as `['png', 'jpg', 'jpeg', 'gif', 'webp']` for a model that
  reads images. Null means the types are unknown, and the model is told
  that a file came, not shown it; `[]` means it reads none.

## Spike

A program uploaded a PNG and ran a turn of each of two models of one
compatible entry:

- The model that lists `png` was sent the image, then the line that names
  the conversation's attachment, `<attachment id="a1" name="receipt.png" …/>`.
- The model whose `acceptedExtensions` is null was sent
  `[image:image/png, not sent: the model does not accept it]` in its place,
  and the same line.
- The same upload went in a third conversation's message, and an upload of
  a `Blob` worked as one of a `Uint8Array`.
- An upload of 26 MiB was refused with 413 before the core read its bytes,
  on Node and on Bun.
- An upload whose record was dated two days back, and that no message held,
  was removed at the next collection; a message that sent it then gave the
  model `[attachment <id> is not available]`.

## Prior art

- The Claude TypeScript SDK uploads with
  [`client.beta.files.upload`](https://platform.claude.com/docs/en/api/typescript/beta/files/upload),
  and a message's block names the answer's id (`source: { type: 'file',
  file_id }`); `client.beta.files.delete` removes the file.
- The OpenAI SDK uploads with
  [`client.files.create`](https://developers.openai.com/api/reference/typescript/resources/files/methods/create),
  and a message's content names the `file_id`; a file stays until it is
  deleted, or until an `expires_after` the caller sets.
- Both keep the upload apart from the message, so one upload serves several
  messages. Jush departs in having no delete: an upload stays while a
  message holds it, as the web app's do.

## Alternatives

- Bytes inside a message's own content, as base64: a message goes as a
  frame on the page connection, whose WebSocket messages hold at most 1 MiB,
  where a file may hold 25 MiB; and a steer sent again would send the bytes
  again.

## Open questions

None.
