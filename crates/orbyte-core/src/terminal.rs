//! 终端配置与项目内相对路径规则。
//! Author: jaswtao
//! Date: 2026-10-13

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalConfig {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub shell_type: String,
    pub cwd: String,
    pub launch_preset: Option<String>,
}

pub fn validate_shell(shell: &str) -> Result<(), String> {
    if matches!(shell, "powershell" | "cmd" | "wsl") {
        Ok(())
    } else {
        Err(format!("不支持的 Shell：{shell}"))
    }
}

/// 空路径仅用于项目根目录；文件路径调用方应另行拒绝空值。
pub fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.chars().count() > 1024 {
        return Err("项目内路径不能超过 1024 个字符".into());
    }
    if path.is_empty() {
        return Ok(());
    }
    for segment in path.split('/') {
        let stem = segment.split('.').next().unwrap_or_default().to_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix).is_some_and(|n| {
                    matches!(
                        n,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
        if segment.is_empty()
            || matches!(segment, "." | "..")
            || segment.ends_with([' ', '.'])
            || segment
                .chars()
                .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
            || reserved
        {
            return Err(
                "项目内路径必须使用 / 分隔，不能包含上级目录、保留名或 Windows 非法字符".into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_relative_paths() {
        for path in [
            "/absolute",
            "C:/root",
            "../outside",
            "a/./b",
            "a//b",
            "a/",
            "a\\b",
            "a:stream",
            "NUL.txt",
            "com1",
            "LPT².log",
            "a.",
            "a ",
            "a\0b",
        ] {
            assert!(validate_relative_path(path).is_err(), "{path:?}");
        }
        assert!(validate_relative_path(&"a".repeat(1025)).is_err());
        for path in ["", "src", "中文 空格/文件", "company/COM10.txt"] {
            validate_relative_path(path).unwrap();
        }
        assert!(validate_shell("bash").is_err());
    }
}

/// 启动预设只接受固定枚举。
pub fn validate_launch_preset(preset: Option<&str>) -> Result<(), String> {
    if matches!(preset, None | Some("codex" | "claude-code")) { Ok(()) }
    else { Err("不支持的启动预设".into()) }
}
