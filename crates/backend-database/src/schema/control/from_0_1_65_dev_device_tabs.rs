//! From the control schema whose browser and simulator tabs were the
//! conversation's primary device's alone, named by the device's own numbers:
//! each such tab now names its device, and its id names it too, since each
//! device numbers its own tabs. The device a stored tab came from is not
//! recorded, so the tabs go, their ids not retired, and each device lists its
//! tabs again when its runner connects to the upgraded server, as a package
//! says its whole state then.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_DEVICE_TABS);

const CONTROL_FROM_0_1_65_DEV_DEVICE_TABS: &str = "
UPDATE conversation_work SET
  document = json_set(
    document,
    '$.tabs', (
      SELECT json_group_array(json(tab.value))
      FROM (
        SELECT value FROM json_each(document, '$.tabs')
        WHERE json_extract(value, '$.kind') NOT IN ('browser', 'mobile')
        ORDER BY key
      ) AS tab
    )
  ),
  revision = revision + 1
WHERE EXISTS (
  SELECT 1 FROM json_each(document, '$.tabs') AS tab
  WHERE json_extract(tab.value, '$.kind') IN ('browser', 'mobile')
);
";
