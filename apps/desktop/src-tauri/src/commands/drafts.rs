//! Recovery storage IPC. Author: jaswtao · Date: 2026-10-24
use super::{project::StorageState, CmdResult};
use crate::state::AppState;
use orbyte_storage::EditorDraft;
use tauri::State;
use std::sync::atomic::Ordering;

#[tauri::command]
pub async fn list_editor_drafts(storage: State<'_, StorageState>) -> CmdResult<Vec<EditorDraft>> {
    storage.database()?.list_editor_drafts().await
}

#[tauri::command]
pub async fn put_editor_draft(storage: State<'_, StorageState>, runtime: State<'_, AppState>, draft: EditorDraft) -> CmdResult<()> {
    let _gate = runtime.file_writes.read().await;
    let _guard = runtime.terminal_lifecycle.lock().await;
    if runtime.close_started.load(Ordering::Acquire) { return Err("Application is closing".into()); }
    // Drafts remain writable when the original file or project directory is missing.
    // This command only writes to the managed database, never to a caller-provided file path.
    debug_assert_eq!(orbyte_storage::MAX_DRAFT_BYTES, orbyte_files::ops::MAX_FILE_BYTES);
    storage.database()?.put_editor_draft(&draft).await
}

#[tauri::command]
pub async fn delete_editor_draft(storage: State<'_, StorageState>, project_id: String, path: String, revision: String) -> CmdResult<()> {
    storage.database()?.delete_editor_draft(&project_id, &path, &revision).await
}

#[tauri::command]
pub async fn rename_editor_drafts(storage: State<'_, StorageState>, runtime: State<'_, AppState>, project_id: String, from: String, to: String) -> CmdResult<()> {
    let _guard = runtime.terminal_lifecycle.lock().await;
    let root = storage.database()?.list_projects().await?.into_iter().find(|p| p.id == project_id)
        .ok_or("Project no longer exists")?.path;
    storage.database()?.rename_editor_drafts(&project_id, &root, &from, &to).await
}
