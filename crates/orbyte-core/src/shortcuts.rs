//! 快捷键设置校验。Author: jaswtao · Date: 2026-10-16
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Key {
    code: String,
    ctrl_key: bool,
    alt_key: bool,
    shift_key: bool,
    meta_key: bool,
}
#[derive(Deserialize)]
struct Command {
    id: String,
    label: String,
    scope: String,
    defaults: Vec<Key>,
}

fn valid(key: &Key) -> bool {
    let code = key.code.as_str();
    let function = code
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n) && code == format!("F{n}"));
    let letter = code
        .strip_prefix("Key")
        .is_some_and(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_uppercase());
    let digit = ["Digit", "Numpad"].iter().any(|prefix| {
        code.strip_prefix(prefix)
            .is_some_and(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_digit())
    });
    let named = [
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
        "Tab",
        "Space",
        "Enter",
        "Backspace",
        "Delete",
        "Insert",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Backquote",
        "Minus",
        "Equal",
        "BracketLeft",
        "BracketRight",
        "Backslash",
        "Semicolon",
        "Quote",
        "Comma",
        "Period",
        "Slash",
    ]
    .contains(&code);
    (function || letter || digit || named)
        && (function || key.ctrl_key || key.alt_key || key.meta_key)
}

pub fn validate(value: &Value) -> Result<(), String> {
    let overrides: BTreeMap<String, Vec<Key>> =
        serde_json::from_value(value.clone()).map_err(|_| "快捷键配置格式无效".to_string())?;
    let commands: Vec<Command> =
        serde_json::from_str(include_str!("shortcuts.json")).map_err(|e| e.to_string())?;
    if overrides
        .keys()
        .any(|id| !commands.iter().any(|command| &command.id == id))
    {
        return Err("未知快捷键命令".into());
    }
    for (index, command) in commands.iter().enumerate() {
        let keys = overrides.get(&command.id).unwrap_or(&command.defaults);
        if keys.iter().any(|key| !valid(key)) {
            return Err(format!("{}：快捷键无效", command.label));
        }
        if keys
            .iter()
            .enumerate()
            .any(|(i, key)| keys[..i].contains(key))
        {
            return Err(format!("{}：绑定重复", command.label));
        }
        for other in &commands[..index] {
            if command.scope != other.scope
                && ["terminal", "editor"].contains(&command.scope.as_str())
                && ["terminal", "editor"].contains(&other.scope.as_str())
            {
                continue;
            }
            let candidates = overrides.get(&other.id).unwrap_or(&other.defaults);
            if keys.iter().any(|key| candidates.contains(key)) {
                return Err(format!("{} 与 {} 快捷键冲突", command.label, other.label));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validates_defaults_overrides_and_conflicts() {
        validate(&json!({})).unwrap();
        validate(&json!({"newTerminal": []})).unwrap();
        let key =
            json!({"code":"KeyQ","ctrlKey":true,"altKey":false,"shiftKey":false,"metaKey":false});
        validate(&json!({"terminalCopy":[key.clone()],"editorSave":[key.clone()]})).unwrap();
        assert!(validate(&json!({"newTerminal":[key.clone()],"editorSave":[key]})).is_err());
        for bad in [
            json!(null),
            json!({"unknown":[]}),
            json!({"newTerminal":[{"code":"KeyQ"}]}),
            json!({"newTerminal":[{"code":"KeyQ","ctrlKey":false,"altKey":false,"shiftKey":true,"metaKey":false}]}),
        ] {
            assert!(validate(&bad).is_err());
        }
    }
}
