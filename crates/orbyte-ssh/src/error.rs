//! SSH errors shown to the user. They never include passwords, passphrases, or key material.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("应用正在退出，不能发起新的远程连接或传输")]
    ShuttingDown,
    #[error("连接尝试已取消")]
    Cancelled,
    #[error("连接超时")]
    Timeout,
    #[error("主机验证已取消或已过期")]
    ChallengeRejected,
    #[error("协商到了不允许的算法，连接已在发送凭据之前断开")]
    WeakAlgorithm,
    #[error("已保存的主机密钥算法不能降级到另一把尚未信任的密钥")]
    AlgorithmDowngrade,
    #[error("主机密钥已变化。已保存指纹 {saved}，本次指纹 {presented}。请在连接管理中按算法移除旧记录后再连接")]
    HostKeyChanged { saved: String, presented: String },
    #[error("该服务器只提供 keyboard-interactive 认证，不在首期范围内，口令未发送")]
    KeyboardInteractiveUnsupported,
    #[error("服务器没有提供可用的密码或公钥认证")]
    AuthMethodMissing,
    #[error("认证失败")]
    AuthenticationFailed,
    #[error("远程会话不存在或已结束")]
    NotRunning,
    #[error("输入队列已满")]
    InputFull,
    #[error("传输队列已满")]
    TransferQueueFull,
    #[error("SFTP 不可用")]
    SftpUnavailable,
    #[error("传输任务不存在")]
    TransferNotFound,
    #[error("文件冲突确认已过期")]
    ConflictExpired,
    #[error("{0}")]
    Message(String),
}

impl From<io::Error> for SshError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::TimedOut {
            Self::Timeout
        } else {
            Self::Message("网络连接失败".into())
        }
    }
}

pub type SshResult<T> = Result<T, SshError>;

pub fn is_eagain(error: &ssh2::Error) -> bool {
    matches!(error.code(), ssh2::ErrorCode::Session(-37))
}

pub fn is_sftp_eof(error: &ssh2::Error) -> bool {
    // ssh2's directory reader reports end-of-list as LIBSSH2_ERROR_FILE (-16),
    // not as the SFTP EOF status. FX_EOF is 1.
    matches!(error.code(), ssh2::ErrorCode::SFTP(1) | ssh2::ErrorCode::Session(-16))
}
