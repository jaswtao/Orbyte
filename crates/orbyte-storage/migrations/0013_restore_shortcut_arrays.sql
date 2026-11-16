-- Author: jaswtao · Date: 2026-11-16
-- Restore JSON subtypes lost across the scalar subqueries in migration 0011.
-- Leave the published migration and its checksum unchanged.

UPDATE t_app_setting
SET value_json = json_set(value_json, '$.remotePrevious', json(json_extract(value_json, '$.remotePrevious')))
WHERE key = 'shortcuts'
  AND json_type(value_json, '$.remotePrevious') = 'text'
  AND CASE WHEN json_valid(json_extract(value_json, '$.remotePrevious'))
      THEN json_type(json_extract(value_json, '$.remotePrevious')) = 'array'
      ELSE 0 END;

UPDATE t_app_setting
SET value_json = json_set(value_json, '$.remoteNext', json(json_extract(value_json, '$.remoteNext')))
WHERE key = 'shortcuts'
  AND json_type(value_json, '$.remoteNext') = 'text'
  AND CASE WHEN json_valid(json_extract(value_json, '$.remoteNext'))
      THEN json_type(json_extract(value_json, '$.remoteNext')) = 'array'
      ELSE 0 END;
