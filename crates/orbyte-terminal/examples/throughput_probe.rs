//! 输出吞吐基线：不经过前端、立即 ack，测量 PowerShell 大量输出所需时间。
//!
//! 运行：`cargo run --release -p orbyte-terminal --example throughput_probe -- [行数]`
//!
//! Author: jaswtao
//! Date: 2026-10-09

#[cfg(windows)]
fn main() {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use orbyte_platform::shell::{detect_shell, ShellType, SystemShellEnv};
    use orbyte_terminal::output::{FRAME_HEADER_LEN, FRAME_OUTPUT};
    use orbyte_terminal::{FrameSink, SpawnRequest, TerminalManager, TerminalSize};

    struct Sink {
        manager: Arc<TerminalManager>,
        id: Mutex<Option<String>>,
        bytes: Mutex<(usize, Vec<u8>)>,
    }
    impl FrameSink for Sink {
        fn send(&self, frame: Vec<u8>) -> bool {
            let Some(id) = self.id.lock().unwrap().clone() else { return true };
            if frame[0] == FRAME_OUTPUT {
                let payload = &frame[FRAME_HEADER_LEN..];
                if payload.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = self.manager.write(&id, b"\x1b[1;1R".to_vec());
                }
                let mut b = self.bytes.lock().unwrap();
                b.0 += payload.len();
                b.1.extend_from_slice(payload);
                let keep = b.1.len().saturating_sub(4096);
                b.1.drain(..keep);
                let seq = u64::from_le_bytes(frame[1..9].try_into().unwrap());
                let _ = self.manager.ack(&id, seq);
            }
            true
        }
    }

    let lines: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(200_000);
    let manager = Arc::new(TerminalManager::default());
    let sink = Arc::new(Sink { manager: manager.clone(), id: Mutex::new(None), bytes: Mutex::new((0, vec![])) });
    let req = SpawnRequest {
        program: detect_shell(&SystemShellEnv, ShellType::Powershell).program.unwrap(),
        env: vec![],
        args: vec!["-NoLogo".into(), "-NoProfile".into()],
        cwd: std::env::temp_dir(),
        size: TerminalSize { cols: 120, rows: 30 },
        config_id: None,
    };
    let id = manager.spawn(&req, sink.clone()).unwrap();
    *sink.id.lock().unwrap() = Some(id.clone());
    std::thread::sleep(Duration::from_secs(2));

    let cmd = format!("1..{lines} | ForEach-Object {{ $_ }}; 'probe-finished'\r");
    let started = Instant::now();
    manager.write(&id, cmd.into_bytes()).unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let b = sink.bytes.lock().unwrap();
        if String::from_utf8_lossy(&b.1).contains("\r\nprobe-finished") || started.elapsed() > Duration::from_secs(600) {
            println!("{lines} 行：{:?}，收到 {} 字节", started.elapsed(), b.0);
            break;
        }
    }
    manager.remove(&id).unwrap();
}

#[cfg(not(windows))]
fn main() {}
