//! One SFTP connection per remote workspace, on its own thread.
//!
//! Directory listings are served between transfer chunks.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use ssh2::Sftp;
use zeroize::Zeroizing;

use crate::cancel::CancelToken;
use crate::connect::{self, Credential};
use crate::error::{is_sftp_eof, SshError, SshResult};
use crate::hostkey::PresentedKey;
use crate::io_wait::readable_now;
use crate::keepalive::{Keepalive, KeepaliveAction};
use crate::path::normalize_remote;
use crate::transfer::{self, Choice, Snapshot, TransferSpec};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub kind: String,
    pub size: u64,
    pub mtime: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirBatch {
    pub request_id: String,
    pub path: String,
    pub entries: Vec<DirEntry>,
    pub done: bool,
    pub message: Option<String>,
}

pub enum SftpJob {
    List {
        path: String,
        request_id: String,
        cancel: Arc<AtomicBool>,
        emit: Arc<dyn Fn(DirBatch) + Send + Sync>,
    },
    Mkdir { path: String, done: Sender<SshResult<()>> },
    Realpath { path: String, done: Sender<SshResult<String>> },
    Transfer {
        spec: TransferSpec,
        cancelled: Arc<AtomicBool>,
        progress: Arc<dyn Fn(Snapshot) + Send + Sync>,
        conflict: Arc<dyn Fn(&str, Option<u64>, Option<u64>) -> SshResult<Choice> + Send + Sync>,
        done: Sender<Snapshot>,
    },
    Shutdown,
}

pub struct SftpLaunch {
    pub host: String,
    pub port: u16,
    pub saved_algorithms: Vec<String>,
    pub username: String,
    pub credential: Credential,
    pub cancel: Arc<CancelToken>,
    pub prepare_auth: Box<dyn FnOnce(&PresentedKey) -> SshResult<Zeroizing<String>> + Send>,
    pub finish_auth: Box<dyn FnOnce() + Send>,
    pub on_state: Box<dyn Fn(&str, Option<String>) + Send + Sync>,
}

pub fn start(launch: SftpLaunch) -> SshResult<SyncSender<SftpJob>> {
    let (tx, rx) = mpsc::sync_channel(128);
    thread::Builder::new()
        .name("ssh-sftp".into())
        .spawn(move || worker(launch, rx))
        .map_err(|_| SshError::Message("无法启动 SFTP 线程".into()))?;
    Ok(tx)
}

struct Defer(Option<Box<dyn FnOnce() + Send>>);
impl Drop for Defer {
    fn drop(&mut self) {
        if let Some(finish) = self.0.take() { finish(); }
    }
}

fn worker(launch: SftpLaunch, rx: Receiver<SftpJob>) {
    let SftpLaunch { host, port, saved_algorithms, username, credential, cancel, prepare_auth, finish_auth, on_state } = launch;
    let result = (|| -> SshResult<()> {
        let finish_auth = Defer(Some(finish_auth));
        on_state("connecting", None);
        let mut connected = connect::connect_transport(&host, port, &saved_algorithms, &cancel)?;
        on_state("verifyingHost", None);
        let secret = prepare_auth(&connected.presented)?;
        on_state("authenticating", None);
        connect::authenticate(&mut connected, &cancel, &username, &credential, secret.as_str())?;
        drop(secret);
        drop(finish_auth);
        let sftp = connect::drive(&connected.stream, &mut connected.session, &cancel, Instant::now() + Duration::from_secs(10), |session| {
            session.sftp()
        })?;
        // File and directory calls block on this thread only. The socket timeout is the stall limit,
        // and shutting the socket down still aborts them.
        connected.session.set_blocking(true);
        connected.session.set_timeout(30_000);
        let _ = connected.stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = connected.stream.set_write_timeout(Some(Duration::from_secs(30)));
        on_state("ready", None);
        serve(&connected, sftp, rx, &cancel)
    })();
    if let Err(error) = result {
        on_state("failed", Some(error.to_string()));
    } else {
        on_state("closed", None);
    }
}

fn serve(connected: &connect::Connected, sftp: Sftp, rx: Receiver<SftpJob>, cancel: &CancelToken) -> SshResult<()> {
    let mut keepalive = Keepalive::default();
    let mut probing = false;
    let mut probe_written = false;
    connected.session.set_keepalive(true, 1);
    let mut queued: Option<SftpJob> = None;
    loop {
        if cancel.is_cancelled() {
            return Err(SshError::Cancelled);
        }
        let now = Instant::now();
        match keepalive.on_tick(now) {
            KeepaliveAction::Failed => return Err(SshError::Message("SFTP 保活没有收到应答".into())),
            KeepaliveAction::SendProbe => {
                keepalive.note_probe_sent(now);
                probing = true;
                probe_written = false;
            }
            KeepaliveAction::Wait => {}
        }
        if probing && !probe_written {
            match connected.session.keepalive_send() {
                Ok(_) => probe_written = true,
                Err(_) if cancel.is_cancelled() => return Err(SshError::Cancelled),
                Err(_) => return Err(SshError::Message("SFTP 连接已断开".into())),
            }
        }
        if probing && probe_written && readable_now(&connected.stream) {
            // Drain the global-request reply. A successful follow-up call shows the peer answered.
            match sftp.realpath(Path::new(".")) {
                Ok(_) => {
                    keepalive.note_reply();
                    probing = false;
                    probe_written = false;
                }
                Err(_) if cancel.is_cancelled() => return Err(SshError::Cancelled),
                Err(_) => return Err(SshError::Message("SFTP 连接已断开".into())),
            }
        }
        let job = if let Some(job) = queued.take() {
            job
        } else {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(job) => job,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        };
        match job {
            SftpJob::Shutdown => return Ok(()),
            SftpJob::List { path, request_id, cancel: list_cancel, emit } => {
                list_dir(&sftp, &path, &request_id, &list_cancel, &emit);
            }
            SftpJob::Mkdir { path, done } => { let _ = done.send(mkdir(&sftp, &path)); }
            SftpJob::Realpath { path, done } => { let _ = done.send(realpath(&sftp, &path)); }
            SftpJob::Transfer { spec, cancelled, progress, conflict, done } => {
                let mut hooks = transfer::Hooks {
                    cancelled: &cancelled,
                    progress: &|snapshot| progress(snapshot),
                    conflict: &|relative, size, mtime| conflict(relative, size, mtime),
                    between: &mut |sftp| {
                        loop {
                            match rx.try_recv() {
                                Ok(SftpJob::Transfer { done, .. }) => {
                                    let _ = done.send(transfer::Snapshot {
                                        state: "failed".into(),
                                        transferred: 0,
                                        total: None,
                                        bytes_per_second: 0,
                                        files: Vec::new(),
                                        message: Some("已有传输正在进行".into()),
                                    });
                                }
                                Ok(SftpJob::Shutdown) => cancelled.store(true, Ordering::Release),
                                Ok(SftpJob::List { path, request_id, cancel, emit }) => list_dir(sftp, &path, &request_id, &cancel, &emit),
                                Ok(SftpJob::Mkdir { path, done }) => { let _ = done.send(mkdir(sftp, &path)); }
                                Ok(SftpJob::Realpath { path, done }) => { let _ = done.send(realpath(sftp, &path)); }
                                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                            }
                        }
                    },
                };
                let snapshot = transfer::run_transfer(&sftp, &spec, &mut hooks);
                let _ = done.send(snapshot);
            }
        }
        let _ = queued;
    }
}

fn list_dir(sftp: &Sftp, path: &str, request_id: &str, cancel: &AtomicBool, emit: &Arc<dyn Fn(DirBatch) + Send + Sync>) {
    let normalized = match normalize_remote(path).or_else(|_| realpath(sftp, path).map_err(|e| e.to_string())) {
        Ok(path) => path,
        Err(message) => {
            emit(DirBatch { request_id: request_id.into(), path: path.into(), entries: Vec::new(), done: true, message: Some(message) });
            return;
        }
    };
    let mut dir = match sftp.opendir(Path::new(&normalized)) {
        Ok(dir) => dir,
        Err(_) => {
            emit(DirBatch { request_id: request_id.into(), path: normalized, entries: Vec::new(), done: true, message: Some("无法打开目录，可能没有权限或 SFTP 不可用".into()) });
            return;
        }
    };
    let mut batch = Vec::new();
    loop {
        if cancel.load(Ordering::Acquire) {
            emit(DirBatch { request_id: request_id.into(), path: normalized, entries: batch, done: true, message: Some("已取消".into()) });
            return;
        }
        match dir.readdir() {
            Ok((entry, stat)) => {
                let name = entry.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if name.is_empty() || name == "." || name == ".." { continue; }
                let kind = if name.contains('\u{FFFD}') {
                    "unsupported"
                } else if stat.file_type().is_symlink() {
                    "symlink"
                } else if stat.is_dir() {
                    "dir"
                } else if stat.is_file() {
                    "file"
                } else {
                    "other"
                };
                batch.push(DirEntry { name, kind: kind.into(), size: stat.size.unwrap_or(0), mtime: stat.mtime });
                if batch.len() >= 200 {
                    emit(DirBatch { request_id: request_id.into(), path: normalized.clone(), entries: std::mem::take(&mut batch), done: false, message: None });
                }
            }
            Err(error) if is_sftp_eof(&error) => break,
            Err(_) => {
                emit(DirBatch { request_id: request_id.into(), path: normalized, entries: batch, done: true, message: Some("读取目录失败".into()) });
                return;
            }
        }
    }
    emit(DirBatch { request_id: request_id.into(), path: normalized, entries: batch, done: true, message: None });
}

fn mkdir(sftp: &Sftp, path: &str) -> SshResult<()> {
    let path = normalize_remote(path).map_err(SshError::Message)?;
    sftp.mkdir(Path::new(&path), 0o755).map_err(|_| SshError::Message("无法创建远程目录".into()))
}

pub fn realpath(sftp: &Sftp, path: &str) -> SshResult<String> {
    let path = if path.is_empty() { "." } else { path };
    let resolved = sftp.realpath(Path::new(path)).map_err(|_| SshError::Message("无法解析远程路径".into()))?;
    let text = resolved.to_string_lossy();
    if text.contains('\u{FFFD}') {
        return Err(SshError::Message("远程路径不是合法的 UTF-8".into()));
    }
    normalize_remote(&text).map_err(SshError::Message)
}
