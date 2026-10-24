-- Author: jaswtao · Date: 2026-10-24
CREATE TABLE t_editor_draft (
    project_id TEXT NOT NULL REFERENCES t_project(id) ON DELETE CASCADE,
    path TEXT NOT NULL COLLATE NOCASE,
    project_path TEXT NOT NULL,
    content TEXT NOT NULL CHECK(length(CAST(content AS BLOB)) <= 5242880),
    baseline TEXT NOT NULL,
    encoding TEXT NOT NULL CHECK(encoding IN ('utf-8', 'gbk')),
    bom INTEGER NOT NULL,
    line_ending TEXT NOT NULL CHECK(line_ending IN ('LF', 'CRLF')),
    revision TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY(project_id, path)
);
