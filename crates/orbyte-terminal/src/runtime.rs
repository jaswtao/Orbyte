//! Operations both a local PTY and an SSH shell must provide.
//!
//! The desktop host routes by session id. Local process activation and native
//! paste stay on the PTY implementation.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use crate::manager::STOP_GRACE;
use crate::{SharedSink, TerminalResult, TerminalSession, TerminalSessionInfo, TerminalSize};

pub trait SessionRuntime: Send + Sync {
    fn write(&self, data: Vec<u8>) -> TerminalResult<()>;
    fn resize(&self, size: TerminalSize) -> TerminalResult<()>;
    fn redraw(&self, size: TerminalSize) -> TerminalResult<()>;
    fn stop(&self) -> TerminalResult<Option<u32>>;
    fn subscribe(&self, sink: SharedSink) -> TerminalResult<()>;
    fn ack(&self, seq: u64) -> TerminalResult<()>;
    fn pause_output(&self) -> TerminalResult<()>;
    fn info(&self) -> TerminalSessionInfo;
    fn supports_native_paste(&self) -> bool;
    fn supports_process_activation(&self) -> bool;
}

impl SessionRuntime for TerminalSession {
    fn write(&self, data: Vec<u8>) -> TerminalResult<()> {
        TerminalSession::write(self, data)
    }

    fn resize(&self, size: TerminalSize) -> TerminalResult<()> {
        TerminalSession::resize(self, size)
    }

    fn stop(&self) -> TerminalResult<Option<u32>> {
        Ok(TerminalSession::stop(self, STOP_GRACE))
    }

    fn redraw(&self, size: TerminalSize) -> TerminalResult<()> {
        TerminalSession::redraw(self, size)
    }

    fn subscribe(&self, sink: SharedSink) -> TerminalResult<()> {
        TerminalSession::subscribe(self, sink);
        Ok(())
    }

    fn ack(&self, seq: u64) -> TerminalResult<()> {
        TerminalSession::ack(self, seq);
        Ok(())
    }

    fn info(&self) -> TerminalSessionInfo {
        TerminalSession::info(self)
    }

    fn pause_output(&self) -> TerminalResult<()> {
        TerminalSession::pause_output(self);
        Ok(())
    }

    fn supports_native_paste(&self) -> bool {
        true
    }

    fn supports_process_activation(&self) -> bool {
        true
    }
}
