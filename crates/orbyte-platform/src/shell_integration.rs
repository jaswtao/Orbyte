//! Shell 启动参数与集成脚本。Author: jaswtao · Date: 2026-10-14
use crate::shell::ShellType;
use std::path::Path;

pub const SCRIPT: &str = include_str!("../shell-integration/orbyte.ps1");

pub fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn install(directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join("orbyte.ps1");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(path, SCRIPT).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn cmd_prompt(original: &str) -> String {
    let original = if original.is_empty() {
        "$P$G"
    } else {
        original
    };
    format!("$e]133;D$e\\$e]9;9;$P$e\\$e]133;A$e\\{original}$e]133;B$e\\")
}

pub fn launch_spec(
    shell: ShellType,
    directory: &Path,
    enabled: bool,
) -> (Vec<String>, Vec<(String, String)>) {
    if !enabled {
        return (vec![], vec![]);
    }
    match shell {
        ShellType::Powershell => (
            vec![
                "-NoLogo".into(),
                "-NoExit".into(),
                "-Command".into(),
                format!(
                    ". {}",
                    quote_powershell(&directory.join("orbyte.ps1").to_string_lossy())
                ),
            ],
            vec![],
        ),
        ShellType::Cmd => (
            vec![],
            vec![(
                "PROMPT".into(),
                cmd_prompt(&std::env::var("PROMPT").unwrap_or_default()),
            )],
        ),
        ShellType::Wsl => (vec![], vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_parameters_preserve_shell_and_escape_paths() {
        let path = Path::new("C:/Tao's data");
        for shell in ShellType::ALL {
            assert_eq!(launch_spec(shell, path, false), (vec![], vec![]));
        }
        let (args, env) = launch_spec(ShellType::Powershell, path, true);
        assert!(args[3].contains("Tao''s data"));
        assert!(env.is_empty());
        assert!(launch_spec(ShellType::Wsl, path, true).0.is_empty());
        assert_eq!(launch_spec(ShellType::Cmd, path, true).1[0].0, "PROMPT");
        assert!(cmd_prompt("").contains("$P$G"));
        assert!(cmd_prompt("custom$G").contains("custom$G"));
    }
}
