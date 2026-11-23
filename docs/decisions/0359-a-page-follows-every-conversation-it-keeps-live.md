---
status: accepted
---

# A page follows every conversation it keeps live

Supersedes: 0346 (A page follows the live output of the conversation it
shows), the part that has a page follow only the status of a conversation it
does not show, or shows on a page in the background.

## Problem

Alex has *Fix the login page* running and reads *Plan the release*. They
switch back to *Fix the login page*. Under 0346 its lane followed only its
status meanwhile, so the page shows the transcript as it was when Alex left,
asks the server for the patches since, and only then draws the turn that ran:
old rows first, then a jump, and the catching-up spinner when the answer
takes more than 500 ms. A page that comes back from another browser tab does
the same with the conversation it shows.

This is the jump 0090 removed. On a server over four days, about 500
switches back to a conversation the page held waited for such an answer; 57
of them carried more than 200 patches, and 41 were stale and read the latest
page again.

## Decision

Every conversation the page keeps, the eight shown most recently and each
one that runs (0090), follows its transcripts live, whether the page shows it
or not and whether the page is in front or not. Switching to one shows what
it holds now, with no wait and no jump.

The cost 0346 removed comes back: a page in the background, a locked phone's
included, receives the live output of the conversations it keeps, about 5
messages and 6 KB a second more with five running, by 0346's measure.
Showing current data at once is worth more than that traffic.

What goes, as before 0346:

* The client's `pauseTranscripts` and `resumeTranscripts`, the root marked
  behind, and the store's choice of which conversations follow live.
* The `followed` frame and its `understands` entry. `understands` stays, for
  `metadata_changed`.

What stays from 0346, none of which makes the page wait:

* The metadata sent by the fields that changed (`MetadataChanged`).
* The root's recent patches after its turn ends, so a page whose connection
  dropped while the turn ended, as a phone that slept, comes back by patches
  rather than a read.
* A conversation the page lets go is written to its copy on disk whether its
  watch saw the last change or not.

An older page names `followed` in its `open`, which the backend no longer
knows, so the backend refuses each of its lanes' opens and its conversations
show that they failed to open, until the page reloads into the new web app,
which a page does once for each new build it learns of. A development page,
which has no build, is reloaded by hand.

## Spike

None.

## Prior art

* Element X's
  [sliding sync](https://github.com/matrix-org/matrix-spec-proposals/blob/main/proposals/4186-simplified-sliding-sync.md),
  0346's precedent, follows only the rooms on screen and pays a wait on each
  room opened. [matrix-js-sdk](https://github.com/matrix-org/matrix-js-sdk)'s
  classic sync (`src/sync.ts`), which Element Web uses, receives every joined
  room's timeline, so a room shows current the moment it is chosen; Jush
  keeps that, as 0090 did, for the conversations it keeps.

## Alternatives

* **Pause only while the page is in the background.** Switching between
  conversations stays immediate, but coming back to the page waits and jumps
  in the conversation it shows.
* **Keep 0346 and hide the old rows until the answer comes.** The wait stays;
  it only moves from a jump to a blank.

## Open questions

None.
