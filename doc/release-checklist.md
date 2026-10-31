# 发布检查表

Author: jaswtao · Date: 2026-10-20

首次版本为 v0.1.0，发布为 GitHub prerelease。以下项目需逐项填写结果；未执行项目不得标记通过。

## 自动检查

- [ ] 从独立目录执行 `pnpm install --frozen-lockfile`。
- [ ] `python scripts/check-release.py --tag v0.1.0`。
- [ ] 维护目标版本的 `doc/releases/v<version>.json`，核对发布日期、中英文功能改进与修复摘要；应用会打包该文件供离线查看。允许某个分类为空，但每种语言至少保留一条更新内容。
- [ ] `cargo +1.90.0 check --workspace --all-targets --locked` 验证最低 Rust 版本。
- [ ] `pwsh scripts/release-check.ps1 -Gitleaks <gitleaks.exe>`。
- [ ] 启动隔离 OpenSSH 夹具后执行 `pwsh scripts/release-check.ps1 -Gitleaks <gitleaks.exe> -SshProbe`；8 项协议探针通过，并将夹具版本、服务器和结果写入 SSH 验收记录。未启动夹具时不得用普通 `cargo test` 替代。
- [ ] 确认 RSA 审计豁免对应依赖未进入 Windows 实际构建；新漏洞和警告须重新评估，不盲目豁免。
- [ ] 许可证声明可重复生成，包含第三方完整声明及 MPL 对应源码，并包含 ssh2、libssh2 与 OpenSSL 条目。
- [ ] 源码提交中没有公司邮箱、密钥、证书或本机运行数据。
- [ ] 发布标签存在且指向当前已审查提交；标签流程的完整 CI（含历史扫描与最低 Rust 检查）通过。
- [ ] 默认构建目录和自定义 `CARGO_TARGET_DIR` 均能定位当前版本的新安装包，旧包不会被复用。
- [ ] `pwsh scripts/prepare-release.ps1 -Tag v0.1.0`，校验安装包与 SHA256SUMS。

## 干净 Windows 安装验收

- [ ] 安装、首次启动、缺少 WebView2 时的引导。
- [ ] PowerShell、CMD、已安装发行版的 WSL；中文和空格路径、中文输入与粘贴。
- [ ] Tab 与分屏切换、窗口缩放、标题栏拖动和最大化、第二实例前置。
- [ ] “设置 → 关于 Orbyte”显示安装版本；在断网、中英文、深浅主题下查看更新说明，验证长内容滚动、键盘关闭、设置草稿保留和焦点恢复。
- [ ] 文件保存、外部修改冲突、未保存文件退出确认、回收站删除。
- [ ] Shell/应用退出后的受管理子进程清理；保留 WSL 生命周期限制说明。
- [ ] 安装版通知；真实 Codex / Claude Code 启动、退出与重启。
- [ ] Git 状态刷新及 Worktree 新建、已有目录提示。
- [ ] 导出对话框取消、UTF-8 内容落盘、截图缓存路径。
- [ ] SSH：密码与 Ed25519／RSA（含加密）私钥登录；首次按算法确认 SHA-256 主机指纹；同算法密钥变化被拒绝；仅 keyboard-interactive 的服务器明确失败且不发送口令。
- [ ] 远程终端：多 Tab／分屏、中文与 Ctrl+C、窗口 resize、断线保留屏幕并手动重连；对端无响应约 90 秒标为失效。
- [ ] SFTP：浏览、刷新、新建目录、中文／空格名称、符号链接显示且不跟随；上传下载空文件、空目录、多层目录、覆盖／跳过／取消；1 GiB 往返 SHA-256 一致。
- [ ] 传输退出：有传输时的退出确认与取消退出后任务继续；WebView 重载恢复任务快照；断网残留 `.part` 路径可见且文件面板不能删除。
- [ ] 卸载及用户数据是否保留的实际行为，并写入发行说明。

## 发布操作

- [ ] 仓库公开时启用 GitHub private vulnerability reporting，并验证 SECURITY.md 的私密反馈入口可用。
- [ ] main 分支检查通过；限制发布凭据权限。
- [ ] 截图仅展示公开示例数据。
- [ ] 检查 Draft Release 附件、已知限制、未签名说明和校验文件。
- [ ] 人工完成安装验收后再将草稿公开为 prerelease。

未完成的可选功能必须注明具体限制；安装、启动、文件安全和进程清理失败时不得发布。

## 2026-10-24 下一阶段工作区验收

当前 0.1.0 源码工作区新增草稿恢复及文件索引优化，尚未发布。环境、基线提交、自动检查、性能原始数据、原生逐步执行表统一见[下一阶段验收记录](next-stage-validation.md)。公开下载的旧包不作为本轮安装版验收证据。

- [x] 草稿存储与编辑器竞争条件、索引结构变化回归检查。
- [x] 1 万/9 万文件优化前后重复测量与资源采样。
- [x] Rust 1.90 全目标兼容检查。
- [ ] 干净安装、升级、输入法、截图粘贴、真实 AI CLI、通知和退出清理，逐项执行 N01–N11 后填写实际结果及证据。
- [x] 连续 Git difftool 前台激活专项：安装 TortoiseGitMerge，设置 `ORBYTE_TEST_DIFFTOOL` 后运行下列命令；本机在 `C:/apps/TortoiseGit/bin/` 找到工具，本轮实际结果见验收记录。

```powershell
cargo test -p orbyte-terminal --test windows_pty consecutive_git_difftool_windows_receive_foreground -- --ignored --exact --nocapture
```

本轮原生验收未完成，保持不发布。输入法、安装和通知不以逻辑测试通过代替。

## 2026-10-31 SSH／SFTP 验证

环境：Windows x64 构建机；隔离夹具为 Alpine 3.20.10 + OpenSSH_9.7p1 的 normal／weak／keyboard 三个容器；客户端使用 OpenSSL 后端的 libssh2（`ssh2` 0.9.6，启用 `vendored-openssl` 与 `openssl-on-win32`）。完整矩阵与手动场景见 [SSH 协议探针](ssh-probe.md)。

- [x] 隔离 OpenSSH 探针 8/8：密码；Ed25519、RSA（PEM）与加密 Ed25519 私钥；按算法主机密钥优先；keyboard-interactive 仅提供时拒绝；弱算法拒绝；PTY resize；SFTP `lstat`／rename／symlink；双向传输含空文件、空目录、多层与中文／空格名称、覆盖、跳过、取消和符号链接跳过。
- [x] `cargo test --workspace --tests --locked` 与探针命令在工作区通过；普通测试不依赖真实服务器。
- [ ] 1 GiB 往返 SHA-256、worker 额外内存不超过 64 MiB、持续传输时终端回显 p95 不高于 200 ms；需在安装版采样并记录服务器与网络条件。
- [ ] 保活实测：暂停 normal 容器后约 90 秒进入失效；`keepalive.rs` 单元测试不能替代。
- [ ] 只读目录或其他无法安全替换的远程目标：保留原文件、该项失败并报告结果。
- [ ] 安装版主机指纹确认、取消过期、断线保留屏幕与手动重连、WebView 重载恢复任务、退出确认与 3 秒预算。
