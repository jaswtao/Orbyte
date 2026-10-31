-- Author: jaswtao
-- Date: 2026-10-31
CREATE TABLE t_remote_project (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);
INSERT INTO t_remote_project(id, name)
SELECT 'legacy', 'Remote' WHERE EXISTS (SELECT 1 FROM t_remote_workspace);

-- Preserve existing layouts and terminal IDs while removing the one-workspace-per-connection constraint.
CREATE TABLE t_remote_workspace_new (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES t_ssh_connection(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES t_remote_project(id) ON DELETE CASCADE,
    schema_version INTEGER NOT NULL,
    layout_json TEXT NOT NULL,
    last_directory TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO t_remote_workspace_new
SELECT id, connection_id, 'legacy', schema_version, layout_json, last_directory, created_at, updated_at
FROM t_remote_workspace;
CREATE TEMP TABLE remote_terminal_backup AS SELECT * FROM t_remote_terminal_config;
DROP TABLE t_remote_terminal_config;
DROP TABLE t_remote_workspace;
ALTER TABLE t_remote_workspace_new RENAME TO t_remote_workspace;
CREATE INDEX idx_remote_workspace_project ON t_remote_workspace(project_id, created_at, id);
CREATE INDEX idx_remote_workspace_connection ON t_remote_workspace(connection_id);
CREATE TABLE t_remote_terminal_config (
    id TEXT PRIMARY KEY,
    remote_workspace_id TEXT NOT NULL REFERENCES t_remote_workspace(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO t_remote_terminal_config SELECT * FROM remote_terminal_backup;
DROP TABLE remote_terminal_backup;
CREATE INDEX idx_remote_terminal_workspace ON t_remote_terminal_config(remote_workspace_id, sort_order, id);
