//! 单个终端会话：一个读取线程、一个发送线程、一个写入线程、一个等待退出线程。
//!
//! ```text
//! PTY Reader 线程 -> FlowState 环形缓冲 -> 发送线程（按 ack 窗口）-> FrameSink
//! ```
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{Child, MasterPty};
use serde::Serialize;

use crate::backend::{spawn_pty, SpawnRequest, TerminalSize};
use crate::cursor::CursorVisibility;
use crate::error::{TerminalError, TerminalResult};
use crate::output::{encode_frame, FlowConfig, FlowState, PushOutcome, FRAME_ERROR, FRAME_STATUS};

/// 输出帧的接收方，例如 Tauri Channel。发送失败返回 `false`。
pub trait FrameSink: Send + Sync {
    fn send(&self, frame: Vec<u8>) -> bool;
}

pub type SharedSink = Arc<dyn FrameSink>;

/// 会话状态，作为 `terminal_status` 帧的 JSON 负载下发。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TerminalStatus {
    Starting,
    Running,
    Exited { exit_code: Option<u32> },
    Failed { message: String },
}

/// 运行态查询结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionInfo {
    pub terminal_id: String,
    pub terminal_config_id: Option<String>,
    pub status: TerminalStatus,
    pub output: OutputHealth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputHealth {
    pub paused: bool,
    pub dropped_bytes: u64,
    pub subscribed: bool,
    pub inflight_bytes: usize,
    pub buffered_bytes: usize,
    pub ack_idle_ms: Option<u64>,
}

const WAIT_SLICE: Duration = Duration::from_millis(200);
const READ_CHUNK: usize = 64 * 1024;
/// 合并细碎输出，避免逐字节读取造成 WebView IPC 洪峰；交互最多增加 8 ms 延迟。
const OUTPUT_BATCH_DELAY: Duration = Duration::from_millis(8);
const MIN_OUTPUT_BATCH: usize = 16 * 1024;
/// 发送线程一次最多取出的帧数，避免长时间持锁。
const MAX_FRAMES_PER_ROUND: usize = 8;

struct State {
    flow: FlowState,
    cursor: CursorVisibility,
    sink: Option<SharedSink>,
    status: TerminalStatus,
    pending_status: VecDeque<TerminalStatus>,
    pending_errors: VecDeque<String>,
    reader_done: bool,
    /// 进程已退出且会话 Job 已关闭。
    exited: bool,
    exit_code: Option<u32>,
    exit_frame_sent: bool,
    disposed: bool,
}

struct Shared {
    terminal_id: String,
    state: Mutex<State>,
    cv: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

type MasterSlot = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

pub struct TerminalSession {
    id: String,
    config_id: Option<String>,
    pid: Option<u32>,
    shared: Arc<Shared>,
    master: MasterSlot,
    writer_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    #[cfg(windows)]
    job: Arc<Mutex<Option<orbyte_platform::windows::job::JobObject>>>,
}

impl TerminalSession {
    /// 启动 PTY 子进程并建立首个订阅，Shell 启动横幅不会因订阅晚到而丢失。
    pub fn spawn(
        id: String,
        req: &SpawnRequest,
        sink: SharedSink,
        flow: FlowConfig,
    ) -> TerminalResult<Arc<Self>> {
        let pty = spawn_pty(req)?;
        let now = Instant::now();
        let mut flow = FlowState::new(flow, now);
        flow.subscribe(now);

        let shared = Arc::new(Shared {
            terminal_id: id.clone(),
            state: Mutex::new(State {
                flow,
                cursor: CursorVisibility::default(),
                sink: Some(sink),
                status: TerminalStatus::Running,
                pending_status: VecDeque::from([TerminalStatus::Running]),
                pending_errors: VecDeque::new(),
                reader_done: false,
                exited: false,
                exit_code: None,
                exit_frame_sent: false,
                disposed: false,
            }),
            cv: Condvar::new(),
        });

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let session = Arc::new(Self {
            id,
            config_id: req.config_id.clone(),
            pid: pty.pid,
            shared: shared.clone(),
            master: Arc::new(Mutex::new(Some(pty.master))),
            writer_tx: Arc::new(Mutex::new(Some(tx))),
            #[cfg(windows)]
            job: Arc::new(Mutex::new(Some(pty.job))),
        });

        let tag = &session.id[..8.min(session.id.len())];
        spawn_named(format!("pty-read-{tag}"), {
            let shared = shared.clone();
            move || reader_loop(pty.reader, shared)
        });
        spawn_named(format!("pty-send-{tag}"), {
            let shared = shared.clone();
            move || sender_loop(shared)
        });
        spawn_named(format!("pty-write-{tag}"), move || writer_loop(pty.writer, rx));
        spawn_named(format!("pty-wait-{tag}"), {
            let session = session.clone();
            move || session.wait_exit(pty.child)
        });

        Ok(session)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn config_id(&self) -> Option<&str> {
        self.config_id.as_deref()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn info(&self) -> TerminalSessionInfo {
        let st = self.shared.lock();
        TerminalSessionInfo {
            terminal_id: self.id.clone(),
            terminal_config_id: self.config_id.clone(),
            status: st.status.clone(),
            output: OutputHealth {
                paused: st.flow.is_paused(),
                dropped_bytes: st.flow.dropped_bytes(),
                subscribed: st.flow.is_subscribed(),
                inflight_bytes: st.flow.inflight_bytes(),
                buffered_bytes: st.flow.buffered_len(),
                ack_idle_ms: st.flow.ack_idle_ms(Instant::now()),
            },
        }
    }

    pub fn is_exited(&self) -> bool {
        self.shared.lock().exited
    }

    /// 按收到的顺序写入 PTY。
    pub fn write(&self, data: Vec<u8>) -> TerminalResult<()> {
        let guard = self.writer_tx.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(tx) => tx.send(data).map_err(|_| TerminalError::NotRunning),
            None => Err(TerminalError::NotRunning),
        }
    }

    pub fn resize(&self, size: TerminalSize) -> TerminalResult<()> {
        let size = size.validate()?;
        let guard = self.master.lock().unwrap_or_else(|e| e.into_inner());
        let master = guard.as_ref().ok_or(TerminalError::NotRunning)?;
        master.resize(size.to_pty()).map_err(|e| TerminalError::Io(e.to_string()))
    }

    /// Nudge one column off and restore the original size so the program redraws. Hold the same lock until restore finishes, so the size cannot stay one column off.
    pub fn redraw(&self, size: TerminalSize) -> TerminalResult<()> {
        let size = size.validate()?;
        let nudged = TerminalSize {
            cols: if size.cols == u16::MAX { size.cols - 1 } else { size.cols + 1 },
            rows: size.rows,
        };
        let guard = self.master.lock().unwrap_or_else(|e| e.into_inner());
        let master = guard.as_ref().ok_or(TerminalError::NotRunning)?;
        master.resize(nudged.to_pty()).map_err(|e| TerminalError::Io(e.to_string()))?;
        if let Err(error) = master.resize(size.to_pty()) {
            let _ = master.resize(size.to_pty());
            return Err(TerminalError::Io(error.to_string()));
        }
        Ok(())
    }

    pub fn ack(&self, seq: u64) {
        self.shared.lock().flow.ack(seq, Instant::now());
        self.shared.cv.notify_all();
    }

    pub fn pause_output(&self) {
        self.shared.lock().flow.pause();
        self.shared.cv.notify_all();
        log::info!(target: "orbyte_terminal", "output_paused session={}", self.id);
    }

    /// 替换订阅：丢弃未发送的缓冲，只推后续输出；已退出的会话会再收到一次退出状态。
    pub fn subscribe(&self, sink: SharedSink) {
        let (buffered, inflight) = {
            let mut st = self.shared.lock();
            let buffered = st.flow.buffered_len();
            let inflight = st.flow.inflight_bytes();
            st.sink = Some(sink);
            st.flow.subscribe(Instant::now());
            // ConPTY 不会重复发送未改变的显隐模式，新建的 xterm 必须先恢复此状态。
            let cursor = st.cursor.restore_sequence();
            st.flow.push(cursor, Instant::now());
            st.pending_status.clear();
            if st.exited {
                st.exit_frame_sent = false;
            } else {
                let status = st.status.clone();
                st.pending_status.push_back(status);
            }
            (buffered, inflight)
        };
        log::info!(target: "orbyte_terminal", "subscribe session={} buffered={} inflight={}", self.id, buffered, inflight);
        self.shared.cv.notify_all();
    }

    /// 推送一条可展示的错误。
    pub fn push_error(&self, message: impl Into<String>) {
        self.shared.lock().pending_errors.push_back(message.into());
        self.shared.cv.notify_all();
    }

    /// 结束会话：关闭伪终端（客户端收到 `CTRL_CLOSE_EVENT`），等待退出，超时后结束整个会话 Job。
    ///
    /// 阻塞调用，返回退出码。
    pub fn stop(&self, grace: Duration) -> Option<u32> {
        log::info!(target: "orbyte_terminal", "stop session={}", self.id);
        {
            let mut st = self.shared.lock();
            if st.exited {
                return st.exit_code;
            }
            // 关闭期间读取线程不再等待前端，持续读空管道，避免 ClosePseudoConsole 卡住。
            st.flow.set_closing();
        }
        self.shared.cv.notify_all();

        let master = self.master.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(master) = master {
            // Windows 10 上 ClosePseudoConsole 可能阻塞到输出读空，放到独立线程。
            spawn_named(format!("pty-close-{}", &self.id[..8.min(self.id.len())]), move || {
                drop(master)
            });
        }

        if !self.wait_exited(grace) {
            #[cfg(windows)]
            if let Some(job) = self.job.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                if let Err(error) = job.terminate(1) {
                    log::warn!(target: "orbyte_terminal", "job_terminate_failed session={} error={error}", self.id);
                }
            }
            self.wait_exited(Duration::from_secs(2));
        }
        self.shared.lock().exit_code
    }

    /// 标记会话已移除，发送线程随之退出。调用前应确保进程已结束。
    pub fn dispose(&self) {
        self.shared.lock().disposed = true;
        self.shared.cv.notify_all();
    }

    /// 查询关闭风险时排除 Shell 自身和控制台宿主，保留 Node/脚本等包装进程。
    pub fn has_running_tasks(&self) -> bool {
        #[cfg(windows)]
        {
            use orbyte_platform::windows::process::process_image;
            let job = self.job.lock().unwrap_or_else(|e| e.into_inner());
            let Some(job) = job.as_ref() else { return false; };
            let Ok(pids) = job.process_ids() else { return true; };
            pids.into_iter().any(|pid| {
                // 查询失败时保留确认，避免权限或退出竞态绕过保护。
                let Ok(image) = process_image(pid) else { return true; };
                let name = image.file_name().unwrap_or_default().to_string_lossy();
                // Windows Job 看不到 Linux 后代；包括主进程在内的 WSL 会话保留确认。
                name.eq_ignore_ascii_case("wsl.exe")
                    || (Some(pid) != self.pid
                        && !name.eq_ignore_ascii_case("conhost.exe")
                        && !name.eq_ignore_ascii_case("OpenConsole.exe"))
            })
        }
        #[cfg(not(windows))]
        { !self.is_exited() }
    }

    /// 会话 Job 中仍存活的进程 ID，用于诊断和测试。
    #[cfg(windows)]
    pub fn job_process_ids(&self) -> Vec<u32> {
        self.job
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|j| j.process_ids().ok())
            .unwrap_or_default()
    }

    fn wait_exited(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut st = self.shared.lock();
        while !st.exited {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            st = self.shared.cv.wait_timeout(st, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
        true
    }

    fn wait_exit(&self, mut child: Box<dyn Child + Send + Sync>) {
        let exit_code = child.wait().ok().map(|s| s.exit_code());

        // 先关闭会话 Job，结束 shell 退出后残留的子进程，再关闭伪终端。
        #[cfg(windows)]
        drop(self.job.lock().unwrap_or_else(|e| e.into_inner()).take());
        self.writer_tx.lock().unwrap_or_else(|e| e.into_inner()).take();

        {
            let mut st = self.shared.lock();
            st.exited = true;
            st.exit_code = exit_code;
            st.status = TerminalStatus::Exited { exit_code };
        }
        log::info!(target: "orbyte_terminal", "exit session={} code={:?}", self.id, exit_code);
        self.shared.cv.notify_all();

        // 进程退出后 ConPTY 不会主动关闭输出管道，需要关闭伪终端让读取线程收到 EOF。
        let master = self.master.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(master);
    }
}

fn spawn_named(name: String, f: impl FnOnce() + Send + 'static) {
    thread::Builder::new()
        .name(name)
        .spawn(f)
        .expect("failed to spawn terminal thread");
}

fn reader_loop(mut reader: Box<dyn Read + Send>, shared: Arc<Shared>) {
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Err(error) => {
                log::warn!(target: "orbyte_terminal", "reader_failed session={} kind={:?}", shared.terminal_id, error.kind());
                break;
            }
            Ok(n) => n,
        };
        let mut st = shared.lock();
        while st.flow.push(&buf[..n], Instant::now()) == PushOutcome::MustWait {
            if st.disposed {
                break;
            }
            st = shared.cv.wait_timeout(st, WAIT_SLICE).unwrap_or_else(|e| e.into_inner()).0;
        }
        st.cursor.observe(&buf[..n]);
        drop(st);
        shared.cv.notify_all();
    }
    shared.lock().reader_done = true;
    shared.cv.notify_all();
}

fn sender_loop(shared: Arc<Shared>) {
    let mut stalled_reported = false;
    loop {
        let (sink, frames) = {
            let mut st = shared.lock();
            loop {
                if st.disposed {
                    return;
                }
                let deadline = Instant::now() + OUTPUT_BATCH_DELAY;
                while !st.disposed && !st.reader_done && !st.exited
                    && st.pending_status.is_empty() && st.pending_errors.is_empty()
                    && st.flow.is_subscribed() && st.flow.buffered_len() > 0
                    && st.flow.buffered_len() < MIN_OUTPUT_BATCH.min(st.flow.config().max_batch)
                {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() { break; }
                    st = shared.cv.wait_timeout(st, remaining).unwrap_or_else(|e| e.into_inner()).0;
                }
                if st.disposed { return; }
                let frames = collect_frames(&mut st);
                if !frames.is_empty() {
                    break (st.sink.clone().expect("frames require a sink"), frames);
                }
                let stale = st.flow.is_stale(Instant::now());
                if stale && !stalled_reported {
                    let buffered = st.flow.buffered_len();
                    let inflight = st.flow.inflight_bytes();
                    let idle_ms = st.flow.ack_idle_ms(Instant::now());
                    stalled_reported = true;
                    drop(st);
                    log::warn!(target: "orbyte_terminal", "ack_stalled session={} buffered={buffered} inflight={inflight} idle_ms={idle_ms:?}", shared.terminal_id);
                    st = shared.lock();
                    continue;
                }
                stalled_reported = stale;
                st = shared.cv.wait_timeout(st, WAIT_SLICE).unwrap_or_else(|e| e.into_inner()).0;
            }
        };

        for frame in frames {
            if !sink.send(frame) {
                log::warn!(target: "orbyte_terminal", "channel_send_failed session={}", shared.terminal_id);
                let mut st = shared.lock();
                if st.sink.as_ref().is_some_and(|s| Arc::ptr_eq(s, &sink)) {
                    st.sink = None;
                    st.flow.unsubscribe();
                }
                break;
            }
        }
        // 缓冲腾出空间，唤醒可能在等待的读取线程。
        shared.cv.notify_all();
    }
}

fn collect_frames(st: &mut State) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    if st.sink.is_none() || !st.flow.is_subscribed() {
        return frames;
    }
    while let Some(status) = st.pending_status.pop_front() {
        frames.push(status_frame(st, &status));
    }
    while let Some(message) = st.pending_errors.pop_front() {
        let payload = serde_json::json!({ "message": message }).to_string();
        frames.push(encode_frame(FRAME_ERROR, st.flow.alloc_seq(), payload.as_bytes()));
    }
    let now = Instant::now();
    while frames.len() < MAX_FRAMES_PER_ROUND {
        match st.flow.take_frame(now) {
            Some(f) => frames.push(encode_frame(f.kind, f.seq, &f.payload)),
            None => break,
        }
    }
    // 输出全部发出后才下发退出状态。
    if frames.is_empty()
        && st.exited
        && st.reader_done
        && st.flow.buffered_len() == 0
        && !st.exit_frame_sent
    {
        st.exit_frame_sent = true;
        let status = st.status.clone();
        frames.push(status_frame(st, &status));
    }
    frames
}

fn status_frame(st: &mut State, status: &TerminalStatus) -> Vec<u8> {
    let payload = serde_json::to_vec(status).unwrap_or_default();
    encode_frame(FRAME_STATUS, st.flow.alloc_seq(), &payload)
}

fn writer_loop(mut writer: Box<dyn Write + Send>, rx: mpsc::Receiver<Vec<u8>>) {
    while let Ok(data) = rx.recv() {
        #[cfg(windows)]
        let data = crate::input::encode_input(&data);
        if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
            break;
        }
    }
}
