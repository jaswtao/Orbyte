//! 终端会话注册表。运行时会话由 Rust 持有，前端只保存 `terminal_id`。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread;
use std::time::Duration;

use crate::backend::{SpawnRequest, TerminalSize};
use crate::error::{TerminalError, TerminalResult};
use crate::output::FlowConfig;
use crate::session::{SharedSink, TerminalSession, TerminalSessionInfo};

/// 优雅结束的等待上限，超时后结束会话 Job。
pub const STOP_GRACE: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct TerminalManager {
    sessions: RwLock<HashMap<String, Arc<TerminalSession>>>,
    flow: FlowConfig,
    mutation: Mutex<()>,
    shutting_down: AtomicBool,
    active_terminal: Mutex<Option<String>>,
}

impl TerminalManager {
    pub fn new(flow: FlowConfig) -> Self {
        Self {
            flow,
            ..Self::default()
        }
    }

    /// 启动新会话，返回 `terminal_id`。
    pub fn spawn(&self, req: &SpawnRequest, sink: SharedSink) -> TerminalResult<String> {
        let _guard = self.mutation.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_shutting_down() {
            return Err(TerminalError::ShuttingDown);
        }
        let previous = req
            .config_id
            .as_deref()
            .and_then(|id| self.find_by_config(id));
        if previous.as_ref().is_some_and(|s| !s.is_exited()) {
            return Err(TerminalError::AlreadyRunning);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let session = TerminalSession::spawn(id.clone(), req, sink, self.flow.clone())?;
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = previous {
            previous.dispose();
            sessions.remove(previous.id());
        }
        sessions.insert(id.clone(), session);
        Ok(id)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    /// 只接受已存在的会话；退出应用后不再允许激活子窗口。
    pub fn set_active_terminal(&self, terminal_id: Option<String>) -> TerminalResult<()> {
        if let Some(id) = &terminal_id { self.get(id)?; }
        *self.active_terminal.lock().unwrap_or_else(|e| e.into_inner()) = terminal_id;
        Ok(())
    }

    #[cfg(windows)]
    pub fn is_active_process(&self, pid: u32) -> bool {
        if self.is_shutting_down() { return false; }
        let id = self.active_terminal.lock().unwrap_or_else(|e| e.into_inner()).clone();
        id.and_then(|id| self.get(&id).ok())
            .is_some_and(|session| session.job_process_ids().contains(&pid))
    }

    /// 与启动使用同一把锁，退出标记设置完成后不会再出现新会话。
    pub fn begin_shutdown(&self) {
        let _guard = self.mutation.lock().unwrap_or_else(|e| e.into_inner());
        self.shutting_down.store(true, Ordering::Release);
    }

    pub fn get(&self, terminal_id: &str) -> TerminalResult<Arc<TerminalSession>> {
        self.sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(terminal_id)
            .cloned()
            .ok_or(TerminalError::NotFound)
    }

    /// 按终端配置查找会话（运行中或已退出但未移除）。
    pub fn find_by_config(&self, config_id: &str) -> Option<Arc<TerminalSession>> {
        self.sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .find(|s| s.config_id() == Some(config_id))
            .cloned()
    }

    pub fn list(&self) -> Vec<TerminalSessionInfo> {
        self.sessions.read().unwrap_or_else(|e| e.into_inner()).values().map(|s| s.info()).collect()
    }

    pub fn write(&self, terminal_id: &str, data: Vec<u8>) -> TerminalResult<()> {
        self.get(terminal_id)?.write(data)
    }

    pub fn resize(&self, terminal_id: &str, size: TerminalSize) -> TerminalResult<()> {
        self.get(terminal_id)?.resize(size)
    }

    pub fn ack(&self, terminal_id: &str, seq: u64) -> TerminalResult<()> {
        self.get(terminal_id)?.ack(seq);
        Ok(())
    }

    pub fn subscribe(&self, terminal_id: &str, sink: SharedSink) -> TerminalResult<()> {
        self.get(terminal_id)?.subscribe(sink);
        Ok(())
    }

    /// 结束会话但保留注册项，返回退出码。阻塞调用。
    pub fn stop(&self, terminal_id: &str) -> TerminalResult<Option<u32>> {
        Ok(self.get(terminal_id)?.stop(STOP_GRACE))
    }

    /// 结束会话（若在运行）并移除注册项。阻塞调用。
    pub fn remove(&self, terminal_id: &str) -> TerminalResult<()> {
        let _guard = self.mutation.lock().unwrap_or_else(|e| e.into_inner());
        let session = self.get(terminal_id)?;
        session.stop(STOP_GRACE);
        session.dispose();
        self.sessions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(terminal_id);
        Ok(())
    }

    /// 并发结束全部会话，总等待受 `STOP_GRACE` 约束。阻塞调用。
    pub fn stop_all(&self) {
        self.begin_shutdown();
        let sessions: Vec<_> = self
            .sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        let handles: Vec<_> = sessions
            .into_iter()
            .map(|s| thread::spawn(move || s.stop(STOP_GRACE)))
            .collect();
        for h in handles {
            let _ = h.join();
        }
    }
}
