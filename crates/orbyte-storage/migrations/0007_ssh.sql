-- Author: jaswtao
-- Date: 2026-10-31
CREATE TABLE t_ssh_connection (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    username TEXT NOT NULL,
    auth_type TEXT NOT NULL CHECK (auth_type IN ('password', 'privateKey')),
    private_key_path TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (
        (auth_type = 'password' AND private_key_path IS NULL)
        OR (auth_type = 'privateKey' AND private_key_path IS NOT NULL AND length(private_key_path) > 0)
    )
);
CREATE INDEX idx_ssh_connection_order ON t_ssh_connection(sort_order, id);
CREATE TABLE t_ssh_known_host (
    id TEXT PRIMARY KEY,
    host TEXT NOT NULL,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    algorithm TEXT NOT NULL,
    public_key BLOB NOT NULL,
    fingerprint_sha256 TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (host, port, algorithm)
);
CREATE TABLE t_remote_workspace (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL UNIQUE REFERENCES t_ssh_connection(id) ON DELETE CASCADE,
    schema_version INTEGER NOT NULL,
    layout_json TEXT NOT NULL,
    last_directory TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE t_remote_terminal_config (
    id TEXT PRIMARY KEY,
    remote_workspace_id TEXT NOT NULL REFERENCES t_remote_workspace(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_remote_terminal_workspace ON t_remote_terminal_config(remote_workspace_id, sort_order, id);
