# Orbyte

[简体中文](README.md) · [Download for Windows](https://github.com/jaswtao/Orbyte/releases/download/v0.1.0/Orbyte_0.1.0_x64-setup.exe) · [Quick start](doc/getting-started.en.md) · [Releases](https://github.com/jaswtao/Orbyte/releases) · [Contributing](CONTRIBUTING.md)

**Keep your projects, terminals and AI coding CLIs in one Windows workspace.**

Orbyte is an open-source local development workbench. Organize terminals and files by project, switch between tabs and split panes, and launch your installed Codex or Claude Code CLI.

- **Keep each project's work at hand.** Terminals keep running when you switch projects or tabs; unsaved edits remain available during the current app session.
- **Put commands beside files.** Run PowerShell, CMD or an installed WSL distribution alongside a text editor.
- **Separate tasks with worktrees.** Check Git status, create a branch worktree or add an existing worktree to a project group.

Chinese and English interfaces. Built with Tauri 2, Vue 3, Rust and CodeMirror; first-party code is MIT-licensed.

![Orbyte desktop](doc/images/workspace.png)

## Status and installation

The first public preview, **v0.1.0, is available for Windows x64**. Using the installer does not require Rust, Node.js or build tools.

1. Download [Orbyte_0.1.0_x64-setup.exe](https://github.com/jaswtao/Orbyte/releases/download/v0.1.0/Orbyte_0.1.0_x64-setup.exe) and [SHA256SUMS.txt](https://github.com/jaswtao/Orbyte/releases/download/v0.1.0/SHA256SUMS.txt).
2. Run the command below and compare the hash with the installer's entry in the checksum file before installing.
3. Open Orbyte, add a local project directory and select **New terminal**. Follow the [quick start](doc/getting-started.en.md) for a first walkthrough.

The installer is unsigned and Windows may display a trust warning.

```powershell
Get-FileHash .\Orbyte_0.1.0_x64-setup.exe -Algorithm SHA256
```

PowerShell and CMD are supported. WSL requires an installed distribution. Git, Codex and Claude Code are optional tools discovered from your environment; Orbyte does not include those CLIs.

## Build from source

Install Visual Studio C++ Build Tools with the Windows SDK, Rust MSVC, Microsoft Edge WebView2 Runtime, Node.js 24.15.0, pnpm 12.8.1 and Python 3.13. Rust 1.95.0 is pinned by rust-toolchain.toml; the minimum supported Rust version is 1.90.

```powershell
git clone https://github.com/jaswtao/Orbyte.git
cd Orbyte
pnpm install --frozen-lockfile
pnpm dev
```

Development uses its own application identifier and data directory. To verify and package:

```powershell
cargo install cargo-audit --version 0.22.2 --locked
$scanner = & ./scripts/install-gitleaks.ps1
./scripts/release-check.ps1 -Gitleaks $scanner
pnpm build
```

The unsigned NSIS installer is written to target/release/bundle/nsis/ by default. To produce source archives, third-party MPL sources and checksums from a clean reviewed commit, create a tag pointing to that commit (for example, `git tag v0.1.0`), then run `./scripts/prepare-release.ps1 -Tag v0.1.0`. Packaging resolves the actual Cargo target directory, including `CARGO_TARGET_DIR`, and requires a newly built installer for the requested version. Tag releases must pass the full CI workflow, including commit history scanning and the minimum Rust version check, before packaging a draft prerelease.

## Features and limitations

- Grouped projects, favorites, terminal tabs and nested splits; terminals keep running when projects change.
- PowerShell/CMD shell integration, background activity indicators and configurable notifications.
- UTF-8 and GBK editing, disk conflict detection, recycle-bin deletion and Git status/worktree helpers.
- Chinese/English interface, light/dark themes and configurable shortcuts. Ctrl+F4 closes a tab; Ctrl+Shift+W closes a pane.
- Restart restores projects, layouts and file paths, but does not preserve unsaved edits, processes or terminal output.
- Clipboard screenshots remain in the application cache until manually removed.
- Windows managed child processes stop with their terminal. WSL detached processes and services have different lifetime limits.
- No UNC/WSL network project directories. The text editor has a 5 MiB file limit.
- Installation, native notifications and desktop interactions require the checks in [the release checklist](doc/release-checklist.md). Automated tests do not certify these scenarios.

## Local data and privacy

The release stores its database and shell integration scripts under `%APPDATA%\com.orbyte.desktop\`; clipboard images are under `%LOCALAPPDATA%\com.orbyte.desktop\clipboard-images\`. Development uses the `com.orbyte.desktop.dev` identifier. Window state may also be stored in the app's local configuration directory. Back up data before deleting application directories; do not assume uninstall removes them.

Orbyte itself has no configured telemetry or application update service. Shell commands, Git and third-party CLIs may access the network according to their own behavior. WebView2 installation may download Microsoft runtime components. Terminal exports and screenshots can contain sensitive data.

Application diagnostics are JSON Lines files at `logs/orbyte-<UTC-day-number>-<part>.log` under the application data directory. Records include Unix millisecond timestamps, severity, process/module, startup/shutdown, database errors, terminal state, frontend failures and command outcomes/durations with request and project/session IDs. Terminal text, keyboard input, file contents, clipboard contents and setting values are excluded. Files rotate by UTC day and at 2 MiB. Retention is limited to the most recent 30 days, with cleanup on startup, day changes and hourly while running; legacy `terminal.log*` files expire by modification time. While the app is closed, cleanup waits until the next launch. Cleanup failures are reported and retried.

## License

First-party project content is licensed under [MIT](LICENSE), Copyright (c) 2026 jaswtao. Existing author comments are retained.

Third-party code retains its original licenses. See [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) and [THIRD_PARTY_LICENSES.json](THIRD_PARTY_LICENSES.json). Matching releases include third-party-mpl-sources.zip with MPL sources.

[Security reporting](SECURITY.md) · [Project structure](doc/project-structure.en.md)
