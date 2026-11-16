---
status: proposed
---

# An account's usage is read one probe at a time, and less often after a failure

Supersedes:

- 0172 (Smart rotation spends the quota that resets first), the rule that a
  choice under Smart reads every snapshot older than five minutes again
  first. A choice now reads only the accounts whose wait has passed.
- 0323 (The backend reads an account's usage again when one of its windows
  resets), for a reset that passes while its account waits: it is read when
  the wait ends, not 30 seconds after the reset.

## Problem

Alex has four Claude Code accounts on one provider entry under Smart
rotation, and three conversations at work. Before each account choice, Smart
probes every account whose snapshot was read more than five minutes ago, and
each probe starts a Claude Code CLI process that asks Anthropic's usage
endpoint (`/api/oauth/usage`).

- **Several probes run at once.** The three conversations choose an account
  within the same second, each finds account A's snapshot six minutes old, and
  each starts a probe of A. Anthropic counts three reads.
- **A failure brings more probes.** Anthropic refuses one of A's reads with
  HTTP 429. The CLI then answers every probe with no usage until the time the
  refusal named, at most an hour. A's snapshot stays old, so every following
  choice probes A again. In an hour of busy work, a hundred or more CLI
  processes start, each for an answer that cannot change.
- **The refusal outlasts the hour.** When the CLI's wait ends, the next burst
  of choices reads A several times at once, and Anthropic can refuse again.

Settings › Providers then shows *The provider's usage couldn't be read* for
A, with *Claude Code's usage answer cannot be read: it holds no rate limits*.
That message reads as a fault in Jush. The server's log does not say why a
Refresh usage failed.

## Decision

The backend reads each account's usage one probe at a time. A probe the
backend starts itself waits between attempts, and a failed attempt makes the
next wait longer. The user's Refresh usage never waits. The pages change
nothing: an account shows its kept snapshot and *Updated 12 minutes ago*, as
it does today, and a failed Refresh usage shows its reason on the account's
line.

This holds for every family whose probe is free: Claude Code, Codex and Grok
Build. A probe that failed, whatever the reason, a vendor's refusal or a
device that is offline, says that another one soon would most likely fail
the same; meanwhile each response the account gets still updates its
windows.

- **One probe at a time per account.** A probe that starts while another of
  the same account runs waits for it and answers what it answered, with no
  read of its own. This holds for every probe: Smart's choice, the read after
  a reset, the one a Claude Code process start runs for the CLI's feature
  flags, and Refresh usage. A read that has not ended after 60 seconds, twice
  Claude Code's own limit on its usage request, fails, so that none holds the
  account's other probes for good.
- **Routine probes wait between attempts.** Smart's choice, the read after a
  reset and the flag refresh are routine. A routine probe runs only when the
  account's last attempt, successful or not, started at least its wait ago:
  - 5 minutes after a success, as the five minutes 0172 set.
  - After failures in a row: 5, 10, 20, then 40 minutes, then 60 minutes
    for each further failure. The cap is the longest wait Claude Code's CLI
    keeps after a refusal: waiting longer would only leave the snapshot
    older.
  - A success sets the wait back to 5 minutes.
- **A routine probe that must wait reads nothing.** It answers
  `QuotaError::Waiting`. Smart then chooses from the kept snapshot, as it
  does after a failed probe today, and logs the wait at debug level, not as
  a failure. A reset that passes while its account waits is read for as the
  wait ends, once, unless another probe read past it meanwhile. A flag
  refresh that must wait is tried at the next process start three hours on,
  as a failed one is today.
- **Refresh usage never waits.** It reads, or, asked while a probe of the
  account runs, answers what that probe read. Its failure counts towards the
  wait as any other: pressed four times while the vendor refuses, it leaves
  routine probes waiting 40 minutes after the last press, until a Refresh
  usage that succeeds.
- **The wait is the backend's memory.** It is kept in memory beside the
  snapshot, per account, and forgotten as the backend stops: the first
  routine probe after a restart runs. A new sign-in to the account does not
  end it; Refresh usage reads at once, and its success does.
- **Claude Code says what happened.** When the CLI answers with no rate
  limits while it says they apply (`rate_limits_available: true`), the probe
  fails with *Claude Code could not read the account's usage from
  Anthropic, which can ask it to wait up to an hour after a refused read.*
- **The server logs a failed Refresh usage**, `account_usage.refresh_failed`
  at warn level with the provider, the account and the reason, as Smart's
  failed read already logs `account_usage.read_failed`.

With four accounts, a routine probe reads each at most 12 times an hour while
it succeeds, and at most 4 times in the first hour after it starts failing,
then once an hour.

The design to build from:

```rust
// crates/provider-common/src/quota.rs
/// Who asks for a probe.
pub enum ProbeAsk {
    /// The user's Refresh usage: it never waits, and reads, or answers what a
    /// probe in progress read.
    User,
    /// The backend's own: it runs only once the account's wait has passed.
    Routine,
}

pub enum QuotaError {
    // ...as today, and:
    /// A routine probe met the account's wait, which ends at `until`.
    Waiting { until: Timestamp },
}

/// 5 minutes after a success; 5, 10, 20, 40, then 60 after failures.
pub fn routine_wait(failures: u32) -> Duration;

/// The probes of one account, shared by every provider of the account.
pub struct ProbeGate {
    /// The attempts that ended: a probe that waited for the gate and finds
    /// the count changed answers the last attempt's outcome.
    ended: AtomicU64,
    /// Held for the length of a probe: when the last attempt started, its
    /// failures in a row, and its outcome.
    last: tokio::sync::Mutex<Option<Attempt>>,
}

impl ProviderQuota {
    /// Asks the vendor's usage endpoint about the account, one probe at a
    /// time, under the account's wait for `ask`.
    pub async fn probe(&self, ask: ProbeAsk) -> Result<Arc<QuotaSnapshot>, QuotaError>;
}

pub trait QuotaSnapshotStore: Send + Sync {
    // ...as today, and:
    /// The account's probe gate, the same for every provider of the account.
    fn gate(&self) -> Arc<ProbeGate>;
}

// crates/provider-registry/src/quotas.rs
pub struct AccountQuotas {
    // ...as today, and each account's gate, forgotten with the account:
    gates: Mutex<HashMap<Account, Arc<ProbeGate>>>,
}
```

`MemorySnapshots`, the store of a provider built during a login, holds a gate
of its own. Nothing stored changes, the protocol does not change, and an
older page sees the same answers.

## Spike

Reading Claude Code 2.1.325's code, and running its `get_usage` request by
hand against accounts on a development machine, settled how the CLI reads
usage:

- A `get_usage` request reads `/api/oauth/usage`, unless the account's
  configuration directory holds a reading less than 60 seconds old.
- An answer of HTTP 429 or 403 is kept for the bearer token for the
  `Retry-After` the answer names, or 5 minutes without one, and at most an
  hour. Until then, every process of the account answers `rate_limits: null`
  with `rate_limits_available: true`, and asks nothing. The CLI's debug log
  says `429 remembered for this bearer; not asking again for 3283s`.
- The same request sent to the endpoint directly with the account's token
  was answered at once, while the CLI still waited.

So a probe during the CLI's wait costs a process start and reads nothing.
Anthropic's own limit on the endpoint is not published; Claude Code's
issue tracker reports refusals at low polling rates and `Retry-After` values
of hours.

## Prior art

- [CodexBar](https://github.com/steipete/CodexBar) keeps one gate per
  Claude access token
  ([`ClaudeOAuthUsageRateLimitGate.swift`](https://github.com/steipete/CodexBar/blob/1f6da9bec44455288ddee5ef6f3617a803d8eb2a/Sources/CodexBarCore/Providers/Claude/ClaudeOAuth/ClaudeOAuthUsageRateLimitGate.swift)):
  a 429 blocks reads until its `Retry-After`, or 5 minutes, never shortening
  a block already set, and a success clears it. Its refresh loop reads every
  2 to 30 minutes by how recently the user worked
  ([`AdaptiveRefreshPolicyCore.swift`](https://github.com/steipete/CodexBar/blob/1f6da9bec44455288ddee5ef6f3617a803d8eb2a/Sources/AdaptiveRefreshCore/AdaptiveRefreshPolicyCore.swift)).
- Claude Code's CLI keeps the same kind of gate per bearer token, capped at
  an hour, as the spike found.

Jush cannot see the `Retry-After` behind the CLI's empty answer, so it backs
off by doubling up to the CLI's cap instead of waiting the named time.

## Alternatives

- **Read the endpoint directly** with the account's token, for the
  `Retry-After` and no process start. Jush would then send requests to
  Anthropic itself with the CLI's credentials, which today only the CLI does,
  and would have to follow every change the CLI makes to that request.
- **Only one probe at a time, no wait.** It removes the burst of parallel
  probes, but a failing account is still read at every choice.
- **A longer interval for every account,** such as 15 minutes. Routing would
  then use older numbers for accounts that read without trouble, while the
  failures, the cause of the refusals, stay unbounded.

## Open questions

None. The wait after a success stays five minutes: the refusals came from
the probes that followed failures, not from that rate.
