//! 文件命令边界使用真实磁盘和 SQLite。Author: jaswtao · Date: 2026-10-13
#![cfg(windows)]
#[allow(dead_code)]
#[path = "../src/commands/mod.rs"]
mod commands;
#[allow(dead_code)]
#[path = "../src/state.rs"]
mod state;
use commands::{files::*, project::StorageState};
use orbyte_files::ops::{Encoding, LineEnding, WriteRequest};
use tauri::{
    test::{mock_builder, mock_context, noop_assets},
    Manager,
};

#[tokio::test]
async fn file_commands_use_project_root_reject_escape_and_track_unsaved_count() {
    let root = std::env::temp_dir().join(format!("orbyte-command-files-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let db = orbyte_storage::Database::open(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let id = db
        .create_project(orbyte_core::ProjectInput {
            name: "文件项目".into(),
            path: root.to_string_lossy().into_owned(),
            group_id: None,
            favorite: false,
        })
        .await
        .unwrap();
    let app = mock_builder()
        .manage(state::AppState::new())
        .manage(StorageState(Ok(db)))
        .build(mock_context(noop_assets()))
        .unwrap();
    assert!(
        create_file(app.state(), app.state(), id.clone(), "../escape".into())
            .await
            .is_err()
    );
    for (project, path) in [
        (id.clone(), "../escape"),
        (id.clone(), "missing-file.txt"),
        ("missing-project".into(), ""),
    ] {
        assert!(reveal_path(app.state(), app.state(), project, path.into())
            .await
            .is_err());
    }
    assert!(read_file(
        app.state(),
        app.state(),
        "missing-project".into(),
        "file.txt".into(),
        None
    )
    .await
    .is_err());
    create_file(app.state(), app.state(), id.clone(), "file.txt".into())
        .await
        .unwrap();
    let initial = read_file(
        app.state(),
        app.state(),
        id.clone(),
        "file.txt".into(),
        None,
    )
    .await
    .unwrap();
    write_file(
        app.state(),
        app.state(),
        id.clone(),
        "file.txt".into(),
        WriteRequest {
            content: "中文内容\n".into(),
            expected_version: Some(initial.version),
            encoding: Encoding::Gbk,
            bom: false,
            line_ending: LineEnding::CRLF,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        read_file(
            app.state(),
            app.state(),
            id.clone(),
            "file.txt".into(),
            None
        )
        .await
        .unwrap()
        .encoding,
        Encoding::Gbk
    );
    assert_eq!(
        list_directory(app.state(), app.state(), id.clone(), "".into())
            .await
            .unwrap()
            .entries
            .len(),
        1
    );
    rename_path(
        app.state(),
        app.state(),
        id.clone(),
        "file.txt".into(),
        "renamed.txt".into(),
    )
    .await
    .unwrap();
    assert!(root.join("renamed.txt").exists());
    set_unsaved_editor_count(app.state(), 2);
    assert_eq!(
        app.state::<state::AppState>()
            .unsaved_editor_count
            .load(std::sync::atomic::Ordering::Acquire),
        2
    );
    set_unsaved_editor_count(app.state(), 0);
    let runtime = app.state::<state::AppState>();
    let cleanup = runtime.file_writes.write().await;
    let blocked = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        create_file(app.state(), app.state(), id.clone(), "gated.txt".into()),
    ).await;
    assert!(blocked.is_err(), "文件修改必须等待退出清理持有的写入闸门");
    drop(cleanup);
    create_file(app.state(), app.state(), id.clone(), "gated.txt".into()).await.unwrap();
    app.state::<state::AppState>().close_started.store(true, std::sync::atomic::Ordering::Release);
    assert!(create_file(app.state(), app.state(), id.clone(), "during-exit.txt".into()).await.is_err());
    assert!(rename_path(app.state(), app.state(), id, "renamed.txt".into(), "during-exit.txt".into()).await.is_err());
    assert!(root.join("renamed.txt").exists());
    assert!(!root.join("during-exit.txt").exists());
    std::fs::remove_dir_all(root).unwrap();
}
