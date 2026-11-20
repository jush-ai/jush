//! The quota of an Antigravity account: its plan, from a probe's new session
//! (`probe.rs`). `agy` reports no usage, so the account has no windows to
//! show.

use futures_util::future::BoxFuture;
use jush_provider_common::quota::{
    ObservedWindows, Observation, ProbeCost, ProbeReading, QuotaError, QuotaSource,
};
use jush_shared_types::QuotaPlan;

use crate::probe::Prober;

/// The quota source of one Antigravity account.
pub(crate) struct AgyQuota(pub(crate) Prober);

impl QuotaSource for AgyQuota {
    fn probe_cost(&self) -> Option<ProbeCost> {
        Some(ProbeCost::Free)
    }

    /// The plan a new session of the account names.
    fn probe(&self) -> BoxFuture<'_, Result<ProbeReading, QuotaError>> {
        Box::pin(async move {
            let session = self.0.session().await.map_err(|failure| failure.quota_error())?;
            let tier = session
                .meta
                .and_then(|meta| meta.antigravity)
                .and_then(|antigravity| antigravity.tier);
            Ok(ProbeReading {
                plan: tier.map(|tier| QuotaPlan {
                    id: tier.id,
                    label: tier.name,
                }),
                account_label: None,
                windows: Vec::new(),
                absent: Vec::new(),
            })
        })
    }

    fn observe(&self, _: Observation<'_>) -> Option<ObservedWindows> {
        None
    }
}
