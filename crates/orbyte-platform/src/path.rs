//! Windows 项目目录规范化。
//! Author: jaswtao
//! Date: 2026-10-12

use std::path::Path;

/// 终端目录必须存在，且规范化后仍位于项目根内（拒绝逃逸的 junction / 符号链接）。
pub fn resolve_terminal_directory(
    root: &str,
    relative: &str,
) -> Result<std::path::PathBuf, String> {
    orbyte_core::validate_relative_path(relative)?;
    let root = normalize_project_path(root)?;
    let directory = Path::new(&root).join(relative);
    let normalized = normalize_project_path(directory.to_str().ok_or("工作目录路径无效")?)?;
    let root_key = root.trim_end_matches(['\\', '/']).to_lowercase();
    let directory_key = normalized.to_lowercase();
    if directory_key != root_key && !directory_key.starts_with(&format!("{root_key}\\")) {
        return Err("终端工作目录不能位于项目根目录之外".into());
    }
    Ok(normalized.into())
}

pub fn normalize_project_path(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return Err("项目目录必须是本地盘符绝对路径，不支持 UNC 或相对路径".into());
    }
    let path =
        dunce::canonicalize(Path::new(input)).map_err(|e| format!("无法访问项目目录：{e}"))?;
    let normalized = path.to_str().ok_or("项目路径不是有效的 Unicode 文本")?;
    if normalized.starts_with("\\\\") || !path.is_dir() {
        return Err("项目目录必须是本地磁盘上的文件夹，不支持 UNC 路径".into());
    }
    std::fs::read_dir(&path).map_err(|e| format!("无法读取项目目录：{e}"))?;
    Ok(normalized.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn terminal_directory_rejects_missing_paths_and_junction_escape() {
        use std::os::windows::process::CommandExt;
        let base = std::env::temp_dir().join(format!("orbyte-path-{}", std::process::id()));
        let root = base.join("project");
        let outside = base.join("project-outside");
        let sub = root.join("中文 空格");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let project = root.to_str().unwrap();
        assert_eq!(resolve_terminal_directory(project, "").unwrap(), root);
        assert_eq!(
            resolve_terminal_directory(project, "中文 空格").unwrap(),
            sub
        );
        assert!(resolve_terminal_directory(project, "missing").is_err());
        assert!(resolve_terminal_directory(project, "../project-outside").is_err());
        let link = root.join("escape");
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(resolve_terminal_directory(project, "escape")
            .unwrap_err()
            .contains("之外"));
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(&base).unwrap();
    }
    #[test]
    fn rejects_unc_relative_and_missing_paths() {
        for path in [
            r"\\server\share",
            r"\\wsl$\Ubuntu",
            r"\\?\C:\",
            "relative",
            "C:relative",
            "C:\\orbyte-missing-path-961f122",
        ] {
            assert!(normalize_project_path(path).is_err(), "{path}");
        }
    }
    #[cfg(windows)]
    #[test]
    fn canonicalizes_existing_directory() {
        let root = std::env::temp_dir().join(format!("Orbyte 中文 空格 {}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let normalized = normalize_project_path(root.to_str().unwrap()).unwrap();
        assert!(!normalized.starts_with(r"\\?\"));
        assert_eq!(
            normalized,
            normalize_project_path(&format!("{normalized}\\.")).unwrap()
        );
        std::fs::remove_dir(root).unwrap();
    }
}
