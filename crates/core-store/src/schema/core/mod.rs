//! The core database's history: each schema a core's database may hold
//! before the current one, oldest first, and the step from it to the next.
//! The schema a server's control database is split into
//! (`crates/backend-database/src/schema/control/core_at_one_core_per_user.sql`)
//! is the current one until the first change, and then the first of these.

type Shipped = super::Shipped<()>;
/// The step of each schema of the history, as its own module says it.
type Migration = super::Migration<()>;

mod from_0_2_6247;

pub(super) const HISTORY: &[Shipped] = &[Shipped {
    sql: include_str!("from_0_2_6247.sql"),
    migration: from_0_2_6247::MIGRATION,
}];
