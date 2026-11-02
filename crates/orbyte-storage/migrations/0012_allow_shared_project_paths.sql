-- Author: jaswtao
-- Date: 2026-11-02
CREATE TABLE t_project_new (
    id TEXT PRIMARY KEY,
    group_id TEXT REFERENCES t_project_group(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    path TEXT NOT NULL COLLATE NOCASE,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    hidden INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1))
);
INSERT INTO t_project_new SELECT id, group_id, name, path, favorite, sort_order, created_at, updated_at, hidden FROM t_project;
DROP TABLE t_project;
ALTER TABLE t_project_new RENAME TO t_project;
CREATE INDEX idx_project_group ON t_project(group_id);
CREATE INDEX idx_project_order ON t_project(sort_order);
CREATE TEMP TABLE project_migration_foreign_key_check (violations INTEGER CHECK (violations = 0));
INSERT INTO project_migration_foreign_key_check SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE project_migration_foreign_key_check;
