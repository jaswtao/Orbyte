//! 将 WebView 从剪贴板读取的 PNG 保存为 CLI 可读取的本地文件。
//! Author: jaswtao · Date: 2026-10-15
use std::path::{Path, PathBuf};
use tauri::Manager;

fn save_png(directory: &Path, png: &[u8]) -> Result<PathBuf, String> {
    if png.len() > 50 * 1024 * 1024 {
        return Err("截图不能超过 50 MiB".into());
    }
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("剪贴板图片不是 PNG 格式".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| format!("无法创建截图缓存目录：{e}"))?;
    let path = directory.join(format!("screenshot-{}.png", uuid::Uuid::new_v4()));
    std::fs::write(&path, png).map_err(|e| format!("无法保存截图：{e}"))?;
    Ok(path)
}

#[tauri::command]
pub fn save_clipboard_image(app: tauri::AppHandle, request: tauri::ipc::Request<'_>) -> Result<String, String> {
    // 原始字节避免把 PNG 编成 JSON 数字数组。同步命令由 Tauri 放在阻塞线程池执行。
    let png = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => bytes.to_vec(),
        tauri::ipc::InvokeBody::Json(_) => return Err("需要原始 PNG 数据".into()),
    };
    let directory = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("clipboard-images");
    save_png(&directory, &png).map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_unique_png_files_and_rejects_other_data() {
        let directory = std::env::temp_dir().join(format!("orbyte-clipboard-{}", uuid::Uuid::new_v4()));
        let png = include_bytes!("../../icons/32x32.png");
        let first = save_png(&directory, png).unwrap();
        let second = save_png(&directory, png).unwrap();
        assert_ne!(first, second);
        assert_eq!(first.extension().unwrap(), "png");
        assert_eq!(std::fs::read(&first).unwrap(), png);
        assert!(save_png(&directory, b"not an image").is_err());
        assert!(save_png(&directory, &vec![0; 50 * 1024 * 1024 + 1]).is_err());
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
