-- Author: jaswtao · Date: 2026-10-14
ALTER TABLE t_terminal_config ADD COLUMN launch_preset TEXT
    CHECK (launch_preset IS NULL OR launch_preset IN ('codex', 'claude-code'));
