//! PTY 启动后端：通过 `portable-pty` 启动 ConPTY 子进程，并把子进程加入会话 Job。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::io::{Read, Write};
use std::path::PathBuf;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::error::{TerminalError, TerminalResult};

/// 终端行列数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSize {
    pub fn validate(self) -> TerminalResult<Self> {
        if self.cols == 0 || self.rows == 0 {
            return Err(TerminalError::InvalidSize);
        }
        Ok(self)
    }

    pub(crate) fn to_pty(self) -> PtySize {
        PtySize { rows: self.rows, cols: self.cols, pixel_width: 0, pixel_height: 0 }
    }
}

/// 启动请求。`program` 必须是 Rust 侧解析出的可执行文件，不接受前端传入的任意路径。
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub size: TerminalSize,
    /// 持久化的终端配置 ID，仅用于运行态查询。
    pub config_id: Option<String>,
}

pub(crate) struct SpawnedPty {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub pid: Option<u32>,
    #[cfg(windows)]
    pub job: orbyte_platform::windows::job::JobObject,
}

pub(crate) fn spawn_pty(req: &SpawnRequest) -> TerminalResult<SpawnedPty> {
    let size = req.size.validate()?;
    if !req.cwd.is_dir() {
        return Err(TerminalError::InvalidCwd(req.cwd.display().to_string()));
    }

    #[cfg(windows)]
    let job = orbyte_platform::windows::job::JobObject::new_kill_on_close()
        .map_err(|e| TerminalError::Spawn(format!("创建会话 Job 失败：{e}")))?;

    let pair = native_pty_system()
        .openpty(size.to_pty())
        .map_err(|e| TerminalError::Spawn(format!("创建伪终端失败：{e}")))?;

    let mut cmd = CommandBuilder::new(&req.program);
    cmd.args(&req.args);
    for (key, value) in &req.env { cmd.env(key, value); }
    cmd.cwd(&req.cwd);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| TerminalError::Spawn(e.to_string()))?;
    // 释放 slave 端，伪终端只由 master 持有，关闭 master 即关闭伪终端。
    drop(pair.slave);

    #[cfg(windows)]
    {
        let assigned = child
            .as_raw_handle()
            .ok_or_else(|| std::io::Error::other("无法获取进程句柄"))
            .and_then(|h| job.assign_process(h as _));
        if let Err(e) = assigned {
            let _ = child.kill();
            return Err(TerminalError::Spawn(format!("加入会话 Job 失败：{e}")));
        }
    }

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| TerminalError::Spawn(format!("获取输出管道失败：{e}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| TerminalError::Spawn(format!("获取输入管道失败：{e}")))?;

    Ok(SpawnedPty {
        pid: child.process_id(),
        master: pair.master,
        child,
        reader,
        writer,
        #[cfg(windows)]
        job,
    })
}
