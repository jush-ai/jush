---
status: proposed
---

# A page waits for no vendor's media model list

## Problem

A user has an OpenAI key and a Google key among their providers. Each time
a page opens or reconnects, it reads the product state, and the state lists
each entry's image, video and audio models for Settings › Providers. The
backend reads those lists from the vendors at that moment: for each of the
three kinds in turn, it asks every key's vendor for its models and waits for
the answers. The page shows nothing until the state arrives.

- With two such keys whose vendors answered at once (an invented key, which
  they refuse), the state took 2.9 to 4.8 seconds instead of 35 ms: the text
  models took 0 ms, and images, videos and audio 0.7 to 1.1 seconds each.
- A server with real keys answers the state in 2.0 seconds at the median and
  3.1 seconds at the 90th percentile, of which reading the model lists takes
  1.7 seconds.
- A vendor that is slow or down holds every page back by as long.

The text models do not cost this: since the catalog cache, a page never
waits for a source. The media lists the page shows never got the same rule.

## Decision

The page's state shows each entry's media models as they were last listed,
and never waits for a vendor to list them. Everything else that lists media
models still lists them from the vendor at the moment, as it does now:
`jush image models` (and video and audio) and each call before it is made,
whose listing also tells a lent entry's maker what it may ask
(`crates/provider-common/src/media/channel.rs`).

- **The last listing.** The backend keeps, in memory, the models each entry
  listed of each medium, with the vendor its maker named after listing,
  which a lent entry's maker knows only then. A list belongs to the entry's
  catalog key (its family, configuration and active account) and to the
  media catalog's date. Under a new key or catalog the list shows on until
  the listing it starts ends, so renaming an entry or turning a switch never
  empties the page's lists meanwhile.
- **Who writes it.** The listing the page's state starts, `jush image
  models` (and video and audio), and a call's.
- **The page reads it.** The state answers the kept lists at once. A list
  that was never listed, is older than 15 minutes, or is asked again by
  Settings' Refresh, is listed again in the background, one listing per
  entry and medium at a time, for at most a vendor request's 120 seconds.
  When it ends with other models than were kept, the pages hear of it and
  read the models again, as they do when a text catalog changes.
- **A listing that fails holds none**, as the state shows none today for a
  vendor that fails, and the next read after a minute lists again.
- **After a restart** the lists are empty until the first listings end, a
  second or two later; the pages then hear of them.
- **A deleted entry's lists** go with it.

```rust
// crates/backend-user-shard/src/shard/media_listings.rs
pub struct MediaListings { /* entry, medium → models, vendor, when listed */ }

impl MediaListings {
    /// What `route`'s entry listed last, and a background listing when none
    /// was, it is old, or `force`, Settings' Refresh, asks for one.
    pub(super) fn shown(&self, services: &Services, route: &Route, force: bool) -> Shown;
    /// Holds what a listing of `route` answered under the media catalog of
    /// date `catalog`.
    pub(super) fn listed(&self, services: &Services, route: &Route, catalog: Timestamp, models: &[MediaModelInfo]);
    pub fn forget(&self, provider: &ProviderId);
}
```

Nothing is stored, and nothing a page or a runner reads changes shape.

## Spike

- **Where the time goes.** A probe timed each step of the state's model
  catalog on a development backend with an invented OpenAI key and an
  invented Google key: text 0 ms, then image, video and audio about 1 second
  each, 2.9 to 3.3 seconds in all. The production log shows the same step at
  1.7 seconds.
- **Why calls keep listing.** A first design kept the media lists in the
  text catalog's stored record and had calls read them from there. A review
  found what that changes beyond the page:
  - A lent entry's maker learns what it may ask only from its listing, so
    calls lost it while the record was fresh.
  - One refresh limit of 10 seconds tied the text catalog to the slowest
    vendor's media list.
  - A list kept after a key was refused kept routing calls to that entry.
  The page alone needed the change, so the page alone gets it.

## Prior art

- [Open WebUI](https://github.com/open-webui/open-webui/blob/main/backend/open_webui/utils/models.py)
  keeps the list it fetched from each connection's `/models` and answers it
  until a refresh is asked (`get_all_models(refresh=...)`,
  `models.base_models_cache`).
- [LibreChat](https://github.com/danny-avila/LibreChat/blob/main/packages/api/src/endpoints/models.ts)
  caches each endpoint's fetched model list, and concurrent misses for one
  key share one request.
- Jush answers the page even before a first listing, and tells the page
  when one ends, as its text catalog does.

## Alternatives

- **Keep the media lists in the catalog cache's record, for calls too.** See
  the spike: it changed what calls do.
- **Read the three media kinds at the same time.** The state would still wait
  about a second for the slowest vendor, and as long as a slow or down
  vendor takes.
- **Take the media lists out of the state, and have Settings read them when
  it opens.** Settings would wait for the vendors, and its lists would appear
  late, moving what is around them.

## Open questions

None.
