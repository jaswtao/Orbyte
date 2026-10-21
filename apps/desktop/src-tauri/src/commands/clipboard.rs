//! Native clipboard access and screenshot files for terminal paste.
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

fn optional_content<T>(result: Result<T, arboard::Error>) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

// Native clipboard calls must not block the window or async runtime threads.
#[tauri::command]
pub async fn read_clipboard_text() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        optional_content(clipboard.get_text())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn write_clipboard_text(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(text).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn encode_image(image: &arboard::ImageData<'_>) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&image.bytes).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(windows)]
fn decode_dib(bytes: &[u8]) -> Result<arboard::ImageData<'static>, String> {
    let decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(std::io::Cursor::new(bytes))
        .map_err(|e| format!("无法解码剪贴板位图：{e}"))?;
    let pixels = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("无法读取剪贴板位图像素：{e}"))?
        .into_rgba8();
    Ok(arboard::ImageData {
        width: pixels.width() as usize,
        height: pixels.height() as usize,
        bytes: pixels.into_raw().into(),
    })
}

fn read_image(clipboard: &mut arboard::Clipboard) -> Result<Option<arboard::ImageData<'static>>, String> {
    let result = clipboard.get_image();
    #[cfg(windows)]
    if matches!(&result, Err(arboard::Error::ConversionFailure | arboard::Error::ContentNotAvailable)) {
        // PixPin's synthesized DIBV5 can fail decoding while its original DIB remains valid.
        let _guard = clipboard_win::Clipboard::new_attempts(10).map_err(|e| format!("无法打开剪贴板：{e}"))?;
        if clipboard_win::is_format_avail(clipboard_win::formats::CF_DIB) {
            let mut bytes = Vec::new();
            clipboard_win::raw::get_vec(clipboard_win::formats::CF_DIB, &mut bytes)
                .map_err(|e| format!("无法读取剪贴板位图：{e}"))?;
            return decode_dib(&bytes).map(Some);
        }
    }
    optional_content(result)
}

#[tauri::command]
pub async fn save_clipboard_image(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let Some(image) = read_image(&mut clipboard)? else { return Ok(None) };
        let png = encode_image(&image)?;
        let directory = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("clipboard-images");
        save_png(&directory, &png).map(|path| Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn decodes_pixpin_dib_with_rgb_masks_and_bottom_up_rows() {
        // A 1x2, 32-bit BITMAPINFOHEADER with BI_BITFIELDS and three RGB masks.
        let mut dib = vec![0u8; 52];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&2i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        dib[16..20].copy_from_slice(&3u32.to_le_bytes());
        dib[40..44].copy_from_slice(&0x00ff0000u32.to_le_bytes());
        dib[44..48].copy_from_slice(&0x0000ff00u32.to_le_bytes());
        dib[48..52].copy_from_slice(&0x000000ffu32.to_le_bytes());
        dib.extend_from_slice(&[255, 0, 0, 255, 0, 0, 255, 255]);
        let image = decode_dib(&dib).unwrap();
        assert_eq!((image.width, image.height), (1, 2));
        assert_eq!(&*image.bytes, &[255, 0, 0, 255, 0, 0, 255, 255]);
        assert!(encode_image(&image).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(decode_dib(&dib[..52]).is_err());
        assert!(decode_dib(b"invalid bitmap").is_err());
    }

    #[test]
    fn absent_format_is_empty_but_access_errors_are_reported() {
        assert_eq!(optional_content::<String>(Err(arboard::Error::ContentNotAvailable)), Ok(None));
        assert!(optional_content::<String>(Err(arboard::Error::ClipboardOccupied)).is_err());
        assert_eq!(optional_content(Ok("text")), Ok(Some("text")));
    }

    #[test]
    fn native_rgba_is_encoded_as_png() {
        let image = arboard::ImageData { width: 1, height: 1, bytes: std::borrow::Cow::Borrowed(&[255, 0, 0, 255]) };
        let bytes = encode_image(&image).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        assert_eq!(pixels, [255, 0, 0, 255]);
    }

    #[test]
    fn saves_large_rgba_screenshots_when_encoded_png_is_within_limit() {
        let directory = std::env::temp_dir().join(format!("orbyte-clipboard-{}", uuid::Uuid::new_v4()));
        let image = arboard::ImageData {
            width: 7680,
            height: 2160,
            bytes: std::borrow::Cow::Owned(vec![255; 7680 * 2160 * 4]),
        };
        assert!(image.bytes.len() > 50 * 1024 * 1024);
        let png = encode_image(&image).unwrap();
        assert!(png.len() < 50 * 1024 * 1024);
        let path = save_png(&directory, &png).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), png);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

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
