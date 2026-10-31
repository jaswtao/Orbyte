//! POSIX remote paths and Windows download destinations.
//!
//! Remote paths are not passed through Windows canonicalization. Download paths
//! are checked against a directory the backend itself selected.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::path::{Path, PathBuf};

const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL"];

pub fn normalize_remote(path: &str) -> Result<String, String> {
    if path.is_empty() || path.contains('\0') {
        return Err("远程路径无效".into());
    }
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if parts.pop().is_none() && !absolute {
                return Err("远程路径不能越过起始目录".into());
            }
            continue;
        }
        if segment.contains('\\') {
            return Err("远程路径使用 / 分隔".into());
        }
        parts.push(segment);
    }
    if absolute {
        Ok(if parts.is_empty() { "/".into() } else { format!("/{}", parts.join("/")) })
    } else if parts.is_empty() {
        Ok(".".into())
    } else {
        Ok(parts.join("/"))
    }
}

pub fn join_remote(directory: &str, name: &str) -> Result<String, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(format!("不支持的远程文件名：{name}"));
    }
    let directory = normalize_remote(directory)?;
    if directory == "/" {
        Ok(format!("/{name}"))
    } else if directory == "." {
        Ok(name.to_owned())
    } else {
        Ok(format!("{directory}/{name}"))
    }
}

pub fn part_file_name(id: &str) -> String {
    format!(".orbyte-transfer-{id}.part")
}

pub fn validate_download_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("下载名称无效：{name}"));
    }
    if name.ends_with([' ', '.']) {
        return Err(format!("Windows 不允许该文件名以空格或点结尾：{name}"));
    }
    if name.chars().any(|ch| ch.is_control() || "\\:<>\"|?*".contains(ch)) {
        return Err(format!("文件名包含 Windows 非法字符：{name}"));
    }
    let stem = name.split('.').next().unwrap_or_default().to_uppercase();
    let reserved = RESERVED.contains(&stem.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
            })
        });
    if reserved {
        return Err(format!("文件名是 Windows 保留名称：{name}"));
    }
    Ok(())
}

/// Join a relative POSIX path onto a granted directory and reject escapes.
pub fn guarded_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty() {
        return Ok(root.to_path_buf());
    }
    if relative.starts_with('/') || relative.contains(':') {
        return Err(format!("下载路径不能是绝对路径：{relative}"));
    }
    for segment in relative.split('/') {
        validate_download_name(segment)?;
    }
    let mut target = root.to_path_buf();
    for segment in relative.split('/') {
        target.push(segment);
    }
    let root_canon = dunce::canonicalize(root).map_err(|_| "无法访问下载目录".to_owned())?;
    if let Some(parent) = deepest_existing_ancestor(&target) {
        let ancestor = dunce::canonicalize(&parent).map_err(|_| format!("无法解析下载路径：{relative}"))?;
        if !is_within(&root_canon, &ancestor) {
            return Err(format!("下载路径越过了所选目录（重解析点或链接）：{relative}"));
        }
    }
    let lexical = lexical_normalize(&target);
    if !is_within(&lexical_normalize(&root_canon), &lexical) && !is_within(&root_canon, &lexical) {
        return Err(format!("下载路径越过了所选目录：{relative}"));
    }
    Ok(target)
}

fn deepest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn is_within(root: &Path, path: &Path) -> bool {
    let root = normalize_key(root);
    let path = normalize_key(path);
    path == root || path.starts_with(&format!("{root}/"))
}

fn normalize_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => { out.pop(); }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

pub fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING};
        let source: Vec<u16> = from.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let target: Vec<u16> = to.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let ok = unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_REPLACE_EXISTING) };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        // Same-directory rename replaces the destination without deleting it first.
        std::fs::rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn remote_paths_stay_posix() {
        assert_eq!(normalize_remote("/a/./b/../c").unwrap(), "/a/c");
        assert_eq!(join_remote("/tmp", "文件 1").unwrap(), "/tmp/文件 1");
        assert!(join_remote("/tmp", "..").is_err());
        assert!(normalize_remote("/a\0b").is_err());
        assert_ne!(normalize_remote("/Case").unwrap(), normalize_remote("/case").unwrap());
    }

    #[test]
    fn download_names_and_escapes_are_rejected_explicitly() {
        for name in ["CON", "aux.txt", "LPT1.log", "a:b", "a?", "nul.txt", ".."] {
            assert!(validate_download_name(name).is_err(), "{name}");
        }
        let root = std::env::temp_dir().join(format!(
            "orbyte-dl-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        assert!(guarded_join(&root, "../outside").is_err());
        assert!(guarded_join(&root, "C:/abs").is_err());
        assert!(guarded_join(&root, "ok/CON/child.txt").is_err());
        assert!(guarded_join(&root, "ok/invalid:/child.txt").is_err());
        assert!(guarded_join(&root, "ok/文件.txt").is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nested_download_links_cannot_escape_the_granted_directory() {
        let directory = std::env::temp_dir().join(format!("orbyte-download-link-{}", uuid::Uuid::new_v4()));
        let root = directory.join("granted");
        let outside = directory.join("outside");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::create_dir(&outside).unwrap();
        let link = root.join("nested/link");
        #[cfg(windows)]
        {
            // Directory junctions do not require the symlink privilege on Windows.
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link.as_os_str().to_string_lossy().replace('/', "\\"))
                .arg(&outside).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(guarded_join(&root, "nested/link").is_err());
        assert!(guarded_join(&root, "nested/link/child.txt").is_err());
        #[cfg(windows)]
        fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn replace_rename_leaves_the_original_when_the_source_is_missing() {
        let root = std::env::temp_dir().join(format!(
            "orbyte-rename-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let original = root.join("file.txt");
        fs::write(&original, b"kept").unwrap();
        let missing = root.join("missing.part");
        assert!(replace_file(&missing, &original).is_err());
        assert_eq!(fs::read(&original).unwrap(), b"kept");
        let part = root.join(".orbyte-transfer-test.part");
        fs::write(&part, b"new").unwrap();
        replace_file(&part, &original).unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"new");
        assert!(!part.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
