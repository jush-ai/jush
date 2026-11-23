---
status: proposed
---

# A program sets the language and time zone its agents work in

Supersedes: 0362 (Each turn knows the user's time), the part that takes the
zone from the page's report alone.

## Problem

Every command runs in the time zone and languages the user's browser last
reported (`Preferences.locale`): the conversation's Chrome starts with them,
so a page renders in the user's language and shows times in the user's
zone, and a new schedule counts in that zone. A program's core has no
browser that reports, so its commands run in the default, `en-US` and UTC.

A program whose people read Chinese and work in Shanghai gets English
sign-in pages, whose labels its automation does not find, and reports that
show times in UTC, in screenshots its agents send and in the times they
read off a page. Its runner's own `LANG` and `TZ` change nothing: Chrome's
environment is the command context's, not the runner's.

## Decision

A core's preferences hold `commandLocale`, the time zone and languages its
commands run in, which a program sets; without one, commands follow the
browser's report as now.

```ts
await core.updatePreferences({ commandLocale: { timeZone: 'Asia/Shanghai', languages: ['zh-CN'] } })
// Back to the browser's report:
await core.updatePreferences({ commandLocale: null })
```

```rust
pub struct Preferences {
    // …
    /// The time zone and languages commands run in, in place of what the
    /// user's browser reported; none follows the browser's.
    pub command_locale: Option<CommandLocale>,
}
```

- The command context takes `commandLocale`, else the browser's `locale`,
  else the default (`crates/core-devices/src/command_context.rs`). The time
  each turn tells the model is in the same zone, so the model and its
  commands agree on the time.
- A page never writes it, so a page that reports its own browser changes
  nothing a program set.
- It is checked as a report is: a time zone the core knows, in its IANA
  spelling, and each language a BCP 47 tag, kept canonical.
- A conversation's browser takes the locale as it starts, as it takes the
  color scheme: one already open keeps its own until it starts again, after
  its last tab closed. Starting it again at a change would close the tabs of
  every conversation that shares its profile, whose Chrome is one process,
  and the change of a browser's own report, such as a user's phone and
  computer reporting other languages, would reload every conversation's
  tabs.
- `core.updatePreferences(patch)` sends a `PreferencesPatch`, the web app's
  own, and answers the preferences; `core.state.preferences` holds them.

## Spike

A program's core opened a page that shows `navigator.language`, the time
zone `Intl` resolves and the `Accept-Language` it was sent:

- Before the program set a locale: `en-US`, UTC, `en-US,en;q=0.9`.
- After `commandLocale: { timeZone: 'asia/shanghai', languages: ['zh-cn'] }`,
  saved as `Asia/Shanghai` and `zh-CN`, a browser started anew showed
  `zh-CN`, `Asia/Shanghai` and `zh-CN,zh;q=0.9`.
- A first version started a running browser again when a command came in
  another locale. A page's report of other languages then reloaded the
  conversation's tabs as well, so a browser keeps its locale while it runs.
- The language changed only once the profile's `selected_languages` was
  written too: Chrome puts it back over `accept_languages` as it starts,
  so a profile kept the languages of its first start, the browser's report
  of a user whose languages changed included.
- `Mars/Olympus` was refused, naming `commandLocale.timeZone`; `null` cleared
  it.

## Prior art

- Playwright sets a context's `locale` and `timezoneId`
  ([options](https://playwright.dev/docs/api/class-browser#browser-new-context)),
  and Puppeteer a page's time zone (`page.emulateTimezone`): the automation
  chooses them, not the machine. Jush keeps one locale for a conversation's
  Chrome, since conversations that share a browser profile share its
  process, whose own interface language is one.

## Alternatives

- A locale for each conversation, as `browserProfile` is: conversations that
  share a profile share one Chrome, which can show one interface language;
  a program that needs several sets one core for each.
- The runner's environment: a device serves every core and conversation
  that reaches it, and its `LANG` is the device's, not the work's.

## Open questions

None.
