//! From the core schema the SDK's release 0.2.6346 shipped, whose work
//! named the id its program gave its sender: work names its program's
//! labels in its place. The usage ledger's rows, the command history's, a
//! kept wake and a schedule an agent made keep labels in place of the
//! sender, null for every row before: no program relied on the sender yet,
//! so none is carried over. A kept wake's notice and a media job's kept
//! usage, format 4, drop the sender they named.

use super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(
    "ALTER TABLE usage_ledger DROP COLUMN sender_id;
     ALTER TABLE usage_ledger ADD COLUMN labels TEXT;
     ALTER TABLE command_history DROP COLUMN sender_id;
     ALTER TABLE command_history ADD COLUMN labels TEXT;
     ALTER TABLE schedules DROP COLUMN created_sender_id;
     ALTER TABLE schedules ADD COLUMN created_labels TEXT;
     ALTER TABLE pending_wakes DROP COLUMN message_sender_id;
     ALTER TABLE pending_wakes ADD COLUMN message_labels TEXT;
     UPDATE pending_wakes SET notice = json_remove(notice, '$.senderId')
       WHERE json_type(notice, '$.senderId') IS NOT NULL;
     UPDATE media_jobs SET usage = json_remove(usage, '$.senderId')
       WHERE json_type(usage, '$.senderId') IS NOT NULL;",
);
