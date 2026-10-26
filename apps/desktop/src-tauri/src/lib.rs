//! Orbyte 桌面宿主：Tauri 命令与应用生命周期。
//!
//! Author: jaswtao
//! Date: 2026-10-09

mod commands;
mod state;
mod app_log;

use commands::project::StorageState;
use state::{AppState, ClosePhase};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};
use tauri_plugin_window_state::{StateFlags, WindowExt};

/// 窗口关闭与退出请求共用；确认期间重复请求仅激活窗口。
fn request_close(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let mut phase = state.close_phase.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = phase.request(state.unsaved_editor_count.load(Ordering::Acquire));
    let pending = *phase == ClosePhase::Pending;
    log::info!("close_requested fresh={fresh} phase={phase:?}");
    drop(phase);
    if pending {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        if fresh {
            if let Err(error) = app.emit("app_close_requested", ()) {
                log::warn!("无法请求未保存确认：{error}");
                let _ = state.close_phase.lock().unwrap_or_else(|e| e.into_inner()).resolve(false);
            }
        }
        return;
    }
    if fresh { begin_shutdown(app); }
}

#[tauri::command]
fn resolve_close_request(app: tauri::AppHandle, proceed: bool) -> Result<(), String> {
    app.state::<AppState>().close_phase.lock().unwrap_or_else(|e| e.into_inner()).resolve(proceed)?;
    if proceed { begin_shutdown(&app); }
    Ok(())
}

/// 从进入清理时计时，包含进行中文件修改与生命周期锁等待；超时退出由全局 Job 兜底。
fn begin_shutdown(app: &tauri::AppHandle) {
    log::info!("shutdown_started");
    app.state::<AppState>().close_started.store(true, Ordering::Release);
    let _ = app.emit("app_shutdown_started", ());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let cleanup = async {
            let _writes = state.file_writes.write().await;
            let _guard = state.terminal_lifecycle.lock().await;
            let terminals = state.terminals.clone();
            let files = state.files.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                files.clear();
                terminals.stop_all();
            }).await;
            if let Err(error) = result { log::error!("shutdown_cleanup_failed error={error}"); }
        };
        if tokio::time::timeout(orbyte_terminal::STOP_GRACE, cleanup).await.is_err() {
            log::warn!("shutdown_cleanup_timeout");
        }
        log::info!("shutdown_complete");
        log::logger().flush();
        state.close_complete.store(true, Ordering::Release);
        app.exit(0);
    });
}

pub fn run() {
    // 尽早加入全局 Job，之后创建的 WebView2 与终端进程都继承该 Job，崩溃时由系统清理。
    #[cfg(windows)]
    let global_job_error = orbyte_platform::windows::job::install_global_job().err();

    let state = AppState::new();
    #[cfg(windows)]
    let activation = std::sync::Arc::new(std::sync::Mutex::new(None));
    #[cfg(windows)]
    let activation_setup = activation.clone();
    state.shells.warm_up();
    std::thread::spawn(|| { orbyte_platform::launch::presets(); orbyte_git::find_git(); });

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED,
                )
                .with_filter(|label| label == "main")
                .skip_initial_state("main")
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(commands::git::GitQueries::default())
        .manage(state)
        .setup(move |app| {
            let database = (|| -> Result<_, String> {
                let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
                if let Err(error) = app_log::init(directory.join("logs")) {
                    eprintln!("无法初始化应用诊断日志：{error}");
                }
                log::info!("app_start version={} os={} arch={} log_directory={}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH, directory.join("logs").display());
                #[cfg(windows)]
                if let Some(error) = &global_job_error { log::error!("global_job_install_failed error={error}"); }
                let integration = directory.join("shell-integration");
                match orbyte_platform::shell_integration::install(&integration) {
                    Ok(()) => { let _ = app.state::<AppState>().integration_dir.set(integration); }
                    Err(e) => log::warn!("Shell 集成脚本安装失败，新终端将不启用集成：{e}"),
                }
                std::fs::create_dir_all(&directory).map_err(|e| format!("无法创建数据目录：{e}"))?;
                tauri::async_runtime::block_on(orbyte_storage::Database::open(&directory.join("orbyte.db")))
            })();
            match &database {
                Ok(_) => log::info!("database_ready"),
                Err(error) => log::error!("database_init_failed error={error}"),
            }
            app.manage(StorageState(database));
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(windows)]
                match window.hwnd() {
                    Ok(hwnd) => {
                        // SAFETY: setup runs on the UI thread and the main window is still alive.
                        if let Err(error) = unsafe {
                            orbyte_platform::windows::system_menu::disable_keyboard_menu(hwnd.0 as _)
                        } {
                            log::warn!("禁用窗口菜单的键盘激活失败：{error}");
                        }
                    }
                    Err(error) => log::warn!("读取主窗口句柄失败，无法禁用菜单的键盘激活：{error}"),
                }
                // 开发标识与安装版分开，窗口标题也分开，避免两套进程叠在同一个任务栏入口下。
                if app.config().identifier.ends_with(".dev") {
                    window.set_title("Orbyte Dev")?;
                }
                // 隐藏期间完成状态恢复，避免默认尺寸先显示再调整造成闪烁。
                if let Err(error) = window.restore_state(
                    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED,
                ) {
                    log::warn!("恢复窗口状态失败：{error}");
                }
                window.show()?;
                window.set_focus()?;
                #[cfg(windows)]
                {
                    let terminals = app.state::<AppState>().terminals.clone();
                    match orbyte_platform::windows::activation::WindowActivation::start(
                        window.hwnd()?.0 as usize,
                        move |pid| terminals.is_active_process(pid),
                    ) {
                        Ok(service) => *activation_setup.lock().unwrap_or_else(|e| e.into_inner()) = Some(service),
                        Err(error) => log::warn!("启动终端窗口激活服务失败：{error}"),
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::diagnostics::report_app_diagnostic,
            commands::clipboard::read_clipboard_text,
            commands::clipboard::write_clipboard_text,
            commands::clipboard::save_clipboard_image,
            commands::terminal::list_launch_presets,
            commands::terminal::report_terminal_diagnostic,
            commands::export::export_terminal_output,
            commands::git::git_available,
            commands::git::git_status,
            commands::git::git_worktree_list,
            commands::git::git_worktree_add,
            resolve_close_request,
            commands::drafts::list_editor_drafts,
            commands::drafts::put_editor_draft,
            commands::drafts::delete_editor_draft,
            commands::drafts::rename_editor_drafts,
            commands::files::list_directory,
            commands::files::read_file,
            commands::files::write_file,
            commands::files::create_file,
            commands::files::create_directory,
            commands::files::rename_path,
            commands::files::delete_path,
            commands::files::search_files,
            commands::files::watch_project_files,
            commands::files::set_unsaved_editor_count,
            commands::project::list_projects,
            commands::project::create_project,
            commands::project::update_project,
            commands::project::delete_project,
            commands::project::list_groups,
            commands::project::create_group,
            commands::project::update_group,
            commands::project::delete_group,
            commands::project::reorder_projects,
            commands::project::reorder_groups,
            commands::project::get_settings,
            commands::project::set_setting,
            commands::project::set_settings,
            commands::workspace::load_workspace_layout,
            commands::workspace::save_workspace_layout,
            commands::terminal::list_available_shells,
            commands::terminal::create_terminal,
            commands::terminal::start_terminal,
            commands::terminal::remove_terminal,
            commands::terminal::list_terminal_configs,
            commands::terminal::list_running_workspaces,
            commands::terminal::list_terminal_sessions,
            commands::terminal::terminals_have_running_tasks,
            commands::terminal::subscribe_terminal,
            commands::terminal::ack_terminal,
            commands::terminal::write_terminal,
            commands::terminal::terminal_uses_native_paste,
            commands::terminal::set_active_terminal,
            commands::terminal::resize_terminal,
            commands::terminal::stop_terminal,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Orbyte")
        .run(move |app, event| {
            #[cfg(windows)]
            if matches!(event, tauri::RunEvent::Exit) {
                drop(activation.lock().unwrap_or_else(|e| e.into_inner()).take());
            }
            if app
                .state::<AppState>()
                .close_complete
                .load(Ordering::Acquire)
            {
                return;
            }
            match event {
                tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::CloseRequested { api, .. },
                    ..
                } => {
                    api.prevent_close();
                    request_close(app);
                }
                tauri::RunEvent::ExitRequested { api, .. } => {
                    api.prevent_exit();
                    request_close(app);
                }
                _ => {}
            }
        });
}
