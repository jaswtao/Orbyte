//! 项目文件服务。Author: jaswtao · Date: 2026-10-13
pub mod index;
pub mod ops;
pub mod path_guard;
pub mod watcher;

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileError {
    Missing { message: String },
    Message { message: String },
    Conflict { message: String },
    Unmappable { message: String, position: usize, length: usize },
}
impl From<String> for FileError {
    fn from(message: String) -> Self {
        Self::Message { message }
    }
}
impl From<std::io::Error> for FileError {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::NotFound {
            Self::Missing {
                message: "文件或父目录不存在".into(),
            }
        } else {
            format!("无法访问文件：{error}").into()
        }
    }
}
pub type FileResult<T> = Result<T, FileError>;
