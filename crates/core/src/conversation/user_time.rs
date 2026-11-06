//! The user's local time, which the first request of each turn, and the first
//! after the turn resumes, starts with: the model has no clock of its own, and
//! a device's clock tells that device's zone, not the user's.
//!
//! The zone is the one the user's page last reported, which schedules and
//! history searches read too; the minute is the core's clock. Before any
//! page reported a zone, or for one this build does not know, the block gives
//! UTC and says the user's zone is unknown, so the model does not take UTC
//! for the user's. The block follows the turn's input, never in the system
//! prompt, whose cache then holds across days.

use std::rc::Weak;

use futures_util::future::LocalBoxFuture;
use jiff::tz::TimeZone;
use jush_agent_tools::{ContextAnswer, ContextSource, NodeContext, SeenBlock};
use jush_shared_types::{Timestamp, TurnId, USER_TIME_SOURCE};

use crate::Core;
use crate::schedules::plan;

/// The `user_time` context source: a block when the node's turn has none
/// since it started or last resumed, so a compaction that drops it brings
/// it again.
pub(crate) struct UserTime {
    pub(crate) core: Weak<Core>,
}

impl ContextSource for UserTime {
    fn name(&self) -> &str {
        USER_TIME_SOURCE
    }

    fn context<'a>(
        &'a self,
        _: NodeContext<'a>,
        _: &'a TurnId,
        seen: &'a [SeenBlock<'a>],
    ) -> LocalBoxFuture<'a, Result<Option<ContextAnswer>, String>> {
        Box::pin(async move {
            if seen.iter().any(|block| block.current) {
                return Ok(None);
            }
            let Some(core) = self.core.upgrade() else {
                return Ok(None);
            };
            let preferences = core
                .store()
                .preferences()
                .await
                .map_err(|error| format!("the user's time zone cannot be read: {error}"))?;
            let now = core.services().clock.now();
            let zone = preferences.locale.map(|locale| locale.time_zone);
            Ok(Some(ContextAnswer::from(user_time(now, zone.as_deref()))))
        })
    }
}

/// The block's text for `now` in `zone`, the user's IANA zone; UTC when the
/// zone is not known.
fn user_time(now: Timestamp, zone: Option<&str>) -> String {
    let Some((name, found)) = zone.and_then(|name| Some((name, plan::zone(name).ok()?))) else {
        let utc = now.to_jiff().to_zoned(TimeZone::UTC);
        return format!(
            "Now: {} UTC; the user's time zone is not known.",
            utc.strftime("%A, %Y-%m-%d %H:%M"),
        );
    };
    let local = now.to_jiff().to_zoned(found);
    format!(
        "Now: {} in the user's time zone, {name} (UTC{}).",
        local.strftime("%A, %Y-%m-%d %H:%M"),
        local.strftime("%:z"),
    )
}
