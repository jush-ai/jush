---
status: accepted
---

# An image limit is its own window

## Problem

Alex's Codex account has a weekly limit for its turns, 0% used, and a daily
limit for its images, 12% used. Settings shows the account's usage as one
`primary` window: after Alex makes an image it reads *Daily limit, 12% used*,
and after Refresh usage it reads *Weekly limit, 0% used*, and back again with
the next image. Neither is wrong, and both look like noise.

The cause: the answers of Codex's image endpoint carry the same
`x-codex-primary-*` headers as a turn's, describing the image limit, and
every answer's headers are read into the account's windows by their names.
Codex's usage status (`GET /wham/usage`), which Refresh usage reads, reports
the turns' limit only: its `additional_rate_limits` is `null` for this
account, so the image limit is known only from an image's answer.

The same goes for a refusal: an image refused as used up marks the whole
account used up, so the turns move to another account, or stop, while the
account's own limit has room.

## Decision

A window a response reports belongs to what the request asked for. The
answer of an image request reports image windows, which never take the place
of a turn's window, and the other way round.

```text
alex@example.com  Plus  Current
Weekly limit                 Resets Thu 12:58 AM · 0% used
████████████░░░░░░░░░░░░░░░░░
Updated 1 minute ago
Daily image limit            Resets in 21h 35m · 12% used
███░░░░░░░░░░░░░░░░░░░░░░░░░░
Updated 2 hours ago
```

* **What it shows.** The account's windows as they are now, with when they
  were last read, then each image window, named *Daily image limit* in the
  page's language, with when the image's answer that reported it came:
  Refresh usage does not read it, so its own time says how old it is. The
  account's time is that of its own windows, which an image's answer leaves.
  An image window full puts no *Limit reached* on the account, and the
  conversation's context card leaves it out: neither is about images.
* **What it counts for.** Rotation and the line that says an agent moved
  count an image window only for an image request, as a window scoped to a
  model counts only for that model. An account's own windows count for an
  image as they do today. A used-up refusal marks the account for what the
  refused request was for: an image's leaves the turns, and the account's
  own holds for images too.
* **Where it applies.** Every subscription whose image answers report
  windows. Codex's do. Grok Build reads its image answers' rate-limit
  headers the same way, but none of its 30 image calls since October 25
  reported a window, so it shows no image window today and takes one by the
  same rule if it starts to.

The design to build from:

* `provider-common`: `Observation::Response` says what the request was for
  (`Metered::Account` or `Metered::Images`), and a media call's quota says it
  with the quota (`MediaQuota`). `ProviderQuota` gives a window an image's
  answer reports the id `images:<id>` and the scope
  `{ kind: "images", label: null }` (`QuotaWindow::of_images`), for every
  family alike; the families' readers stay as they are.
* `provider-registry`: a window scoped to images counts only for an image
  request (`QuotaWindow::counts_for`), and a used-up mark is kept per
  account and purpose; the rotation's calls say which they make
  (`Metered`, from `MediaKind::metered` for media). What a snapshot says of
  each purpose is as old as its windows of that purpose
  (`QuotaSnapshot::read_at`): a mark holds until a read of its own windows
  after it, and Smart reads an account again by the age of its own windows.
  A refusal's moment comes from the windows that count for the request
  (`ProviderQuota::used_up_until`).
* `web-ui`: `windowName` names a window on images in the page's language;
  `ProviderQuota` shows a window with `readAt` apart, with its own update
  time (`QuotaReadRow`); the context card and *Limit reached* leave image
  windows out (`onImages`).
* A snapshot stored before keeps its windows; the next turn's answer or
  refresh writes the turns' window again over a stale image value.

## Spike

The account's usage status, read once with its stored sign-in and no
renewal, answered `rate_limit.primary_window` of 604800 seconds, 0% used,
`secondary_window: null` and `additional_rate_limits: null`. The usage
ledger's twelve image calls of the same hour each kept a window of 86400
seconds, 12% used, resetting the next day.

## Prior art

* **Codex CLI** reads each family of rate-limit headers as a limit of its
  own, by its id (`codex`, `codex_bengalfox`), and names it by its length
  (`codex-rs/codex-api/src/rate_limits.rs`); it reads no image answers, since
  its image tool runs in the service. Jush takes its rule that each limit
  stays its own, and adds the request's purpose, which the headers' names do
  not tell apart.

## Alternatives

* **Leave image answers' windows out.** The daily image limit, which an
  account does reach, would show nowhere until an image fails.
* **Name the scope by the image model** (`gpt-image`), which rotation already
  reads. The limit is the endpoint's, not one model's, and the name would be
  an id.

## Open questions

None.
