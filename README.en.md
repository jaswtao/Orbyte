# Orbyte

[简体中文](README.md) · [Releases](https://github.com/jaswtao/Orbyte/releases) · [Contributing](CONTRIBUTING.md)

Orbyte is a local Windows development workbench built with Tauri 2, Vue 3 and Rust. Organize projects, keep multiple terminals running across tabs and split panes, and edit text alongside terminal sessions.

![Orbyte desktop](doc/images/workspace.png)

## Status and installation

The first public preview is 0.1.0 for Windows x64. Download an installer only from the repository's Releases page once a release is published. Preview installers are unsigned and Windows may display a trust warning. Compare the downloaded file's SHA-256 hash with the release's SHA256SUMS.txt.

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

Terminal diagnostics are stored in `logs/terminal.log` under the application data directory. Logs contain session state and output transport metadata, excluding terminal text, keyboard input and clipboard content. Files rotate at 2 MiB, keeping at most three files.

## License

First-party project content is licensed under [MIT](LICENSE), Copyright (c) 2026 jaswtao. Existing author comments are retained.

Third-party code retains its original licenses. See [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) and [THIRD_PARTY_LICENSES.json](THIRD_PARTY_LICENSES.json). Matching releases include third-party-mpl-sources.zip with MPL sources.

[Security reporting](SECURITY.md) · [Project structure](doc/project-structure.en.md)
