//! 命令层集成测试：模拟 Tauri 宿主，使用真实 SQLite 与 Windows ConPTY。
//! Author: jaswtao
//! Date: 2026-10-13

#![cfg(windows)]

#[allow(dead_code)]
#[path = "../src/commands/mod.rs"]
mod commands;
#[path = "../src/state.rs"]
#[allow(dead_code)]
mod state;

use commands::project::StorageState;
use commands::terminal::*;
use commands::workspace::{load_workspace_layout, save_workspace_layout};
use orbyte_core::ProjectInput;
use orbyte_storage::Database;
use state::AppState;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{
    test::{mock_builder, mock_context, noop_assets},
    Manager,
};

fn channel() -> Channel<InvokeResponseBody> {
    Channel::new(|_| Ok(()))
}

#[tokio::test]
async fn ordinary_shell_does_not_use_native_clipboard_paste() {
    let (app, _, project) = fixture().await;
    let created = create_terminal(app.state(), app.state(), project.id, project.workspace_id,
        "cmd".into(), None, None, 120, 30, channel()).await.unwrap();
    assert!(!terminal_uses_native_paste(app.state(), created.terminal_id).await.unwrap());
    assert!(terminal_uses_native_paste(app.state(), "unknown-session".into()).await.is_err());
    app.state::<AppState>().terminals.stop_all();
}

#[tokio::test]
async fn native_paste_only_detects_codex_in_the_session_console() {
    use orbyte_platform::windows::process::process_image;
    use std::time::{Duration, Instant};

    struct Cleanup {
        terminals: std::sync::Arc<orbyte_terminal::TerminalManager>,
        directory: std::path::PathBuf,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.terminals.stop_all();
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    for (mode, expected) in [("separate", false), ("shared", true), ("detached", false)] {
        let (app, _, project) = fixture().await;
        let directory = std::env::temp_dir().join(format!("orbyte-native-paste-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let cleanup = Cleanup { terminals: app.state::<AppState>().terminals.clone(), directory };
        // A renamed cmd stays alive without depending on a Codex installation.
        let probe = cleanup.directory.join("codex.exe");
        std::fs::copy(std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/cmd.exe"), &probe).unwrap();
        let ready = cleanup.directory.join("ready.txt");
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = output.clone();
        let created = create_terminal(app.state(), app.state(), project.id, project.workspace_id,
            "cmd".into(), None, None, 120, 30, Channel::new(move |body| {
                if let InvokeResponseBody::Raw(frame) = body {
                    if frame.first() == Some(&orbyte_terminal::output::FRAME_OUTPUT) {
                        captured.lock().unwrap().extend_from_slice(&frame[orbyte_terminal::output::FRAME_HEADER_LEN..]);
                    }
                }
                Ok(())
            })).await.unwrap();
        cleanup.terminals.write(&created.terminal_id, b"\x1b[1;1R".to_vec()).unwrap();
        let probe_path = probe.to_string_lossy().replace('\'', "''");
        let ready_path = ready.to_string_lossy().replace('\'', "''");
        let launch = match mode {
            "shared" => format!("$p = Start-Process '{probe_path}' -ArgumentList '/D /Q /C ping.exe -n 120 127.0.0.1 >NUL' -NoNewWindow -PassThru"),
            "separate" => format!("$p = Start-Process '{probe_path}' -ArgumentList '/D /Q /C ping.exe -n 120 127.0.0.1 >NUL' -WindowStyle Hidden -PassThru"),
            _ => format!("$s = New-Object System.Diagnostics.ProcessStartInfo; $s.FileName = '{probe_path}'; $s.Arguments = '/D /Q /C ping.exe -n 120 127.0.0.1 >NUL'; $s.UseShellExecute = $false; $s.CreateNoWindow = $true; $p = [System.Diagnostics.Process]::Start($s)"),
        };
        cleanup.terminals.write(&created.terminal_id, format!(
            "powershell.exe -NoLogo -NoProfile -Command \"{launch}; $p.Id | Set-Content -LiteralPath '{ready_path}'\"\r"
        ).into_bytes()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&ready).ok().and_then(|text| text.trim().parse::<u32>().ok()) {
                break pid;
            }
            assert!(Instant::now() < deadline, "{mode}: probe did not start: {}", String::from_utf8_lossy(&output.lock().unwrap()));
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let session = cleanup.terminals.get(&created.terminal_id).unwrap();
        assert!(session.job_process_ids().contains(&pid), "{mode}: probe must remain in the session Job");
        assert_eq!(process_image(pid).unwrap(), probe);
        assert_eq!(terminal_uses_native_paste(app.state(), created.terminal_id).await.unwrap(), expected, "{mode}");
    }
}

#[tokio::test]
async fn active_terminal_limits_window_activation_to_its_own_live_processes() {
    let (app, _, project) = fixture().await;
    let mut sessions = Vec::new();
    for _ in 0..2 {
        let created = create_terminal(app.state(), app.state(), project.id.clone(), project.workspace_id.clone(),
            "cmd".into(), None, None, 120, 30, channel()).await.unwrap();
        let pid = app.state::<AppState>().terminals.get(&created.terminal_id).unwrap().pid().unwrap();
        sessions.push((created, pid));
    }
    let terminals = app.state::<AppState>().terminals.clone();
    assert!(!terminals.is_active_process(sessions[0].1));
    set_active_terminal(app.state(), Some(sessions[0].0.terminal_id.clone())).unwrap();
    assert!(terminals.is_active_process(sessions[0].1));
    assert!(!terminals.is_active_process(sessions[1].1));
    assert!(set_active_terminal(app.state(), Some("unknown-session".into())).is_err());
    assert!(terminals.is_active_process(sessions[0].1));
    set_active_terminal(app.state(), Some(sessions[1].0.terminal_id.clone())).unwrap();
    assert!(!terminals.is_active_process(sessions[0].1));
    assert!(terminals.is_active_process(sessions[1].1));
    set_active_terminal(app.state(), None).unwrap();
    assert!(!terminals.is_active_process(sessions[1].1));
    set_active_terminal(app.state(), Some(sessions[1].0.terminal_id.clone())).unwrap();
    terminals.begin_shutdown();
    assert!(!terminals.is_active_process(sessions[1].1));
    terminals.stop_all();
}

#[tokio::test]
async fn invalid_or_unavailable_launch_preset_does_not_create_config() {
    let (app, db, project) = fixture().await;
    let mut rejected = vec!["invalid-preset".to_string()];
    rejected.extend(orbyte_platform::launch::presets().iter().filter(|p| !p.available).map(|p| p.preset.clone()));
    for preset in rejected {
        assert!(create_terminal(app.state(), app.state(), project.id.clone(), project.workspace_id.clone(), "powershell".into(), Some(preset), None, 120, 30, channel()).await.is_err());
    }
    assert!(db.list_terminal_configs(&project.workspace_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn layout_load_cleans_only_orphan_sessions_and_rejects_invalid_saves() {
    let (app, db, project) = fixture().await;
    let mut created = Vec::new();
    for _ in 0..2 {
        created.push(
            create_terminal(
                app.state(),
                app.state(),
                project.id.clone(),
                project.workspace_id.clone(),
                "cmd".into(),
                None,
                None,
                120,
                30,
                channel(),
            )
            .await
            .unwrap(),
        );
    }
    let kept = &created[0];
    let orphan = &created[1];
    let raw = serde_json::json!({"schemaVersion":1,"activeTabId":"tab","tabs":[{"id":"tab","title":"CMD","root":{"type":"terminal","terminalConfigId":kept.terminal_config_id}}]}).to_string();
    save_workspace_layout(app.state(), app.state(), project.workspace_id.clone(), raw)
        .await
        .unwrap();
    let layout = load_workspace_layout(app.state(), app.state(), project.workspace_id.clone())
        .await
        .unwrap();
    let sessions = list_terminal_sessions(app.state()).await.unwrap();
    assert!(sessions.iter().any(|s| s.terminal_id == kept.terminal_id));
    assert!(!sessions.iter().any(|s| s.terminal_id == orphan.terminal_id));
    assert_eq!(
        db.list_terminal_configs(&project.workspace_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(save_workspace_layout(
        app.state(),
        app.state(),
        project.workspace_id.clone(),
        "{\"schemaVersion\":2,\"activeTabId\":null,\"tabs\":[]}".into()
    )
    .await
    .is_err());
    assert_eq!(
        load_workspace_layout(app.state(), app.state(), project.workspace_id.clone())
            .await
            .unwrap(),
        layout
    );
    remove_terminal(app.state(), app.state(), kept.terminal_config_id.clone())
        .await
        .unwrap();
}

async fn fixture() -> (
    tauri::App<tauri::test::MockRuntime>,
    Database,
    orbyte_core::Project,
) {
    let db = Database::open(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let root =
        orbyte_platform::path::normalize_project_path(std::env::temp_dir().to_str().unwrap())
            .unwrap();
    db.create_project(ProjectInput {
        name: "中文 空格项目".into(),
        path: root,
        group_id: None,
        favorite: false,
    })
    .await
    .unwrap();
    let project = db.list_projects().await.unwrap().remove(0);
    let app = mock_builder()
        .manage({
            let state = AppState::new();
            let directory = std::env::temp_dir().join("orbyte-shell-test");
            orbyte_platform::shell_integration::install(&directory).unwrap();
            state.integration_dir.set(directory).unwrap();
            state
        })
        .manage(StorageState(Ok(db.clone())))
        .build(mock_context(noop_assets()))
        .unwrap();
    (app, db, project)
}

#[tokio::test]
async fn create_restart_remove_and_failure_rollback() {
    let (app, db, project) = fixture().await;
    for (owner, shell, cwd, cols) in [
        ("wrong-owner", "cmd", "", 120),
        (project.id.as_str(), "bash", "", 120),
        (project.id.as_str(), "cmd", "../outside", 120),
        (
            project.id.as_str(),
            "cmd",
            "orbyte-nonexistent-0f9d3c2a",
            120,
        ),
        (project.id.as_str(), "cmd", "", 0),
    ] {
        assert!(create_terminal(
            app.state(),
            app.state(),
            owner.into(),
            project.workspace_id.clone(),
            shell.into(),
            None,
            Some(cwd.into()),
            cols,
            30,
            channel()
        )
        .await
        .is_err());
        assert!(db
            .list_terminal_configs(&project.workspace_id)
            .await
            .unwrap()
            .is_empty());
        assert!(app.state::<AppState>().terminals.list().is_empty());
    }
    let created = create_terminal(
        app.state(),
        app.state(),
        project.id.clone(),
        project.workspace_id.clone(),
        "cmd".into(),
        None,
        None,
        120,
        30,
        channel(),
    )
    .await
    .unwrap();
    assert_eq!(
        db.list_terminal_configs(&project.workspace_id)
            .await
            .unwrap()[0]
            .id,
        created.terminal_config_id
    );
    assert!(start_terminal(
        app.state(),
        app.state(),
        created.terminal_config_id.clone(),
        120,
        30,
        channel()
    )
    .await
    .unwrap_err()
    .contains("已有"));
    stop_terminal(app.state(), created.terminal_id.clone())
        .await
        .unwrap();
    assert_eq!(
        db.list_terminal_configs(&project.workspace_id)
            .await
            .unwrap()
            .len(),
        1
    );
    let restarted = start_terminal(
        app.state(),
        app.state(),
        created.terminal_config_id.clone(),
        120,
        30,
        channel(),
    )
    .await
    .unwrap();
    assert_ne!(restarted, created.terminal_id);
    assert_eq!(list_terminal_sessions(app.state()).await.unwrap().len(), 1);
    app.state::<AppState>()
        .close_started
        .store(true, std::sync::atomic::Ordering::Release);
    assert!(create_terminal(
        app.state(),
        app.state(),
        project.id.clone(),
        project.workspace_id.clone(),
        "cmd".into(),
        None,
        None,
        120,
        30,
        channel()
    )
    .await
    .unwrap_err()
    .contains("退出"));
    assert!(start_terminal(
        app.state(),
        app.state(),
        created.terminal_config_id.clone(),
        120,
        30,
        channel()
    )
    .await
    .unwrap_err()
    .contains("退出"));
    remove_terminal(app.state(), app.state(), created.terminal_config_id)
        .await
        .unwrap();
    assert!(db
        .list_terminal_configs(&project.workspace_id)
        .await
        .unwrap()
        .is_empty());
    assert!(list_terminal_sessions(app.state())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn project_path_change_and_deletion_clean_runtime_sessions() {
    let (app, db, project) = fixture().await;
    let created = create_terminal(
        app.state(),
        app.state(),
        project.id.clone(),
        project.workspace_id.clone(),
        "cmd".into(),
        None,
        None,
        120,
        30,
        channel(),
    )
    .await
    .unwrap();
    let root = std::env::temp_dir().join(format!(
        "Orbyte 中文 空格 {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    // Create through the command boundary so equivalent Windows paths are normalized.
    let duplicate = crate::commands::project::create_project(
        app.state(),
        ProjectInput {
            name: "另一项目".into(),
            path: root.join(".").to_str().unwrap().into(),
            group_id: None,
            favorite: false,
        },
    )
    .await
    .unwrap();
    let error = crate::commands::project::update_project(
        app.state(),
        app.state(),
        project.id.clone(),
        ProjectInput {
            name: project.name.clone(),
            path: root.to_str().unwrap().into(),
            group_id: None,
            favorite: false,
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("重复"));
    assert!(!app
        .state::<AppState>()
        .terminals
        .get(&created.terminal_id)
        .unwrap()
        .is_exited());
    db.delete_project(&duplicate).await.unwrap();
    let error = crate::commands::project::update_project(
        app.state(),
        app.state(),
        project.id.clone(),
        ProjectInput {
            name: project.name.clone(),
            path: root.to_str().unwrap().into(),
            group_id: Some("missing-group".into()),
            favorite: false,
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("分组不存在"));
    assert!(!app
        .state::<AppState>()
        .terminals
        .get(&created.terminal_id)
        .unwrap()
        .is_exited());
    crate::commands::project::update_project(
        app.state(),
        app.state(),
        project.id.clone(),
        ProjectInput {
            name: project.name,
            path: root.to_str().unwrap().into(),
            group_id: None,
            favorite: false,
        },
    )
    .await
    .unwrap();
    assert!(list_terminal_sessions(app.state())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        db.list_terminal_configs(&project.workspace_id)
            .await
            .unwrap()
            .len(),
        1
    );
    start_terminal(
        app.state(),
        app.state(),
        created.terminal_config_id,
        120,
        30,
        channel(),
    )
    .await
    .unwrap();
    crate::commands::project::delete_project(app.state(), app.state(), project.id)
        .await
        .unwrap();
    assert!(list_terminal_sessions(app.state())
        .await
        .unwrap()
        .is_empty());
    assert!(db
        .list_terminal_configs(&project.workspace_id)
        .await
        .unwrap()
        .is_empty());
    assert!(db.list_projects().await.unwrap().is_empty());
    std::fs::remove_dir(root).unwrap();
}
