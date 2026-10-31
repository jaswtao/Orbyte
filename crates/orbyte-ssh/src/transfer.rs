//! Streaming upload and download. Existing targets are not truncated until a commit succeeds.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ssh2::{OpenFlags, OpenType, Sftp};
use uuid::Uuid;

use crate::error::{is_eagain, is_sftp_eof, SshError, SshResult};
use crate::path::{self, guarded_join, part_file_name};

const CHUNK: usize = 64 * 1024;
const STALL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Overwrite,
    Skip,
    Cancel,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub relative: String,
    pub status: String,
    pub message: Option<String>,
    pub bytes: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub state: String,
    pub transferred: u64,
    pub total: Option<u64>,
    pub bytes_per_second: u64,
    pub files: Vec<FileRow>,
    pub message: Option<String>,
}

pub struct TransferSpec {
    pub direction: Direction,
    pub remote_directory: String,
    pub local_root: PathBuf,
    pub names: Vec<String>,
}

pub struct Hooks<'a> {
    pub cancelled: &'a AtomicBool,
    pub progress: &'a dyn Fn(Snapshot),
    pub conflict: &'a dyn Fn(&str, Option<u64>, Option<u64>) -> SshResult<Choice>,
    pub between: &'a mut dyn FnMut(&Sftp),
}

struct Item {
    relative: String,
    remote: String,
    local: PathBuf,
    size: u64,
    skipped: Option<String>,
}

pub fn run_transfer(sftp: &Sftp, spec: &TransferSpec, hooks: &mut Hooks<'_>) -> Snapshot {
    let mut files = Vec::new();
    let mut snapshot = Snapshot {
        state: "scanning".into(),
        transferred: 0,
        total: None,
        bytes_per_second: 0,
        files: Vec::new(),
        message: None,
    };
    emit(hooks, &snapshot);
    let mut items = Vec::new();
    for name in &spec.names {
        if stop(hooks) {
            snapshot.state = "cancelled".into();
            return finish(hooks, snapshot, &files);
        }
        if let Err(error) = scan(sftp, spec, name, &mut items, &mut files, hooks) {
            files.push(failed(name, error.to_string()));
        }
        snapshot.files = files.clone();
        emit(hooks, &snapshot);
        (hooks.between)(sftp);
    }
    mark_case_conflicts(&mut items);
    let total = items.iter().filter(|item| item.skipped.is_none()).map(|item| item.size).sum();
    snapshot.total = Some(total);
    snapshot.state = "running".into();
    snapshot.files = files.clone();
    emit(hooks, &snapshot);
    let started = Instant::now();
    for item in items {
        if stop(hooks) {
            files.push(row(&item.relative, "cancelled", None, item.size));
            continue;
        }
        // Directory placeholders go through copy_one so they can be created;
        // every other pre-marked entry is reported and skipped here.
        if let Some(message) = item.skipped.clone() {
            if message != "directory" {
                files.push(row(&item.relative, "skipped", Some(message), 0));
                snapshot.files = files.clone();
                emit(hooks, &snapshot);
                continue;
            }
        }
        match copy_one(sftp, spec, &item, hooks, &mut snapshot) {
            Ok(status) => files.push(status),
            Err(error) => {
                if matches!(error, SshError::Cancelled) {
                    files.push(row(&item.relative, "cancelled", None, item.size));
                    snapshot.state = "cancelled".into();
                    snapshot.files = files.clone();
                    return finish(hooks, snapshot, &files);
                }
                files.push(failed(&item.relative, error.to_string()));
            }
        }
        snapshot.files = files.clone();
        snapshot.bytes_per_second = rate(snapshot.transferred, started.elapsed());
        emit(hooks, &snapshot);
        (hooks.between)(sftp);
    }
    snapshot.state = if files.iter().any(|file| file.status == "cancelled") {
        "cancelled".into()
    } else if files.iter().any(|file| file.status == "failed") {
        "completed".into()
    } else {
        "completed".into()
    };
    finish(hooks, snapshot, &files)
}

fn scan(sftp: &Sftp, spec: &TransferSpec, name: &str, items: &mut Vec<Item>, files: &mut Vec<FileRow>, hooks: &mut Hooks<'_>) -> SshResult<()> {
    match spec.direction {
        Direction::Upload => {
            let path = Path::new(name);
            let leaf = path.file_name().and_then(|value| value.to_str()).unwrap_or(name);
            scan_local(sftp, spec, path, leaf, items, files, hooks)
        }
        Direction::Download => {
            let remote = path::join_remote(&spec.remote_directory, name).map_err(SshError::Message)?;
            scan_remote(sftp, &remote, &spec.local_root, name, items, files, hooks)
        }
    }
}

fn scan_local(sftp: &Sftp, spec: &TransferSpec, path: &Path, relative: &str, items: &mut Vec<Item>, files: &mut Vec<FileRow>, hooks: &mut Hooks<'_>) -> SshResult<()> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(relative);
    let remote = path::join_remote(&spec.remote_directory, name).map_err(SshError::Message)?;
    walk_local(path, &remote, relative, items, files, hooks)?;
    let _ = sftp;
    Ok(())
}

fn walk_local(path: &Path, remote: &str, relative: &str, items: &mut Vec<Item>, files: &mut Vec<FileRow>, hooks: &mut Hooks<'_>) -> SshResult<()> {
    if hooks.cancelled.load(Ordering::Acquire) {
        return Err(SshError::Cancelled);
    }
    let meta = fs::symlink_metadata(path).map_err(|_| SshError::Message(format!("无法读取本地文件：{relative}")))?;
    if meta.file_type().is_symlink() {
        files.push(row(relative, "skipped", Some("符号链接已跳过，传输不跟随链接".into()), 0));
        return Ok(());
    }
    if meta.is_dir() {
        items.push(Item { relative: relative.into(), remote: remote.into(), local: path.to_path_buf(), size: 0, skipped: Some("directory".into()) });
        for entry in fs::read_dir(path).map_err(|_| SshError::Message(format!("无法读取本地目录：{relative}")))? {
            let entry = entry.map_err(|_| SshError::Message(format!("无法读取本地目录：{relative}")))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains('\u{FFFD}') {
                files.push(row(&format!("{relative}/{name}"), "skipped", Some("文件名不是合法的 UTF-8".into()), 0));
                continue;
            }
            walk_local(&entry.path(), &format!("{remote}/{name}"), &format!("{relative}/{name}"), items, files, hooks)?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        files.push(row(relative, "skipped", Some("已跳过非常规文件".into()), 0));
        return Ok(());
    }
    items.push(Item { relative: relative.into(), remote: remote.into(), local: path.to_path_buf(), size: meta.len(), skipped: None });
    Ok(())
}

fn scan_remote(sftp: &Sftp, remote: &str, local_root: &Path, relative: &str, items: &mut Vec<Item>, files: &mut Vec<FileRow>, hooks: &mut Hooks<'_>) -> SshResult<()> {
    if hooks.cancelled.load(Ordering::Acquire) {
        return Err(SshError::Cancelled);
    }
    if relative.contains('\u{FFFD}') {
        files.push(row(relative, "skipped", Some("文件名不是合法的 UTF-8，未开始传输".into()), 0));
        return Ok(());
    }
    let local = guarded_join(local_root, relative).map_err(SshError::Message)?;
    let stat = sftp_lstat(sftp, remote)?;
    if stat.file_type().is_symlink() {
        files.push(row(relative, "skipped", Some("符号链接已跳过，传输不跟随链接".into()), 0));
        return Ok(());
    }
    if stat.is_dir() {
        items.push(Item { relative: relative.into(), remote: remote.into(), local: local.to_path_buf(), size: 0, skipped: Some("directory".into()) });
        let mut dir = sftp.opendir(Path::new(remote)).map_err(|_| SshError::SftpUnavailable)?;
        loop {
            if hooks.cancelled.load(Ordering::Acquire) {
                return Err(SshError::Cancelled);
            }
            match dir.readdir() {
                Ok((path, _)) => {
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        // Keep the lossy spelling only as a display label; never transfer this entry.
                        let display = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        files.push(row(&format!("{relative}/{display}"), "skipped", Some("文件名不是合法的 UTF-8，未开始传输".into()), 0));
                        continue;
                    };
                    if name.is_empty() || name == "." || name == ".." {
                        continue;
                    }
                    if name.contains('\u{FFFD}') {
                        files.push(row(&format!("{relative}/{name}"), "skipped", Some("文件名不是合法的 UTF-8，未开始传输".into()), 0));
                        continue;
                    }
                    let child_remote = path::join_remote(remote, name).map_err(SshError::Message)?;
                    scan_remote(sftp, &child_remote, local_root, &format!("{relative}/{name}"), items, files, hooks)?;
                }
                Err(error) if is_sftp_eof(&error) => break,
                Err(_) => return Err(SshError::Message(format!("无法读取远程目录：{relative}"))),
            }
            (hooks.between)(sftp);
        }
        return Ok(());
    }
    if !stat.is_file() {
        files.push(row(relative, "skipped", Some("已跳过设备、管道或套接字".into()), 0));
        return Ok(());
    }
    items.push(Item {
        relative: relative.into(),
        remote: remote.into(),
        local: local.to_path_buf(),
        size: stat.size.unwrap_or(0),
        skipped: None,
    });
    Ok(())
}

fn copy_one(sftp: &Sftp, spec: &TransferSpec, item: &Item, hooks: &mut Hooks<'_>, snapshot: &mut Snapshot) -> SshResult<FileRow> {
    if spec.direction == Direction::Download {
        // Recheck after scanning: an existing ancestor may now be a junction or symlink.
        guarded_join(&spec.local_root, &item.relative).map_err(SshError::Message)?;
    }
    if item.skipped.as_deref() == Some("directory") {
        match spec.direction {
            Direction::Upload => { let _ = sftp.mkdir(Path::new(&item.remote), 0o755); }
            Direction::Download => { fs::create_dir_all(&item.local).map_err(|_| SshError::Message(format!("无法创建本地目录：{}", item.relative)))?; }
        }
        return Ok(row(&item.relative, "completed", None, 0));
    }
    let before = source_stamp(sftp, spec.direction, item)?;
    let decision = match target_stamp(sftp, spec.direction, item) {
        Ok(Some((size, mtime))) => {
            let choice = (hooks.conflict)(&item.relative, size, mtime)?;
            if choice == Choice::Cancel {
                return Err(SshError::Cancelled);
            }
            if choice == Choice::Skip {
                return Ok(row(&item.relative, "skipped", Some("已跳过同名文件".into()), 0));
            }
            Some((size, mtime))
        }
        Ok(None) => None,
        Err(error) => return Err(error),
    };
    let part_id = Uuid::new_v4().to_string();
    let copied = match spec.direction {
        Direction::Upload => upload_parts(sftp, item, &part_id, hooks, snapshot)?,
        Direction::Download => download_parts(sftp, item, &part_id, hooks, snapshot)?,
    };
    let after = source_stamp(sftp, spec.direction, item)?;
    if before != after {
        let _ = cleanup_part(sftp, spec.direction, item, &part_id);
        return Ok(failed(&item.relative, "源文件在传输过程中发生了变化".into()));
    }
    if let Some((size, mtime)) = decision {
        match target_stamp(sftp, spec.direction, item) {
            Ok(Some(current)) if current != (size, mtime) => {
                let again = (hooks.conflict)(&item.relative, current.0, current.1)?;
                if again != Choice::Overwrite {
                    let _ = cleanup_part(sftp, spec.direction, item, &part_id);
                    return Ok(row(&item.relative, "skipped", Some("目标在确认后发生变化，未覆盖".into()), copied));
                }
            }
            _ => {}
        }
    }
    if spec.direction == Direction::Download {
        guarded_join(&spec.local_root, &item.relative).map_err(SshError::Message)?;
    }
    commit(sftp, spec.direction, item, &part_id, decision.is_some())?;
    Ok(row(&item.relative, "completed", None, copied))
}

fn upload_parts(sftp: &Sftp, item: &Item, part_id: &str, hooks: &mut Hooks<'_>, snapshot: &mut Snapshot) -> SshResult<u64> {
    let part = sibling(&item.remote, &part_file_name(part_id)).map_err(SshError::Message)?;
    let mut remote = sftp.open_mode(Path::new(&part), OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE, 0o644, OpenType::File)
        .map_err(|_| SshError::Message(format!("无法创建远程临时文件：{part}")))?;
    let mut local = File::open(&item.local).map_err(|_| SshError::Message(format!("无法读取本地文件：{}", item.relative)))?;
    copy_stream(&mut local, &mut remote, sftp, item, hooks, snapshot)
}

fn download_parts(sftp: &Sftp, item: &Item, part_id: &str, hooks: &mut Hooks<'_>, snapshot: &mut Snapshot) -> SshResult<u64> {
    let parent = item.local.parent().ok_or_else(|| SshError::Message("下载路径无效".into()))?;
    fs::create_dir_all(parent).map_err(|_| SshError::Message(format!("无法创建本地目录：{}", item.relative)))?;
    let part = parent.join(part_file_name(part_id));
    let mut local = File::create(&part).map_err(|_| SshError::Message(format!("无法创建本地临时文件：{}", part.display())))?;
    let mut remote = sftp.open_mode(Path::new(&item.remote), OpenFlags::READ, 0, OpenType::File)
        .map_err(|_| SshError::Message(format!("无法读取远程文件：{}", item.relative)))?;
    let copied = copy_stream(&mut remote, &mut local, sftp, item, hooks, snapshot)?;
    local.sync_all().ok();
    Ok(copied)
}

fn copy_stream(reader: &mut impl Read, writer: &mut impl Write, sftp: &Sftp, item: &Item, hooks: &mut Hooks<'_>, snapshot: &mut Snapshot) -> SshResult<u64> {
    let mut buf = [0u8; CHUNK];
    let mut copied = 0u64;
    let mut last = Instant::now();
    let mut last_progress = Instant::now();
    loop {
        if stop(hooks) {
            return Err(SshError::Cancelled);
        }
        let read = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock || error.kind() == std::io::ErrorKind::TimedOut => {
                if last.elapsed() > STALL || error.kind() == std::io::ErrorKind::TimedOut {
                    return Err(SshError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(_) if stop(hooks) => return Err(SshError::Cancelled),
            Err(_) => return Err(SshError::Message(format!("读取失败：{}", item.relative))),
        };
        last = Instant::now();
        if let Err(error) = write_chunk(writer, &buf[..read]) {
            if stop(hooks) {
                return Err(SshError::Cancelled);
            }
            if error.kind() == std::io::ErrorKind::TimedOut {
                return Err(SshError::Timeout);
            }
            return Err(SshError::Message(format!("写入失败：{}", item.relative)));
        }
        copied += read as u64;
        snapshot.transferred += read as u64;
        if last_progress.elapsed() >= Duration::from_millis(200) {
            emit(hooks, snapshot);
            last_progress = Instant::now();
        }
        (hooks.between)(sftp);
    }
    Ok(copied)
}

fn write_chunk(writer: &mut impl Write, mut data: &[u8]) -> std::io::Result<()> {
    while !data.is_empty() {
        match writer.write(data) {
            Ok(0) => return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "write")),
            Ok(n) => data = &data[n..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn commit(sftp: &Sftp, direction: Direction, item: &Item, part_id: &str, replace: bool) -> SshResult<()> {
    match direction {
        Direction::Upload => {
            let part = sibling(&item.remote, &part_file_name(part_id)).map_err(SshError::Message)?;
            if !replace {
                return sftp
                    .rename(Path::new(&part), Path::new(&item.remote), None)
                    .map_err(|_| {
                        let _ = sftp.unlink(Path::new(&part));
                        SshError::Message("远程文件无法提交，已清理临时文件".into())
                    });
            }
            // OpenSSH's SFTP v3 rename refuses an existing target and the ssh2
            // crate does not expose posix-rename, so the old file is moved
            // aside first and restored if the replacement cannot be committed.
            let backup = sibling(&item.remote, &format!(".orbyte-transfer-{part_id}.old")).map_err(SshError::Message)?;
            let moved = match sftp.rename(Path::new(&item.remote), Path::new(&backup), None) {
                Ok(()) => true,
                Err(_) => match sftp.lstat(Path::new(&item.remote)) {
                    // The target vanished after confirmation; nothing to preserve.
                    Err(_) => false,
                    Ok(_) => {
                        let _ = sftp.unlink(Path::new(&part));
                        return Err(SshError::Message("无法移走远程原文件，已保留原文件".into()));
                    }
                },
            };
            if sftp.rename(Path::new(&part), Path::new(&item.remote), None).is_err() {
                if moved {
                    if sftp.rename(Path::new(&backup), Path::new(&item.remote), None).is_ok() {
                        let _ = sftp.unlink(Path::new(&part));
                        return Err(SshError::Message("远程文件无法安全替换，已保留原文件".into()));
                    }
                    return Err(SshError::Message(format!(
                        "远程替换失败，原文件保存在 {backup}，临时文件：{part}"
                    )));
                }
                let _ = sftp.unlink(Path::new(&part));
                return Err(SshError::Message("远程文件无法提交，已清理临时文件".into()));
            }
            if moved {
                let _ = sftp.unlink(Path::new(&backup));
            }
        }
        Direction::Download => {
            let part = item.local.parent().unwrap().join(part_file_name(part_id));
            path::replace_file(&part, &item.local).map_err(|_| {
                SshError::Message(format!("本地替换失败，原文件仍在。临时文件：{}", part.display()))
            })?;
        }
    }
    Ok(())
}

fn cleanup_part(sftp: &Sftp, direction: Direction, item: &Item, part_id: &str) -> SshResult<()> {
    match direction {
        Direction::Upload => {
            let part = sibling(&item.remote, &part_file_name(part_id)).map_err(SshError::Message)?;
            let _ = sftp.unlink(Path::new(&part));
        }
        Direction::Download => {
            if let Some(parent) = item.local.parent() {
                let _ = fs::remove_file(parent.join(part_file_name(part_id)));
            }
        }
    }
    Ok(())
}

fn source_stamp(sftp: &Sftp, direction: Direction, item: &Item) -> SshResult<(u64, Option<u64>)> {
    match direction {
        Direction::Upload => local_stamp(&item.local),
        Direction::Download => {
            let stat = sftp_lstat(sftp, &item.remote)?;
            Ok((stat.size.unwrap_or(0), stat.mtime))
        }
    }
}

fn target_stamp(sftp: &Sftp, direction: Direction, item: &Item) -> SshResult<Option<(Option<u64>, Option<u64>)>> {
    match direction {
        Direction::Upload => match sftp.lstat(Path::new(&item.remote)) {
            Ok(stat) if stat.is_dir() => Err(SshError::Message(format!("远程目标是目录：{}", item.relative))),
            Ok(stat) => Ok(Some((stat.size, stat.mtime))),
            Err(_) => Ok(None),
        },
        Direction::Download => match fs::symlink_metadata(&item.local) {
            Ok(meta) => {
                let mtime = meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
                Ok(Some((Some(meta.len()), mtime)))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(SshError::Message(format!("无法检查本地目标：{}", item.relative))),
        },
    }
}

fn local_stamp(path: &Path) -> SshResult<(u64, Option<u64>)> {
    let meta = fs::symlink_metadata(path).map_err(|_| SshError::Message("无法读取本地文件".into()))?;
    let mtime = meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
    Ok((meta.len(), mtime))
}

fn sftp_lstat(sftp: &Sftp, remote: &str) -> SshResult<ssh2::FileStat> {
    sftp.lstat(Path::new(remote)).map_err(|error| {
        if is_eagain(&error) { SshError::Timeout } else { SshError::Message(format!("无法读取远程属性：{remote}")) }
    })
}

fn sibling(remote: &str, name: &str) -> Result<String, String> {
    let parent = remote.rsplit_once('/').map(|(dir, _)| if dir.is_empty() { "/" } else { dir }).unwrap_or(".");
    path::join_remote(parent, name)
}

fn stop(hooks: &Hooks<'_>) -> bool {
    hooks.cancelled.load(Ordering::Acquire)
}

fn emit(hooks: &Hooks<'_>, snapshot: &Snapshot) {
    (hooks.progress)(snapshot.clone());
}

fn finish(hooks: &Hooks<'_>, mut snapshot: Snapshot, files: &[FileRow]) -> Snapshot {
    snapshot.files = files.to_vec();
    emit(hooks, &snapshot);
    snapshot
}

fn row(relative: &str, status: &str, message: Option<String>, total: u64) -> FileRow {
    FileRow { relative: relative.into(), status: status.into(), message, bytes: total, total: Some(total) }
}

fn failed(relative: &str, message: String) -> FileRow {
    row(relative, "failed", Some(message), 0)
}

fn rate(bytes: u64, elapsed: Duration) -> u64 {
    let millis = elapsed.as_millis().max(1);
    (bytes as u128 * 1000 / millis) as u64
}

#[cfg(test)]
pub fn apply_choice(current: &mut Option<Choice>, incoming: Choice, apply_all: bool) -> Choice {
    if apply_all {
        *current = Some(incoming);
    }
    current.unwrap_or(incoming)
}

fn mark_case_conflicts(items: &mut [Item]) {
    let mut seen = HashSet::new();
    for item in items {
        if item.skipped.is_some() {
            continue;
        }
        let key = item.local.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            item.skipped = Some("与另一文件只在大小写上不同，已跳过以避免覆盖".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(local: &str, skipped: Option<&str>) -> Item {
        Item {
            relative: local.into(),
            remote: format!("/remote/{local}"),
            local: PathBuf::from(local),
            size: 1,
            skipped: skipped.map(str::to_owned),
        }
    }

    #[test]
    fn later_case_folded_collisions_are_skipped() {
        let mut items = vec![item("Readme.txt", None), item("README.TXT", None), item("other.txt", None)];
        mark_case_conflicts(&mut items);
        assert!(items[0].skipped.is_none());
        assert!(items[1].skipped.is_some());
        assert!(items[2].skipped.is_none());
    }

    #[test]
    fn already_skipped_items_do_not_claim_the_local_name() {
        let mut items = vec![item("a.txt", Some("directory")), item("A.txt", None)];
        mark_case_conflicts(&mut items);
        assert!(items[1].skipped.is_none());
    }

    #[test]
    fn apply_choice_is_sticky_only_when_requested() {
        let mut current = None;
        assert_eq!(apply_choice(&mut current, Choice::Skip, false), Choice::Skip);
        assert!(current.is_none());
        assert_eq!(apply_choice(&mut current, Choice::Overwrite, true), Choice::Overwrite);
        assert_eq!(apply_choice(&mut current, Choice::Skip, false), Choice::Overwrite);
    }
}
