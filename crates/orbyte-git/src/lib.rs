//! 系统 Git 调用与 porcelain 解析。Author: jaswtao · Date: 2026-10-14
use serde::Serialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

pub fn find_git() -> Option<&'static Path> {
    static GIT: OnceLock<Option<PathBuf>> = OnceLock::new();
    GIT.get_or_init(|| {
        orbyte_platform::shell::find_in_path(
            &orbyte_platform::shell::SystemShellEnv,
            if cfg!(windows) { "git.exe" } else { "git" },
        )
    })
    .as_deref()
}

fn run(root: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let git = find_git().ok_or("未安装 Git")?;
    let mut command = Command::new(git);
    command
        .current_dir(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        // 错误判断依赖英文文本，禁止本地化输出。
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| format!("无法运行 Git：{e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    // 同时排空两条管道，避免大量状态输出阻塞进程。
    let out = std::thread::spawn(move || {
        let mut data = vec![];
        stdout.read_to_end(&mut data).map(|_| data)
    });
    let err = std::thread::spawn(move || {
        let mut data = vec![];
        stderr.read_to_end(&mut data).map(|_| data)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(match result {
                    Err(e) => e.to_string(),
                    _ => "Git 操作超时".into(),
                });
            }
        }
    };
    let output = out
        .join()
        .map_err(|_| "Git 输出读取失败")?
        .map_err(|e| e.to_string())?;
    let error = err
        .join()
        .map_err(|_| "Git 错误读取失败")?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(String::from_utf8_lossy(&error).trim().to_owned());
    }
    Ok(output)
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFile>,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct GitFile {
    pub path: String,
    pub kind: String,
}

pub fn parse_status(bytes: &[u8]) -> GitStatus {
    let mut status = GitStatus {
        is_repo: true,
        ..Default::default()
    };
    let mut records = bytes.split(|b| *b == 0);
    while let Some(record) = records.next() {
        let text = String::from_utf8_lossy(record);
        if let Some(branch) = text.strip_prefix("# branch.head ") {
            status.detached = branch == "(detached)";
            status.branch = (!status.detached).then(|| branch.to_owned());
        } else if let Some(upstream) = text.strip_prefix("# branch.upstream ") {
            status.upstream = Some(upstream.into());
        } else if let Some(ab) = text.strip_prefix("# branch.ab ") {
            let mut parts = ab.split_whitespace();
            status.ahead = parts
                .next()
                .unwrap_or("")
                .trim_start_matches('+')
                .parse()
                .unwrap_or(0);
            status.behind = parts
                .next()
                .unwrap_or("")
                .trim_start_matches('-')
                .parse()
                .unwrap_or(0);
        } else {
            let (path, kind) = if let Some(path) = text.strip_prefix("? ") {
                (path, "untracked")
            } else if text.starts_with("1 ") || text.starts_with("2 ") || text.starts_with("u ") {
                let count = match text.as_bytes()[0] {
                    b'1' => 9,
                    b'2' => 10,
                    _ => 11,
                };
                let fields: Vec<_> = text.splitn(count, ' ').collect();
                if fields.len() < count {
                    continue;
                }
                let xy = fields[1];
                let kind = if text.starts_with("u ") {
                    "conflicted"
                } else if xy.contains('R') || text.starts_with("2 ") {
                    "renamed"
                } else if xy.contains('D') {
                    "deleted"
                } else if xy.contains('A') {
                    "added"
                } else {
                    "modified"
                };
                if text.starts_with("2 ") {
                    records.next();
                }
                (*fields.last().unwrap(), kind)
            } else {
                continue;
            };
            if status.files.len() < 5000 {
                status.files.push(GitFile {
                    path: path.replace('\\', "/"),
                    kind: kind.into(),
                });
            } else {
                status.truncated = true;
            }
        }
    }
    status
}

/// 文件路径相对于项目根目录；项目是仓库子目录时只包含该子目录内的改动。
pub fn status(root: &Path) -> Result<GitStatus, String> {
    let prefix = match run(root, &["rev-parse", "--show-prefix"], Duration::from_secs(5)) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).trim_end_matches(['\r', '\n']).to_owned(),
        Err(e) if e.contains("not a git repository") => return Ok(GitStatus::default()),
        Err(e) => return Err(e),
    };
    let bytes = run(
        root,
        &[
            "-c",
            "core.quotepath=false",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
            "--",
            ".",
        ],
        Duration::from_secs(5),
    )?;
    let mut status = parse_status(&bytes);
    if !prefix.is_empty() {
        for file in &mut status.files {
            if let Some(path) = file.path.strip_prefix(&prefix) {
                file.path = path.to_owned();
            }
        }
    }
    Ok(status)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub detached: bool,
}
pub fn worktree_list(root: &Path) -> Result<Vec<Worktree>, String> {
    let bytes = run(
        root,
        // 不用 -z：该参数需要 Git 2.36+。
        &["worktree", "list", "--porcelain"],
        Duration::from_secs(5),
    )?;
    let mut result: Vec<Worktree> = vec![];
    for field in bytes.split(|b| *b == b'\n') {
        let field = String::from_utf8_lossy(field.strip_suffix(b"\r").unwrap_or(field));
        if let Some(path) = field.strip_prefix("worktree ") {
            result.push(Worktree {
                path: path.into(),
                head: String::new(),
                branch: None,
                detached: false,
            });
        } else if let Some(last) = result.last_mut() {
            if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                last.branch = Some(branch.into());
            }
            if let Some(head) = field.strip_prefix("HEAD ") {
                last.head = head.into();
            }
            if field == "detached" {
                last.detached = true;
            }
        }
    }
    Ok(result)
}
pub fn worktree_add(
    root: &Path,
    path: &Path,
    branch: &str,
    create_branch: bool,
) -> Result<(), String> {
    let path_text = path.to_string_lossy();
    if !path.is_absolute()
        || path_text.starts_with("\\\\")
        || path_text.starts_with("//")
        || path.exists()
    {
        return Err("目标必须是尚不存在的本地绝对路径".into());
    }
    if branch.is_empty() || branch.starts_with('-') {
        return Err("分支名无效".into());
    }
    run(
        root,
        &["check-ref-format", "--branch", branch],
        Duration::from_secs(5),
    )
    .map_err(|_| "分支名无效".to_owned())?;
    let mut args = vec!["worktree", "add"];
    if create_branch {
        args.extend(["-b", branch]);
    }
    args.extend(["--", &path_text]);
    if !create_branch {
        args.push(branch);
    }
    run(root, &args, Duration::from_secs(30))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_branch_changes_rename_conflict_and_chinese() {
        let status = parse_status(b"# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\01 .M N... 100644 100644 100644 aaa bbb src/file x\02 R. N... 100644 100644 100644 aaa bbb R100 new.txt\0old.txt\0u UU N... 100644 100644 100644 100644 a b c conflict\0? \xe4\xb8\xad\xe6\x96\x87.txt\0");
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!((status.ahead, status.behind), (2, 3));
        assert_eq!(
            status
                .files
                .iter()
                .map(|f| f.kind.as_str())
                .collect::<Vec<_>>(),
            ["modified", "renamed", "conflicted", "untracked"]
        );
        assert_eq!(status.files[0].path, "src/file x");
        assert_eq!(status.files[1].path, "new.txt");
        assert_eq!(status.files[2].path, "conflict");
        assert_eq!(status.files[3].path, "中文.txt");
        let detached = parse_status(b"# branch.head (detached)\0");
        assert!(detached.detached);
        assert!(detached.upstream.is_none());
    }
    #[test]
    fn real_repository_and_worktree() {
        if find_git().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        run(&root, &["init", "-b", "main"], Duration::from_secs(5)).unwrap();
        std::fs::write(root.join("中文.txt"), "test").unwrap();
        assert_eq!(status(&root).unwrap().files[0].kind, "untracked");
        run(&root, &["add", "."], Duration::from_secs(5)).unwrap();
        assert_eq!(status(&root).unwrap().files[0].kind, "added");
        run(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "initial",
            ],
            Duration::from_secs(5),
        )
        .unwrap();
        let work = temp.path().join("work");
        worktree_add(&root, &work, "feature", true).unwrap();
        assert_eq!(worktree_list(&root).unwrap().len(), 2);
        assert!(worktree_add(&root, &work, "other", true).is_err());
        assert!(worktree_add(&root, &temp.path().join("invalid"), "bad name", true).is_err());
        let existing = temp.path().join("existing");
        run(&root, &["branch", "existing"], Duration::from_secs(5)).unwrap();
        worktree_add(&root, &existing, "existing", false).unwrap();
        assert!(worktree_add(&root, Path::new(r"\\server\share\work"), "unc", true).is_err());
        std::fs::write(root.join("中文.txt"), "changed").unwrap();
        assert_eq!(status(&root).unwrap().files[0].kind, "modified");
        std::fs::remove_file(root.join("中文.txt")).unwrap();
        assert_eq!(status(&root).unwrap().files[0].kind, "deleted");
        let sub = root.join("sub");
        std::fs::create_dir_all(sub.join("nested")).unwrap();
        std::fs::write(sub.join("nested").join("a.txt"), "x").unwrap();
        std::fs::write(sub.join("keep.txt"), "x").unwrap();
        run(&root, &["add", "sub/keep.txt"], Duration::from_secs(5)).unwrap();
        let nested = status(&sub).unwrap();
        let files: Vec<_> = nested.files.iter().map(|f| (f.path.as_str(), f.kind.as_str())).collect();
        assert_eq!(files, [("keep.txt", "added"), ("nested/", "untracked")]);
        assert!(!status(temp.path()).unwrap().is_repo);
    }
    #[test]
    fn limits_records_and_parses_added_deleted() {
        let status = parse_status(b"1 A. N... 000000 100644 100644 a b added\01 .D N... 100644 100644 000000 a b deleted\0");
        assert_eq!(status.files[0].kind, "added");
        assert_eq!(status.files[1].kind, "deleted");
        let status = parse_status("? file\0".repeat(5001).as_bytes());
        assert!(status.truncated);
        assert_eq!(status.files.len(), 5000);
    }
    #[test]
    #[ignore = "显式运行的 5 万文件性能验收"]
    fn status_fifty_thousand_files() {
        if find_git().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        run(temp.path(), &["init", "-b", "main"], Duration::from_secs(5)).unwrap();
        for i in 0..50_000 {
            std::fs::write(temp.path().join(format!("file-{i}.txt")), "x").unwrap();
        }
        let started = Instant::now();
        let status = status(temp.path()).unwrap();
        println!(
            "50000 文件 git_status: {} ms",
            started.elapsed().as_millis()
        );
        assert_eq!(status.files.len(), 5000);
        assert!(status.truncated);
    }
}
