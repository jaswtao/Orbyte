//! 终端运行时错误。消息面向用户展示。
//!
//! Author: jaswtao
//! Date: 2026-10-09

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("终端不存在或已被移除")]
    NotFound,
    #[error("该终端配置已有运行中的会话")]
    AlreadyRunning,
    #[error("应用正在退出，不能创建或启动终端")]
    ShuttingDown,
    #[error("终端行数和列数必须大于 0")]
    InvalidSize,
    #[error("工作目录不存在或不可访问：{0}")]
    InvalidCwd(String),
    #[error("启动终端失败：{0}")]
    Spawn(String),
    #[error("终端已结束")]
    NotRunning,
    #[error("终端操作失败：{0}")]
    Io(String),
}

pub type TerminalResult<T> = Result<T, TerminalError>;
