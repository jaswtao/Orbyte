# Changelog

## Unreleased

- Persist local editor recovery drafts with revision-conditional cleanup, rename migration, startup discovery and editor restore/copy/discard actions.
- Reuse filename indexes for content edits, Git internals and atomic saves; preserve structural and ignore-rule invalidation.
- Add repeatable 10k/90k file benchmarks and installed desktop acceptance records.
- Update Tokio to 1.53.3 and clipboard-win to 5.4.3; handle advisory-free yanked warnings in release auditing.
- Installed desktop interaction acceptance is pending; these changes are not a published release.

## 0.1.0 — first public preview (draft)

- Windows local development workbench with grouped projects, terminal tabs and split panes.
- PowerShell, CMD and WSL terminals; optional Codex and Claude Code launch presets.
- Text editing, Git status and worktree integration, configurable shortcuts, Chinese and English UI.
- MIT-licensed first-party source and bundled third-party notices.

This is a prerelease. The Windows installer is unsigned. Native installation and desktop acceptance must be completed using [the release checklist](doc/release-checklist.md) before publication.
