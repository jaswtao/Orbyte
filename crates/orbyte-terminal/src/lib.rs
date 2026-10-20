//! 终端运行时：PTY 会话、输出流控与订阅。
//!
//! Author: jaswtao
//! Date: 2026-10-09

mod backend;
mod cursor;
mod error;
#[cfg(any(windows, test))]
mod input;
mod manager;
pub mod output;
mod session;

pub use backend::{SpawnRequest, TerminalSize};
pub use error::{TerminalError, TerminalResult};
pub use manager::{TerminalManager, STOP_GRACE};
pub use output::FlowConfig;
pub use session::{FrameSink, SharedSink, TerminalSession, TerminalSessionInfo, TerminalStatus};
