//! 终端命令。终端持续输出经命令参数中的 Channel 以原始二进制帧下发。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::sync::Arc;

use orbyte_core::TerminalConfig;
use orbyte_platform::shell::{unavailable_error, ShellInfo, ShellType};
use orbyte_terminal::{FrameSink, SpawnRequest, TerminalSessionInfo, TerminalSize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use super::project::StorageState;
use super::{err_text, CmdResult};
use crate::state::AppState;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedTerminal {
    pub terminal_config_id: String,
    pub terminal_id: String,
}

/// 把 Tauri Channel 适配为终端帧接收方。
struct ChannelSink(Channel<InvokeResponseBody>);

impl FrameSink for ChannelSink {
    fn send(&self, frame: Vec<u8>) -> bool {
        self.0.send(InvokeResponseBody::Raw(frame)).is_ok()
    }
}

fn sink(channel: Channel<InvokeResponseBody>) -> Arc<dyn FrameSink> {
    Arc::new(ChannelSink(channel))
}

#[tauri::command]
pub fn set_active_terminal(state: State<'_, AppState>, terminal_id: Option<String>) -> CmdResult<()> {
    state.terminals.set_active_terminal(terminal_id).map_err(err_text)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err_text)?
}

#[tauri::command]
pub async fn list_available_shells(state: State<'_, AppState>) -> CmdResult<Vec<ShellInfo>> {
    let shells = state.shells.clone();
    blocking(move || Ok(shells.all().to_vec())).await
}

#[tauri::command]
pub async fn create_terminal(
    state: State<'_, AppState>,
    storage: State<'_, StorageState>,
    project_id: String,
    workspace_id: String,
    shell: String,
    launch_preset: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    channel: Channel<InvokeResponseBody>,
) -> CmdResult<CreatedTerminal> {
    let _guard = state.terminal_lifecycle.lock().await;
    if state
        .close_started
        .load(std::sync::atomic::Ordering::Acquire)
        || state.terminals.is_shutting_down()
    {
        return Err("应用正在退出，不能创建或启动终端".into());
    }
    orbyte_core::validate_launch_preset(launch_preset.as_deref())?;
    let preset = launch_preset.as_ref().map(|id| orbyte_platform::launch::presets().iter().find(|p| &p.preset == id).filter(|p| p.available).ok_or("启动预设不可用")).transpose()?;
    let shell = if preset.is_some() { "powershell".to_owned() } else { shell };
    orbyte_core::validate_shell(&shell)?;
    let size = TerminalSize { cols, rows }.validate().map_err(err_text)?;
    let db = storage.database()?;
    let root = db
        .workspace_project_path(&project_id, &workspace_id)
        .await?;
    let cwd = cwd.unwrap_or_default();
    let config = db
        .create_terminal_config_with_preset(&project_id, &workspace_id, preset.map(|p| p.description.as_str()).unwrap_or(&shell), &shell, &cwd, launch_preset.as_deref())
        .await?;
    match spawn_config(&state, &db, config.clone(), root, size, channel).await {
        Ok(terminal_id) => Ok(CreatedTerminal {
            terminal_config_id: config.id,
            terminal_id,
        }),
        Err(error) => {
            db.delete_terminal_config(&config.id)
                .await
                .map_err(|cleanup| format!("{error}；清理配置失败：{cleanup}"))?;
            Err(error)
        }
    }
}

async fn spawn_config(
    state: &AppState,
    db: &orbyte_storage::Database,
    config: TerminalConfig,
    root: String,
    size: TerminalSize,
    channel: Channel<InvokeResponseBody>,
) -> CmdResult<String> {
    let shell = ShellType::parse(&config.shell_type)
        .ok_or_else(|| format!("不支持的 Shell：{}", config.shell_type))?;
    let directory = state.integration_dir.get().cloned().unwrap_or_default();
    let enabled = db.get_settings().await?["shell_integration"].as_bool().unwrap_or(true)
        && state.integration_dir.get().is_some();
    let shells = state.shells.clone();
    let terminals = state.terminals.clone();
    blocking(move || {
        let info = shells.get(shell).ok_or("Shell 探测结果缺失")?;
        let program = info.program.clone().ok_or_else(|| {
            unavailable_error(
                &info.description,
                info.unavailable_reason.as_deref().unwrap_or_default(),
            )
        })?;
        let cwd = orbyte_platform::path::resolve_terminal_directory(&root, &config.cwd)?;
        let (mut args, env) = orbyte_platform::shell_integration::launch_spec(shell, &directory, enabled);
        if let Some(preset) = &config.launch_preset {
            let program = orbyte_platform::launch::presets().iter().find(|p| &p.preset == preset).and_then(|p| p.program.as_ref()).ok_or("启动预设不可用")?;
            let command = format!("& {}", orbyte_platform::shell_integration::quote_powershell(&program.to_string_lossy()));
            if enabled { let last = args.last_mut().ok_or("预设需要 PowerShell")?; last.push_str("; "); last.push_str(&command); }
            else { args = vec!["-NoLogo".into(), "-NoExit".into(), "-Command".into(), command]; }
        }
        let req = SpawnRequest {
            program,
            args,
            env,
            cwd,
            size,
            config_id: Some(config.id),
        };
        terminals.spawn(&req, sink(channel)).map_err(err_text)
    })
    .await
}

#[tauri::command]
pub async fn start_terminal(
    state: State<'_, AppState>,
    storage: State<'_, StorageState>,
    terminal_config_id: String,
    cols: u16,
    rows: u16,
    channel: Channel<InvokeResponseBody>,
) -> CmdResult<String> {
    let _guard = state.terminal_lifecycle.lock().await;
    if state
        .close_started
        .load(std::sync::atomic::Ordering::Acquire)
        || state.terminals.is_shutting_down()
    {
        return Err("应用正在退出，不能创建或启动终端".into());
    }
    let (config, root) = storage
        .database()?
        .terminal_config_with_path(&terminal_config_id)
        .await?;
    let size = TerminalSize { cols, rows }.validate().map_err(err_text)?;
    spawn_config(&state, storage.database()?, config, root, size, channel).await
}

#[tauri::command]
pub async fn list_terminal_configs(
    storage: State<'_, StorageState>,
    workspace_id: String,
) -> CmdResult<Vec<TerminalConfig>> {
    storage
        .database()?
        .list_terminal_configs(&workspace_id)
        .await
}

#[tauri::command]
pub async fn remove_terminal(
    state: State<'_, AppState>,
    storage: State<'_, StorageState>,
    terminal_config_id: String,
) -> CmdResult<()> {
    let _guard = state.terminal_lifecycle.lock().await;
    let db = storage.database()?;
    db.terminal_config_with_path(&terminal_config_id).await?;
    if let Some(session) = state.terminals.find_by_config(&terminal_config_id) {
        let terminals = state.terminals.clone();
        let id = session.id().to_owned();
        blocking(move || terminals.remove(&id).map_err(err_text)).await?;
    }
    db.delete_terminal_config(&terminal_config_id).await
}

/// 有运行时会话的工作区 ID；重载时一次查询即可确定需恢复订阅的项目。
#[tauri::command]
pub async fn list_running_workspaces(
    state: State<'_, AppState>,
    storage: State<'_, StorageState>,
) -> CmdResult<Vec<String>> {
    let config_ids: Vec<_> = state
        .terminals
        .list()
        .into_iter()
        .filter_map(|s| s.terminal_config_id)
        .collect();
    storage.database()?.workspaces_of_configs(&config_ids).await
}

#[tauri::command]
pub async fn list_terminal_sessions(state: State<'_, AppState>) -> CmdResult<Vec<TerminalSessionInfo>> {
    let terminals = state.terminals.clone();
    blocking(move || Ok(terminals.list())).await
}

/// Strict metadata enum. Rejects terminal text, keystrokes, and arbitrary error strings.
#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TerminalDiagnostic {
    FrameError, AckFailed, ReconnectStarted, ReconnectSucceeded, ReconnectFailed, PollFailed,
}

#[tauri::command]
pub async fn report_terminal_diagnostic(
    state: State<'_, AppState>,
    terminal_id: String,
    event: TerminalDiagnostic,
    generation: u32,
    seq: Option<u64>,
) -> CmdResult<()> {
    if terminal_id.is_empty() || terminal_id.len() > 64 || terminal_id.bytes().any(|byte| byte.is_ascii_control()) {
        return Err("无效的终端标识".into());
    }
    let terminals = state.terminals.clone();
    blocking(move || {
        // A failed session list must not take the session lock, so it cannot pile onto an already stuck query.
        if matches!(event, TerminalDiagnostic::PollFailed) {
            log::info!(target: "orbyte_terminal", "frontend session={terminal_id} event={event:?} generation={generation} seq={seq:?}");
            return Ok(());
        }
        let session = terminals.get(&terminal_id).map_err(err_text)?;
        let info = session.info();
        log::info!(target: "orbyte_terminal", "frontend session={terminal_id} event={event:?} generation={generation} seq={seq:?} status={:?} output={:?}", info.status, info.output);
        Ok(())
    }).await
}

/// 关闭前批量检查运行时进程树；兼容手动启动和脚本包装的智能体。
#[tauri::command]
pub async fn terminals_have_running_tasks(state: State<'_, AppState>, terminal_ids: Vec<String>) -> CmdResult<bool> {
    let terminals = state.terminals.clone();
    blocking(move || {
        for id in terminal_ids {
            if terminals.get(&id).map_err(err_text)?.has_running_tasks() { return Ok(true); }
        }
        Ok(false)
    }).await
}

#[tauri::command]
pub async fn subscribe_terminal(
    state: State<'_, AppState>,
    terminal_id: String,
    channel: Channel<InvokeResponseBody>,
) -> CmdResult<()> {
    state.terminals.subscribe(&terminal_id, sink(channel)).map_err(err_text)
}

#[tauri::command]
pub async fn ack_terminal(state: State<'_, AppState>, terminal_id: String, seq: u64) -> CmdResult<()> {
    state.terminals.ack(&terminal_id, seq).map_err(err_text)
}

#[tauri::command]
pub async fn terminal_uses_native_paste(state: State<'_, AppState>, terminal_id: String) -> CmdResult<bool> {
    let terminals = state.terminals.clone();
    blocking(move || {
        let session = terminals.get(&terminal_id).map_err(err_text)?;
        // Windows Codex reads Ctrl+V from the clipboard as one paste. Sending
        // text through ConPTY instead falls back to timing-based key bursts.
        #[cfg(windows)]
        {
            use orbyte_platform::windows::process::{process_console_host, process_image};
            let Some(host) = session.pid().and_then(process_console_host) else { return Ok(false); };
            Ok(session.job_process_ids().into_iter().any(|pid| {
                process_image(pid).ok().is_some_and(|image| {
                    image.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("codex.exe"))
                        // Job descendants can use a separate console (Start-Process/start).
                        && process_console_host(pid) == Some(host)
                })
            }))
        }
        #[cfg(not(windows))]
        { let _ = session; Ok(false) }
    }).await
}

#[tauri::command]
pub async fn write_terminal(state: State<'_, AppState>, terminal_id: String, data: String) -> CmdResult<()> {
    state.terminals.write(&terminal_id, data.into_bytes()).map_err(err_text)
}

#[tauri::command]
pub async fn resize_terminal(
    state: State<'_, AppState>,
    terminal_id: String,
    cols: u16,
    rows: u16,
    nudge: Option<bool>,
) -> CmdResult<()> {
    let terminals = state.terminals.clone();
    blocking(move || {
        let size = TerminalSize { cols, rows };
        if nudge.unwrap_or(false) {
            terminals.redraw(&terminal_id, size).map_err(err_text)
        } else {
            terminals.resize(&terminal_id, size).map_err(err_text)
        }
    }).await
}

#[tauri::command]
pub async fn stop_terminal(
    state: State<'_, AppState>,
    terminal_id: String,
) -> CmdResult<Option<u32>> {
    let _guard = state.terminal_lifecycle.lock().await;
    let terminals = state.terminals.clone();
    blocking(move || terminals.stop(&terminal_id).map_err(err_text)).await
}

#[tauri::command]
pub fn list_launch_presets() -> Vec<orbyte_platform::launch::LaunchPreset> {
    orbyte_platform::launch::presets().to_vec()
}
