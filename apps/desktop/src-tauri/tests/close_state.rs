//! 关闭确认的取消、重复请求及非法回复。Author: jaswtao · Date: 2026-10-13
#[allow(dead_code)]
#[path = "../src/state.rs"]
mod state;
#[allow(dead_code)]
#[path = "../src/commands/mod.rs"]
mod commands;
use state::{AppState, ClosePhase};
use std::sync::atomic::Ordering;

#[test]
fn cancelled_close_keeps_runtime_available_and_can_be_requested_again() {
    let state = AppState::new();
    state.unsaved_editor_count.store(2, Ordering::Release);
    let mut phase = state.close_phase.lock().unwrap();
    assert!(phase.request(state.unsaved_editor_count.load(Ordering::Acquire), 0));
    assert_eq!(*phase, ClosePhase::Pending);
    assert!(!phase.request(0, 0)); // 重复关闭不能跳过仍挂起的确认。
    assert!(!state.terminals.is_shutting_down());
    phase.resolve(false).unwrap();
    assert_eq!(*phase, ClosePhase::Idle);
    assert!(!state.close_started.load(Ordering::Acquire));
    assert!(phase.request(1, 0));
    phase.resolve(true).unwrap();
    assert_eq!(*phase, ClosePhase::ShuttingDown);
    assert!(!phase.request(0, 0));
    assert!(phase.resolve(false).is_err());
}

#[test]
fn clean_close_bypasses_confirmation_and_unsolicited_responses_are_rejected() {
    let mut phase = ClosePhase::Idle;
    assert!(phase.resolve(true).is_err());
    assert!(phase.request(0, 0));
    assert_eq!(phase, ClosePhase::ShuttingDown);
    assert!(phase.resolve(true).is_err());
    assert!(!phase.request(1, 0));
}

#[test]
fn queued_transfers_require_confirmation_without_unsaved_files() {
    let mut phase = ClosePhase::Idle;
    assert!(phase.request(0, 2));
    assert_eq!(phase, ClosePhase::Pending);
}

#[tokio::test]
async fn migration_failure_remains_visible_and_blocks_database_mutations() {
    use tauri::{test::{mock_builder, mock_context, noop_assets}, Manager};
    let diagnosis = "数据库迁移失败：测试诊断";
    let app = mock_builder()
        .manage(commands::project::StorageState(Err(diagnosis.into())))
        .build(mock_context(noop_assets()))
        .unwrap();
    assert_eq!(commands::project::get_settings(app.state()).await.unwrap_err(), diagnosis);
    assert_eq!(commands::project::list_projects(app.state()).await.unwrap_err(), diagnosis);
    assert_eq!(commands::project::create_group(app.state(), "新分组".into()).await.unwrap_err(), diagnosis);
}
