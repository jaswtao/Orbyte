//! 白名单 AI CLI 探测。Author: jaswtao · Date: 2026-10-14
use crate::shell::{find_in_path, ShellEnv, SystemShellEnv};
use serde::Serialize;
use std::{path::PathBuf, sync::OnceLock};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPreset {
    pub preset: String,
    pub description: String,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    #[serde(skip)]
    pub program: Option<PathBuf>,
}

pub fn presets() -> &'static [LaunchPreset] {
    static CACHE: OnceLock<Vec<LaunchPreset>> = OnceLock::new();
    CACHE.get_or_init(|| detect_presets(&SystemShellEnv))
}

pub fn detect_presets(env: &dyn ShellEnv) -> Vec<LaunchPreset> {
    [
        ("codex", "Codex", ["codex.cmd", "codex.exe"]),
        ("claude-code", "Claude Code", ["claude.exe", "claude.cmd"]),
    ]
    .into_iter()
    .map(|(preset, description, names)| {
        let program = names
            .into_iter()
            .find_map(|name| find_in_path(env, name))
            .map(|p| {
                if p.is_absolute() {
                    p
                } else {
                    std::env::current_dir().unwrap_or_default().join(p)
                }
            });
        LaunchPreset {
            preset: preset.into(),
            description: description.into(),
            available: program.is_some(),
            unavailable_reason: program
                .is_none()
                .then(|| "preset_not_found".into()),
            program,
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, path::Path};
    struct Env(bool);
    impl ShellEnv for Env {
        fn var(&self, key: &str) -> Option<OsString> {
            (key == "PATH").then(|| OsString::from("C:\\bin"))
        }
        fn exists(&self, _: &Path) -> bool {
            self.0
        }
        fn wsl_has_distro(&self, _: &Path) -> bool {
            false
        }
    }
    #[test]
    fn detects_in_declared_order_and_handles_missing_cli() {
        let found = detect_presets(&Env(true));
        assert!(found[0].program.as_ref().unwrap().ends_with("codex.cmd"));
        assert!(found[1].program.as_ref().unwrap().ends_with("claude.exe"));
        assert!(detect_presets(&Env(false))
            .iter()
            .all(|p| !p.available && p.unavailable_reason.is_some()));
        assert_eq!(
            crate::shell_integration::quote_powershell("C:\\Tao's\\cli.cmd"),
            "'C:\\Tao''s\\cli.cmd'"
        );
    }
}
