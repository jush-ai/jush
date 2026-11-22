---
status: proposed
---

# Gemini models read audio, and Antigravity's model reads every medium Gemini reads

Supersedes: 0354 (Antigravity's model reads the tool results agy saves),
the session's workspace holding no file, and the sentence the agent's body
ends with.

## Problem

Alex attaches `standup.mp3` to a message and asks Gemini 4 Neon, through a
Gemini API key, what the team decided. The model answers that it received
no audio: Jush has no audio kind, so the recording reaches the model only
as the attachment's tag. The same happens when `jush file view
standup.mp3` returns the recording as a tool result. Gemini 4 and Gemini
3.8 read text, images, video, audio and PDF; models.dev lists all five,
and Jush drops `audio` when it reads the list.

Through Antigravity it is worse. Alex attaches `spec.pdf` or `demo.mp4`
and the model answers that it sees no attachment: only images reach it.
`agy` carries nothing but images and text in a prompt, and in a tool
result it drops audio and saves any other file as `media_0.png`, whatever
it holds.

## Decision

### Audio is a medium kind

A model whose catalog lists audio reads an audio file natively, in a
message and in a tool result, as it reads a video today.

| Extension | Media type | Recognized by its first bytes |
| --- | --- | --- |
| `mp3` | `audio/mpeg` | `ID3`, or an MPEG audio frame header |
| `wav` | `audio/wav` | `RIFF` … `WAVE` |
| `ogg` | `audio/ogg` | `OggS` |
| `flac` | `audio/flac` | `fLaC` |
| `aiff` | `audio/aiff` | `FORM` … `AIFF` or `AIFC` |
| `m4a` | `audio/mp4` | `ftyp` with the brand `M4A ` |

- `ModelMediaKind::Audio`, the six types in `MODEL_MEDIA_TYPES`,
  `FileExtension`'s six values and `AUDIO_FILE_EXTENSIONS`.
  `modality_extensions` maps models.dev's `audio` to them.
- `sniff_media_type` recognizes the six. An `ftyp` file with the brand
  `M4A ` is audio; every other `ftyp` stays a video.
- A message's upload of audio becomes `UserContentBlock::Audio { source:
  MediaSource }` beside its attachment record, as a video becomes
  `UserContentBlock::Video`. A request carries `UserPart::Audio(MediaBytes)`
  and `ResultPart::Audio(MediaBytes)`. Audio is never an address.
- The page and the backend ship together, so no older page meets the new
  block. An mp3 attached before this change stays the attachment it was
  sent as.

### Each wire says what it carries in a message too

Today a wire says which kinds it carries in a tool result
(`ToolResultKinds`), and each wire handles a message's medium it cannot
carry on its own: Anthropic's writes a video as text, OpenAI's sends it as
an image. Audio would add one more case to every wire. Instead:

```rust
pub struct MediaKinds {
    pub image: bool,
    pub video: bool,
    pub audio: bool,
    pub document: bool,
}

pub struct RequestLimits {
    pub body_bytes: Option<u64>,
    pub images: Option<u32>,
    /// The kinds of media the vendor's wire carries in a user's message.
    pub messages: MediaKinds,
    /// The kinds of media the vendor's wire carries in a tool result.
    pub tool_results: MediaKinds,
}
```

`ToolResultKinds` is renamed `MediaKinds`. Replay gives a message's medium
natively only when the model accepts its type and the wire carries its
kind, as it already decides for a tool result; otherwise the model reads
the medium's text, as for a model that does not accept it. Each wire's
`messages` is what its code carries today:

| Wire | `messages` |
| --- | --- |
| Gemini API, Antigravity | every kind |
| OpenAI API (both formats), Anthropic API, Claude Code | images and PDFs |
| Codex, Grok Build | images |

A wire that still meets a medium it does not carry, which only a caller
that skips replay can give it, names it in text with one function,
`MediaBytes::unsent_text`, which replaces the copies the subscription CLIs
and the OpenAI wire had. The OpenAI wire no longer sends a video as an
image.

### The Gemini API sends audio as it sends video

`provider-google` sends audio as an `inlineData` part, in a message and
beside a function response, as it sends a video.

### Antigravity's model reads a medium Jush puts in its workspace

`agy`'s `read_file` reads PDF, audio and video files by their extension,
and reads files in the session's workspace without asking. So Jush puts
each medium other than an image there, under its type's extension, and
names it in the text where the medium would be:

```text
[audio: /…/<process dir>/workspace/media/6f1c0a9e2b7d4e58.mp3]
read_file -> the recording
answer    -> "The team decided to ship on Friday…"
```

- A file's name is the first 16 hex digits of its bytes' SHA-256 and its
  type's extension, so the same bytes are written once.
- Images keep going natively, in a prompt and in a tool result.
- A medium of a new prompt or of a call's result is written before `agy`
  receives it, through `ConfigDir`, which gains `write(name, bytes)`.
- A history written into a new process's session names its media in text,
  as it already names its images: `read_file` takes only an absolute path
  (measured), and the new process's directory is unknown when its history
  is written. The model read them when they came, and its answers are in
  the history.
- `read_file` does not read `flac` or `aiff`, so Antigravity's catalog
  leaves them out. The model reads their attachment tag and can convert
  them with `shell`.
- Antigravity's `messages` and `tool_results` are every kind.
- Jush keeps answering `agy`'s permission requests as 0354 decided.
- The agent's body ends with this sentence, which names the workspace
  beside 0354's saved results; with 0354's, the model under Jush's whole
  system prompt fetched the attachments with `shell` instead (see Spike):

  ```text
  Your read_file tool reads only the files in your workspace and the files Antigravity saves on its own machine when a tool result is too long for it; read anything else with shell.
  ```

- The files live in the process's directory, as the results `agy` saves
  do, and go with it.

## Spike

All runs used `agy` 1.5.14 with an API key and `gemini-3.8-flash-low`, and
files that say or show a known sentence.

| What reached the model | Result |
| --- | --- |
| Gemini API, `inlineData` of mp3, wav, flac, ogg, aiff, m4a, in a message | Heard |
| Gemini API, mp3 beside a `functionResponse`, and inside its `parts` | Heard |
| `agy` prompt, ACP `audio` block (mp3, wav) | Not received |
| `agy` prompt, ACP `resource` blob (pdf, mp4, mp3) or `resource_link` (pdf) | Not received |
| `agy` MCP tool result, `audio` content | Dropped; the model sees only the text |
| `agy` MCP tool result, `resource` blob over 10,240 bytes | Saved as `media_0.png`; read, a video less accurately |
| `agy` MCP tool result, `resource` blob of an 8 KB mp4 | Error 400; the turn failed |
| `read_file` of a workspace file, every permission request denied | pdf, mp3, wav, ogg, m4a, mp4, mov, webm read; nothing asked |
| `read_file` of a workspace flac or aiff | Not received |
| `read_file` of a workspace file named by a path relative to the workspace | Failed, then asked outside it |
| 0354's sentence after a short system prompt; mp3, pdf, mp4 in the workspace, twice each | Read every time |
| 0354's sentence after Jush's system prompt, in the product; a PDF and an mp3 attached | Neither read: 18 `shell` commands downloaded and transcribed them |
| The new sentence, in the product; the same message | Both read with `read_file` |
| The new sentence, in the product; an mp4 and a wav from `jush file view` | Both read with `read_file` |

`agy`'s `initialize` answers `promptCapabilities: {"image": true, "audio":
false, "embeddedContext": true}`, which matches.

## Prior art

- The [Vercel AI SDK](https://github.com/vercel/ai)'s Google provider sends
  a file part of any `audio/*` type as `inlineData`, as Jush will.
- No harness that runs `agy` sends it anything but images and text.

## Alternatives

- **Send the medium as an ACP or MCP resource.** `agy` ignores it in a
  prompt and names a tool result's file `media_0.png`; a small video fails
  the turn.
- **Upload large media through the Files API.** A separate decision; audio
  goes wherever video goes.
- **Keep each wire's own fallback and add audio to each.** Repeats one
  decision in seven places.

## Open questions

None.
