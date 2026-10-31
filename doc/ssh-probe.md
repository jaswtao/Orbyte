# SSH 协议探针

Author: jaswtao · Date: 2026-10-31

本目录提供隔离的 OpenSSH 夹具，用于运行 `crates/orbyte-ssh/tests/protocol_probe.rs` 中被 `#[ignore]` 的真实协议检查。普通 `cargo test` 不需要服务器，也不会启动这些检查。

## 前置条件

- Docker Desktop（Windows 容器模式不适用，需 Linux 容器）。
- OpenSSH 客户端工具 `ssh-keygen`。
- PowerShell 7（`pwsh`）。
- 已安装项目依赖，工作目录为仓库根目录。

夹具只绑定本机端口，客户端密钥和容器主机密钥都是临时测试材料，不得用于任何真实服务器。完成后执行 `pwsh scripts/ssh-probe/setup.ps1 -Down` 停止容器。

## 启动夹具

```powershell
pwsh scripts/ssh-probe/setup.ps1
```

脚本会：

1. 在 `scripts/ssh-probe/keys/` 生成 Ed25519、RSA（PEM）和加密 Ed25519 三把客户端密钥。
2. 将公钥写入 `scripts/ssh-probe/fixtures/authorized_keys`。
3. 构建并启动三个 Linux 容器：

| 服务 | 端口 | 配置 | 用途 |
| --- | --- | --- | --- |
| normal | 2222 | 密码 + 公钥 + SFTP，Ed25519/RSA 主机密钥 | 密码、私钥、PTY、SFTP、传输 |
| weak | 2223 | 仅 `ssh-rsa` 主机密钥、SHA-1 KEX/MAC | 弱算法拒绝 |
| keyboard | 2224 | 仅 keyboard-interactive（PAM） | 首期不支持的认证方式 |

夹具基于 Alpine 3.20 与 OpenSSH 9.7。keyboard 服务使用 `openssh-server-pam` 提供的 PAM 版 `sshd`，普通 `sshd` 在该镜像中不支持 `UsePAM`。

容器启动时用 `chpasswd` 设置用户 `orbyte` 的密码。默认测试口令为 `orbyte-probe`，可用 `-Password` 覆盖；加密私钥口令默认 `orbyte-key-passphrase`，可用 `-KeyPassphrase` 覆盖。

生成物均在 `.gitignore` 中，不会进入提交。

## 环境变量

脚本结束时会打印可直接粘贴到当前 PowerShell 的变量。完整列表：

| 变量 | 必需 | 说明 |
| --- | --- | --- |
| `ORBYTE_SSH_PROBE` | 是 | 必须为 `1`；缺少时探针主动失败，避免忽略测试空跑通过。 |
| `ORBYTE_SSH_HOST` | 是 | 默认 `127.0.0.1`。 |
| `ORBYTE_SSH_PORT` | 建议 | normal 端口，默认 2222。 |
| `ORBYTE_SSH_USER` | 否 | 默认 `orbyte`。 |
| `ORBYTE_SSH_PASSWORD` | 是 | normal 与 keyboard 服务的测试口令。 |
| `ORBYTE_SSH_ED25519_KEY` | 私钥检查 | 未加密 Ed25519 私钥路径。 |
| `ORBYTE_SSH_RSA_KEY` | 私钥检查 | 未加密 RSA PEM 私钥路径。 |
| `ORBYTE_SSH_ENCRYPTED_KEY` | 私钥检查 | 加密 Ed25519 私钥路径。 |
| `ORBYTE_SSH_KEY_PASSPHRASE` | 私钥检查 | 加密私钥口令。 |
| `ORBYTE_SSH_WEAK_PORT` | 弱算法检查 | 默认 2223。 |
| `ORBYTE_SSH_KBD_PORT` | keyboard 检查 | 默认 2224。 |
| `ORBYTE_SSH_TRANSFER_1GIB` | 否 | 设为 `1` 时额外执行 1 GiB 上传/下载与 SHA-256 比对。耗时和磁盘占用较大。 |

## 运行探针

```powershell
cargo test -p orbyte-ssh --features probe --test protocol_probe -- --ignored --test-threads=1 --nocapture
```

Windows 上还可验证系统凭据存储及真实连接命令：

```powershell
cargo test -p orbyte-platform credentials::tests::system_password_roundtrip -- --ignored
cargo test -p orbyte-desktop --test remote_commands saved_system_password_authenticates_terminals_and_sftp_without_frontend_credentials -- --ignored
```

第二项使用上述 `ORBYTE_SSH_PROBE` 与 `ORBYTE_SSH_PASSWORD`，连接本机默认 2222 端口的夹具。它将密码保存至临时连接的 Windows 凭据条目，验证首个终端认证完成后，同批次后续终端和 SFTP 仍无需前端输入密码；结束时删除测试凭据并关闭会话。普通测试不写入系统凭据管理器。

`--features probe` 只启用测试用的隐藏入口，不改变应用行为。`--test-threads=1` 避免多个检查同时挤占夹具和网络。

发布检查可显式启用：

```powershell
pwsh scripts/release-check.ps1 -Gitleaks <gitleaks.exe> -SshProbe
```

启用后若缺少夹具或环境变量，检查失败。未启用时发布检查不会运行真实服务器场景，不能用普通 `cargo test` 的结果替代。

## 检查内容

| 测试函数 | 验证点 |
| --- | --- |
| `protocol_probe` | 密码认证成功。 |
| `private_key_probe` | Ed25519、RSA（PEM）及加密 Ed25519 私钥认证；失败信息不包含口令。 |
| `saved_host_key_algorithms_are_preferred` | 已保存算法优先协商，在同一服务器上分别得到 `ssh-ed25519` 与 `rsa-sha2-*`。 |
| `keyboard_interactive_only_is_refused_before_sending_the_secret` | 仅提供 keyboard-interactive 的服务器返回明确错误，不把口令送进交互问答。 |
| `weak_algorithms_are_refused` | 仅提供 SHA-1 KEX/MAC 或 `ssh-rsa` 的服务器不能建立连接。 |
| `pty_resize_probe` | PTY 初始尺寸、Shell 启动和 resize 后 `stty size` 返回新行列数。 |
| `sftp_lstat_rename_and_symlink_probe` | `lstat` 类型、中文/空格文件名、rename、符号链接不跟随。 |
| `transfer_probe` | 空文件、空目录、多层目录、中文/空格名称上传与下载；覆盖、跳过、取消；符号链接跳过；可选 1 GiB SHA-256 往返。 |

## 2026-10-31 实测结果

环境：Windows x64 构建机，Alpine 3.20.10 + OpenSSH_9.7p1 夹具，`ssh2` 0.9.6 使用 OpenSSL 后端。命令：

```powershell
cargo test -p orbyte-ssh --features probe --test protocol_probe -- --ignored --test-threads=1 --nocapture
```

8 项全部通过：密码；Ed25519／RSA（PEM）／加密 Ed25519 私钥；按算法主机密钥优先（`rsa-sha2-256` 与 `ssh-ed25519`）；keyboard-interactive 拒绝；弱算法拒绝；PTY resize（43×132）；SFTP `lstat`／rename／symlink；双向传输（空文件、空目录、多层目录、中文／空格名称、覆盖、跳过、取消、符号链接跳过）。

尚未执行：1 GiB 往返与内存／回显采样、保活 90 秒实测、只读目标失败路径、安装版交互。它们仍列在[发布检查表](release-checklist.md)中。

## 未自动化或需人工记录的场景

以下项目依赖服务器或安装环境，探针只覆盖可稳定重复的部分；结果仍需写入发布验收记录：

- 保活失效：对端完全不应答时约 90 秒内标记失效。可在探针运行中暂停 normal 容器（`docker pause orbyte-ssh-probe-normal-1`）观察应用行为；纯逻辑由 `keepalive.rs` 单元测试覆盖。
- 1 GiB 传输期间 worker 额外内存不超过 64 MiB、终端回显 p95 不高于 200 ms：需在安装版中采样，记录服务器和网络条件。
- Windows 安装环境未安装 PuTTY/OpenSSH 客户端时仍可连接。
- 远程无法安全替换时保留原文件：需使用不支持安全 rename 的服务器或只读目录构造，探针对正常服务器只验证成功路径。
- 安装版主机指纹挑战、取消过期、断线保留屏幕与手动重连等交互。

## 结果记录

在 `doc/release-checklist.md` 的 SSH 验收项中填写：

```text
日期：
提交：
夹具版本：
Windows 版本 / WebView2：
normal / weak / keyboard 容器状态：
执行的命令：
通过项：
失败项及原因：
未执行项：
```

未执行或只通过 mock 的项目不得标记为通过。
