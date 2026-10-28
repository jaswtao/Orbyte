-- Author: jaswtao · Date: 2026-10-28
-- Preserve existing Ctrl+P bindings when introducing the openFile default.
UPDATE t_app_setting
SET value_json = json_set(value_json, '$.openFile', json('[]'))
WHERE key = 'shortcuts'
  AND json_type(value_json, '$.openFile') IS NULL
  AND EXISTS (
      SELECT 1
      FROM json_each(value_json) AS command,
           json_each(command.value) AS binding
      WHERE json_extract(binding.value, '$.code') = 'KeyP'
        AND json_extract(binding.value, '$.ctrlKey') = 1
        AND json_extract(binding.value, '$.altKey') = 0
        AND json_extract(binding.value, '$.shiftKey') = 0
        AND json_extract(binding.value, '$.metaKey') = 0
  );
