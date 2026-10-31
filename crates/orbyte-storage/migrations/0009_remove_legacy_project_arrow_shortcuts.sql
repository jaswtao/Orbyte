-- Author: jaswtao · Date: 2026-11-01
-- Left/right cycling now belongs to remote connection tabs; drop the removed project commands.
UPDATE t_app_setting
SET value_json = json_remove(value_json, '$.projectPrevious', '$.projectNext')
WHERE key = 'shortcuts'
  AND (
      json_type(value_json, '$.projectPrevious') IS NOT NULL
      OR json_type(value_json, '$.projectNext') IS NOT NULL
  );
