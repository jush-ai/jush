//! From the core schema the SDK's release 0.2.6342 shipped: a call names its
//! assignment. The usage ledger's rows name the assignment their agent
//! worked in, a kept wake its message's, and a schedule an agent made whom
//! and which assignment the turn that made it worked for, which its runs'
//! messages continue; null for every row before.

use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(
    "ALTER TABLE usage_ledger ADD COLUMN assignment_id TEXT;
     ALTER TABLE pending_wakes ADD COLUMN message_assignment_id TEXT;
     ALTER TABLE schedules ADD COLUMN created_sender_id TEXT;
     ALTER TABLE schedules ADD COLUMN created_assignment_id TEXT;",
);
