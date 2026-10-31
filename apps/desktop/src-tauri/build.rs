//! 声明应用命令清单。capabilities 只授予这里声明的命令，未声明的命令前端无法调用。
//!
//! Author: jaswtao
//! Date: 2026-10-09

const COMMANDS: &[&str] = &[
    "read_clipboard_text",
    "write_clipboard_text",
    "save_clipboard_image",
    "resolve_close_request",
    "list_directory", "read_file", "write_file", "create_file", "create_directory",
    "rename_path", "delete_path", "reveal_path", "search_files", "watch_project_files", "set_unsaved_editor_count",
    "list_editor_drafts",
    "put_editor_draft",
    "delete_editor_draft",
    "rename_editor_drafts",
    "list_projects",
    "create_project",
    "update_project",
    "set_project_hidden",
    "delete_project",
    "list_groups",
    "create_group",
    "update_group",
    "delete_group",
    "reorder_projects",
    "reorder_groups",
    "get_settings",
    "set_setting",
    "set_settings",
    "load_workspace_layout",
    "save_workspace_layout",
    "list_launch_presets",
    "export_terminal_output",
    "git_available",
    "git_status",
    "git_worktree_list",
    "git_worktree_add",
    "list_available_shells",
    "create_terminal",
    "start_terminal",
    "remove_terminal",
    "list_terminal_configs",
    "list_running_workspaces",
    "list_terminal_sessions",
    "report_terminal_diagnostic",
    "terminals_have_running_tasks",
    "subscribe_terminal",
    "ack_terminal",
    "pause_terminal_output",
    "write_terminal",
    "terminal_uses_native_paste",
    "set_active_terminal",
    "resize_terminal",
    "stop_terminal",
    "list_remote_projects",
    "save_remote_project",
    "delete_remote_project",
    "list_remote_connection_tabs",
    "open_remote_connection_tab",
    "get_remote_workspace",
    "close_remote_connection_tab",
    "list_ssh_connections",
    "create_ssh_connection",
    "update_ssh_connection",
    "delete_ssh_connection",
    "list_ssh_known_hosts",
    "forget_ssh_host_key",
    "load_remote_workspace",
    "save_remote_workspace",
    "set_remote_directory",
    "create_remote_terminal_config",
    "remove_remote_terminal_config",
    "begin_ssh_connect",
    "seal_ssh_connect",
    "connect_ssh_terminal",
    "connect_sftp",
    "answer_ssh_challenge",
    "submit_ssh_credential",
    "cancel_ssh_attempt",
    "list_remote_terminal_sessions",
    "list_remote_directory",
    "cancel_remote_browse",
    "create_remote_directory",
    "canonicalize_remote_path",
    "select_transfer_paths",
    "enqueue_remote_transfer",
    "list_remote_transfers",
    "subscribe_remote_transfers",
    "cancel_remote_transfer",
    "resolve_transfer_conflict",
];

fn main() {
    // 图标变化时重新编译 Windows 资源，避免开发构建沿用旧图标。
    println!("cargo:rerun-if-changed=icons");

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri build script");

    // tauri-build 只给主程序链接资源；集成测试也需要 Common Controls v6 manifest。
    if std::env::var("TARGET").is_ok_and(|target| target.contains("windows-msvc")) {
        let resource =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("resource.lib");
        println!("cargo:rustc-link-arg-tests={}", resource.display());
    }
}
