//! 项目 Git 查询与 Worktree 创建。Author: jaswtao · Date: 2026-10-14
use super::project::StorageState;
use crate::state::AppState;
use orbyte_core::{Project, ProjectInput};
use orbyte_git::{GitStatus, Worktree};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tauri::State;
use tokio::sync::{watch, Mutex};
type StatusResult = Result<GitStatus, String>;
#[derive(Default, Clone)]
pub struct GitQueries(Arc<Mutex<HashMap<String, watch::Receiver<Option<StatusResult>>>>>);

async fn project(storage: &StorageState, id: &str) -> Result<Project, String> {
    storage
        .database()?
        .list_projects()
        .await?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("项目已不存在".into())
}
#[tauri::command]
pub fn git_available() -> bool {
    orbyte_git::find_git().is_some()
}

#[tauri::command]
pub async fn git_status(
    storage: State<'_, StorageState>,
    queries: State<'_, GitQueries>,
    project_id: String,
) -> StatusResult {
    let root = PathBuf::from(project(&storage, &project_id).await?.path);
    let mut map = queries.0.lock().await;
    let mut receiver = if let Some(receiver) = map.get(&project_id) {
        receiver.clone()
    } else {
        let (sender, receiver) = watch::channel(None);
        map.insert(project_id.clone(), receiver.clone());
        let queries = queries.inner().clone();
        tauri::async_runtime::spawn(async move {
            let result = tauri::async_runtime::spawn_blocking(move || orbyte_git::status(&root))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
            sender.send_replace(Some(result));
            queries.0.lock().await.remove(&project_id);
        });
        receiver
    };
    drop(map);
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        receiver.changed().await.map_err(|e| e.to_string())?;
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorktree {
    #[serde(flatten)]
    pub worktree: Worktree,
    pub project_id: Option<String>,
    pub is_main: bool,
}
#[tauri::command]
pub async fn git_worktree_list(
    storage: State<'_, StorageState>,
    project_id: String,
) -> Result<Vec<ProjectWorktree>, String> {
    let projects = storage.database()?.list_projects().await?;
    let root = projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or("项目已不存在")?
        .path
        .clone();
    let trees = tauri::async_runtime::spawn_blocking(move || {
        orbyte_git::worktree_list(&PathBuf::from(root))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(trees
        .into_iter()
        .enumerate()
        .map(|(index, worktree)| {
            let project_id = projects
                .iter()
                .find(|p| {
                    p.path
                        .replace('\\', "/")
                        .eq_ignore_ascii_case(&worktree.path.replace('\\', "/"))
                })
                .map(|p| p.id.clone());
            ProjectWorktree {
                worktree,
                project_id,
                is_main: index == 0,
            }
        })
        .collect())
}
#[tauri::command]
pub async fn git_worktree_add(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
    branch: String,
    create_branch: bool,
) -> Result<String, String> {
    let _writes = runtime.file_writes.read().await;
    let _guard = runtime.terminal_lifecycle.lock().await;
    if runtime
        .close_started
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return Err("应用正在退出".into());
    }
    let source = project(&storage, &project_id).await?;
    let name = format!("{} · {}", source.name, branch);
    orbyte_core::validate_name(&name)?;
    if path.len() < 3
        || !path.as_bytes()[0].is_ascii_alphabetic()
        || path.as_bytes()[1] != b':'
        || !matches!(path.as_bytes()[2], b'/' | b'\\')
    {
        return Err("目标必须是本地盘符绝对路径".into());
    }
    let root = PathBuf::from(&source.path);
    let target = PathBuf::from(&path);
    let normalized = tauri::async_runtime::spawn_blocking(move || {
        orbyte_git::worktree_add(&root, &target, &branch, create_branch)?;
        orbyte_platform::path::normalize_project_path(&target.to_string_lossy())
    })
    .await
    .map_err(|e| e.to_string())??;
    storage
        .database()?
        .create_project(ProjectInput {
            name,
            path: normalized.clone(),
            group_id: source.group_id,
            favorite: false,
        })
        .await
        .map_err(|e| format!("Worktree 已创建于 {normalized}，添加项目失败：{e}"))
}
