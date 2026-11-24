---
status: proposed
---

# A program writes its conversations' system prompt

Supersedes: 0324 (A conversation speaks as its program's product, with the
plugins it names), the identity, which replaced only the prompt's first
sentences.

## Problem

A program sets a conversation's identity, "You are the help desk's
assistant.", and Jush's guide follows it. The guide tells the agent how
Jush's web app shows a reply: link a device's file as `[name](device:…)`,
a subagent as `agent:4`, a commit and a conversation by their ids. A reply
read in a team's chat shows those links broken. The conversation's list of
commands (0370) takes out the sentences of the commands it leaves out, but
the rest of the guide is Jush's, and the program changes none of it.

## Decision

A conversation takes the program's whole system prompt:

```ts
await core.conversations.create({
  systemPrompt: "You are the help desk's assistant. Reply in the team's chat format…",
  commands: ['shell', 'browser', 'acme'],
})
```

- `systemPrompt` replaces Jush's prompt, its identity and its guide, for
  every agent of the conversation, its subagents included. Null keeps
  Jush's.
- After it, Jush writes only what the agent cannot work without and the
  program cannot know: the index of the commands the conversation has
  (0370), each with its line, and the line naming the model that serves the
  conversation, which a switch of model changes. A subagent's profile's
  instructions follow, as they follow Jush's. Context blocks, such as the
  execution context and the user's time, stay blocks of the transcript.
- With Jush's prompt, each of its sentences that tells of a command shows
  only when the conversation has that command (0370).
- A change reaches each live agent from its next request, as the identity's
  did.
- The record keeps the prompt in the identity's column, renamed
  `system_prompt`. A conversation's old identity becomes the prompt its
  agents read then: the identity, a blank line, and the guide after it,
  in pending forks too.

## Spike

- A conversation with a program's prompt and the commands
  `['schedule']` sent the model the program's text, the index of
  `jush schedule` and `jush shell`, and the model's line, and nothing of
  Jush's guide.
- With the prompt cleared, the next request opened with Jush's identity and
  guide, the guide without `jush attachment upload`'s sentence, which the
  list leaves out.
- A prompt of white space alone was refused.
- A core of 0.2.6332 whose conversation had the identity "You are the
  desk." opened with the prompt "You are the desk.", a blank line, and
  Jush's guide of then. The step's turn of a pending fork's metadata, run on
  its own, gave a row with an identity a `systemPrompt` of the same text and
  no `identity`, and left a row without one as it was.

## Prior art

- The Claude Agent SDK's `systemPrompt` is the application's own text, or
  Claude Code's preset with the application's text appended
  ([Modifying system prompts](https://docs.claude.com/en/docs/agent-sdk/modifying-system-prompts)).
- The OpenAI Agents SDK's agent takes `instructions`, the whole system
  prompt, its own tools aside
  ([Agents](https://openai.github.io/openai-agents-js/guides/agents/)).
- Jush departs in writing the index of the commands after the program's
  text, since commands reach the agent through its shell, not as tools.

## Alternatives

- Keep the identity, and let the program switch off sentences of the guide
  one by one: the program would learn Jush's guide to edit it.

## Open questions

None.
