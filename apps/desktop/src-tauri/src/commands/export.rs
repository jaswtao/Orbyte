//! 保存对话框决定路径，前端仅提交内容。Author: jaswtao · Date: 2026-10-14
use tauri_plugin_dialog::DialogExt;
#[tauri::command]
pub async fn export_terminal_output(
    app: tauri::AppHandle,
    default_name: String,
    content: String,
) -> Result<Option<String>, String> {
    if content.len() > 50 * 1024 * 1024 {
        return Err("导出内容不能超过 50 MiB".into());
    }
    let name: String = default_name
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .take(200)
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(if name.is_empty() {
                "terminal.txt"
            } else {
                &name
            })
            .add_filter("文本", &["txt"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| format!("导出失败：{e}"))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}
