//! 应用级共享状态。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::sync::{Arc, OnceLock};

use orbyte_platform::shell::{detect_all, ShellInfo, ShellType, SystemShellEnv};
use orbyte_ssh::SshHub;
use orbyte_terminal::TerminalManager;

pub struct AppState {
    pub integration_dir: std::sync::OnceLock<std::path::PathBuf>,
    pub files: Arc<orbyte_files::watcher::FileManager>,
    pub unsaved_editor_count: std::sync::atomic::AtomicUsize,
    pub terminals: Arc<TerminalManager>,
    pub ssh: Arc<SshHub>,
    pub ssh_passwords: Arc<dyn orbyte_platform::credentials::PasswordStore>,
    pub ssh_connections: tokio::sync::Mutex<()>,
    pub shells: Arc<ShellCache>,
    /// 配置创建/重启/移除与项目修改/删除串行，避免数据库与会话注册表竞争。
    pub terminal_lifecycle: tokio::sync::Mutex<()>,
    /// 文件修改持读锁直到 IO 完成，退出清理取写锁等待进行中的修改；须先于 `terminal_lifecycle` 获取。
    pub file_writes: tokio::sync::RwLock<()>,
    pub close_phase: std::sync::Mutex<ClosePhase>,
    pub close_started: std::sync::atomic::AtomicBool,
    pub close_complete: std::sync::atomic::AtomicBool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            integration_dir: std::sync::OnceLock::new(),
            files: Arc::new(orbyte_files::watcher::FileManager::default()),
            unsaved_editor_count: std::sync::atomic::AtomicUsize::new(0),
            terminals: Arc::new(TerminalManager::default()),
            ssh: SshHub::new(),
            ssh_passwords: Arc::new(orbyte_platform::credentials::SystemPasswordStore),
            ssh_connections: tokio::sync::Mutex::new(()),
            shells: Arc::new(ShellCache::default()),
            terminal_lifecycle: tokio::sync::Mutex::new(()),
            file_writes: tokio::sync::RwLock::new(()),
            close_phase: std::sync::Mutex::new(ClosePhase::Idle),
            close_started: std::sync::atomic::AtomicBool::new(false),
            close_complete: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// 挂起确认与清理分开，取消关闭不影响正在运行的终端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosePhase {
    Idle,
    Pending,
    ShuttingDown,
}

impl ClosePhase {
    /// `transfers` counts queued and running remote copies. Either count asks the user to confirm.
    pub fn request(&mut self, unsaved: usize, transfers: usize) -> bool {
        if *self != Self::Idle { return false; }
        *self = if unsaved > 0 || transfers > 0 { Self::Pending } else { Self::ShuttingDown };
        true
    }

    pub fn resolve(&mut self, proceed: bool) -> Result<(), String> {
        if *self != Self::Pending { return Err("没有待确认的关闭请求".into()); }
        *self = if proceed { Self::ShuttingDown } else { Self::Idle };
        Ok(())
    }
}

/// Shell 探测结果，启动时在后台线程探测一次并缓存。
#[derive(Default)]
pub struct ShellCache {
    infos: OnceLock<Vec<ShellInfo>>,
}

impl ShellCache {
    pub fn warm_up(self: &Arc<Self>) {
        let cache = self.clone();
        std::thread::spawn(move || {
            cache.all();
        });
    }

    /// 首次调用时阻塞直到探测完成。
    pub fn all(&self) -> &[ShellInfo] {
        self.infos.get_or_init(|| detect_all(&SystemShellEnv))
    }

    pub fn get(&self, shell: ShellType) -> Option<&ShellInfo> {
        self.all().iter().find(|s| s.shell_type == shell)
    }
}
