//! 项目管理 IPC，路径由后端规范化。
//! Author: jaswtao
//! Date: 2026-10-12

use super::CmdResult;
use crate::state::AppState;
use orbyte_core::{Project, ProjectGroup, ProjectInput};
use orbyte_storage::Database;
use serde_json::Value;
use std::collections::BTreeMap;
use tauri::State;

async fn remove_project_sessions(
    state: &AppState,
    db: &Database,
    project_id: &str,
) -> CmdResult<()> {
    let configs = db.list_project_terminal_configs(project_id).await?;
    let config_ids: std::collections::HashSet<_> = configs.iter().map(|c| c.id.as_str()).collect();
    let ids: Vec<_> = state
        .terminals
        .list()
        .into_iter()
        .filter(|s| {
            s.terminal_config_id
                .as_deref()
                .is_some_and(|id| config_ids.contains(id))
        })
        .map(|s| s.terminal_id)
        .collect();
    let terminals = state.terminals.clone();
    tauri::async_runtime::spawn_blocking(move || {
        for id in ids {
            terminals.remove(&id).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 保留启动错误供界面诊断；初始化失败时不提供数据库连接。
pub struct StorageState(pub Result<Database, String>);
impl StorageState {
    pub(super) fn database(&self) -> CmdResult<&Database> {
        self.0.as_ref().map_err(Clone::clone)
    }
}

async fn normalize(input: String) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        orbyte_platform::path::normalize_project_path(&input)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_projects(state: State<'_, StorageState>) -> CmdResult<Vec<Project>> {
    state.database()?.list_projects().await
}
#[tauri::command]
pub async fn set_project_hidden(
    state: State<'_, StorageState>,
    id: String,
    hidden: bool,
    replacement_id: Option<String>,
) -> CmdResult<()> {
    state.database()?.set_project_hidden(&id, hidden, replacement_id.as_deref()).await
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, StorageState>,
    mut input: ProjectInput,
) -> CmdResult<String> {
    input.path = normalize(input.path).await?;
    state.database()?.create_project(input).await
}
#[tauri::command]
pub async fn update_project(
    state: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    id: String,
    mut input: ProjectInput,
) -> CmdResult<()> {
    let _guard = runtime.terminal_lifecycle.lock().await;
    input.path = normalize(input.path).await?;
    orbyte_core::validate_name(&input.name)?;
    let db = state.database()?;
    let projects = db.list_projects().await?;
    let previous = projects.iter().find(|p| p.id == id).ok_or("项目已不存在")?;
    // 先拒绝可预见的数据校验错误，避免失败的编辑请求结束原项目终端。
    if projects
        .iter()
        .any(|p| p.id != id && p.path.to_lowercase() == input.path.to_lowercase())
    {
        return Err("该项目目录已经添加，请勿重复添加".into());
    }
    if let Some(group_id) = &input.group_id {
        if !db.list_groups().await?.iter().any(|g| &g.id == group_id) {
            return Err("分组不存在，请刷新后重试".into());
        }
    }
    if previous.path.to_lowercase() != input.path.to_lowercase() {
        remove_project_sessions(&runtime, db, &id).await?;
        runtime.files.remove(&id);
    }
    state.database()?.update_project(&id, input).await
}
#[tauri::command]
pub async fn delete_project(
    state: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    id: String,
) -> CmdResult<()> {
    let _guard = runtime.terminal_lifecycle.lock().await;
    remove_project_sessions(&runtime, state.database()?, &id).await?;
    runtime.files.remove(&id);
    state.database()?.delete_project(&id).await
}
#[tauri::command]
pub async fn list_groups(state: State<'_, StorageState>) -> CmdResult<Vec<ProjectGroup>> {
    state.database()?.list_groups().await
}
#[tauri::command]
pub async fn create_group(state: State<'_, StorageState>, name: String) -> CmdResult<String> {
    state.database()?.create_group(&name).await
}
#[tauri::command]
pub async fn update_group(
    state: State<'_, StorageState>,
    id: String,
    name: String,
) -> CmdResult<()> {
    state.database()?.update_group(&id, &name).await
}
#[tauri::command]
pub async fn delete_group(state: State<'_, StorageState>, id: String) -> CmdResult<()> {
    state.database()?.delete_group(&id).await
}
#[tauri::command]
pub async fn reorder_projects(
    state: State<'_, StorageState>,
    entries: Vec<(String, Option<String>)>,
) -> CmdResult<()> {
    state.database()?.reorder_projects(entries).await
}
#[tauri::command]
pub async fn reorder_groups(state: State<'_, StorageState>, ids: Vec<String>) -> CmdResult<()> {
    state.database()?.reorder_groups(ids).await
}
#[tauri::command]
pub async fn get_settings(state: State<'_, StorageState>) -> CmdResult<BTreeMap<String, Value>> {
    state.database()?.get_settings().await
}
#[tauri::command]
pub async fn set_setting(
    state: State<'_, StorageState>,
    key: String,
    mut value: Value,
) -> CmdResult<()> {
    if key == "default_directory" {
        if let Some(path) = value.as_str() {
            value = Value::String(normalize(path.into()).await?);
        }
    }
    state.database()?.set_setting(&key, value).await
}

#[tauri::command]
pub async fn set_settings(
    state: State<'_, StorageState>,
    mut input: BTreeMap<String, Value>,
) -> CmdResult<()> {
    if let Some(Value::String(path)) = input.get("default_directory") {
        let path = normalize(path.clone()).await?;
        input.insert("default_directory".into(), Value::String(path));
    }
    state.database()?.set_settings(input).await
}
