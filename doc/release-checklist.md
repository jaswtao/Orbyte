# 发布检查表

Author: jaswtao · Date: 2026-10-20

首次版本为 v0.1.0，发布为 GitHub prerelease。以下项目需逐项填写结果；未执行项目不得标记通过。

## 自动检查

- [ ] 从独立目录执行 `pnpm install --frozen-lockfile`。
- [ ] `python scripts/check-release.py --tag v0.1.0`。
- [ ] `cargo +1.90.0 check --workspace --all-targets --locked` 验证最低 Rust 版本。
- [ ] `pwsh scripts/release-check.ps1 -Gitleaks <gitleaks.exe>`。
- [ ] 确认 RSA 审计豁免对应依赖未进入 Windows 实际构建；新漏洞和警告须重新评估，不盲目豁免。
- [ ] 许可证声明可重复生成，包含第三方完整声明及 MPL 对应源码。
- [ ] 源码提交中没有公司邮箱、密钥、证书或本机运行数据。
- [ ] `pwsh scripts/prepare-release.ps1 -Tag v0.1.0`，校验安装包与 SHA256SUMS。

## 干净 Windows 安装验收

- [ ] 安装、首次启动、缺少 WebView2 时的引导。
- [ ] PowerShell、CMD、已安装发行版的 WSL；中文和空格路径、中文输入与粘贴。
- [ ] Tab 与分屏切换、窗口缩放、标题栏拖动和最大化、第二实例前置。
- [ ] 文件保存、外部修改冲突、未保存文件退出确认、回收站删除。
- [ ] Shell/应用退出后的受管理子进程清理；保留 WSL 生命周期限制说明。
- [ ] 安装版通知；真实 Codex / Claude Code 启动、退出与重启。
- [ ] Git 状态刷新及 Worktree 新建、已有目录提示。
- [ ] 导出对话框取消、UTF-8 内容落盘、截图缓存路径。
- [ ] 卸载及用户数据是否保留的实际行为，并写入发行说明。

## 发布操作

- [ ] 启用 GitHub private vulnerability reporting。
- [ ] main 分支检查通过；限制发布凭据权限。
- [ ] 截图仅展示公开示例数据。
- [ ] 检查 Draft Release 附件、已知限制、未签名说明和校验文件。
- [ ] 人工完成安装验收后再将草稿公开为 prerelease。

未完成的可选功能必须注明具体限制；安装、启动、文件安全和进程清理失败时不得发布。
