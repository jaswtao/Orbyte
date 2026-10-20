//! 工作区布局 IPC；与终端和项目生命周期串行执行。
//! Author: jaswtao
//! Date: 2026-10-13

use super::{project::StorageState, CmdResult};
use crate::state::AppState;
use orbyte_core::layout::Layout;
use tauri::State;

#[tauri::command]
pub async fn load_workspace_layout(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    workspace_id: String,
) -> CmdResult<Layout> {
    let _guard = runtime.terminal_lifecycle.lock().await;
    let db = storage.database()?;
    let before = db.list_terminal_configs(&workspace_id).await?;
    let layout = db.preview_workspace_layout(&workspace_id).await?;
    let referenced = layout.terminal_ids();
    let orphan_ids: std::collections::HashSet<_> = before
        .into_iter()
        .filter(|c| !referenced.contains(&c.id))
        .map(|c| c.id)
        .collect();
    let sessions: Vec<_> = runtime
        .terminals
        .list()
        .into_iter()
        .filter(|s| {
            s.terminal_config_id
                .as_ref()
                .is_some_and(|id| orphan_ids.contains(id))
        })
        .map(|s| s.terminal_id)
        .collect();
    let terminals = runtime.terminals.clone();
    tauri::async_runtime::spawn_blocking(move || {
        for id in sessions {
            terminals.remove(&id).map_err(|e| e.to_string())?;
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    db.load_workspace_layout(&workspace_id).await
}

#[tauri::command]
pub async fn save_workspace_layout(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    workspace_id: String,
    layout_json: String,
) -> CmdResult<()> {
    let _guard = runtime.terminal_lifecycle.lock().await;
    storage
        .database()?
        .save_workspace_layout(&workspace_id, &layout_json)
        .await
}
