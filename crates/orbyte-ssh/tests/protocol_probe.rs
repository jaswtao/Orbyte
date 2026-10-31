//! Real OpenSSH checks. Not part of ordinary `cargo test`.
//!
//! Run with `ORBYTE_SSH_PROBE=1` and the variables documented in `doc/ssh-probe.md`:
//! `cargo test -p orbyte-ssh --features probe --test protocol_probe -- --ignored --test-threads=1 --nocapture`
//!
//! Author: jaswtao
//! Date: 2026-10-31

#![cfg(feature = "probe")]

use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use orbyte_ssh::probe::{
    authenticate, connect_transport, run_transfer, CancelToken, Choice, Connected, Direction, Hooks, SshError,
    Snapshot, TransferSpec,
};
use orbyte_ssh::Credential;
use sha2::{Digest, Sha256};
use ssh2::Sftp;

fn require(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required; see doc/ssh-probe.md"))
}

fn port_or(name: &str, default: u16) -> u16 {
    env::var(name).ok().map(|value| value.parse().expect("port")).unwrap_or(default)
}

fn require_probe() {
    if env::var("ORBYTE_SSH_PROBE").ok().as_deref() != Some("1") {
        panic!("ORBYTE_SSH_PROBE=1 is required so an ignored run cannot pass without a server");
    }
}

fn host() -> String {
    require("ORBYTE_SSH_HOST")
}

fn user() -> String {
    env::var("ORBYTE_SSH_USER").unwrap_or_else(|_| "orbyte".into())
}

fn password() -> String {
    require("ORBYTE_SSH_PASSWORD")
}

fn connect(host: &str, port: u16, saved: &[String]) -> Result<Connected, SshError> {
    let cancel = CancelToken::new();
    connect_transport(host, port, saved, &cancel)
}

fn connect_authenticated(
    host: &str,
    port: u16,
    credential: &Credential,
    secret: &str,
) -> Result<Connected, SshError> {
    let mut connected = connect(host, port, &[])?;
    let cancel = CancelToken::new();
    authenticate(&mut connected, &cancel, &user(), credential, secret)?;
    Ok(connected)
}

fn open_sftp(connected: Connected) -> Sftp {
    connected.session.set_blocking(true);
    connected.session.set_timeout(30_000);
    connected.session.sftp().expect("sftp subsystem")
}

fn child(parent: &str, name: &str) -> String {
    format!("{}/{}", parent.trim_end_matches('/'), name)
}

fn stamp() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
}

fn local_temp(tag: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("orbyte-{tag}-{}", stamp()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn remove_remote_tree(sftp: &Sftp, path: &str) {
    let Ok(stat) = sftp.lstat(Path::new(path)) else { return };
    if stat.is_dir() && !stat.file_type().is_symlink() {
        if let Ok(mut dir) = sftp.opendir(Path::new(path)) {
            while let Ok((entry, _)) = dir.readdir() {
                let Some(name) = entry.file_name().and_then(|name| name.to_str()) else { continue };
                if name.is_empty() || name == "." || name == ".." {
                    continue;
                }
                remove_remote_tree(sftp, &child(path, name));
            }
        }
        let _ = sftp.rmdir(Path::new(path));
    } else {
        let _ = sftp.unlink(Path::new(path));
    }
}

fn read_remote(sftp: &Sftp, path: &str) -> Vec<u8> {
    let mut file = sftp.open(Path::new(path)).expect("open remote");
    let mut out = Vec::new();
    file.read_to_end(&mut out).expect("read remote");
    out
}

fn run(sftp: &Sftp, spec: TransferSpec, cancelled: &AtomicBool, choice: Choice) -> Snapshot {
    let progress = |_snapshot: Snapshot| {};
    let conflict = move |_name: &str, _size: Option<u64>, _mtime: Option<u64>| Ok::<Choice, SshError>(choice);
    let mut between = |_sftp: &Sftp| {};
    let mut hooks = Hooks { cancelled, progress: &progress, conflict: &conflict, between: &mut between };
    run_transfer(sftp, &spec, &mut hooks)
}

fn download_spec(remote: &str, local: &Path, names: &[&str]) -> TransferSpec {
    TransferSpec {
        direction: Direction::Download,
        remote_directory: remote.into(),
        local_root: local.to_path_buf(),
        names: names.iter().map(|name| (*name).to_owned()).collect(),
    }
}

fn upload_spec(remote: &str, names: Vec<String>) -> TransferSpec {
    TransferSpec { direction: Direction::Upload, remote_directory: remote.into(), local_root: PathBuf::new(), names }
}

fn sha256_file(path: &Path) -> String {
    let mut file = File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    format!("{:x}", hasher.finalize())
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn protocol_probe() {
    require_probe();
    let connected = connect_authenticated(&host(), port_or("ORBYTE_SSH_PORT", 2222), &Credential::Password, &password())
        .expect("password authentication");
    assert!(connected.session.authenticated());
    eprintln!("password authentication ok: {}@{}", user(), host());
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn private_key_probe() {
    require_probe();
    let passphrase = require("ORBYTE_SSH_KEY_PASSPHRASE");
    let cases = [
        ("ed25519", require("ORBYTE_SSH_ED25519_KEY"), String::new()),
        ("rsa", require("ORBYTE_SSH_RSA_KEY"), String::new()),
        ("encrypted-ed25519", require("ORBYTE_SSH_ENCRYPTED_KEY"), passphrase),
    ];
    for (label, path, secret) in cases {
        let credential = Credential::PrivateKey { path: path.clone() };
        let connected = connect_authenticated(&host(), port_or("ORBYTE_SSH_PORT", 2222), &credential, &secret)
            .unwrap_or_else(|error| panic!("{label} key authentication failed: {error}"));
        assert!(connected.session.authenticated(), "{label}");
        eprintln!("{label} key authentication ok: {path}");
    }
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn saved_host_key_algorithms_are_preferred() {
    require_probe();
    let rsa = connect(&host(), port_or("ORBYTE_SSH_PORT", 2222), &["rsa-sha2-256".to_owned()]).expect("rsa host key");
    assert!(rsa.presented.algorithm.starts_with("rsa-sha2-"), "{}", rsa.presented.algorithm);
    let ed25519 = connect(&host(), port_or("ORBYTE_SSH_PORT", 2222), &["ssh-ed25519".to_owned()]).expect("ed25519 host key");
    assert_eq!(ed25519.presented.algorithm, "ssh-ed25519");
    eprintln!("host key algorithms ok: {} / {}", rsa.presented.algorithm, ed25519.presented.algorithm);
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn keyboard_interactive_only_is_refused_before_sending_the_secret() {
    require_probe();
    let port = port_or("ORBYTE_SSH_KBD_PORT", 2224);
    match connect_authenticated(&host(), port, &Credential::Password, "probe-sentinel") {
        Err(SshError::KeyboardInteractiveUnsupported) => eprintln!("keyboard-interactive-only refused as expected"),
        Err(error) => panic!("expected KeyboardInteractiveUnsupported, got {error}"),
        Ok(_) => panic!("keyboard-interactive-only server must not authenticate with a password"),
    }
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn weak_algorithms_are_refused() {
    require_probe();
    let result = connect(&host(), port_or("ORBYTE_SSH_WEAK_PORT", 2223), &[]);
    match result {
        Err(SshError::WeakAlgorithm | SshError::Message(_) | SshError::Timeout) => {
            eprintln!("weak algorithm server refused as expected");
        }
        Err(error) => panic!("unexpected error: {error}"),
        Ok(_) => panic!("a server offering only SHA-1/ssh-rsa must not connect"),
    }
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn pty_resize_probe() {
    require_probe();
    let connected = connect_authenticated(&host(), port_or("ORBYTE_SSH_PORT", 2222), &Credential::Password, &password())
        .expect("password authentication");
    connected.session.set_blocking(true);
    connected.session.set_timeout(15_000);
    let mut channel = connected.session.channel_session().expect("channel");
    channel.request_pty("xterm-256color", None, Some((120, 40, 0, 0))).expect("pty");
    channel.shell().expect("shell");
    channel.request_pty_size(132, 43, None, None).expect("pty resize");
    channel.write_all(b"stty size; exit\n").expect("write");
    let mut output = String::new();
    let _ = channel.read_to_string(&mut output);
    let _ = channel.wait_close();
    assert!(output.contains("43 132"), "resize output: {output:?}");
    eprintln!("pty resize ok: 43 132");
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn sftp_lstat_rename_and_symlink_probe() {
    require_probe();
    let connected = connect_authenticated(&host(), port_or("ORBYTE_SSH_PORT", 2222), &Credential::Password, &password())
        .expect("password authentication");
    let sftp = open_sftp(connected);
    let home = sftp.realpath(Path::new(".")).unwrap().to_string_lossy().into_owned();
    let root = child(&home, &format!("orbyte-probe-sftp-{}", stamp()));
    sftp.mkdir(Path::new(&root), 0o755).expect("mkdir");

    let file = child(&root, "中文 文件.txt");
    let mut remote = sftp.create(Path::new(&file)).expect("create");
    remote.write_all(b"hello").expect("write");
    remote.close().expect("close");
    let stat = sftp.lstat(Path::new(&file)).expect("lstat file");
    assert!(stat.is_file());
    assert_eq!(stat.size, Some(5));

    let renamed = child(&root, "renamed.txt");
    sftp.rename(Path::new(&file), Path::new(&renamed), None).expect("rename");
    assert!(sftp.lstat(Path::new(&file)).is_err(), "old name must be gone");
    assert_eq!(read_remote(&sftp, &renamed), b"hello");

    let link = child(&root, "link.txt");
    sftp.symlink(Path::new(&renamed), Path::new(&link)).expect("symlink");
    let link_stat = sftp.lstat(Path::new(&link)).expect("lstat symlink");
    assert!(link_stat.file_type().is_symlink(), "lstat must not follow the link");

    remove_remote_tree(&sftp, &root);
    eprintln!("sftp lstat/rename/symlink ok: {root}");
}

#[test]
#[ignore = "requires an isolated OpenSSH server; see doc/ssh-probe.md"]
fn transfer_probe() {
    require_probe();
    let connected = connect_authenticated(&host(), port_or("ORBYTE_SSH_PORT", 2222), &Credential::Password, &password())
        .expect("password authentication");
    let sftp = open_sftp(connected);
    let home = sftp.realpath(Path::new(".")).unwrap().to_string_lossy().into_owned();
    let root = child(&home, &format!("orbyte-probe-transfer-{}", stamp()));
    sftp.mkdir(Path::new(&root), 0o755).expect("mkdir");

    let local = local_temp("probe-upload");
    let empty = local.join("empty.txt");
    let directory = local.join("子目录 空格");
    let nested = directory.join("文件 1.txt");
    let empty_dir = local.join("空 目录");
    fs::write(&empty, b"").unwrap();
    fs::create_dir_all(&directory).unwrap();
    fs::write(&nested, "content-中文").unwrap();
    fs::create_dir_all(&empty_dir).unwrap();

    let uploaded = run(
        &sftp,
        upload_spec(&root, vec![empty.display().to_string(), directory.display().to_string(), empty_dir.display().to_string()]),
        &AtomicBool::new(false),
        Choice::Overwrite,
    );
    assert_eq!(uploaded.state, "completed");
    assert!(uploaded.files.iter().all(|file| file.status == "completed"), "{:?}", uploaded.files);
    assert_eq!(read_remote(&sftp, &child(&root, "empty.txt")), b"");
    assert_eq!(read_remote(&sftp, &child(&child(&root, "子目录 空格"), "文件 1.txt")), "content-中文".as_bytes());
    assert!(sftp.lstat(Path::new(&child(&root, "空 目录"))).unwrap().is_dir(), "empty directory must be created");

    let download_dir = local_temp("probe-download");
    let downloaded = run(&sftp, download_spec(&root, &download_dir, &["empty.txt", "子目录 空格", "空 目录"]), &AtomicBool::new(false), Choice::Skip);
    assert_eq!(downloaded.state, "completed");
    assert_eq!(fs::metadata(download_dir.join("empty.txt")).unwrap().len(), 0);
    assert_eq!(fs::read(download_dir.join("子目录 空格").join("文件 1.txt")).unwrap(), "content-中文".as_bytes());
    assert!(download_dir.join("空 目录").is_dir(), "empty directory must be created locally");

    let remote_nested = child(&child(&root, "子目录 空格"), "文件 1.txt");
    fs::write(&nested, "changed").unwrap();
    let skipped = run(&sftp, upload_spec(&root, vec![directory.display().to_string()]), &AtomicBool::new(false), Choice::Skip);
    assert!(skipped.files.iter().any(|file| file.status == "skipped"), "{:?}", skipped.files);
    assert_eq!(read_remote(&sftp, &remote_nested), "content-中文".as_bytes());

    let replaced = run(&sftp, upload_spec(&root, vec![directory.display().to_string()]), &AtomicBool::new(false), Choice::Overwrite);
    assert!(replaced.files.iter().any(|file| file.status == "completed"), "{:?}", replaced.files);
    assert_eq!(read_remote(&sftp, &remote_nested), b"changed");

    let link = child(&root, "link.txt");
    sftp.symlink(Path::new(&child(&root, "empty.txt")), Path::new(&link)).unwrap();
    let link_dir = local_temp("probe-link");
    let linked = run(&sftp, download_spec(&root, &link_dir, &["link.txt"]), &AtomicBool::new(false), Choice::Skip);
    assert!(linked.files.iter().any(|file| file.status == "skipped"), "{:?}", linked.files);
    assert!(!link_dir.join("link.txt").exists(), "download must not follow symlinks");

    let cancelled = AtomicBool::new(false);
    let counter = AtomicUsize::new(0);
    let progress = |_snapshot: Snapshot| {};
    let conflict = |_name: &str, _size: Option<u64>, _mtime: Option<u64>| Ok::<Choice, SshError>(Choice::Overwrite);
    let mut between = |_sftp: &Sftp| {
        if counter.fetch_add(1, Ordering::Relaxed) >= 8 {
            cancelled.store(true, Ordering::Release);
        }
    };
    let big = local.join("big.bin");
    fs::write(&big, vec![0xA5u8; 16 * 1024 * 1024]).unwrap();
    let mut hooks = Hooks { cancelled: &cancelled, progress: &progress, conflict: &conflict, between: &mut between };
    let cancelled_snapshot = run_transfer(&sftp, &upload_spec(&root, vec![big.display().to_string()]), &mut hooks);
    assert_eq!(cancelled_snapshot.state, "cancelled");
    assert!(sftp.lstat(Path::new(&child(&root, "big.bin"))).is_err(), "cancelled upload must not commit");

    if env::var("ORBYTE_SSH_TRANSFER_1GIB").ok().as_deref() == Some("1") {
        let gib = local.join("big-1g.bin");
        let mut file = File::create(&gib).unwrap();
        let block = vec![0x5Au8; 64 * 1024 * 1024];
        for _ in 0..16 {
            file.write_all(&block).unwrap();
        }
        drop(file);
        let large = run(&sftp, upload_spec(&root, vec![gib.display().to_string()]), &AtomicBool::new(false), Choice::Overwrite);
        assert_eq!(large.state, "completed");
        let gib_dir = local_temp("probe-1gib");
        let back = run(&sftp, download_spec(&root, &gib_dir, &["big-1g.bin"]), &AtomicBool::new(false), Choice::Skip);
        assert_eq!(back.state, "completed");
        assert_eq!(sha256_file(&gib), sha256_file(&gib_dir.join("big-1g.bin")));
        let _ = fs::remove_file(&gib);
        let _ = fs::remove_dir_all(&gib_dir);
        eprintln!("1 GiB round trip SHA-256 ok");
    }

    remove_remote_tree(&sftp, &root);
    let _ = fs::remove_dir_all(&local);
    let _ = fs::remove_dir_all(&download_dir);
    let _ = fs::remove_dir_all(&link_dir);
    eprintln!("transfer upload/download/conflict/cancel ok: {root}");
}
