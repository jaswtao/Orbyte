-- Author: jaswtao · Date: 2026-10-27
ALTER TABLE t_project ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1));
