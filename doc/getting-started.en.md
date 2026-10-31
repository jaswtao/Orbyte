# Quick start

Author: jaswtao · Date: 2026-10-22

[简体中文](getting-started.md) · [Back to README](../README.en.md)

## Install

Orbyte currently supports Windows x64 only. Download the [v0.1.0 installer](https://github.com/jaswtao/Orbyte/releases/download/v0.1.0/Orbyte_0.1.0_x64-setup.exe) and [SHA256SUMS.txt](https://github.com/jaswtao/Orbyte/releases/download/v0.1.0/SHA256SUMS.txt). In your download directory, run:

```powershell
Get-FileHash .\Orbyte_0.1.0_x64-setup.exe -Algorithm SHA256
```

Compare the result with the installer's entry in the checksum file before installing. The installer is unsigned and Windows may display a trust warning. Matching hashes verify file consistency, not publisher identity. WebView2 Runtime is required; the installer offers setup if it is missing, which may download Microsoft components.

Rust, Node.js and Visual Studio are not required to use the installer. Install Git, WSL, Codex or Claude Code separately if needed; Orbyte does not bundle them.

## Your first workspace

1. Open Orbyte and select **Add project**. Choose an existing local directory, confirm its name and save. Use a local path such as `C:\projects\demo`; UNC and WSL network directories are unsupported.
2. Select **New terminal**, or use the default `Ctrl+Shift+T` shortcut. Run `Get-Location` to check the current directory.
3. Use `Ctrl+Shift+H` to split left/right, or `Ctrl+Shift+J` to split top/bottom. Run commands in separate panes.
4. Use `Ctrl+P` to find a file by name while respecting ignore rules, or `Ctrl+Shift+P` to include ignored files. You can also open files from the file tree. Save edits with `Ctrl+S`. If you have customized shortcuts, check **Settings → Keyboard shortcuts** for your bindings.
5. Add a second project, switch to it, then return. The original project's terminal sessions remain available while the app and terminals stay open.

## Connect to a remote server

1. Switch the sidebar to **Remote connections**, create a connection with a name, host, port and username, and choose password or private-key authentication. Password authentication accepts an optional password to save with the connection; edit the connection to replace or clear it. Pick an OpenSSH Ed25519 or RSA key (encrypted keys included) in the native file picker.
2. On the first connection with a host-key algorithm, verify the host, port, algorithm and SHA-256 fingerprint, then trust it once, trust and save, or cancel. Algorithms are stored separately per endpoint; a changed key for the same algorithm is rejected.
3. Open a remote workspace to create terminal tabs or splits and use the SFTP file panel. Without a saved password, one connection action asks once for a password or private-key passphrase and shares it across the pending terminals and SFTP. The password prompt offers **Save password**, unchecked by default. Saved passwords are reused for subsequent manual connections, new terminals and SFTP; startup still does not connect automatically. Passwords use Windows Credential Manager; private-key passphrases are not saved. Temporary batch copies are cleared after authentication finishes. Saving does not depend on authentication succeeding; edit or clear an incorrect password before reconnecting. Changing the host, port, username or authentication method clears the previous password unless a replacement is supplied.
4. The file panel can refresh, navigate into and out of directories, create directories, and upload or download files and directories. Conflicting targets require overwrite, skip or cancel; the transfer list shows progress, speed and results.
5. A server that only offers keyboard-interactive fails explicitly without sending the password. The screen stays visible after a disconnect and reconnection is manual; three unanswered keepalive probes (about 90 seconds) mark the connection failed.

## Launch an AI coding CLI

Install Codex or Claude Code yourself and make sure its command is available on PowerShell's PATH before opening Orbyte. Select the corresponding AI CLI from the tab bar's `+` menu. Each CLI uses its own account, authentication and service; Orbyte does not provide a model service or bundle the CLIs.

When a CLI exits, the pane remains a shell. Restarting that CLI pane launches the CLI again.

## Use Git worktrees

With Git installed and a Git repository selected, the sidebar can show Git status. Right-click the project and select **Worktree…** to add an existing worktree, or create one with a branch name and a local absolute target path that does not exist yet.

Removing a project from Orbyte only removes workspace configuration. To remove its Git worktree, use `git worktree remove` in a terminal.

## Before you close the app

Switching projects keeps the current sessions alive. Exiting the app stops managed Windows terminal child processes. Restart restores projects, layouts and file paths, but not running processes or terminal output. Restore, copy or discard local drafts from the editor prompt or the recovery button at the bottom of the workspace. Recovery never writes to the source file automatically. Drafts flush after 750 ms idle or every 5 seconds of continuous input, plus storage time; forced exits may still lose recent input. Discarding changes clears the corresponding draft; cancelling keeps it. Detached WSL processes and Linux services have different lifetime limits.

## Get help

- CLI not found: check its command in a standalone PowerShell window; reopen Orbyte after installing tools or changing PATH.
- WSL cannot start: confirm a WSL distribution is installed and starts independently, or try PowerShell or CMD first.
- Report bugs through [Issues](https://github.com/jaswtao/Orbyte/issues/new/choose), including the app version, Windows version, shell and reproduction steps. Remove private paths, tokens and project content from screenshots and logs.
- Report vulnerabilities through [private security reporting](https://github.com/jaswtao/Orbyte/security/advisories/new).

See the [README](../README.en.md) for all limitations and local data locations.

> Draft recovery, index optimizations and SSH/SFTP in this branch still require installed desktop acceptance. The public download does not imply these capabilities are included. See [SSH protocol probe](ssh-probe.md) for automated protocol results and [validation records](next-stage-validation.md) for other runs.
