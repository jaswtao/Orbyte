//! Tauri 命令。命令内部调用各 crate，错误转为可展示的文本。
//!
//! Author: jaswtao
//! Date: 2026-10-09

pub mod diagnostics;
pub mod terminal;
pub mod clipboard;
pub mod export;
pub mod git;
pub mod project;
pub mod workspace;
pub mod files;

pub type CmdResult<T> = Result<T, String>;

#[track_caller]
pub(crate) fn err_text(e: impl std::fmt::Display) -> String {
    let message = e.to_string();
    log::warn!("command_failed location={} error={message}", std::panic::Location::caller());
    message
}
