-- Author: jaswtao
-- Date: 2026-10-12
CREATE TABLE t_project_group (
    id TEXT PRIMARY KEY,
    parent_id TEXT CHECK (parent_id IS NULL),
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE t_project (
    id TEXT PRIMARY KEY,
    group_id TEXT REFERENCES t_project_group(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE COLLATE NOCASE,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_project_group ON t_project(group_id);
CREATE INDEX idx_project_order ON t_project(sort_order);
CREATE TABLE t_workspace (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL UNIQUE REFERENCES t_project(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '默认工作区',
    is_default INTEGER NOT NULL DEFAULT 1 CHECK (is_default = 1),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE t_terminal_config (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES t_workspace(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    shell_type TEXT NOT NULL CHECK (shell_type IN ('powershell', 'cmd', 'wsl')),
    cwd TEXT NOT NULL DEFAULT '',
    args_json TEXT NOT NULL DEFAULT '[]' CHECK (args_json = '[]'),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_terminal_workspace ON t_terminal_config(workspace_id);
CREATE TABLE t_workspace_layout (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL UNIQUE REFERENCES t_workspace(id) ON DELETE CASCADE,
    schema_version INTEGER NOT NULL,
    layout_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE t_app_setting (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);
