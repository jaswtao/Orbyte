//! 平台差异隔离层：Windows Job Object、Shell 探测等。
//!
//! Author: jaswtao
//! Date: 2026-10-09

pub mod shell;
pub mod file_manager;
pub mod shell_integration;
pub mod launch;
pub mod path;

#[cfg(windows)]
pub mod windows;
