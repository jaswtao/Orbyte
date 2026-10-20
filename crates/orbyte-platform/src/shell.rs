//! Shell 枚举与可执行文件解析。前端只提交枚举，可执行路径由 Rust 侧解析。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// MVP 支持的 Shell 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellType {
    Powershell,
    Cmd,
    Wsl,
}

impl ShellType {
    pub const ALL: [ShellType; 3] = [ShellType::Powershell, ShellType::Cmd, ShellType::Wsl];

    pub fn as_str(self) -> &'static str {
        match self {
            ShellType::Powershell => "powershell",
            ShellType::Cmd => "cmd",
            ShellType::Wsl => "wsl",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// 一个 Shell 的探测结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellInfo {
    pub shell_type: ShellType,
    pub available: bool,
    /// 解析出的可执行文件；不可用时为空。
    pub program: Option<PathBuf>,
    /// 版本说明，例如「PowerShell 7」。
    pub description: String,
    /// 不可用原因。
    pub unavailable_reason: Option<String>,
}

/// Shell 解析所依赖的环境，便于单元测试替换。
pub trait ShellEnv {
    fn var(&self, key: &str) -> Option<OsString>;
    fn exists(&self, path: &Path) -> bool;
    /// `wsl.exe -l -q` 是否至少返回一个发行版。
    fn wsl_has_distro(&self, wsl: &Path) -> bool;
}

/// 按解析顺序探测指定 Shell。
pub fn detect_shell(env: &dyn ShellEnv, shell: ShellType) -> ShellInfo {
    match shell {
        ShellType::Powershell => {
            if let Some(pwsh) = find_in_path(env, "pwsh.exe") {
                return available(shell, pwsh, "PowerShell 7");
            }
            match system_root(env).map(|r| r.join(r"System32\WindowsPowerShell\v1.0\powershell.exe")) {
                Some(p) if env.exists(&p) => available(shell, p, "Windows PowerShell 5.1"),
                _ => unavailable(shell, "PowerShell", "powershell_not_found"),
            }
        }
        ShellType::Cmd => {
            let candidates = [
                env.var("ComSpec").map(PathBuf::from),
                system_root(env).map(|r| r.join(r"System32\cmd.exe")),
            ];
            match candidates.into_iter().flatten().find(|p| env.exists(p)) {
                Some(p) => available(shell, p, "cmd"),
                None => unavailable(shell, "cmd", "cmd_not_found"),
            }
        }
        ShellType::Wsl => {
            let Some(wsl) = system_root(env).map(|r| r.join(r"System32\wsl.exe")) else {
                return unavailable(shell, "WSL", "wsl_system_root_missing");
            };
            if !env.exists(&wsl) {
                return unavailable(shell, "WSL", "wsl_not_installed");
            }
            if !env.wsl_has_distro(&wsl) {
                return unavailable(shell, "WSL", "wsl_no_distro");
            }
            available(shell, wsl, "WSL")
        }
    }
}

/// 探测全部 Shell。
pub fn detect_all(env: &dyn ShellEnv) -> Vec<ShellInfo> {
    ShellType::ALL.into_iter().map(|s| detect_shell(env, s)).collect()
}

fn available(shell_type: ShellType, program: PathBuf, description: &str) -> ShellInfo {
    ShellInfo {
        shell_type,
        available: true,
        program: Some(program),
        description: description.to_string(),
        unavailable_reason: None,
    }
}

/// 启动失败时仍用中文说明；界面下拉框使用稳定原因代码自行翻译。
pub fn unavailable_error(description: &str, reason: &str) -> String {
    let name = match description {
        "cmd" => "命令提示符",
        other => other,
    };
    let detail = match reason {
        "powershell_not_found" => "未找到 pwsh.exe 或 powershell.exe",
        "cmd_not_found" => "未找到 cmd.exe",
        "wsl_system_root_missing" => "未找到 SystemRoot 环境变量",
        "wsl_not_installed" => "未安装 WSL",
        "wsl_no_distro" => "WSL 中没有已安装的发行版",
        "preset_not_found" => "未在 PATH 中找到可执行文件",
        other => other,
    };
    format!("{name}不可用：{detail}")
}

fn unavailable(shell_type: ShellType, description: &str, reason: &str) -> ShellInfo {
    ShellInfo {
        shell_type,
        available: false,
        program: None,
        description: description.to_string(),
        unavailable_reason: Some(reason.to_string()),
    }
}

fn system_root(env: &dyn ShellEnv) -> Option<PathBuf> {
    env.var("SystemRoot").map(PathBuf::from)
}

pub fn find_in_path(env: &dyn ShellEnv, exe: &str) -> Option<PathBuf> {
    let path = env.var("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(exe))
        .find(|p| env.exists(p))
}

/// 读取真实系统环境的实现。
pub struct SystemShellEnv;

impl ShellEnv for SystemShellEnv {
    fn var(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key).filter(|v| !v.is_empty())
    }

    fn exists(&self, path: &Path) -> bool {
        // 应用执行别名（如商店版 pwsh）是重解析点，不能用 is_file 判断。
        path.symlink_metadata().map(|m| !m.is_dir()).unwrap_or(false)
    }

    fn wsl_has_distro(&self, wsl: &Path) -> bool {
        #[cfg(windows)]
        {
            wsl_list_distros(wsl).map(|d| !d.is_empty()).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            let _ = wsl;
            false
        }
    }
}

#[cfg(windows)]
fn wsl_list_distros(wsl: &Path) -> Option<Vec<String>> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut child = Command::new(wsl)
        .args(["-l", "-q"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;

    // WSL 服务异常时 wsl.exe 可能长时间不返回，限制等待时间。
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        decode_wsl_output(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// `wsl.exe -l` 输出 UTF-16LE。
fn decode_wsl_output(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units).replace('\0', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct FakeEnv {
        vars: HashMap<String, String>,
        files: HashSet<PathBuf>,
        wsl_distro: bool,
    }

    impl ShellEnv for FakeEnv {
        fn var(&self, key: &str) -> Option<OsString> {
            self.vars.get(key).map(OsString::from)
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
        fn wsl_has_distro(&self, _wsl: &Path) -> bool {
            self.wsl_distro
        }
    }

    fn base_env() -> FakeEnv {
        let mut env = FakeEnv::default();
        env.vars.insert("SystemRoot".into(), r"C:\Windows".into());
        env.vars.insert("PATH".into(), r"C:\Tools;C:\Program Files\PowerShell\7".into());
        env.files.insert(PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"));
        env.files.insert(PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        env
    }

    #[test]
    fn powershell_prefers_pwsh_in_path() {
        let mut env = base_env();
        env.files.insert(PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe"));
        let info = detect_shell(&env, ShellType::Powershell);
        assert_eq!(info.program.unwrap(), PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe"));
        assert_eq!(info.description, "PowerShell 7");
    }

    #[test]
    fn powershell_falls_back_to_windows_powershell() {
        let env = base_env();
        let info = detect_shell(&env, ShellType::Powershell);
        assert_eq!(
            info.program.unwrap(),
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
    }

    #[test]
    fn cmd_prefers_comspec() {
        let mut env = base_env();
        env.vars.insert("ComSpec".into(), r"D:\cmd\cmd.exe".into());
        env.files.insert(PathBuf::from(r"D:\cmd\cmd.exe"));
        assert_eq!(detect_shell(&env, ShellType::Cmd).program.unwrap(), PathBuf::from(r"D:\cmd\cmd.exe"));
    }

    #[test]
    fn cmd_falls_back_when_comspec_missing() {
        let mut env = base_env();
        env.vars.insert("ComSpec".into(), r"D:\missing\cmd.exe".into());
        assert_eq!(
            detect_shell(&env, ShellType::Cmd).program.unwrap(),
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        );
    }

    #[test]
    fn wsl_requires_executable_and_distro() {
        let mut env = base_env();
        assert!(!detect_shell(&env, ShellType::Wsl).available);
        env.files.insert(PathBuf::from(r"C:\Windows\System32\wsl.exe"));
        let info = detect_shell(&env, ShellType::Wsl);
        assert!(!info.available);
        assert_eq!(info.unavailable_reason.unwrap(), "wsl_no_distro");
        env.wsl_distro = true;
        assert!(detect_shell(&env, ShellType::Wsl).available);
    }

    #[test]
    fn parse_rejects_unknown_shell() {
        assert_eq!(ShellType::parse("cmd"), Some(ShellType::Cmd));
        assert_eq!(ShellType::parse("bash"), None);
    }

    #[test]
    fn decode_utf16_wsl_output() {
        let bytes: Vec<u8> = "Ubuntu\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_wsl_output(&bytes).trim(), "Ubuntu");
    }
}
