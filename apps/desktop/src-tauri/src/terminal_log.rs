//! Records terminal diagnostic metadata only. Rotates at 2 MiB and keeps at most three files.
//! Author: jaswtao · Date: 2026-10-20
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

struct FileState {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
}
struct TerminalLog(Mutex<FileState>);
static LOGGER: OnceLock<TerminalLog> = OnceLock::new();

impl log::Log for TerminalLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("orbyte_terminal") && metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        if state.bytes >= 2 * 1024 * 1024 {
            state.file.take();
            let oldest = state.path.with_extension("log.2");
            let previous = state.path.with_extension("log.1");
            let _ = fs::remove_file(&oldest);
            let _ = fs::rename(&previous, oldest);
            let path = state.path.clone();
            if fs::rename(&path, previous).is_ok() {
                state.bytes = 0;
            } else {
                return;
            } // Stop appending when rotation fails so the log cannot grow without bound.
        }
        if state.file.is_none() {
            state.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&state.path)
                .ok();
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let line = format!(
            "{} {} {} {}\n",
            timestamp,
            record.level(),
            record.target(),
            record.args()
        );
        if let Some(file) = &mut state.file {
            if file.write_all(line.as_bytes()).is_ok() {
                state.bytes += line.len() as u64;
            }
        }
    }
    fn flush(&self) {
        if let Ok(mut state) = self.0.lock() {
            if let Some(file) = &mut state.file {
                let _ = file.flush();
            }
        }
    }
}

pub fn init(directory: PathBuf) -> std::io::Result<()> {
    fs::create_dir_all(&directory)?;
    let path = directory.join("terminal.log");
    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let logger = LOGGER.get_or_init(|| {
        TerminalLog(Mutex::new(FileState {
            path,
            file: Some(file),
            bytes,
        }))
    });
    log::set_logger(logger).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::AlreadyExists, "终端日志器已被占用")
    })?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}
