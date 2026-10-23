//! Application diagnostics with daily/size rotation and a maximum 30-day retention.
//! Author: jaswtao · Date: 2026-10-23
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DAY: u64 = 86_400;
const RETENTION: u64 = 30 * DAY;
const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub(crate) struct FileState {
    directory: PathBuf,
    file: Option<File>,
    day: u64,
    part: u32,
    bytes: u64,
}
struct AppLog(Mutex<FileState>);
static LOGGER: OnceLock<AppLog> = OnceLock::new();

fn seconds(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Only remove files owned by this logger, including the previous terminal logger.
pub(crate) fn cleanup(directory: &Path, now: SystemTime) -> io::Result<()> {
    let cutoff = seconds(now).saturating_sub(RETENTION);
    let mut failure = None;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let bucket = name
            .strip_prefix("orbyte-")
            .and_then(|s| s.strip_suffix(".log"))
            .and_then(|s| s.split_once('-'))
            .and_then(|(day, part)| Some((day.parse::<u64>().ok()?, part.parse::<u32>().ok()?)));
        let expired = if let Some((day, _)) = bucket {
            // Expire a whole UTC-day bucket before any entry could exceed 30 days.
            day.saturating_mul(DAY) <= cutoff
        } else if matches!(
            name.as_ref(),
            "terminal.log" | "terminal.log.1" | "terminal.log.2"
        ) {
            seconds(entry.metadata()?.modified()?) <= cutoff
        } else {
            false
        };
        if expired {
            if let Err(error) = fs::remove_file(entry.path()) {
                failure = Some(error);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

impl FileState {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            file: None,
            day: u64::MAX,
            part: 0,
            bytes: 0,
        }
    }

    pub(crate) fn append(&mut self, now: SystemTime, line: &[u8]) -> io::Result<()> {
        let day = seconds(now) / DAY;
        if self.day != day {
            self.file.take();
            if let Err(error) = cleanup(&self.directory, now) {
                eprintln!("Unable to remove expired logs: {error}");
            }
            self.day = day;
            self.part = 0;
            self.bytes = 0;
        }
        loop {
            if self.file.is_none() {
                let path = self
                    .directory
                    .join(format!("orbyte-{:05}-{:04}.log", self.day, self.part));
                let file = OpenOptions::new().create(true).append(true).open(path)?;
                self.bytes = file.metadata()?.len();
                self.file = Some(file);
            }
            if self.bytes + line.len() as u64 <= MAX_BYTES {
                break;
            }
            self.file.take();
            self.part = self
                .part
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Log rotation exhausted"))?;
        }
        if let Err(error) = self.file.as_mut().unwrap().write_all(line) {
            // Reopen and recheck the actual size after a potentially partial write.
            self.file.take();
            return Err(error);
        }
        self.bytes += line.len() as u64;
        Ok(())
    }
}

impl log::Log for AppLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        (metadata.target().starts_with("orbyte_") && metadata.level() <= log::Level::Info)
            || metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        let now = SystemTime::now();
        let message: String = record.args().to_string().chars().take(4096).collect();
        let mut line = serde_json::json!({
            "timestamp_ms": now.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            "level": record.level().as_str(),
            "target": record.target(),
            "source": record.file(),
            "line": record.line(),
            "pid": std::process::id(),
            "message": message,
        })
        .to_string();
        line.push('\n');
        if let Err(error) = state.append(now, line.as_bytes()) {
            eprintln!("Unable to write diagnostic log: {error}");
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

pub fn init(directory: PathBuf) -> io::Result<()> {
    fs::create_dir_all(&directory)?;
    let mut state = FileState::new(directory.clone());
    state.append(SystemTime::now(), b"")?;
    let logger = LOGGER.get_or_init(|| AppLog(Mutex::new(state)));
    log::set_logger(logger).map_err(|_| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Application logger already installed",
        )
    })?;
    log::set_max_level(log::LevelFilter::Info);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Panic payloads may contain user content; retain the source location and backtrace only.
        log::error!(target: "orbyte_desktop", "panic location={:?} backtrace={}", info.location(), std::backtrace::Backtrace::force_capture());
        log::logger().flush();
        previous(info);
    }));
    std::thread::Builder::new()
        .name("log-retention".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(3600));
            // Close a stale file before cleanup, including on Windows and after clock changes.
            if let Ok(mut state) = logger.0.lock() {
                if state.day != seconds(SystemTime::now()) / DAY {
                    state.file.take();
                }
                if let Err(error) = cleanup(&directory, SystemTime::now()) {
                    drop(state);
                    log::warn!(target: "orbyte_desktop", "log_cleanup_failed error={error}");
                }
            }
        })?;
    Ok(())
}
