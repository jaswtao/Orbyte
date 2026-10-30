//! 文件 IPC：数据库提供项目根，阻塞 IO 在后台执行。Author: jaswtao · Date: 2026-10-13
use super::project::StorageState;
use crate::state::AppState;
use orbyte_files::{
    index::SearchResult,
    ops::{self, DirectoryListing, Encoding, TextFile, WriteRequest},
    watcher::FileChange,
    FileError, FileResult,
};
use std::{path::PathBuf, sync::atomic::Ordering};
use tauri::{ipc::Channel, State};
use tokio::sync::RwLockReadGuard;

async fn root(storage: &StorageState, id: &str) -> FileResult<PathBuf> {
    storage
        .database()?
        .list_projects()
        .await?
        .into_iter()
        .find(|p| p.id == id)
        .map(|p| p.path.into())
        .ok_or_else(|| FileError::from("项目已不存在".to_string()))
}
/// 只在读取项目根时与项目修改/删除串行，耗时 IO 不阻塞终端生命周期操作。
async fn locked_root(storage: &StorageState, runtime: &AppState, id: &str) -> FileResult<PathBuf> {
    let _guard = runtime.terminal_lifecycle.lock().await;
    root(storage, id).await
}
/// 返回的闸门须保持到修改完成，退出清理据此等待进行中的写入。
async fn writable_root<'a>(
    storage: &StorageState,
    runtime: &'a AppState,
    id: &str,
) -> FileResult<(RwLockReadGuard<'a, ()>, PathBuf)> {
    let gate = runtime.file_writes.read().await;
    let _guard = runtime.terminal_lifecycle.lock().await;
    if runtime.close_started.load(Ordering::Acquire) {
        return Err("应用正在退出，不能修改文件".to_string().into());
    }
    Ok((gate, root(storage, id).await?))
}
async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> FileResult<T> + Send + 'static,
) -> FileResult<T> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|e| FileError::from(e.to_string()))?
}
#[tauri::command]
pub async fn reveal_path(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
) -> FileResult<()> {
    let root = locked_root(&storage, &runtime, &project_id).await?;
    blocking(move || {
        let target = orbyte_files::path_guard::resolve(&root, &path, true)?;
        orbyte_platform::file_manager::open(&target)?;
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn list_directory(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
) -> FileResult<DirectoryListing> {
    let root = locked_root(&storage, &runtime, &project_id).await?;
    blocking(move || ops::list(&root, &path)).await
}
#[tauri::command]
pub async fn read_file(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
    encoding: Option<Encoding>,
) -> FileResult<TextFile> {
    let root = locked_root(&storage, &runtime, &project_id).await?;
    blocking(move || ops::read(&root, &path, encoding)).await
}
#[tauri::command]
pub async fn write_file(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
    request: WriteRequest,
) -> FileResult<TextFile> {
    let (_gate, root) = writable_root(&storage, &runtime, &project_id).await?;
    blocking(move || ops::write(&root, &path, request)).await
}
#[tauri::command]
pub async fn create_file(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
) -> FileResult<()> {
    let (_gate, root) = writable_root(&storage, &runtime, &project_id).await?;
    blocking(move || ops::create(&root, &path, false)).await
}
#[tauri::command]
pub async fn create_directory(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
) -> FileResult<()> {
    let (_gate, root) = writable_root(&storage, &runtime, &project_id).await?;
    blocking(move || ops::create(&root, &path, true)).await
}
#[tauri::command]
pub async fn rename_path(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    from: String,
    to: String,
) -> FileResult<()> {
    let (_gate, root) = writable_root(&storage, &runtime, &project_id).await?;
    blocking(move || ops::rename(&root, &from, &to)).await
}
#[tauri::command]
pub async fn delete_path(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    path: String,
) -> FileResult<()> {
    let (_gate, root) = writable_root(&storage, &runtime, &project_id).await?;
    blocking(move || ops::delete(&root, &path)).await
}
#[tauri::command]
pub async fn search_files(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    query: String,
    include_ignored: Option<bool>,
) -> FileResult<SearchResult> {
    // 项目句柄必须在锁内获取，避免项目移除后又登记旧根目录的监听。
    let project = {
        let _guard = runtime.terminal_lifecycle.lock().await;
        if runtime.close_started.load(Ordering::Acquire) {
            return Err("应用正在退出，不能建立文件索引或监听".to_string().into());
        }
        let root = root(&storage, &project_id).await?;
        let files = runtime.files.clone();
        blocking(move || files.project(&project_id, &root)).await?
    };
    blocking(move || project.search_with_ignored(&query, include_ignored.unwrap_or(false))).await
}
#[tauri::command]
pub async fn watch_project_files(
    storage: State<'_, StorageState>,
    runtime: State<'_, AppState>,
    project_id: String,
    on_event: Channel<FileChange>,
) -> FileResult<()> {
    let _guard = runtime.terminal_lifecycle.lock().await;
    if runtime.close_started.load(Ordering::Acquire) {
        return Err("应用正在退出，不能建立文件监听".to_string().into());
    }
    let root = root(&storage, &project_id).await?;
    let files = runtime.files.clone();
    blocking(move || {
        files.watch(&project_id, &root, move |event| {
            let _ = on_event.send(event);
        })
    })
    .await
}
#[tauri::command]
pub fn set_unsaved_editor_count(runtime: State<'_, AppState>, count: usize) {
    runtime.unsaved_editor_count.store(count, Ordering::Release);
}
