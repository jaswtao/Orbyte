//! 文件规则、保存失败与真实监听验证。Author: jaswtao · Date: 2026-10-13
use orbyte_files::{
    index::FileIndex,
    ops::*,
    path_guard::resolve,
    watcher::{FileChange, FileManager},
    FileError,
};
use std::{fs, path::PathBuf, sync::mpsc, time::Duration};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("orbyte-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn request(&self, path: &str) -> WriteRequest {
        let file = read(&self.0, path, None).unwrap();
        WriteRequest {
            content: file.content,
            expected_version: Some(file.version),
            encoding: file.encoding,
            bom: file.bom,
            line_ending: file.line_ending,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bom_crlf_gbk_roundtrip_and_mixed_newlines() {
    let fixture = Fixture::new();
    for (path, data) in [
        ("bom.txt", b"\xef\xbb\xbfhello\r\nworld\r\n".to_vec()),
        (
            "gbk.txt",
            encoding_rs::GBK.encode("中文内容\r\n").0.into_owned(),
        ),
    ] {
        fs::write(fixture.0.join(path), &data).unwrap();
        let request = fixture.request(path);
        assert_eq!(request.line_ending, LineEnding::CRLF);
        write(&fixture.0, path, request).unwrap();
        assert_eq!(fs::read(fixture.0.join(path)).unwrap(), data);
    }
    fs::write(fixture.0.join("mixed.txt"), b"a\r\nb\nc\r\n").unwrap();
    write(&fixture.0, "mixed.txt", fixture.request("mixed.txt")).unwrap();
    assert_eq!(
        fs::read(fixture.0.join("mixed.txt")).unwrap(),
        b"a\r\nb\r\nc\r\n"
    );
    fs::write(fixture.0.join("cr.txt"), b"a\rb\n").unwrap();
    assert_eq!(read(&fixture.0, "cr.txt", None).unwrap().content, "a\nb\n");
    fs::write(fixture.0.join("gb18030.txt"), [0x81, 0x30, 0x81, 0x30]).unwrap();
    assert!(read(&fixture.0, "gb18030.txt", Some(Encoding::Gbk)).is_err());
    assert!(read(&fixture.0, "gbk.txt", Some(Encoding::Utf8)).is_err());
    assert_eq!(
        read(&fixture.0, "gbk.txt", Some(Encoding::Gbk))
            .unwrap()
            .content,
        "中文内容\n"
    );
}
#[test]
fn conflict_and_unmappable_leave_disk_intact_and_deleted_file_can_be_recreated() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file.txt"), "original").unwrap();
    let mut request = fixture.request("file.txt");
    request.content = "mine".into();
    fs::write(fixture.0.join("file.txt"), "external").unwrap();
    assert!(matches!(
        write(&fixture.0, "file.txt", request),
        Err(FileError::Conflict { .. })
    ));
    assert_eq!(
        fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
        "external"
    );
    let mut request = fixture.request("file.txt");
    request.encoding = Encoding::Gbk;
    request.content = "中文😀".into();
    assert!(matches!(
        write(&fixture.0, "file.txt", request),
        Err(FileError::Unmappable { position: 2, length: 2, .. })
    ));
    assert_eq!(
        fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
        "external"
    );
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    let request = fixture.request("file.txt");
    fs::remove_file(fixture.0.join("file.txt")).unwrap();
    write(&fixture.0, "file.txt", request).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
        "external"
    );
}
#[test]
fn rejects_oversize_binary_bad_encoding_missing_parent_and_unsafe_paths() {
    let fixture = Fixture::new();
    for (path, data) in [
        ("large", vec![b'a'; MAX_FILE_BYTES + 1]),
        ("binary", vec![b'a', 0]),
        ("invalid", vec![0xff]),
    ] {
        fs::write(fixture.0.join(path), data).unwrap();
        assert!(read(&fixture.0, path, None).is_err());
    }
    for path in ["", "../escape", "a:stream", "NUL.txt", "C:/outside"] {
        assert!(create(&fixture.0, path, false).is_err());
    }
    assert!(create(&fixture.0, "missing/file.txt", false).is_err());
    create(&fixture.0, "中文 空格", true).unwrap();
    create(&fixture.0, "中文 空格/a.txt", false).unwrap();
    assert!(create(&fixture.0, "中文 空格/a.txt", false).is_err());
    create(&fixture.0, "b.txt", false).unwrap();
    assert!(rename(&fixture.0, "b.txt", "中文 空格/a.txt").is_err());
    rename(&fixture.0, "b.txt", "c.txt").unwrap();
    rename(&fixture.0, "c.txt", "C.txt").unwrap();
    assert!(fs::read_dir(&fixture.0)
        .unwrap()
        .any(|e| e.unwrap().file_name() == "C.txt"));
    fs::create_dir(fixture.0.join(".git")).unwrap();
    let listing = list(&fixture.0, "").unwrap();
    assert!(listing.entries[0].is_directory);
    assert!(listing.entries.iter().all(|e| e.name != ".git"));
}
#[test]
fn index_includes_ignored_files_and_matches_filename_subsequence() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join(".gitignore"), "ignored/\n").unwrap();
    fs::create_dir(fixture.0.join("ignored")).unwrap();
    fs::write(fixture.0.join("ignored/hidden.rs"), "").unwrap();
    fs::write(fixture.0.join(".ignore"), "local.txt\n").unwrap();
    fs::write(fixture.0.join("local.txt"), "local").unwrap();
    fs::create_dir_all(fixture.0.join(".git/info")).unwrap();
    fs::write(fixture.0.join(".git/info/exclude"), "excluded.txt\n").unwrap();
    fs::write(fixture.0.join("excluded.txt"), "excluded").unwrap();
    fs::write(fixture.0.join(".git/config"), "").unwrap();
    fs::write(fixture.0.join(".orbyte-save.tmp"), "temporary").unwrap();
    fs::write(fixture.0.join(".hidden.txt"), "hidden").unwrap();
    fs::write(fixture.0.join("MainFile.rs"), "").unwrap();
    for include_ignored in [false, true] {
        let index = FileIndex::build_with_ignored(&fixture.0, include_ignored).unwrap();
        assert_eq!(index.search("mfr").paths, ["MainFile.rs"]);
        for (query, path) in [("hidden.rs", "ignored/hidden.rs"), ("local.txt", "local.txt"), ("excluded.txt", "excluded.txt")] {
            assert_eq!(index.search(query).paths, if include_ignored { vec![path] } else { vec![] });
        }
        assert_eq!(index.search(".hidden.txt").paths, [".hidden.txt"]);
        assert!(index.search("config").paths.is_empty());
        assert!(index.search(".orbyte-save.tmp").paths.is_empty());
    }
    assert_eq!(read(&fixture.0, "ignored/hidden.rs", None).unwrap().content, "");
}
#[test]
fn watcher_reports_external_writes_invalidates_index_and_replaces_subscription() {
    let fixture = Fixture::new();
    let files = FileManager::default();
    assert!(files
        .search("project", &fixture.0, "new")
        .unwrap()
        .paths
        .is_empty());
    let (tx, rx) = mpsc::channel();
    files
        .watch("project", &fixture.0, move |event| {
            tx.send(event).unwrap();
        })
        .unwrap();
    let (new_tx, new_rx) = mpsc::channel();
    files
        .watch("project", &fixture.0, move |event| {
            let _ = new_tx.send(event);
        })
        .unwrap();
    fs::write(fixture.0.join("new.txt"), "external").unwrap();
    let event = new_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(event, FileChange::Rescan { .. })
            || matches!(event, FileChange::Paths { paths } if paths.contains(&"new.txt".to_string()))
    );
    assert!(rx.try_recv().is_err());
    assert_eq!(
        files.search("project", &fixture.0, "new").unwrap().paths,
        ["new.txt"]
    );
    files.remove("project");
}

#[test]
fn watcher_refreshes_files_inside_ignored_directories() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join(".gitignore"), "ignored/\n").unwrap();
    fs::create_dir(fixture.0.join("ignored")).unwrap();
    let files = FileManager::default();
    let project = files.project("ignored", &fixture.0).unwrap();
    assert!(project.search("tracked").unwrap().paths.is_empty());
    assert!(project.search_with_ignored("tracked", true).unwrap().paths.is_empty());
    assert_eq!(project.rebuild_count(), 2);
    project.search("tracked").unwrap();
    project.search_with_ignored("tracked", true).unwrap();
    assert_eq!(project.rebuild_count(), 2);
    let (tx, rx) = mpsc::channel();
    files.watch("ignored", &fixture.0, move |event| { let _ = tx.send(event); }).unwrap();

    fs::write(fixture.0.join("ignored/tracked.txt"), "content").unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(project.search("tracked").unwrap().paths.is_empty());
    assert_eq!(project.search_with_ignored("tracked", true).unwrap().paths, ["ignored/tracked.txt"]);
    assert_eq!(project.rebuild_count(), 4);

    fs::rename(fixture.0.join("ignored/tracked.txt"), fixture.0.join("ignored/renamed.txt")).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(project.search_with_ignored("tracked", true).unwrap().paths.is_empty());
    assert_eq!(project.search_with_ignored("renamed", true).unwrap().paths, ["ignored/renamed.txt"]);
    assert!(project.search("renamed").unwrap().paths.is_empty());

    fs::remove_file(fixture.0.join("ignored/renamed.txt")).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(project.search_with_ignored("renamed", true).unwrap().paths.is_empty());
    assert!(project.search("renamed").unwrap().paths.is_empty());
}

#[test]
fn watcher_keeps_content_saves_cached_and_refreshes_ignore_rules() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join(".git/info")).unwrap();
    fs::write(fixture.0.join("existing.txt"), "initial").unwrap();
    let files = FileManager::default();
    let project = files.project("cache", &fixture.0).unwrap();
    project.search("existing").unwrap();
    let (tx, rx) = mpsc::channel();
    files.watch("cache", &fixture.0, move |event| { let _ = tx.send(event); }).unwrap();
    for save in [false, true] {
        if save {
            let mut request = fixture.request("existing.txt"); request.content = "saved".into();
            write(&fixture.0, "existing.txt", request).unwrap();
        } else { fs::write(fixture.0.join("existing.txt"), "modified").unwrap(); }
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(project.search("existing").unwrap().paths, ["existing.txt"]);
        assert_eq!(project.rebuild_count(), 1);
    }
    fs::write(fixture.0.join(".ignore"), "existing.txt\n").unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(project.search("existing").unwrap().paths.is_empty());
    assert_eq!(project.rebuild_count(), 2);
    assert_eq!(project.search_with_ignored("existing", true).unwrap().paths, ["existing.txt"]);
    assert_eq!(project.rebuild_count(), 3);
    fs::remove_file(fixture.0.join(".ignore")).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(project.search("existing").unwrap().paths, ["existing.txt"]);
    assert_eq!(project.rebuild_count(), 4);
    assert_eq!(project.search_with_ignored("existing", true).unwrap().paths, ["existing.txt"]);
    assert_eq!(project.rebuild_count(), 5);
    fs::remove_file(fixture.0.join("existing.txt")).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(project.search("existing").unwrap().paths.is_empty());
    assert!(project.search_with_ignored("existing", true).unwrap().paths.is_empty());
}
#[cfg(windows)]
#[test]
fn junction_escape_is_rejected_for_existing_and_new_files() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fs::write(outside.0.join("secret.txt"), "secret").unwrap();
    let link = fixture.0.join("escape");
    let status = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&link)
        .arg(&outside.0)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(resolve(&fixture.0, "escape/secret.txt", false).is_err());
    assert!(resolve(&fixture.0, "escape/new/file.txt", false).is_err());
    assert!(read(&fixture.0, "escape/secret.txt", None).is_err());
    assert!(create(&fixture.0, "escape/new.txt", false).is_err());
    assert!(!list(&fixture.0, "").unwrap().entries[0].accessible);
    assert!(FileIndex::build(&fixture.0)
        .unwrap()
        .search("secret")
        .paths
        .is_empty());
    fs::remove_dir(link).unwrap();
}
#[cfg(windows)]
#[test]
fn read_only_or_locked_file_survives_failed_save() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let path = fixture.0.join("file.txt");
    fs::write(&path, "original").unwrap();
    let mut request = fixture.request("file.txt");
    request.content = "mine".into();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    assert!(write(&fixture.0, "file.txt", request).is_err());
    drop(locked);
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let request = fixture.request("file.txt");
    assert!(write(&fixture.0, "file.txt", request).is_err());
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn atomic_replace_preserves_creation_time_and_hidden_attribute() {
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_ARCHIVE};
    let fixture = Fixture::new();
    let path = fixture.0.join("file.txt");
    fs::write(&path, "original").unwrap();
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_ARCHIVE) }, 0);
    let before = fs::metadata(&path).unwrap();
    let mut request = fixture.request("file.txt");
    request.content = "replacement".into();
    write(&fixture.0, "file.txt", request).unwrap();
    let after = fs::metadata(&path).unwrap();
    assert_eq!(after.creation_time(), before.creation_time());
    assert_eq!(after.file_attributes() & FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_HIDDEN);
    assert_eq!(fs::read_to_string(path).unwrap(), "replacement");
}

#[cfg(windows)]
#[test]
fn delete_uses_recycle_bin_and_can_restore_exact_test_file() {
    let fixture = Fixture::new();
    // trash uses the Shell display name, which can hide known file extensions.
    let filename = "recycle-test";
    let path = fixture.0.join(filename);
    fs::write(&path, "recoverable content").unwrap();
    // Match the Shell's canonical path, including expanded Windows TEMP aliases.
    let path = dunce::canonicalize(&path).unwrap();
    delete(&fixture.0, filename).unwrap();
    assert!(!path.exists());
    let item = trash::os_limited::list()
        .unwrap()
        .into_iter()
        .find(|item| item.original_path() == path)
        .unwrap_or_else(|| panic!("Test file should be in the recycle bin: {}", path.display()));
    trash::os_limited::restore_all([item]).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "recoverable content");
}
