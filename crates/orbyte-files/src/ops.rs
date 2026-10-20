//! 文本编码、磁盘版本和原子保存。Author: jaswtao · Date: 2026-10-13
use crate::{path_guard::resolve, FileError, FileResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

pub const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Encoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "gbk")]
    Gbk,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LineEnding {
    LF,
    CRLF,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFile {
    pub content: String,
    pub version: String,
    pub encoding: Encoding,
    pub bom: bool,
    pub line_ending: LineEnding,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteRequest {
    pub content: String,
    pub expected_version: Option<String>,
    pub encoding: Encoding,
    pub bom: bool,
    pub line_ending: LineEnding,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub accessible: bool,
}
#[derive(Serialize)]
pub struct DirectoryListing {
    pub entries: Vec<DirectoryEntry>,
    pub truncated: bool,
}

fn bytes(path: &Path) -> FileResult<Vec<u8>> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES as u64 {
        return Err("文件超过 5 MiB，无法打开".to_string().into());
    }
    let mut data = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > MAX_FILE_BYTES {
        return Err("文件超过 5 MiB，无法打开".to_string().into());
    }
    Ok(data)
}
fn version(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn decode(data: &[u8], selected: Option<Encoding>) -> FileResult<TextFile> {
    if data.iter().take(8192).any(|b| *b == 0) {
        return Err("二进制文件不能作为文本打开".to_string().into());
    }
    let bom = data.starts_with(&[0xef, 0xbb, 0xbf]);
    let payload = if bom { &data[3..] } else { data };
    let encoding = selected.unwrap_or(if bom || std::str::from_utf8(payload).is_ok() {
        Encoding::Utf8
    } else {
        Encoding::Gbk
    });
    let text = match encoding {
        Encoding::Utf8 => std::str::from_utf8(payload)
            .map(str::to_owned)
            .map_err(|_| FileError::from("不是有效的 UTF-8 文件".to_string()))?,
        Encoding::Gbk => {
            let text = encoding_rs::GBK
                .decode_without_bom_handling_and_without_replacement(payload)
                .ok_or_else(|| FileError::from("不是 UTF-8 或 GBK 文件".to_string()))?
                .into_owned();
            // GBK 解码器兼容 GB18030 四字节序列，但编码器无法写回，必须在打开时拒绝。
            let (encoded, _, errors) = encoding_rs::GBK.encode(&text);
            if errors || *encoded != *payload {
                return Err("文件包含 GBK 无法保存的 GB18030 字符，请改用 UTF-8"
                    .to_string()
                    .into());
            }
            text
        }
    };
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    Ok(TextFile {
        // 编辑器会把单独的 \r 视为换行，基线必须与之一致。
        content: text.replace("\r\n", "\n").replace('\r', "\n"),
        version: version(data),
        encoding,
        bom: bom && encoding == Encoding::Utf8,
        line_ending: if crlf > lf {
            LineEnding::CRLF
        } else {
            LineEnding::LF
        },
    })
}
pub fn read(root: &Path, path: &str, encoding: Option<Encoding>) -> FileResult<TextFile> {
    decode(&bytes(&resolve(root, path, false)?)?, encoding)
}
fn encode(request: &WriteRequest) -> FileResult<Vec<u8>> {
    let text = if request.line_ending == LineEnding::CRLF {
        request.content.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        request.content.replace("\r\n", "\n")
    };
    let data = match request.encoding {
        Encoding::Utf8 => {
            let mut data = if request.bom {
                vec![0xef, 0xbb, 0xbf]
            } else {
                Vec::new()
            };
            data.extend_from_slice(text.as_bytes());
            data
        }
        Encoding::Gbk => {
            let (data, _, errors) = encoding_rs::GBK.encode(&text);
            if errors {
                let mut position = 0;
                for character in request.content.chars() {
                    if encoding_rs::GBK.encode(&character.to_string()).2 {
                        return Err(FileError::Unmappable {
                            message: format!("GBK 无法表示字符「{character}」，请改为 UTF-8 保存"),
                            position,
                            length: character.len_utf16(),
                        });
                    }
                    position += character.len_utf16();
                }
                return Err("GBK 无法表示部分内容，请改为 UTF-8 保存".to_string().into());
            }
            data.into_owned()
        }
    };
    if data.len() > MAX_FILE_BYTES {
        return Err("保存内容超过 5 MiB".to_string().into());
    }
    if data.iter().take(8192).any(|b| *b == 0) {
        return Err("不能保存包含 NUL 的二进制内容".to_string().into());
    }
    Ok(data)
}
pub fn write(root: &Path, path: &str, request: WriteRequest) -> FileResult<TextFile> {
    let target = resolve(root, path, false)?;
    let data = encode(&request)?;
    let temp = target
        .parent()
        .ok_or_else(|| FileError::from("父目录不存在".to_string()))?
        .join(format!(".orbyte-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> FileResult<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&data)?;
        file.sync_all()?;
        drop(file);
        // 临时文件完成后再次校验，尽量缩短与外部写入的竞争窗口。
        resolve(root, path, false)?;
        let exists = match fs::File::open(&target) {
            Ok(mut current) => {
                if current.metadata()?.permissions().readonly() {
                    return Err("文件只读，无法保存".to_string().into());
                }
                if let Some(expected) = &request.expected_version {
                    let mut digest = Sha256::new();
                    let mut buffer = [0; 8192];
                    loop {
                        let count = current.read(&mut buffer)?;
                        if count == 0 {
                            break;
                        }
                        digest.update(&buffer[..count]);
                    }
                    if *expected != format!("{:x}", digest.finalize()) {
                        return Err(FileError::Conflict {
                            message: "文件已在外部修改".into(),
                        });
                    }
                }
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        replace(&target, &temp, exists)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    decode(&data, Some(request.encoding))
}
#[cfg(windows)]
fn replace(target: &Path, temp: &Path, exists: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, ReplaceFileW, SetFileAttributesW};
    let attributes = if exists {
        Some(fs::metadata(target)?.file_attributes())
    } else {
        None
    };
    let target: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let temp: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        if let Some(attributes) = attributes {
            // 普通可设置属性；压缩、加密、ACL 等由 ReplaceFileW 合并保留。
            let flags = attributes & (0x1 | 0x2 | 0x4 | 0x20 | 0x80 | 0x100 | 0x1000 | 0x2000);
            if SetFileAttributesW(temp.as_ptr(), flags) == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        if exists {
            ReplaceFileW(
                target.as_ptr(),
                temp.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(temp.as_ptr(), target.as_ptr(), 0)
        }
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn replace(target: &Path, temp: &Path, _: bool) -> std::io::Result<()> {
    fs::rename(temp, target)
}

pub fn list(root: &Path, path: &str) -> FileResult<DirectoryListing> {
    let directory = resolve(root, path, true)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case(".git") {
            continue;
        }
        if entries.len() == 5000 {
            truncated = true;
            break;
        }
        let relative = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}/{name}")
        };
        entries.push(DirectoryEntry {
            is_directory: entry.path().is_dir(),
            accessible: resolve(root, &relative, false).is_ok(),
            name,
            path: relative,
        });
    }
    entries.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(DirectoryListing { entries, truncated })
}
pub fn create(root: &Path, path: &str, directory: bool) -> FileResult<()> {
    let target = resolve(root, path, false)?;
    if directory {
        fs::create_dir(target)?;
    } else {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
    }
    Ok(())
}
pub fn rename(root: &Path, from: &str, to: &str) -> FileResult<()> {
    let source = resolve(root, from, false)?;
    let target = resolve(root, to, false)?;
    // Windows 路径不区分大小写，仅改大小写时目标就是源文件本身。
    let case_only = from.to_lowercase() == to.to_lowercase();
    if !case_only && target.try_exists()? {
        return Err("目标路径已存在".to_string().into());
    }
    fs::rename(source, target)?;
    Ok(())
}
pub fn delete(root: &Path, path: &str) -> FileResult<()> {
    let target = resolve(root, path, false)?;
    trash::delete(target).map_err(|e| FileError::from(format!("无法移入回收站：{e}")))
}
