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
    "write_terminal",
    "terminal_uses_native_paste",
    "set_active_terminal",
    "resize_terminal",
    "stop_terminal",
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
