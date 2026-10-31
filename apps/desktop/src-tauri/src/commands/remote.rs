//! SSH connection, session, and SFTP commands.
//!
//! Passwords stay in the command argument only long enough to reach the batch.
//! They are not written to logs. Download targets are grants created by a native dialog.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::sync::Arc;

use orbyte_core::{validate_ssh_connection, RemoteLayout};
use orbyte_ssh::{
    Credential, EnqueueRequest, LocalGrant, LocalGrantKind, NewHostKey, SftpRequest, ShellRequest, SshUiEvent, StoredHostKey,
};
use orbyte_terminal::{FrameSink, TerminalSize};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use super::project::StorageState;
use super::{err_text, CmdResult};
use crate::state::AppState;

struct ChannelSink(Channel<InvokeResponseBody>);

impl FrameSink for ChannelSink {
    fn send(&self, frame: Vec<u8>) -> bool {
        self.0.send(InvokeResponseBody::Raw(frame)).is_ok()
    }
}

fn output_sink(channel: Channel<InvokeResponseBody>) -> Arc<dyn FrameSink> {
    Arc::new(ChannelSink(channel))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectBatch {
    pub batch_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedTerminal {
    pub terminal_id: String,
    pub attempt_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedSftp {
    pub attempt_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathGrant {
    pub token: String,
    pub count: usize,
    pub labels: Vec<String>,
}

/// Host lookup runs on SSH or blocking threads, so it may block on the app runtime.
pub fn attach_host_store(ssh: &orbyte_ssh::SshHub, db: orbyte_storage::Database) {
    let handle = tauri::async_runtime::handle();
    let lookup_db = db.clone();
    let lookup_handle = handle.clone();
    ssh.set_host_store(
        Arc::new(move |host, port| {
            lookup_handle.block_on(lookup_db.list_known_host_records(host, port)).map(|rows| {
                rows.into_iter()
                    .map(|row| StoredHostKey {
                        algorithm: row.algorithm,
                        public_key: row.public_key,
                        fingerprint: row.fingerprint_sha256,
                    })
                    .collect()
            })
        }),
        Arc::new(move |key: NewHostKey| {
            handle.block_on(db.save_known_host(&key.host, key.port, &key.algorithm, &key.public_key, &key.fingerprint))
        }),
    );
}

#[tauri::command]
pub async fn list_ssh_connections(storage: tauri::State<'_, StorageState>) -> CmdResult<Vec<orbyte_storage::SshConnection>> {
    storage.database()?.list_ssh_connections().await
}

#[tauri::command]
pub async fn create_ssh_connection(
    storage: tauri::State<'_, StorageState>,
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    auth_type: String,
    private_key_path: Option<String>,
) -> CmdResult<String> {
    let input = connection_input(name, host, port, username, auth_type, private_key_path)?;
    storage.database()?.create_ssh_connection(input).await
}

#[tauri::command]
pub async fn update_ssh_connection(
    storage: tauri::State<'_, StorageState>,
    id: String,
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    auth_type: String,
    private_key_path: Option<String>,
) -> CmdResult<()> {
    let input = connection_input(name, host, port, username, auth_type, private_key_path)?;
    storage.database()?.update_ssh_connection(&id, input).await
}

#[tauri::command]
pub async fn delete_ssh_connection(state: tauri::State<'_, AppState>, storage: tauri::State<'_, StorageState>, id: String) -> CmdResult<()> {
    let db = storage.database()?.clone();
    let ssh = state.ssh.clone();
    let workspace = db.open_remote_workspace(&id).await.ok();
    if let Some(workspace) = workspace {
        let (_, transfers) = ssh.list_transfers();
        for transfer in transfers {
            if transfer.workspace_id == workspace.id && transfer_is_active(&transfer.state) {
                let _ = ssh.cancel_transfer(&transfer.id);
            }
        }
        let configs: std::collections::HashSet<_> = workspace.terminals.iter().map(|terminal| terminal.id.clone()).collect();
        for session in ssh.list_terminals() {
            if session.terminal_config_id.as_ref().is_some_and(|id| configs.contains(id)) {
                let _ = ssh.stop_terminal(&session.terminal_id);
            }
        }
    }
    db.delete_ssh_connection(&id).await
}

#[tauri::command]
pub async fn list_ssh_known_hosts(storage: tauri::State<'_, StorageState>, host: String, port: u16) -> CmdResult<Vec<orbyte_storage::KnownHost>> {
    storage.database()?.list_known_host_views(&host, port).await
}

#[tauri::command]
pub async fn forget_ssh_host_key(storage: tauri::State<'_, StorageState>, id: String) -> CmdResult<()> {
    storage.database()?.forget_known_host(&id).await
}

#[tauri::command]
pub async fn load_remote_workspace(storage: tauri::State<'_, StorageState>, connection_id: String) -> CmdResult<orbyte_storage::RemoteWorkspaceRecord> {
    storage.database()?.open_remote_workspace(&connection_id).await
}

#[tauri::command]
pub async fn save_remote_workspace(storage: tauri::State<'_, StorageState>, workspace_id: String, layout: RemoteLayout) -> CmdResult<()> {
    storage.database()?.save_remote_layout(&workspace_id, &layout).await
}

#[tauri::command]
pub async fn set_remote_directory(storage: tauri::State<'_, StorageState>, workspace_id: String, directory: String) -> CmdResult<()> {
    let directory = orbyte_ssh::normalize_remote(&directory)?;
    storage.database()?.set_remote_directory(&workspace_id, Some(&directory)).await
}

#[tauri::command]
pub async fn create_remote_terminal_config(storage: tauri::State<'_, StorageState>, workspace_id: String, name: String) -> CmdResult<String> {
    storage.database()?.create_remote_terminal(&workspace_id, &name).await
}

#[tauri::command]
pub async fn remove_remote_terminal_config(storage: tauri::State<'_, StorageState>, workspace_id: String, terminal_config_id: String) -> CmdResult<()> {
    storage.database()?.delete_remote_terminal(&workspace_id, &terminal_config_id).await
}

#[tauri::command]
pub fn begin_ssh_connect(state: tauri::State<'_, AppState>, auth_type: String, events: Channel<Value>) -> CmdResult<ConnectBatch> {
    let sink = Arc::new(move |event: SshUiEvent| {
        if let Ok(value) = serde_json::to_value(&event) {
            let _ = events.send(value);
        }
    });
    let batch_id = state.ssh.begin_batch(auth_type, sink).map_err(err_text)?;
    Ok(ConnectBatch { batch_id })
}

#[tauri::command]
pub async fn connect_ssh_terminal(
    state: tauri::State<'_, AppState>,
    storage: tauri::State<'_, StorageState>,
    batch_id: String,
    connection_id: String,
    config_id: String,
    cols: u16,
    rows: u16,
    channel: Channel<InvokeResponseBody>,
) -> CmdResult<ConnectedTerminal> {
    let connection = storage.database()?.ssh_connection(&connection_id).await?;
    let size = TerminalSize { cols, rows }.validate().map_err(err_text)?;
    let terminal_id = Uuid::new_v4().to_string();
    let attempt_id = Uuid::new_v4().to_string();
    let credential = credential(&connection)?;
    let request = ShellRequest {
        batch_id,
        attempt_id: attempt_id.clone(),
        terminal_id: terminal_id.clone(),
        config_id,
        host: connection.host,
        port: connection.port,
        username: connection.username,
        credential,
        cols: size.cols,
        rows: size.rows,
        sink: output_sink(channel),
    };
    let ssh = state.ssh.clone();
    tauri::async_runtime::spawn_blocking(move || ssh.connect_shell(request).map_err(err_text)).await.map_err(err_text)??;
    Ok(ConnectedTerminal { terminal_id, attempt_id })
}

#[tauri::command]
pub async fn connect_sftp(
    state: tauri::State<'_, AppState>,
    storage: tauri::State<'_, StorageState>,
    batch_id: String,
    connection_id: String,
    workspace_id: String,
) -> CmdResult<ConnectedSftp> {
    let connection = storage.database()?.ssh_connection(&connection_id).await?;
    let attempt_id = Uuid::new_v4().to_string();
    let credential = credential(&connection)?;
    let request = SftpRequest {
        batch_id,
        attempt_id: attempt_id.clone(),
        workspace_id,
        host: connection.host,
        port: connection.port,
        username: connection.username,
        credential,
    };
    let ssh = state.ssh.clone();
    tauri::async_runtime::spawn_blocking(move || ssh.connect_sftp(request).map_err(err_text)).await.map_err(err_text)??;
    Ok(ConnectedSftp { attempt_id })
}

#[tauri::command]
pub fn answer_ssh_challenge(state: tauri::State<'_, AppState>, challenge_id: String, decision: String) -> CmdResult<()> {
    state.ssh.answer_challenge(&challenge_id, &decision).map_err(err_text)
}

#[tauri::command]
pub fn submit_ssh_credential(state: tauri::State<'_, AppState>, batch_id: String, secret: String) -> CmdResult<()> {
    state.ssh.submit_secret(&batch_id, secret).map_err(err_text)
}

#[tauri::command]
pub fn cancel_ssh_attempt(state: tauri::State<'_, AppState>, attempt_id: String) -> CmdResult<()> {
    state.ssh.cancel_attempt(&attempt_id).map_err(err_text)
}

#[tauri::command]
pub fn list_remote_terminal_sessions(state: tauri::State<'_, AppState>) -> Vec<orbyte_terminal::TerminalSessionInfo> {
    state.ssh.list_terminals()
}

#[tauri::command]
pub fn list_remote_directory(state: tauri::State<'_, AppState>, workspace_id: String, path: String, events: Channel<Value>) -> CmdResult<String> {
    let request_id = Uuid::new_v4().to_string();
    state
        .ssh
        .list_directory(&workspace_id, &path, &request_id, Arc::new(move |batch| {
            if let Ok(value) = serde_json::to_value(&batch) {
                let _ = events.send(value);
            }
        }))
        .map_err(err_text)?;
    Ok(request_id)
}

#[tauri::command]
pub fn cancel_remote_browse(state: tauri::State<'_, AppState>, request_id: String) {
    state.ssh.cancel_browse(&request_id);
}

#[tauri::command]
pub async fn create_remote_directory(state: tauri::State<'_, AppState>, workspace_id: String, path: String) -> CmdResult<()> {
    let ssh = state.ssh.clone();
    tauri::async_runtime::spawn_blocking(move || ssh.create_directory(&workspace_id, &path).map_err(err_text)).await.map_err(err_text)?
}

#[tauri::command]
pub async fn canonicalize_remote_path(state: tauri::State<'_, AppState>, workspace_id: String, path: String) -> CmdResult<String> {
    let ssh = state.ssh.clone();
    tauri::async_runtime::spawn_blocking(move || ssh.canonicalize(&workspace_id, &path).map_err(err_text)).await.map_err(err_text)?
}

#[tauri::command]
pub async fn select_transfer_paths(app: tauri::AppHandle, workspace_id: String, kind: String) -> CmdResult<Option<PathGrant>> {
    let dialog_app = app.clone();
    let dialog_kind = kind.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || pick_paths(&dialog_app, &dialog_kind)).await.map_err(err_text)??;
    let Some(paths) = picked else { return Ok(None); };
    if paths.is_empty() {
        return Ok(None);
    }
    let labels = paths.iter().filter_map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned())).collect();
    let count = paths.len();
    let grant_kind = if kind == "download" { LocalGrantKind::Download } else { LocalGrantKind::Upload };
    let token = Uuid::new_v4().to_string();
    app.state::<AppState>().ssh.insert_grant(LocalGrant { token: token.clone(), workspace_id, kind: grant_kind, paths });
    Ok(Some(PathGrant { token, count, labels }))
}

#[tauri::command]
pub fn enqueue_remote_transfer(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
    token: String,
    client_request_id: String,
    remote_directory: String,
    remote_names: Vec<String>,
) -> CmdResult<String> {
    state.ssh.enqueue_transfer(EnqueueRequest { workspace_id, token, client_request_id, remote_directory, remote_names }).map_err(err_text)
}

#[tauri::command]
pub fn list_remote_transfers(state: tauri::State<'_, AppState>) -> Value {
    let (revision, transfers) = state.ssh.list_transfers();
    json!({ "revision": revision, "transfers": transfers })
}

#[tauri::command]
pub fn subscribe_remote_transfers(state: tauri::State<'_, AppState>, events: Channel<Value>) -> CmdResult<u64> {
    let (revision, _) = state.ssh.list_transfers();
    state.ssh.subscribe_transfers(Arc::new(move |views| {
        let _ = events.send(json!({ "transfers": views }));
    }));
    Ok(revision)
}

#[tauri::command]
pub fn cancel_remote_transfer(state: tauri::State<'_, AppState>, transfer_id: String) -> CmdResult<()> {
    state.ssh.cancel_transfer(&transfer_id).map_err(err_text)
}

#[tauri::command]
pub fn resolve_transfer_conflict(state: tauri::State<'_, AppState>, transfer_id: String, choice: String, apply_all: bool) -> CmdResult<()> {
    state.ssh.resolve_conflict(&transfer_id, &choice, apply_all).map_err(err_text)
}

fn connection_input(
    name: String,
    host: String,
    port: Option<u16>,
    username: String,
    auth_type: String,
    private_key_path: Option<String>,
) -> CmdResult<orbyte_core::SshConnectionInput> {
    validate_ssh_connection(&name, &host, port, &username, &auth_type, private_key_path.as_deref())
}

fn transfer_is_active(state: &str) -> bool {
    matches!(state, "queued" | "scanning" | "running" | "awaitingConflict" | "cancelling")
}

fn credential(connection: &orbyte_storage::SshConnection) -> CmdResult<Credential> {
    match connection.auth_type.as_str() {
        "password" => Ok(Credential::Password),
        "privateKey" => Ok(Credential::PrivateKey {
            path: connection.private_key_path.clone().filter(|path| !path.is_empty()).ok_or("缺少私钥路径")?,
        }),
        _ => Err("认证方式无效".into()),
    }
}

fn pick_paths(app: &tauri::AppHandle, kind: &str) -> CmdResult<Option<Vec<std::path::PathBuf>>> {
    let dialog = app.dialog().file();
    let files = match kind {
        "upload-files" => dialog.blocking_pick_files(),
        "upload-folders" => dialog.blocking_pick_folders(),
        "download" => dialog.blocking_pick_folder().map(|file| vec![file]),
        _ => return Err("不支持的路径选择类型".into()),
    };
    let Some(files) = files else { return Ok(None); };
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        paths.push(file.into_path().map_err(|error| error.to_string())?);
    }
    Ok(Some(paths))
}
