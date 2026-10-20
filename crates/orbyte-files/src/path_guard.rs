//! 每次操作重新检查路径与最深已存在祖先。Author: jaswtao · Date: 2026-10-13
use std::path::{Path, PathBuf};

pub fn resolve(root: &Path, relative: &str, allow_root: bool) -> Result<PathBuf, String> {
    orbyte_core::validate_relative_path(relative)?;
    if relative.is_empty() && !allow_root {
        return Err("文件路径不能为空".into());
    }
    let root = dunce::canonicalize(root).map_err(|e| format!("无法访问项目目录：{e}"))?;
    let target = root.join(relative);
    let mut ancestor = target.as_path();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or("无法解析文件路径")?;
            }
            Err(e) => return Err(format!("无法访问文件路径：{e}")),
        }
    }
    let canonical = dunce::canonicalize(ancestor).map_err(|e| format!("无法解析文件路径：{e}"))?;
    let root_key = root
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    let key = canonical
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    if key != root_key && !key.starts_with(&format!("{root_key}/")) {
        return Err("文件路径不能位于项目根目录之外".into());
    }
    Ok(target)
}
