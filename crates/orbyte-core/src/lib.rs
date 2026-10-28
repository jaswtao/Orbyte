//! 项目与设置的业务规则。
//! Author: jaswtao
//! Date: 2026-10-12

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

mod terminal;
mod shortcuts;
pub mod layout;
pub use terminal::{validate_launch_preset, validate_relative_path, validate_shell, TerminalConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub group_id: Option<String>,
    pub name: String,
    pub path: String,
    pub favorite: bool,
    pub hidden: bool,
    pub sort_order: i64,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroup {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectInput {
    pub name: String,
    pub path: String,
    pub group_id: Option<String>,
    pub favorite: bool,
}

pub fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err("名称需要包含 1–100 个字符".into());
    }
    Ok(name)
}

pub fn default_settings() -> BTreeMap<String, Value> {
    [
        ("active_project_id", Value::Null),
        ("show_workspace_header", json!(false)),
        ("show_hidden_projects", json!(false)),
        ("theme", json!("dark")),
        ("workspace_border_color", Value::Null),
        ("workspace_border_intensity", json!(100)),
        ("shortcuts", json!({})),
        ("shell_integration", json!(true)),
        ("confirm_close_terminal", json!(true)),
        ("notify_command_seconds", json!(30)),
        ("font_family", json!("Cascadia Mono, Consolas, monospace")),
        ("font_size", json!(14)),
        ("scrollback", json!(5000)),
        ("default_shell", json!("powershell")),
        ("default_directory", Value::Null),
        ("language", json!("en")),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value))
    .collect()
}

pub fn validate_setting(key: &str, value: &Value) -> Result<(), String> {
    if key == "shortcuts" { return shortcuts::validate(value); }
    let valid = match key {
        "active_project_id" | "default_directory" => {
            value.is_null() || value.as_str().is_some_and(|v| !v.is_empty())
        }
        "theme" => matches!(value.as_str(), Some("dark" | "light" | "system")),
        "workspace_border_color" => value.is_null() || value.as_str().is_some_and(|v| {
            v.len() == 7 && v.starts_with('#') && v.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        }),
        "workspace_border_intensity" => value.as_u64().is_some_and(|v| v <= 100),
        "show_workspace_header" | "show_hidden_projects" | "shell_integration" | "confirm_close_terminal" => value.is_boolean(),
        "notify_command_seconds" => value.as_u64().is_some_and(|v| v <= 3600),
        "font_family" => value
            .as_str()
            .is_some_and(|v| !v.trim().is_empty() && v.len() <= 500),
        "font_size" => value.as_u64().is_some_and(|v| (8..=40).contains(&v)),
        "scrollback" => value.as_u64().is_some_and(|v| (0..=100_000).contains(&v)),
        "default_shell" => matches!(value.as_str(), Some("powershell" | "cmd" | "wsl")),
        "language" => matches!(value.as_str(), Some("en" | "zh")),
        _ => return Err(format!("未知设置项：{key}")),
    };
    if valid {
        Ok(())
    } else {
        Err(format!("设置值无效：{key}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_workspace_border_settings() {
        assert_eq!(default_settings()["workspace_border_color"], Value::Null);
        assert_eq!(default_settings()["workspace_border_intensity"], json!(100));
        for value in [Value::Null, json!("#0A84FF"), json!("#aabbcc")] {
            validate_setting("workspace_border_color", &value).unwrap();
        }
        for value in [json!(""), json!("#ABC"), json!("#AABBCCDD"), json!("#GGGGGG"), json!("AABBCC"), json!(12), json!("#蓝色")] {
            assert!(validate_setting("workspace_border_color", &value).is_err());
        }
        for value in [json!(0), json!(45), json!(100)] {
            validate_setting("workspace_border_intensity", &value).unwrap();
        }
        for value in [Value::Null, json!(-1), json!(101), json!(45.5), json!("45")] {
            assert!(validate_setting("workspace_border_intensity", &value).is_err());
        }
    }
    #[test]
    fn validates_hidden_project_setting() {
        assert_eq!(default_settings()["show_hidden_projects"], json!(false));
        for value in [json!(true), json!(false)] { validate_setting("show_hidden_projects", &value).unwrap(); }
        for value in [Value::Null, json!(1), json!("true")] { assert!(validate_setting("show_hidden_projects", &value).is_err()); }
    }

    #[test]
    fn validates_shell_and_notification_settings() {
        assert_eq!(default_settings()["shell_integration"], json!(true));
        for value in [json!(0), json!(30), json!(3600)] { validate_setting("notify_command_seconds", &value).unwrap(); }
        for value in [json!(-1), json!(3601), json!("30"), Value::Null] { assert!(validate_setting("notify_command_seconds", &value).is_err()); }
        assert!(validate_setting("shell_integration", &json!("true")).is_err());
        assert!(validate_launch_preset(Some("unknown")).is_err());
    }
    #[test]
    fn workspace_header_is_hidden_by_default_and_requires_boolean() {
        assert_eq!(default_settings()["show_workspace_header"], json!(false));
        validate_setting("show_workspace_header", &json!(true)).unwrap();
        validate_setting("show_workspace_header", &json!(false)).unwrap();
        for value in [Value::Null, json!("true"), json!(0), json!([]), json!({})] {
            assert!(validate_setting("show_workspace_header", &value).is_err());
        }
    }
    #[test]
    fn terminal_close_confirmation_defaults_to_true_and_requires_boolean() {
        assert_eq!(default_settings()["confirm_close_terminal"], json!(true));
        for value in [json!(true), json!(false)] {
            validate_setting("confirm_close_terminal", &value).unwrap();
        }
        for value in [Value::Null, json!("false"), json!(0), json!([]), json!({})] {
            assert!(validate_setting("confirm_close_terminal", &value).is_err());
        }
    }
    #[test]
    fn rejects_invalid_settings_and_names() {
        for (key, value) in default_settings() {
            validate_setting(&key, &value).unwrap();
        }
        assert!(validate_setting("unknown", &json!(true)).is_err());
        assert!(validate_setting("font_size", &json!(-1)).is_err());
        assert!(validate_setting("default_shell", &json!("bash")).is_err());
        assert!(validate_name("  ").is_err());
        assert_eq!(validate_name(" 财务 ").unwrap(), "财务");
    }
    #[test]
    fn language_defaults_to_english_and_accepts_chinese() {
        assert_eq!(default_settings()["language"], json!("en"));
        validate_setting("language", &json!("en")).unwrap();
        validate_setting("language", &json!("zh")).unwrap();
        for value in [Value::Null, json!("zh-CN"), json!("en-US"), json!(true), json!(1)] {
            assert!(validate_setting("language", &value).is_err());
        }
    }
}
