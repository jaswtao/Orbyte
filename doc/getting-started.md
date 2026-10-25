# 快速上手

Author: jaswtao · Date: 2026-10-22

[English](getting-started.en.md) · [返回 README](../README.md)

## 安装

Orbyte 当前仅支持 Windows x64。下载 [v0.1.0 安装包](https://github.com/jaswtao/Orbyte/releases/download/v0.1.0/Orbyte_0.1.0_x64-setup.exe) 和 [SHA256SUMS.txt](https://github.com/jaswtao/Orbyte/releases/download/v0.1.0/SHA256SUMS.txt)，在下载目录执行：

```powershell
Get-FileHash .\Orbyte_0.1.0_x64-setup.exe -Algorithm SHA256
```

将输出与校验文件中安装包对应的值比较，一致后再运行安装程序。安装包尚未签名，Windows 可能显示信任提示；哈希比对用于确认文件一致性，不代替发布者身份验证。安装需要 WebView2 Runtime，缺少时安装程序提供引导，可能需要联网下载微软组件。

日常使用无需安装 Rust、Node.js 或 Visual Studio。Git、WSL、Codex 和 Claude Code 根据需要自行安装，不随 Orbyte 捆绑。

## 第一次使用

1. 打开 Orbyte，点击「添加项目」，选择一个已有的本地目录，确认名称后保存。使用普通本地路径，例如 `C:\projects\demo`；不支持 UNC 或 WSL 网络目录。
2. 在项目工作区点击「新建终端」，或使用默认快捷键 `Ctrl+Shift+T`。在终端执行 `Get-Location`，查看当前目录。
3. 使用 `Ctrl+Shift+H` 左右分屏，或 `Ctrl+Shift+J` 上下分屏。在不同窗格分别执行命令。
4. 使用 `Ctrl+Shift+P` 按文件名快速打开文件；如已修改快捷键，以「设置 → 快捷键设置」为准。也可从文件树打开文件，编辑后按 `Ctrl+S` 保存。
5. 添加第二个项目，切换过去再返回。只要应用和终端保持打开，原项目的终端会话仍然保留。

## 使用 AI 编码 CLI

先自行安装 Codex 或 Claude Code，并确保命令在 PowerShell 的 PATH 中可用，再打开 Orbyte。在 Tab 栏的 `+` 菜单中选择对应 AI CLI。CLI 使用自己的账户、认证和服务；Orbyte 不提供模型服务，也不捆绑 CLI。

CLI 退出后窗格仍可作为 Shell 使用。重启该 CLI 窗格会再次启动 CLI。

## 使用 Git worktree

本机已安装 Git、项目为 Git 仓库时，可以在侧栏查看 Git 状态。右键项目选择「Worktree…」，可将已有 worktree 添加为项目，或填写分支和一个尚不存在的本地绝对路径来新建 worktree。

删除 Orbyte 中的项目只移除工作区配置，不删除目录或 Git worktree；需要清理时在终端使用 `git worktree remove`。

## 退出前需要知道

切换项目会保留当前会话；退出应用会结束受管理的 Windows 终端子进程。重启恢复项目、布局和文件路径，不恢复运行中的进程或终端输出。启动时自动检测未保存草稿，有草稿才弹出恢复提示，可选择打开编辑器、查看、复制、丢弃或暂不恢复；没有草稿则直接进入工作区。暂不恢复会保留草稿，之后可通过「＋」菜单中的「恢复未保存草稿」重新打开列表，或打开对应文件通过编辑器提示恢复；恢复不会自动写回源文件。草稿在停止输入 750 毫秒后写入，持续输入最长间隔 5 秒，另需存储耗时；强退仍可能丢失最后一段输入。选择「放弃修改」会清理对应草稿，取消则保留。WSL 脱离会话的进程和 Linux 服务具有不同的生命周期限制。

## 遇到问题

- 找不到 AI CLI：先在独立 PowerShell 中确认命令可用；安装或更改 PATH 后重新打开 Orbyte。
- 无法启动 WSL：先确认已安装并可正常启动 WSL 发行版；也可先使用 PowerShell 或 CMD。
- 反馈缺陷：在 [Issues](https://github.com/jaswtao/Orbyte/issues/new/choose) 中提供版本、Windows 版本、Shell 类型和复现步骤。请移除截图或日志中的私人路径、令牌和项目内容。
- 安全漏洞请使用[私密安全反馈](https://github.com/jaswtao/Orbyte/security/advisories/new)。

完整限制与本地数据位置见 [README](../README.md)。

> 本分支新增的草稿恢复尚待安装版交互验收，公开下载版本不代表包含此能力。见[验收记录](next-stage-validation.md)。
