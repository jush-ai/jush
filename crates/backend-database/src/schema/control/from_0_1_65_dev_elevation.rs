//! From the control schema before a Windows runner reported its elevation: a
//! device keeps whether its runner runs elevated, as it last reported.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_ELEVATION);

const CONTROL_FROM_0_1_65_DEV_ELEVATION: &str = "
ALTER TABLE devices ADD COLUMN elevation TEXT CHECK (elevation IN ('elevated', 'elevatable', 'unavailable'));
";
