//! Diagnostic rotation, retention and privacy regression tests. Author: jaswtao · Date: 2026-10-23
#[path = "../src/app_log.rs"]
mod app_log;

use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const DAY: u64 = 86_400;
fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("orbyte-log-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}
fn at(day: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(day * DAY)
}
fn path(dir: &std::path::Path, day: u64, part: u32) -> PathBuf {
    dir.join(format!("orbyte-{day:05}-{part:04}.log"))
}

#[test]
fn rotates_by_size_day_and_restart_without_losing_history() {
    let dir = directory();
    fs::File::create(path(&dir, 100, 0))
        .unwrap()
        .set_len(2 * 1024 * 1024)
        .unwrap();
    {
        let mut state = app_log::FileState::new(dir.clone());
        state.append(at(100), b"first\n").unwrap();
    }
    {
        let mut state = app_log::FileState::new(dir.clone());
        state.append(at(100), b"restart\n").unwrap();
        state.append(at(101), b"next-day\n").unwrap();
    }
    assert_eq!(
        fs::read_to_string(path(&dir, 100, 1)).unwrap(),
        "first\nrestart\n"
    );
    assert_eq!(
        fs::read_to_string(path(&dir, 101, 0)).unwrap(),
        "next-day\n"
    );
    assert_eq!(
        fs::metadata(path(&dir, 100, 0)).unwrap().len(),
        2 * 1024 * 1024
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn removes_expired_buckets_including_recently_modified_old_files() {
    let dir = directory();
    for day in [69, 70, 71, 100] {
        fs::write(path(&dir, day, 0), "metadata").unwrap();
    }
    fs::write(path(&dir, 70, 1), "another-part").unwrap();
    fs::write(dir.join("other.log"), "unrelated").unwrap();
    fs::write(dir.join("orbyte-invalid-0000.log"), "unrelated").unwrap();
    let legacy = fs::File::create(dir.join("terminal.log.1")).unwrap();
    legacy
        .set_times(fs::FileTimes::new().set_modified(at(69)))
        .unwrap();
    drop(legacy);
    fs::write(dir.join("terminal.log"), "recent-legacy").unwrap();
    app_log::cleanup(&dir, at(100)).unwrap();
    assert!(!path(&dir, 69, 0).exists());
    assert!(!path(&dir, 70, 0).exists());
    assert!(!path(&dir, 70, 1).exists());
    assert!(path(&dir, 71, 0).exists());
    assert!(path(&dir, 100, 0).exists());
    assert!(!dir.join("terminal.log.1").exists());
    assert!(dir.join("terminal.log").exists());
    assert!(dir.join("other.log").exists());
    assert!(dir.join("orbyte-invalid-0000.log").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn day_transition_cleans_without_restart_and_handles_clock_rollback() {
    let dir = directory();
    {
        let mut state = app_log::FileState::new(dir.clone());
        state.append(at(100), b"initial\n").unwrap();
        state.append(at(130), b"later\n").unwrap();
        assert!(!path(&dir, 100, 0).exists());
        state.append(at(129), b"clock-rollback\n").unwrap();
        assert!(path(&dir, 129, 0).exists());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rotation_failure_does_not_grow_full_file_and_recovers_after_retry() {
    let dir = directory();
    fs::File::create(path(&dir, 100, 0))
        .unwrap()
        .set_len(2 * 1024 * 1024)
        .unwrap();
    fs::create_dir(path(&dir, 100, 1)).unwrap();
    {
        let mut state = app_log::FileState::new(dir.clone());
        assert!(state.append(at(100), b"metadata\n").is_err());
        assert_eq!(
            fs::metadata(path(&dir, 100, 0)).unwrap().len(),
            2 * 1024 * 1024
        );
        fs::remove_dir(path(&dir, 100, 1)).unwrap();
        state.append(at(100), b"recovered\n").unwrap();
    }
    assert_eq!(
        fs::read_to_string(path(&dir, 100, 1)).unwrap(),
        "recovered\n"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn structured_logs_capture_application_and_terminal_events_and_escape_lines() {
    let dir = directory();
    app_log::init(dir.clone()).unwrap();
    log::info!(target: "orbyte_terminal", "reconnect session=test");
    log::info!(target: "orbyte_desktop", "database_ready");
    log::error!(target: "orbyte_frontend", "event=window_error\ninjected-line");
    log::warn!(target: "dependency", "dependency warning");
    log::info!(target: "dependency", "unrelated-info");
    log::debug!(target: "orbyte_terminal", "debug-noise");
    log::logger().flush();
    let text = fs::read_to_string(path(
        &dir,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / DAY,
        0,
    ))
    .unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert!(text.contains("database_ready"));
    assert!(text.contains("reconnect session=test"));
    assert!(!text.contains("unrelated-info"));
    assert!(!text.contains("debug-noise"));
    assert_eq!(lines[2]["message"], "event=window_error\ninjected-line");
    assert!(lines[0]["timestamp_ms"].is_u64());
    assert_eq!(lines[0]["pid"], std::process::id());
    assert_eq!(
        app_log::init(dir).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    // The process-wide logger retains its Windows file handle until process exit.
}
