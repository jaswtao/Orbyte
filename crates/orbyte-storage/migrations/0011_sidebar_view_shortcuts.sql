-- Author: jaswtao · Date: 2026-11-01
-- Ctrl+Shift+Left/Right now switches sidebar views. Drop stored remote tab bindings
-- that still use those keys, keeping any other bindings the user configured.
UPDATE t_app_setting
SET value_json = CASE
    WHEN json_array_length(value_json, '$.remotePrevious') = 1
        THEN json_remove(value_json, '$.remotePrevious')
    ELSE json_set(value_json, '$.remotePrevious', (
        SELECT json_group_array(json(binding.value) ORDER BY binding.key)
        FROM json_each(t_app_setting.value_json, '$.remotePrevious') AS binding
        WHERE NOT (
            json_extract(binding.value, '$.code') = 'ArrowLeft'
            AND json_extract(binding.value, '$.ctrlKey') = 1
            AND json_extract(binding.value, '$.altKey') = 0
            AND json_extract(binding.value, '$.shiftKey') = 1
            AND json_extract(binding.value, '$.metaKey') = 0
        )
    ))
END
WHERE key = 'shortcuts'
  AND json_type(value_json, '$.remotePrevious') = 'array'
  AND EXISTS (
      SELECT 1 FROM json_each(t_app_setting.value_json, '$.remotePrevious') AS binding
      WHERE json_extract(binding.value, '$.code') = 'ArrowLeft'
        AND json_extract(binding.value, '$.ctrlKey') = 1
        AND json_extract(binding.value, '$.altKey') = 0
        AND json_extract(binding.value, '$.shiftKey') = 1
        AND json_extract(binding.value, '$.metaKey') = 0
  );

UPDATE t_app_setting
SET value_json = CASE
    WHEN json_array_length(value_json, '$.remoteNext') = 1
        THEN json_remove(value_json, '$.remoteNext')
    ELSE json_set(value_json, '$.remoteNext', (
        SELECT json_group_array(json(binding.value) ORDER BY binding.key)
        FROM json_each(t_app_setting.value_json, '$.remoteNext') AS binding
        WHERE NOT (
            json_extract(binding.value, '$.code') = 'ArrowRight'
            AND json_extract(binding.value, '$.ctrlKey') = 1
            AND json_extract(binding.value, '$.altKey') = 0
            AND json_extract(binding.value, '$.shiftKey') = 1
            AND json_extract(binding.value, '$.metaKey') = 0
        )
    ))
END
WHERE key = 'shortcuts'
  AND json_type(value_json, '$.remoteNext') = 'array'
  AND EXISTS (
      SELECT 1 FROM json_each(t_app_setting.value_json, '$.remoteNext') AS binding
      WHERE json_extract(binding.value, '$.code') = 'ArrowRight'
        AND json_extract(binding.value, '$.ctrlKey') = 1
        AND json_extract(binding.value, '$.altKey') = 0
        AND json_extract(binding.value, '$.shiftKey') = 1
        AND json_extract(binding.value, '$.metaKey') = 0
  );
