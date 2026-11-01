-- Author: jaswtao
-- Date: 2026-11-01
ALTER TABLE t_remote_project ADD COLUMN layout_json TEXT NOT NULL
    DEFAULT '{"schemaVersion":1,"activeTabId":null,"tabs":[]}';

-- Connection workspaces remain transport contexts; projects now own the visible layout.
WITH ordered_tabs AS (
    SELECT w.project_id,
           json_set(tab.value, '$.id', w.id || ':' || json_extract(tab.value, '$.id')) AS tab_json
    FROM t_remote_workspace w,
         json_each(CASE WHEN json_valid(w.layout_json) THEN w.layout_json ELSE '{}' END, '$.tabs') tab
    WHERE w.project_id IS NOT NULL
    ORDER BY w.created_at, w.id, CAST(tab.key AS INTEGER)
), merged AS (
    SELECT project_id, json_group_array(json(tab_json)) AS tabs FROM ordered_tabs GROUP BY project_id
)
UPDATE t_remote_project SET layout_json = (
    SELECT json_object('schemaVersion', 1, 'activeTabId', json_extract(tabs, '$[0].id'), 'tabs', json(tabs))
    FROM merged WHERE merged.project_id = t_remote_project.id
) WHERE id IN (SELECT project_id FROM merged);

-- Duplicate contexts for a profile share the most recently browsed directory.
UPDATE t_remote_workspace AS target SET last_directory = (
    SELECT source.last_directory FROM t_remote_workspace source
    WHERE source.project_id = target.project_id AND source.connection_id = target.connection_id
    ORDER BY source.updated_at DESC, source.created_at DESC, source.id DESC LIMIT 1
) WHERE project_id IS NOT NULL;
