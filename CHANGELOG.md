# Changelog

## Unreleased

## 0.3.0 — 2026-10-29

- Show the installed Orbyte version in Settings → About Orbyte and view bundled release highlights offline in Chinese or English.
- Add project hiding and a setting to show hidden projects; keep projects with recovered drafts visible.
- Add separate file search modes: Ctrl+P respects ignore rules, while Ctrl+Shift+P includes ignored files. Preserve existing custom shortcuts during upgrades.
- Add configurable workspace highlight border color and intensity, with preview and reset controls.
- Arrange sidebar project filters in a wrapping row and reveal the active project during vertical shortcut navigation.
- Restore terminal focus on window activation and initial load.
- Trim trailing whitespace from terminal line selections and preserve drag anchors after scrollback trimming.
- Restore terminal cursor blinking when reduced motion is enabled and remove the highlight border from active sidebar view buttons.
- Native installation and desktop acceptance remain incomplete; see the [release notes](doc/releases/v0.3.0.md) for verification limits and known limitations.

## 0.2.0 — 2026-10-26

- Persist local editor recovery drafts with revision-conditional cleanup, rename migration, startup discovery and editor restore/copy/discard actions.
- Access draft recovery from the workspace menu, skip recovery, refresh the list on reopen, and recover drafts even when workspace loading fails.
- Reuse filename indexes for content edits, Git internals and atomic saves; preserve structural and ignore-rule invalidation.
- Add structured JSON Lines diagnostics for application lifecycle, database, terminal, frontend errors and command outcomes, with daily/2 MiB rotation and 30-day retention.
- Fix terminal frame decoding when IPC falls back to JSON byte arrays, including reconnect and background output recovery.
- Fix terminal IME candidate positioning for RGB visual cursors, wide characters and long compositions.
- Fix text clipboard paste in native Windows Codex sessions by delivering the native Ctrl+V shortcut only to sessions sharing the console host.
- Fix terminal unread indicators across redraws and reconnects while preserving the last-read content baseline.
- Suppress Windows system-menu activation through Alt, F10 and Alt+Space.
- Add Chinese and English quick-start guides and improve onboarding and download links.
- Add repeatable 10k/90k file benchmarks and installed desktop acceptance records.
- Update Tokio to 1.53.3 and clipboard-win to 5.4.3; handle advisory-free yanked warnings in release auditing.
- Installed desktop interaction acceptance remains incomplete; see the [release notes](doc/releases/v0.2.0.md) for verification limits and known limitations.

## 0.1.0 — first public preview

- Windows local development workbench with grouped projects, terminal tabs and split panes.
- PowerShell, CMD and WSL terminals; optional Codex and Claude Code launch presets.
- Text editing, Git status and worktree integration, configurable shortcuts, Chinese and English UI.
- MIT-licensed first-party source and bundled third-party notices.

This is a prerelease. The Windows installer is unsigned. Native installation and desktop acceptance must be completed using [the release checklist](doc/release-checklist.md) before publication.
