# Contributing / 参与贡献

感谢参与 Orbyte。项目目前仅支持 Windows x64；请先通过 Issue 描述问题或较大的功能改动。

## Development

Use Node.js 24.15.0, pnpm 12.8.1, Python 3.13 and the Rust MSVC toolchain pinned in `rust-toolchain.toml`. Install Visual Studio C++ Build Tools and WebView2 Runtime.

```powershell
pnpm install --frozen-lockfile
pnpm dev
```

Run `cargo test --workspace --tests --locked`, `pnpm --filter orbyte-desktop test` and `pnpm --filter orbyte-desktop build` before submitting. The development configuration uses a separate application identifier and data directory.

安装依赖必须保留锁文件。修改依赖后执行 `pnpm run licenses:generate`，提交更新后的锁文件及第三方声明。不要提交项目数据、终端输出、截图缓存、访问令牌或签名证书。

## Pull requests

Explain the problem, the resulting behavior and how you verified it. Keep changes focused. Include desktop interaction results when changing terminal, window or file behavior; automated tests do not replace native Windows checks.

新增代码沿用现有风格；作者注释使用 jaswtao 和实际编写日期，已有作者信息保留。不要批量更改历史署名。

Contributions are submitted under the project's MIT license. Preserve existing third-party notices. Report security issues privately as described in [SECURITY.md](SECURITY.md).
