# 第三方许可补充

Author: jaswtao · Date: 2026-10-20

根目录 THIRD_PARTY_NOTICES.txt 和 THIRD_PARTY_LICENSES.json 由 `pnpm run licenses:generate` 生成，包含 Windows x64 实际 Cargo normal/build 依赖及 npm 生产依赖。构建依赖一并保留声明，以覆盖资源生成时使用的第三方内容。

部分发布包未附带完整许可文本；overrides.json 按生态、包名和版本固定补充文件及来源。Rust 上游地址固定到包内 .cargo_vcs_info.json 对应的提交；css-render 地址固定到 npm 发布元数据的 gitHead。

selectors 使用其 lib.rs 明示的 MPL-2.0 对应标准全文。vdirs 0.1.8 的发布元数据声明 MIT、作者 07akioni，但发布包没有许可文件，其 gitHead 在公开仓库不可访问；这里按该声明附上标准 MIT 条款及作者署名，不推测版权年份。此依据也会写入生成清单。

升级上述依赖时必须重新核实来源；缺少许可文本的新依赖会阻止声明生成。不要修改依赖缓存里的许可证。

发布时 `scripts/prepare-release.ps1` 将当前使用的 MPL 包原始源码打包为 third-party-mpl-sources.zip，并保留许可补充。声明随安装包放入安装目录下 licenses/。
