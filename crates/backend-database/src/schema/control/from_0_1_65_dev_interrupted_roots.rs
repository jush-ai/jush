//! From the control schema whose marks were each a root whose turn the
//! shutdown interrupted: a mark now also stands for a conversation whose
//! subagents worked while its root rested, and says which it is. Every mark
//! stored before was a root's.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_INTERRUPTED_ROOTS);

const CONTROL_FROM_0_1_65_DEV_INTERRUPTED_ROOTS: &str = "
ALTER TABLE interrupted_turns ADD COLUMN root INTEGER NOT NULL DEFAULT 1 CHECK (root IN (0, 1));
";
