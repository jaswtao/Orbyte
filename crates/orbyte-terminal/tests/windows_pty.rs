//! Windows ConPTY 集成测试：输出读回、进程树清理、背压下的大量输出。
//!
//! Author: jaswtao
//! Date: 2026-10-09
#![cfg(windows)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orbyte_platform::shell::{detect_shell, ShellType, SystemShellEnv};
use orbyte_platform::windows::process::is_process_alive;
use orbyte_terminal::output::{FRAME_GAP, FRAME_HEADER_LEN, FRAME_OUTPUT, FRAME_STATUS};
use orbyte_terminal::{FlowConfig, FrameSink, SpawnRequest, TerminalManager, TerminalSize};

#[derive(Default)]
struct Collector {
    frames: Mutex<Vec<Vec<u8>>>,
}

impl FrameSink for Collector {
    fn send(&self, frame: Vec<u8>) -> bool {
        self.frames.lock().unwrap().push(frame);
        true
    }
}

/// 模拟前端：消费帧、累计输出并 ack。
#[derive(Default)]
struct Consumer {
    output: Vec<u8>,
    output_frames: usize,
    statuses: Vec<String>,
    gaps: usize,
    last_seq: Option<u64>,
}

impl Consumer {
    fn drain(&mut self, collector: &Collector, manager: &TerminalManager, id: &str) {
        let frames: Vec<_> = std::mem::take(&mut *collector.frames.lock().unwrap());
        for frame in frames {
            let seq = u64::from_le_bytes(frame[1..9].try_into().unwrap());
            let payload = &frame[FRAME_HEADER_LEN..];
            match frame[0] {
                FRAME_OUTPUT => {
                    self.output_frames += 1;
                    // ConPTY 启动时查询光标位置并等待应答，xterm.js 会自动应答。
                    if payload.windows(4).any(|w| w == b"\x1b[6n") {
                        let _ = manager.write(id, b"\x1b[1;1R".to_vec());
                    }
                    self.output.extend_from_slice(payload);
                    self.last_seq = Some(seq);
                }
                FRAME_STATUS => self
                    .statuses
                    .push(String::from_utf8_lossy(payload).into_owned()),
                FRAME_GAP => self.gaps += 1,
                _ => {}
            }
        }
        if let Some(seq) = self.last_seq {
            let _ = manager.ack(id, seq);
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    fn wait_for(
        &mut self,
        collector: &Collector,
        manager: &TerminalManager,
        id: &str,
        timeout: Duration,
        mut done: impl FnMut(&Consumer) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.drain(collector, manager, id);
            if done(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

fn cmd_request() -> SpawnRequest {
    let info = detect_shell(&SystemShellEnv, ShellType::Cmd);
    SpawnRequest {
        program: info.program.expect("cmd.exe should exist"),
        env: vec![],
        args: vec![],
        cwd: std::env::temp_dir(),
        size: TerminalSize {
            cols: 120,
            rows: 30,
        },
        config_id: None,
    }
}

#[test]
fn exit_status_is_queryable_when_output_window_is_full_and_channel_does_not_ack() {
    let manager = TerminalManager::new(FlowConfig { window: 16, max_batch: 16, ..FlowConfig::default() });
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&cmd_request(), collector).unwrap();
    let session = manager.get(&id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while session.info().output.inflight_bytes == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let info = session.info();
    assert!(info.output.inflight_bytes > 0);
    assert!(info.output.ack_idle_ms.is_some());
    manager.stop(&id).unwrap();
    assert!(session.is_exited());
    assert!(matches!(session.info().status, orbyte_terminal::TerminalStatus::Exited { .. }));
    manager.remove(&id).unwrap();
    assert!(manager.list().is_empty());
}

#[test]
fn close_risk_tracks_live_child_tasks_and_clears_after_exit() {
    let manager = TerminalManager::default();
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&cmd_request(), collector.clone()).unwrap();
    let mut consumer = Consumer::default();
    manager.write(&id, b"echo IDLE_READY\r".to_vec()).unwrap();
    assert!(consumer.wait_for(&collector, &manager, &id, Duration::from_secs(10), |c| c.text().contains("IDLE_READY")));
    let session = manager.get(&id).unwrap();
    assert!(!session.has_running_tasks(), "空闲 Shell 和控制台宿主不应要求确认");
    manager.write(&id, b"ping -n 4 127.0.0.1 >nul\r".to_vec()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.has_running_tasks() && Instant::now() < deadline {
        consumer.drain(&collector, &manager, &id);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(session.has_running_tasks(), "子任务运行时必须确认");
    let deadline = Instant::now() + Duration::from_secs(8);
    while session.has_running_tasks() && Instant::now() < deadline {
        consumer.drain(&collector, &manager, &id);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!session.has_running_tasks(), "子任务退出后恢复空闲状态");
    manager.remove(&id).unwrap();
    assert!(!session.has_running_tasks());
}

#[test]
fn close_risk_keeps_confirmation_for_wsl_until_session_exits() {
    // 用同名 CMD 替身验证 WSL 进程识别，无需安装发行版或访问 Linux 服务。
    let directory = std::env::temp_dir().join(format!("orbyte-wsl-risk-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let mut request = cmd_request();
    let program = directory.join("wsl.exe");
    std::fs::copy(&request.program, &program).unwrap();
    request.program = program.clone();
    let manager = TerminalManager::default();
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&request, collector.clone()).unwrap();
    let mut consumer = Consumer::default();
    manager.write(&id, b"echo WSL_READY\r".to_vec()).unwrap();
    assert!(consumer.wait_for(&collector, &manager, &id, Duration::from_secs(10), |c| c.text().contains("WSL_READY")));
    let session = manager.get(&id).unwrap();
    let requires_confirmation = session.has_running_tasks();
    manager.remove(&id).unwrap();
    std::fs::remove_file(program).unwrap();
    std::fs::remove_dir(directory).unwrap();
    assert!(requires_confirmation, "无法检查 WSL 内部任务时必须保留确认");
    assert!(!session.has_running_tasks(), "WSL 会话退出后不再要求确认");
}

/// 在交互式 Windows 桌面运行；会短暂显示测试宿主和真实比较窗口。
#[test]
#[ignore = "需要交互式桌面及 ORBYTE_TEST_DIFFTOOL 指定的 TortoiseGitMerge.exe"]
fn consecutive_git_difftool_windows_receive_foreground() {
    use std::process::Command;
    use windows_sys::core::w;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use orbyte_platform::windows::activation::WindowActivation;

    struct DesktopFixture {
        main: windows_sys::Win32::Foundation::HWND,
        manager: Arc<TerminalManager>,
        directory: std::path::PathBuf,
    }
    impl Drop for DesktopFixture {
        fn drop(&mut self) {
            self.manager.stop_all();
            unsafe { DestroyWindow(self.main); }
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
    unsafe extern "system" fn collect(hwnd: windows_sys::Win32::Foundation::HWND, data: isize) -> i32 {
        if unsafe { IsWindowVisible(hwnd) } != 0 {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid); }
            unsafe { &mut *(data as *mut Vec<(usize, u32)>) }.push((hwnd as usize, pid));
        }
        1
    }
    fn pump() {
        let mut message = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe { TranslateMessage(&message); DispatchMessageW(&message); }
        }
    }
    let tool = std::env::var("ORBYTE_TEST_DIFFTOOL").expect("请指定 ORBYTE_TEST_DIFFTOOL");
    assert!(std::path::Path::new(&tool).is_file());
    let directory = std::env::temp_dir().join(format!("orbyte-activation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let manager = Arc::new(TerminalManager::default());
    let main = unsafe {
        CreateWindowExW(0, w!("STATIC"), w!("Orbyte 窗口激活回归验证"), WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100, 100, 640, 400, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null())
    };
    let fixture = DesktopFixture { main, manager: manager.clone(), directory };
    assert!(!main.is_null(), "无法创建测试窗口");
    unsafe { SetForegroundWindow(main); }
    assert_eq!(unsafe { GetForegroundWindow() }, main, "当前桌面不允许测试宿主进入前台");
    let git = |args: &[&str]| {
        let output = Command::new("git").current_dir(&fixture.directory).args(args).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    };
    git(&["init", "--quiet"]);
    git(&["config", "alias.dt", "difftool"]);
    git(&["config", "diff.tool", "orbyte-test"]);
    git(&["config", "difftool.prompt", "false"]);
    git(&["config", "difftool.orbyte-test.cmd", &format!("\"{}\" -base:\"$LOCAL\" -mine:\"$REMOTE\"", tool.replace('\\', "/"))]);
    for file in ["a.txt", "b.txt", "c.txt"] { std::fs::write(fixture.directory.join(file), "before\n").unwrap(); }
    git(&["add", "."]);
    for file in ["a.txt", "b.txt", "c.txt"] { std::fs::write(fixture.directory.join(file), "after\n").unwrap(); }

    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&SpawnRequest { cwd: fixture.directory.clone(), ..cmd_request() }, collector.clone()).unwrap();
    manager.set_active_terminal(Some(id.clone())).unwrap();
    let resolver = manager.clone();
    // 设为 1 可对照未启用协调服务时的原始行为。
    let _activation = if std::env::var_os("ORBYTE_ACTIVATION_BASELINE").is_some() { None }
        else { Some(WindowActivation::start(main as usize, move |pid| resolver.is_active_process(pid)).unwrap()) };
    let mut consumer = Consumer::default();
    let mut previous = 0;
    for round in 0..3 {
        manager.write(&id, format!("git dt & echo ACTIVATION^_DONE_{round}\r").into_bytes()).unwrap();
        for index in 0..3 {
            let deadline = Instant::now() + Duration::from_secs(15);
            let target = loop {
                pump();
                consumer.drain(&collector, &manager, &id);
                let mut windows = Vec::<(usize, u32)>::new();
                unsafe { EnumWindows(Some(collect), &mut windows as *mut _ as isize); }
                if let Some((hwnd, pid)) = windows.into_iter().find(|(hwnd, pid)| *hwnd != previous && manager.is_active_process(*pid)) {
                    eprintln!("round={round} file={index} hwnd={hwnd} pid={pid}");
                    break hwnd as windows_sys::Win32::Foundation::HWND;
                }
                assert!(Instant::now() < deadline, "未出现比较窗口：{}", consumer.text());
                std::thread::sleep(Duration::from_millis(10));
            };
            let deadline = Instant::now() + Duration::from_millis(1500);
            while unsafe { GetForegroundWindow() } != target && Instant::now() < deadline {
                pump();
                consumer.drain(&collector, &manager, &id);
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(unsafe { GetForegroundWindow() }, target, "第 {} 轮第 {} 个比较窗口未获得焦点", round + 1, index + 1);
            previous = target as usize;
            // 实际键盘输入会改变 Windows 的前台激活资格，不能用 WM_CLOSE 替代。
            let mut keys: [INPUT; 2] = unsafe { std::mem::zeroed() };
            for (index, key) in keys.iter_mut().enumerate() {
                key.r#type = INPUT_KEYBOARD;
                key.Anonymous.ki.wVk = VK_ESCAPE;
                key.Anonymous.ki.dwFlags = if index == 0 { 0 } else { KEYEVENTF_KEYUP };
            }
            assert_eq!(unsafe { SendInput(2, keys.as_ptr(), std::mem::size_of::<INPUT>() as i32) }, 2);
            let deadline = Instant::now() + Duration::from_secs(5);
            while unsafe { IsWindow(target) } != 0 && Instant::now() < deadline {
                pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(unsafe { IsWindow(target) }, 0);
        }
        assert!(consumer.wait_for(&collector, &manager, &id, Duration::from_secs(5), |c| c.text().contains(&format!("ACTIVATION_DONE_{round}"))), "{}", consumer.text());
    }
}

// 使用与 TUI 相同的 ReadConsoleInputW，不能用会兼容 Alt 数字输入的 Console.ReadKey。
fn console_input_probe(mode: u32, count: usize, bytes: bool) -> SpawnRequest {
    let source = r#"Add-Type -TypeDefinition @'
// Author: jaswtao · Date: 2026-10-15
using System;
using System.Runtime.InteropServices;
public class InputProbe {
    [StructLayout(LayoutKind.Explicit, Size=20)] public struct Record {
        [FieldOffset(0)] public ushort Type;
        [FieldOffset(4)] public int Down;
        [FieldOffset(10)] public ushort Vk;
        [FieldOffset(14)] public ushort Char;
        [FieldOffset(16)] public uint State;
    }
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int n);
    [DllImport("kernel32.dll")] static extern bool SetConsoleMode(IntPtr h, uint mode);
    [DllImport("kernel32.dll")] static extern bool SetConsoleCP(uint cp);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] static extern bool ReadConsoleW(IntPtr h, [Out] char[] buffer, uint count, out uint read, IntPtr control);
    [DllImport("kernel32.dll")] static extern bool ReadConsoleInputW(IntPtr h, out Record r, uint count, out uint read);
    public static void Run(uint mode, int count, bool bytes) {
        var h = GetStdHandle(-10);
        if(!SetConsoleMode(h, mode) || !SetConsoleCP(65001)) throw new Exception("console setup failed");
        Console.WriteLine("probe-ready");
        if(bytes) {
            // Older console hosts cannot reliably read UTF-8 through ReadFile.
            var buffer = new char[256];
            int received = 0;
            while(received < count) {
                uint read;
                if(!ReadConsoleW(h, buffer, (uint)buffer.Length, out read, IntPtr.Zero) || read == 0)
                    throw new Exception("console read failed");
                var data = System.Text.Encoding.UTF8.GetBytes(buffer, 0, (int)read);
                foreach(var value in data) Console.WriteLine("byte:{0:X2}", value);
                received += data.Length;
            }
            return;
        }
        int n = 0;
        while(n < count) {
            Record r; uint read;
            if(!ReadConsoleInputW(h, out r, 1, out read)) throw new Exception("read failed");
            if(r.Type != 1 || r.Down == 0 || r.Char == 0) continue;
            Console.WriteLine("key:{0:X4}:{1}:vk={2}", r.Char, r.State, r.Vk); n++;
        }
    }
}
'@
"#;
    SpawnRequest {
        program: detect_shell(&SystemShellEnv, ShellType::Powershell).program.unwrap(),
        args: vec!["-NoLogo".into(), "-NoProfile".into(), "-Command".into(),
            format!("{source}[InputProbe]::Run({mode}, {count}, ${bytes})")],
        ..cmd_request()
    }
}

fn probe_console_input(request: SpawnRequest, chunks: &[&str]) -> String {
    let manager = TerminalManager::default();
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&request, collector.clone()).unwrap();
    let mut consumer = Consumer::default();
    assert!(consumer.wait_for(&collector, &manager, &id, Duration::from_secs(15), |c| c.text().contains("probe-ready")), "{}", consumer.text());
    for chunk in chunks {
        manager.write(&id, chunk.as_bytes().to_vec()).unwrap();
    }
    let exited = consumer.wait_for(&collector, &manager, &id, Duration::from_secs(15), |c| c.statuses.iter().any(|s| s.contains("\"exited\"")));
    manager.remove(&id).unwrap();
    assert!(exited, "{}", consumer.text());
    consumer.text()
}

#[test]
fn unicode_punctuation_reaches_raw_console_without_modifiers() {
    for chunks in [vec!["“”‘’中文\"'\r"], vec!["“", "”", "‘", "’", "中文\"'\r"]] {
        let expected = chunks.concat();
        let text = probe_console_input(console_input_probe(0x98, expected.chars().count(), false), &chunks);
        let actual: Vec<_> = text.lines().filter_map(|line| line.find("key:").map(|index| &line[index..])).collect();
        let expected: Vec<_> = expected.chars().map(|ch| {
            let modifiers = if matches!(ch, '‘' | '’' | '“' | '”') { "0:" } else { "" };
            format!("key:{:04X}:{modifiers}", ch as u32)
        }).collect();
        assert_eq!(actual.len(), expected.len(), "{text}");
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(actual.starts_with(&expected), "{text}");
        }
    }
}

#[test]
fn native_clipboard_shortcut_reaches_raw_console_as_ctrl_v() {
    let text = probe_console_input(console_input_probe(0x98, 1, false), &["\x16"]);
    let key = text.lines().find_map(|line| line.find("key:0016:").map(|index| &line[index..]))
        .unwrap_or_else(|| panic!("Ctrl+V was not received: {text}"));
    let state: u32 = key.split(':').nth(2).unwrap().parse().unwrap();
    // LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED, without Alt modifiers.
    assert_ne!(state & 0x0c, 0, "{text}");
    assert_eq!(state & 0x03, 0, "{text}");
}

#[test]
fn stalled_output_keeps_ipc_bounded_and_accepts_escape_and_ctrl_c() {
    let source = r#"Add-Type -TypeDefinition @'
// Author: jaswtao · Date: 2026-10-31
using System;
using System.Threading;
public class FloodInputProbe {
    static volatile bool stopped;
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Explicit, Size=20)]
    public struct Record {
        [System.Runtime.InteropServices.FieldOffset(0)] public ushort Type;
        [System.Runtime.InteropServices.FieldOffset(4)] public int Down;
        [System.Runtime.InteropServices.FieldOffset(14)] public ushort Char;
    }
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int n);
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern bool SetConsoleMode(IntPtr h, uint mode);
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern bool ReadConsoleInputW(IntPtr h, out Record r, uint count, out uint read);
    static ushort ReadKey(IntPtr input) {
        while (true) {
            Record record; uint read;
            if (!ReadConsoleInputW(input, out record, 1, out read)) throw new Exception("console read failed");
            if (record.Type == 1 && record.Down != 0 && record.Char != 0) return record.Char;
        }
    }
    public static void Run() {
        var input = GetStdHandle(-10);
        if (!SetConsoleMode(input, 0x98)) throw new Exception("console setup failed");
        Console.WriteLine("flood-ready");
        var writer = new Thread(() => {
            var text = new string('x', 1024);
            while (!stopped) { Console.Write(text); Thread.Sleep(1); }
        });
        writer.IsBackground = true;
        writer.Start();
        var first = ReadKey(input);
        var second = ReadKey(input);
        stopped = true;
        writer.Join();
        Console.WriteLine("\r\ninterrupts-received:{0:X4}:{1:X4}", (int)first, (int)second);
    }
}
'@
[FloodInputProbe]::Run()
"#;
    let request = SpawnRequest {
        program: detect_shell(&SystemShellEnv, ShellType::Powershell).program.unwrap(),
        args: vec!["-NoLogo".into(), "-NoProfile".into(), "-Command".into(), source.into()],
        ..cmd_request()
    };
    let manager = TerminalManager::new(FlowConfig {
        buffer_capacity: 4096,
        window: 256,
        max_batch: 128,
        stale_after: Duration::from_millis(100),
    });
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&request, collector.clone()).unwrap();
    let mut consumer = Consumer::default();
    let ready = consumer.wait_for(&collector, &manager, &id, Duration::from_secs(15), |c| c.text().contains("flood-ready"));
    if !ready { manager.remove(&id).unwrap(); panic!("Probe did not start: {}", consumer.text()); }

    // Stop consuming and acknowledging while the process continues to redraw.
    std::thread::sleep(Duration::from_millis(600));
    let before = collector.frames.lock().unwrap().len();
    std::thread::sleep(Duration::from_millis(200));
    let after = collector.frames.lock().unwrap().len();
    let health = manager.get(&id).unwrap().info().output;
    manager.write(&id, b"\x1b".to_vec()).unwrap();
    // ConPTY interprets adjacent ESC + key bytes as an Alt-modified key.
    std::thread::sleep(Duration::from_millis(500));
    manager.write(&id, b"\x03".to_vec()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !manager.get(&id).unwrap().is_exited() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let exited_without_ack = manager.get(&id).unwrap().is_exited();
    let received = consumer.wait_for(&collector, &manager, &id, Duration::from_secs(10), |c| {
        c.text().contains("interrupts-received:001B:0003")
    });
    manager.remove(&id).unwrap();

    assert_eq!(health.inflight_bytes, 256);
    assert!(before <= 4, "Output window was bypassed: {before} queued frames");
    assert_eq!(after, before, "Stalled output kept sending IPC frames");
    assert!(exited_without_ack, "Interrupts depended on output acknowledgement; received={received}; {}", consumer.text());
    assert!(received, "{}", consumer.text());
}

#[test]
fn unicode_paste_reaches_vt_input_without_protocol_leaking() {
    let input = "\x1b[200~甲“乙”\r‘丙’\"丁\"\x1b[201~";
    let text = probe_console_input(console_input_probe(0x298, input.len(), true), &[input]);
    let actual: Vec<_> = text.lines().filter_map(|line| line.find("byte:").map(|index| &line[index..])).collect();
    let expected: Vec<_> = input.as_bytes().iter().map(|byte| format!("byte:{byte:02X}")).collect();
    assert_eq!(actual, expected, "{text}");
}

#[test]
fn resubscribe_restores_hidden_cursor_before_subsequent_output() {
    let manager = TerminalManager::default();
    let collector = Arc::new(Collector::default());
    let request = SpawnRequest {
        program: detect_shell(&SystemShellEnv, ShellType::Powershell).program.unwrap(),
        args: vec!["-NoLogo".into(), "-NoProfile".into(), "-Command".into(),
            "[Console]::Write(([char]27 + '[?25lhidden-marker')); Start-Sleep -Seconds 15".into()],
        ..cmd_request()
    };
    let id = manager.spawn(&request, collector.clone()).unwrap();
    let mut consumer = Consumer::default();
    let ready = consumer.wait_for(&collector, &manager, &id, Duration::from_secs(10), |c| c.text().contains("hidden-marker"));
    if !ready { manager.stop(&id).unwrap(); panic!("未收到隐藏光标指令：{}", consumer.text()); }
    let restored = Arc::new(Collector::default());
    manager.subscribe(&id, restored.clone()).unwrap();
    let mut consumer = Consumer::default();
    let received = consumer.wait_for(&restored, &manager, &id, Duration::from_secs(5), |c| !c.output.is_empty());
    manager.stop(&id).unwrap();
    assert!(received, "重连后未收到光标状态");
    assert!(consumer.output.starts_with(b"\x1b[?25l"), "恢复指令必须先于后续输出：{}", consumer.text());
}

#[test]
fn integrated_shells_report_directory_and_powershell_exit_code() {
    let directory = std::env::temp_dir().join(format!("orbyte-osc-Tao's-{}", std::process::id()));
    orbyte_platform::shell_integration::install(&directory).unwrap();
    // Shells report expanded paths even when TEMP uses an 8.3 alias.
    let directory = std::path::PathBuf::from(
        orbyte_platform::path::normalize_project_path(directory.to_str().unwrap()).unwrap(),
    );
    let mut candidates: Vec<_> = [ShellType::Powershell, ShellType::Cmd].into_iter().filter_map(|shell| detect_shell(&SystemShellEnv, shell).program.map(|program| (shell, program))).collect();
    if let Some(root) = std::env::var_os("SystemRoot") {
        let program = std::path::PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if program.is_file() && !candidates.iter().any(|(_, found)| found == &program) { candidates.push((ShellType::Powershell, program)); }
    }
    for (shell, program) in candidates {
        let (args, env) = orbyte_platform::shell_integration::launch_spec(shell, &directory, true);
        let request = SpawnRequest { program, args, env, ..cmd_request() };
        let collector = Arc::new(Collector::default());
        let manager = TerminalManager::default();
        let id = manager.spawn(&request, collector.clone()).unwrap();
        let mut consumer = Consumer::default();
        let ready = consumer.wait_for(&collector, &manager, &id, Duration::from_secs(20), |c| c.text().contains("\x1b]133;B"));
        if !ready { manager.stop(&id).unwrap(); panic!("集成提示符未输出：{}", consumer.text()); }
        consumer.output.clear();
        let command = if shell == ShellType::Powershell { format!("Set-Location {}; cmd /c exit 3\r", orbyte_platform::shell_integration::quote_powershell(&directory.to_string_lossy())) } else { format!("cd /d \"{}\"\r", directory.display()) };
        manager.write(&id, command.into_bytes()).unwrap();
        let updated = consumer.wait_for(&collector, &manager, &id, Duration::from_secs(10), |c| c.text().contains(&format!("\x1b]9;9;{}", directory.display())) && (shell == ShellType::Cmd || c.text().contains("\x1b]133;D;3")));
        manager.stop(&id).unwrap();
        assert!(updated, "未收到目录或退出码：{}", consumer.text());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn concurrent_start_is_unique_and_restart_replaces_runtime_id() {
    let manager = Arc::new(TerminalManager::default());
    let collector = Arc::new(Collector::default());
    let mut req = cmd_request();
    req.config_id = Some("persistent-config".into());
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let manager = manager.clone();
            let collector = collector.clone();
            let req = req.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                manager.spawn(&req, collector)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .any(|r| matches!(r, Err(orbyte_terminal::TerminalError::AlreadyRunning))));
    let id = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(manager.list().len(), 1);
    manager.stop(&id).unwrap();
    assert_eq!(
        manager.find_by_config("persistent-config").unwrap().id(),
        id
    );
    let new_id = manager.spawn(&req, collector).unwrap();
    assert_ne!(id, new_id);
    assert!(manager.get(&id).is_err());
    assert_eq!(manager.list().len(), 1);
    assert_eq!(
        manager.list()[0].terminal_config_id.as_deref(),
        Some("persistent-config")
    );
    manager.remove(&new_id).unwrap();
    assert!(manager.list().is_empty());
    manager.begin_shutdown();
    assert!(matches!(
        manager.spawn(&req, Arc::new(Collector::default())),
        Err(orbyte_terminal::TerminalError::ShuttingDown)
    ));
}

#[test]
fn simultaneous_sessions_do_not_mix_output_or_stop_each_other() {
    let manager = TerminalManager::default();
    let left = Arc::new(Collector::default());
    let right = Arc::new(Collector::default());
    let left_id = manager.spawn(&cmd_request(), left.clone()).unwrap();
    let right_id = manager.spawn(&cmd_request(), right.clone()).unwrap();
    let mut a = Consumer::default();
    let mut b = Consumer::default();
    manager
        .write(&left_id, b"echo left-%COMPUTERNAME:~0,0%only\r\n".to_vec())
        .unwrap();
    manager
        .write(
            &right_id,
            b"echo right-%COMPUTERNAME:~0,0%only\r\n".to_vec(),
        )
        .unwrap();
    assert!(
        a.wait_for(&left, &manager, &left_id, Duration::from_secs(15), |c| c
            .text()
            .contains("left-only"))
    );
    assert!(
        b.wait_for(&right, &manager, &right_id, Duration::from_secs(15), |c| c
            .text()
            .contains("right-only"))
    );
    assert!(!a.text().contains("right-only"));
    assert!(!b.text().contains("left-only"));
    manager.remove(&left_id).unwrap();
    assert!(!manager.get(&right_id).unwrap().is_exited());
    manager
        .write(
            &right_id,
            b"echo still-%COMPUTERNAME:~0,0%alive\r\n".to_vec(),
        )
        .unwrap();
    assert!(
        b.wait_for(&right, &manager, &right_id, Duration::from_secs(15), |c| c
            .text()
            .contains("still-alive"))
    );
    manager.remove(&right_id).unwrap();
}

#[test]
fn cmd_echo_roundtrip_and_exit_code() {
    let manager = TerminalManager::default();
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&cmd_request(), collector.clone()).unwrap();
    let mut consumer = Consumer::default();

    // 回显的命令行包含 `%...%`，只有真正的输出是连续的 orbyte-probe。
    manager.write(&id, b"echo orbyte-%COMPUTERNAME:~0,0%probe\r\n".to_vec()).unwrap();
    assert!(
        consumer.wait_for(&collector, &manager, &id, Duration::from_secs(15), |c| c
            .text()
            .contains("orbyte-probe")),
        "output: {}",
        consumer.text()
    );

    manager.write(&id, b"exit 7\r\n".to_vec()).unwrap();
    assert!(
        consumer.wait_for(&collector, &manager, &id, Duration::from_secs(15), |c| c
            .statuses
            .iter()
            .any(|s| s.contains("\"exited\""))),
        "statuses: {:?}",
        consumer.statuses
    );
    assert!(consumer.statuses.first().unwrap().contains("running"));
    assert!(consumer.statuses.last().unwrap().contains("\"exitCode\":7"));
    manager.remove(&id).unwrap();
}

#[test]
fn stop_terminates_whole_process_tree() {
    let manager = TerminalManager::default();
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&cmd_request(), collector.clone()).unwrap();
    let session = manager.get(&id).unwrap();
    let mut consumer = Consumer::default();

    // 嵌套 cmd -> ping，外加一个独立控制台窗口中的进程（收不到伪终端的 CTRL_CLOSE_EVENT）。
    manager
        .write(&id, b"start \"\" /min cmd /c ping -n 120 127.0.0.1 & cmd /c ping -n 120 127.0.0.1\r\n".to_vec())
        .unwrap();
    let mut pids = Vec::new();
    assert!(
        consumer.wait_for(&collector, &manager, &id, Duration::from_secs(15), |_| {
            pids = session.job_process_ids();
            pids.len() >= 5
        }),
        "job processes: {pids:?}"
    );
    assert!(pids.iter().all(|&p| is_process_alive(p)));

    let started = Instant::now();
    manager.stop(&id).unwrap();
    assert!(started.elapsed() < Duration::from_secs(6));
    assert!(session.is_exited());

    let deadline = Instant::now() + Duration::from_secs(3);
    while pids.iter().any(|&p| is_process_alive(p)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let alive: Vec<_> = pids.iter().copied().filter(|&p| is_process_alive(p)).collect();
    assert!(alive.is_empty(), "still alive: {alive:?}");
    manager.remove(&id).unwrap();
}

#[test]
fn heavy_output_under_backpressure_has_no_gap() {
    // 用很小的缓冲和窗口放大背压效果。
    let manager = TerminalManager::new(FlowConfig {
        buffer_capacity: 64 * 1024,
        window: 16 * 1024,
        stale_after: Duration::from_secs(5),
        max_batch: 8 * 1024,
    });
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&cmd_request(), collector.clone()).unwrap();
    let mut consumer = Consumer::default();

    manager.write(&id, b"for /L %i in (1,1,20000) do @echo line-%i\r\n".to_vec()).unwrap();
    assert!(
        consumer.wait_for(&collector, &manager, &id, Duration::from_secs(120), |c| c
            .text()
            .contains("line-20000")),
        "tail: {}",
        consumer.text().chars().rev().take(300).collect::<String>()
    );
    assert_eq!(consumer.gaps, 0);
    manager.remove(&id).unwrap();
}

#[test]
fn paused_display_drains_process_output_without_affecting_another_terminal() {
    let manager = TerminalManager::default();
    let overloaded = Arc::new(Collector::default());
    let healthy = Arc::new(Collector::default());
    let id = manager.spawn(&cmd_request(), overloaded.clone()).unwrap();
    let other = manager.spawn(&cmd_request(), healthy.clone()).unwrap();
    let mut first = Consumer::default();
    let mut second = Consumer::default();
    manager.write(&id, b"echo pause-%COMPUTERNAME:~0,0%ready\r".to_vec()).unwrap();
    assert!(first.wait_for(&overloaded, &manager, &id, Duration::from_secs(10), |c| c.text().contains("pause-ready")));
    let session = manager.get(&id).unwrap();
    session.pause_output();
    manager.write(&id, b"for /L %i in (1,1,10000) do @echo discarded-%i\r".to_vec()).unwrap();
    manager.write(&other, b"echo other-%COMPUTERNAME:~0,0%responsive\r".to_vec()).unwrap();
    let responsive = second.wait_for(&healthy, &manager, &other, Duration::from_secs(2), |c| c.text().contains("other-responsive"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.info().output.dropped_bytes == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let health = session.info().output;
    // Exit remains observable even though no output can be acknowledged.
    manager.write(&id, b"exit 0\r".to_vec()).unwrap();
    let exited = first.wait_for(&overloaded, &manager, &id, Duration::from_secs(10), |c| c.statuses.iter().any(|status| status.contains("\"exited\"")));
    manager.remove(&id).unwrap();
    manager.remove(&other).unwrap();
    assert!(responsive, "Healthy terminal was starved");
    assert!(health.paused);
    assert!(health.dropped_bytes > 0);
    assert_eq!(health.buffered_bytes, 0);
    assert_eq!(health.inflight_bytes, 0);
    assert!(exited, "Paused display hid process exit");
}

#[test]
fn fragmented_output_is_batched_without_losing_tail_or_interactive_reply() {
    let manager = TerminalManager::default();
    let collector = Arc::new(Collector::default());
    let id = manager.spawn(&cmd_request(), collector.clone()).unwrap();
    let mut consumer = Consumer::default();
    manager.write(&id, b"node -e \"for(let i=1;i<=20000;i++)process.stdout.write('fragment-'+i+'\\n')\"\r".to_vec()).unwrap();
    assert!(consumer.wait_for(&collector, &manager, &id, Duration::from_secs(60), |c| c.text().contains("fragment-20000")));
    assert_eq!(consumer.gaps, 0);
    // 回归：细碎读取曾使每行产生多个 IPC 帧。批量后远低于行数。
    assert!(consumer.output_frames < 2000, "{} output frames", consumer.output_frames);
    manager.write(&id, b"echo batched-interaction-ok\r".to_vec()).unwrap();
    assert!(consumer.wait_for(&collector, &manager, &id, Duration::from_secs(10), |c| c.text().contains("\r\nbatched-interaction-ok")));
    manager.remove(&id).unwrap();
}
