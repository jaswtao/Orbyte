//! Terminal diagnostic log rotation and filtering. Author: jaswtao · Date: 2026-10-20
#[path = "../src/terminal_log.rs"]
mod terminal_log;

#[test]
fn rotates_logs_and_filters_unrelated_content() {
    let directory =
        std::env::temp_dir().join(format!("orbyte-terminal-log-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let current = directory.join("terminal.log");
    std::fs::File::create(&current)
        .unwrap()
        .set_len(2 * 1024 * 1024)
        .unwrap();
    std::fs::write(directory.join("terminal.log.1"), "previous-metadata").unwrap();
    std::fs::write(directory.join("terminal.log.2"), "oldest-metadata").unwrap();
    terminal_log::init(directory.clone()).unwrap();
    log::info!(target: "orbyte_terminal", "reconnect session=test inflight=1024");
    log::info!(target: "unrelated", "must-not-appear");
    log::logger().flush();
    let text = std::fs::read_to_string(&current).unwrap();
    assert!(text.contains("reconnect session=test inflight=1024"));
    assert!(!text.contains("must-not-appear"));
    assert_eq!(
        std::fs::read_to_string(directory.join("terminal.log.2")).unwrap(),
        "previous-metadata"
    );
    assert_eq!(
        std::fs::metadata(directory.join("terminal.log.1"))
            .unwrap()
            .len(),
        2 * 1024 * 1024
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 3);
    let occupied = terminal_log::init(directory.clone()).unwrap_err();
    assert_eq!(occupied.kind(), std::io::ErrorKind::AlreadyExists);
    // On Windows the process-wide logger holds the current file handle until this test process exits.
}
