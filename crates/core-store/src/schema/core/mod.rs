//! The core database's history: each schema a core's database may hold
//! before the current one, oldest first, and the step from it to the next.
//! Each is one that a release of the SDK made its cores with.
//!
//! The schema a server's control database is split into
//! (`crates/backend-database/src/schema/control/core_at_one_core_per_user.sql`)
//! is the current one until a server's release carries the split, and then
//! one of these.

type Shipped = super::Shipped<()>;
/// The step of each schema of the history, as its own module says it.
type Migration = super::Migration<()>;

mod from_0_2_6247;
mod from_0_2_6254;
mod from_0_2_6320;
mod from_0_2_6342;
mod from_0_2_6346;

pub(super) const HISTORY: &[Shipped] = &[
    Shipped {
        sql: include_str!("from_0_2_6247.sql"),
        migration: from_0_2_6247::MIGRATION,
    },
    Shipped {
        sql: include_str!("from_0_2_6254.sql"),
        migration: from_0_2_6254::MIGRATION,
    },
    Shipped {
        sql: include_str!("from_0_2_6320.sql"),
        migration: from_0_2_6320::MIGRATION,
    },
    Shipped {
        sql: include_str!("from_0_2_6342.sql"),
        migration: from_0_2_6342::MIGRATION,
    },
    Shipped {
        sql: include_str!("from_0_2_6346.sql"),
        migration: from_0_2_6346::MIGRATION,
    },
];
