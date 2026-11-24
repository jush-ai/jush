---
status: proposed
---

# An Antigravity account signs in with a refresh token another app holds

Supersedes: 0352 (An Antigravity subscription runs through Jush's copy of
Google's agy), the sentence "Jush never reads, copies or uses the account's
OAuth tokens": Jush now passes on a refresh token the user pastes, once, to
`agy`. Every other part of the rule still holds.

## Problem

Alex keeps five Google accounts in an Antigravity account manager, a desktop
app that holds each account's refresh token and switches Antigravity between
them. To add them to Jush, Alex signs in to each one again: Add Account, open
Google's page in a web browser, choose the account, copy the code, paste it
back. That is five round trips through Google's page for sign-ins Alex
already has, and an account whose password Alex keeps elsewhere, or which
asks for a second factor, costs more each time.

The manager offers the other way: it exports an account's refresh token, a
string starting with `1//`, which Antigravity account managers import by
pasting.

## Decision

Antigravity's sign-in dialog offers two ways, side by side under its title:
**Browser**, chosen when it opens, and **Refresh Token**. With Refresh
Token the user pastes a refresh token, and Jush adds the account it belongs
to, with its email as its name, as a sign-in in a web browser does.

```text
+-- Sign In to Antigravity -----------------------------------+
| (Browser) Refresh Token                                     |
| Sign in on the vendor’s page, then paste the code it shows. |
|                                                             |
| [Open in Browser ↗]  [Copy link]                            |
| [ Paste the code                          ]  [Continue]     |
| ◌ Waiting for the code · expires in 9:41                    |
|                                                             |
|                                                  [Cancel]   |
+-------------------------------------------------------------+

+-- Sign In to Antigravity -----------------------------------+
| Browser (Refresh Token)                                     |
| Paste a refresh token from another app signed in to         |
| Antigravity, such as an account manager.                    |
|                                                             |
| [ •••••••••••••••••••••••••••••••••• (o) ]  [Continue]      |
| ◌ Signing in…                    (once Continue is clicked) |
|                                                             |
|                                                  [Cancel]   |
+-------------------------------------------------------------+
```

- The two ways are a segmented control, the way in use chosen, so the
  dialog always says which way it signs in and which other it offers. A
  click on the chosen way does nothing, since a change ends the sign-in
  under way; while a sign-in is starting, before Jush knows it to end it,
  its way is kept.
- The panel hangs from the top of the window, so the choice stays under
  the pointer while the phases below it change the dialog's height.
- The token's phase counts down to the sign-in's end, as the code's does.
- The token's field draws it as dots, with the eye to show it, as an API
  key's field does: the token signs in to the account until it is revoked.
- A failed sign-in keeps the choice, so the user can try the other way.
- The terms notice that every Add Account shows comes first, as before.
- Choosing the other way ends the sign-in that ran and starts one that
  way; the dialog's stages (waiting for the machine, installing, starting)
  show as for any sign-in.
- A token Google refuses, a mistyped one or one another Google app issued
  (below), ends the sign-in with *Antigravity did not accept this refresh
  token.* and Retry; text with anything but a token's characters ends it
  with *This is not a refresh token.* An account Jush has already is signed
  in again, as a sign-in in a web browser does.
- The token is one line of text, kept nowhere: Jush stores no copy, logs
  none, and the field clears when the dialog closes.

### How it signs in

The token sign-in is a login of the Subscriptions machine like the one in a
web browser (`crates/provider-subscription-cli/src/login.rs`): it starts in
a new directory, which becomes the account's when it succeeds and is removed
otherwise, and the account is read from the sign-in `agy` wrote, by its
email. Only the start differs:

```text
backend                     Subscriptions machine, new directory as HOME
  token -> process stdin -->  sh: read one line, check it is a token,
                                write .gemini/antigravity-cli/
                                  antigravity-oauth-token  (0600)
                                  {"token": {"refresh_token": <token>,
                                   "access_token": "", "expiry": <past>},
                                   "auth_method": "consumer"}
                              exec agy --print /usage --output-format json
                                -> agy renews the expired sign-in with the
                                   token and writes it back whole, with
                                   id_token_email
  sed reads id_token_email <-
```

- `agy --print /usage` is the usage read Jush runs already; it makes no
  model request. A token Google refuses makes `agy` print its sign-in link,
  on its standard error, and wait: the login reads a link on either stream
  as the refusal and ends the process.
- The token reaches the machine as a code does, on the process's standard
  input, never in its arguments or environment, and the script accepts only
  a token's characters (`[A-Za-z0-9/_.-]`), so it writes valid JSON.
- `agy` alone calls Google, with the token; Jush never does.

The types it becomes:

```rust
/// How a sign-in proves the account (web API).
pub enum LoginMethod {
    Browser,
    RefreshToken,
}

pub trait ProviderFamily {
    // as now, and:
    /// The ways an account of a subscription family signs in, the first
    /// the one a sign-in starts with.
    fn login_methods(&self) -> &'static [LoginMethod] {
        &[LoginMethod::Browser]
    }
}

/// What the user does in a family's login.
pub enum LoginKind {
    Device,
    PastedCode,
    /// The user pastes a refresh token another app of the vendor's
    /// holds, which Jush hands to the login as it hands a code.
    RefreshToken,
}

pub struct LoginIo<'a> {
    /// The way the user chose: the family's login, or `RefreshToken`.
    pub kind: LoginKind,
    // as now
}

pub trait CliLogin {
    // as now, and:
    /// How the CLI signs in with a refresh token: the request that starts
    /// it at a site; none for a CLI that cannot.
    fn refresh_token_request(&self) -> Option<fn(&CliSite) -> CliStart> {
        None
    }
}
```

- The vendor catalog lists each family's `loginMethods`, which the dialog
  offers; Antigravity's family lists both.
- `POST /providers/subscription-login` and
  `POST /providers/:id/accounts/login` take `method`, `"browser"` (the
  default, as today) or `"refreshToken"`; the backend refuses a method the
  family does not list. A listed login names its `method`, so a page that
  reopens a running sign-in shows its way.
- A token login's pending state has no `verificationUrl`; `needsCode` is
  true, and the token goes through `POST …/subscription-login/:id/code`
  like a code. Its account's `source` is `login:refresh_token`.
- An older page sends no `method` and signs in in a web browser. A page
  and its backend are one release, so a newer page never meets an older
  backend.
- A token sign-in takes one token; a second paste is refused, as the
  process reads one line.

## Spike

1. **Does a refresh token Antigravity account managers export work for
   `agy`?** Google renews a token only for the OAuth client that issued it.
   The client in the sign-in link `agy acp --login` prints is the leading
   manager's personal client (`modules/oauth.rs`). Its enterprise and GCP
   clients' tokens, and Gemini CLI's, are not; `agy` cannot renew those.
2. **Does `agy` renew a sign-in that holds only a refresh token?** On the
   Subscriptions machine, a sign-in holding a real account's refresh token,
   an empty access token, an expiry in 2000 and no email: `agy --print
   /usage` logged `keyringAuth: loaded token … expired=true`, then `token
   refreshed`, answered the usage, and wrote the sign-in back with a new
   access token, its expiry and `id_token_email`. With a made-up token it
   logged `invalid_grant` and printed its sign-in link on its standard
   error.
3. **Does the renewal change the token?** No: the token in the file after
   renewal hashes the same as before, and the account's own sign-in, which
   shares the token, renewed again afterwards. The manager and Jush can
   hold the same token.

## Prior art

- **[Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager)**,
  the leading account manager, adds an account by OAuth or by a pasted
  refresh token, and switches `agy` between its accounts by writing the
  same file with the same fields (`modules/integration.rs`). It calls
  Google itself with the token, to read the email and the quota; Jush lets
  `agy` do it.
- **[GitHub CLI](https://cli.github.com/manual/gh_auth_login)** offers
  *Login with a web browser* or *Paste an authentication token*;
  **[Codex](https://github.com/openai/codex)** offers *Sign in with
  ChatGPT* or an API key. Both ask first; Jush starts with the web browser,
  which most users take, and shows both ways at once, as the leading
  manager's dialog shows its OAuth, Refresh Token and Import tabs.

## Alternatives

- **Read the email from Google with the token (B).** Jush would call
  Google's user-info endpoint itself, which the rule of 0352 that `agy`
  alone calls Google forbids, for nothing `agy` does not already give.
- **Let the user type the email (C).** A mistyped email names the account
  wrongly and breaks the identity a second sign-in of the account matches.
- **Import a manager's export of many accounts at once.** Not now: one token
  at a time.
- **A menu on Add Account with both ways.** One more click for everyone who
  signs in in a web browser, the way most users take.
- **A link under the form, or a button beside Cancel naming the other
  way.** Either names only the way not in use, so the dialog never says
  which way it is in, and the button reads as the way in use.

## Open questions

None.
