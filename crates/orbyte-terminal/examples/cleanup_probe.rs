//! 步骤 1 进程清理探针：在真实终端会话中启动多层子进程，结束会话后检查进程是否全部退出。
//!
//! 运行：`cargo run -p orbyte-terminal --example cleanup_probe -- [pwsh-node|wsl|claude|codex]...`
//!
//! Author: jaswtao
//! Date: 2026-10-09

#[cfg(windows)]
fn main() {
    probe::run();
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod probe {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use orbyte_platform::shell::{detect_shell, ShellType, SystemShellEnv};
    use orbyte_platform::windows::process::is_process_alive;
    use orbyte_terminal::output::{FRAME_HEADER_LEN, FRAME_OUTPUT};
    use orbyte_terminal::{FrameSink, SpawnRequest, TerminalManager, TerminalSize};

    struct AutoAck {
        manager: Arc<TerminalManager>,
        id: Mutex<Option<String>>,
    }

    impl FrameSink for AutoAck {
        fn send(&self, frame: Vec<u8>) -> bool {
            let Some(id) = self.id.lock().unwrap().clone() else { return true };
            if frame[0] == FRAME_OUTPUT {
                let payload = &frame[FRAME_HEADER_LEN..];
                if payload.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = self.manager.write(&id, b"\x1b[1;1R".to_vec());
                }
                let seq = u64::from_le_bytes(frame[1..9].try_into().unwrap());
                let _ = self.manager.ack(&id, seq);
            }
            true
        }
    }

    struct Scenario {
        name: &'static str,
        shell: ShellType,
        input: &'static str,
        min_processes: usize,
        settle: Duration,
    }

    pub fn run() {
        let all = [
            Scenario {
                name: "pwsh-node",
                shell: ShellType::Powershell,
                input: "node -e \"require('child_process').spawn('node',['-e','setInterval(()=>{},1000)'],{stdio:'inherit'});setInterval(()=>{},1000)\"\r",
                min_processes: 3,
                settle: Duration::from_secs(3),
            },
            Scenario {
                name: "wsl",
                shell: ShellType::Wsl,
                input: "sleep 3001 & sleep 3002\n",
                min_processes: 1,
                settle: Duration::from_secs(8),
            },
            Scenario {
                name: "claude",
                shell: ShellType::Powershell,
                input: "claude\r",
                min_processes: 2,
                settle: Duration::from_secs(10),
            },
            Scenario {
                name: "codex",
                shell: ShellType::Powershell,
                input: "codex\r",
                min_processes: 3,
                settle: Duration::from_secs(10),
            },
        ];
        let wanted: Vec<String> = std::env::args().skip(1).collect();
        for s in all.iter().filter(|s| wanted.is_empty() || wanted.iter().any(|w| w == s.name)) {
            run_scenario(s);
        }
    }

    fn run_scenario(s: &Scenario) {
        let info = detect_shell(&SystemShellEnv, s.shell);
        let Some(program) = info.program else {
            println!("[{}] 跳过：{}", s.name, info.unavailable_reason.unwrap_or_default());
            return;
        };
        let manager = Arc::new(TerminalManager::default());
        let sink = Arc::new(AutoAck { manager: manager.clone(), id: Mutex::new(None) });
        let req = SpawnRequest {
            program,
            env: vec![],
            args: vec![],
            cwd: std::env::temp_dir(),
            size: TerminalSize { cols: 120, rows: 30 },
            config_id: None,
        };
        let id = manager.spawn(&req, sink.clone()).expect("spawn");
        *sink.id.lock().unwrap() = Some(id.clone());
        let session = manager.get(&id).unwrap();

        std::thread::sleep(Duration::from_secs(2));
        manager.write(&id, s.input.as_bytes().to_vec()).unwrap();
        std::thread::sleep(s.settle);

        let pids = session.job_process_ids();
        println!("[{}] {} 会话 Job 进程：{:?}", s.name, info.description, describe(&pids));
        if pids.len() < s.min_processes {
            println!("[{}] 警告：进程数少于预期 {}", s.name, s.min_processes);
        }

        let started = Instant::now();
        let code = manager.stop(&id).unwrap();
        let elapsed = started.elapsed();
        std::thread::sleep(Duration::from_millis(500));
        let alive: Vec<_> = pids.iter().copied().filter(|&p| is_process_alive(p)).collect();
        println!(
            "[{}] stop 用时 {:?}，退出码 {:?}，残留 Windows 进程：{:?}",
            s.name,
            elapsed,
            code,
            describe(&alive)
        );
        if s.shell == ShellType::Wsl {
            report_wsl_leftovers(s.name);
        }
        manager.remove(&id).unwrap();
    }

    fn report_wsl_leftovers(name: &str) {
        let out = std::process::Command::new("wsl.exe")
            .args(["-e", "sh", "-c", "pgrep -af 'sleep 300[12]' || true"])
            .output();
        match out {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                println!(
                    "[{name}] 残留 Linux 进程：{}",
                    if text.is_empty() { "无" } else { text.as_str() }
                );
                if !text.is_empty() {
                    let _ = std::process::Command::new("wsl.exe")
                        .args(["-e", "sh", "-c", "pkill -f 'sleep 300[12]'"])
                        .output();
                }
            }
            Err(e) => println!("[{name}] 无法检查 Linux 进程：{e}"),
        }
    }

    fn describe(pids: &[u32]) -> Vec<String> {
        let names = process_names();
        pids.iter()
            .map(|p| format!("{}({p})", names.iter().find(|(id, _)| id == p).map(|(_, n)| n.as_str()).unwrap_or("?")))
            .collect()
    }

    fn process_names() -> Vec<(u32, String)> {
        let out = std::process::Command::new("tasklist").args(["/FO", "CSV", "/NH"]).output();
        let Ok(out) = out else { return vec![] };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let cols: Vec<&str> = l.split("\",\"").collect();
                let name = cols.first()?.trim_start_matches('"').to_string();
                let pid = cols.get(1)?.parse().ok()?;
                Some((pid, name))
            })
            .collect()
    }
}
