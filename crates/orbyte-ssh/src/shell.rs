//! One SSH shell on one operating-system thread, with the shared terminal output protocol.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use orbyte_terminal::output::{encode_frame, FlowConfig, FlowState, PushOutcome, FRAME_ERROR, FRAME_STATUS};
use orbyte_terminal::{SessionRuntime, SharedSink, TerminalResult, TerminalSessionInfo, TerminalSize, TerminalStatus};
use ssh2::{Channel, Session};

use crate::cancel::CancelToken;
use zeroize::Zeroizing;

use crate::connect::{self, Credential};
use crate::hostkey::PresentedKey;
use crate::error::{SshError, SshResult};
use crate::io_wait::{readable_now, wait_session};
use crate::keepalive::{Keepalive, KeepaliveAction};

const INPUT_CAP: usize = 256 * 1024;
const READ_CHUNK: usize = 64 * 1024;

struct OutputState {
    flow: FlowState,
    sink: Option<SharedSink>,
    status: TerminalStatus,
    pending_status: VecDeque<TerminalStatus>,
    pending_errors: VecDeque<String>,
    exited: bool,
    disposed: bool,
}

struct OutputShared {
    state: Mutex<OutputState>,
    cv: Condvar,
}

impl OutputShared {
    fn lock(&self) -> MutexGuard<'_, OutputState> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }
}

pub struct SshTerminal {
    id: String,
    config_id: String,
    shared: Arc<OutputShared>,
    input_tx: Mutex<Option<SyncSender<Vec<u8>>>>,
    resize: Arc<Mutex<Option<(TerminalSize, bool)>>>,
    cancel: Arc<CancelToken>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub struct ShellLaunch {
    pub terminal_id: String,
    pub config_id: String,
    pub host: String,
    pub port: u16,
    pub saved_algorithms: Vec<String>,
    pub username: String,
    pub credential: Credential,
    pub cols: u16,
    pub rows: u16,
    pub sink: SharedSink,
    pub cancel: Arc<CancelToken>,
    /// Runs on the SSH thread after the handshake and before authentication.
    pub prepare_auth: Box<dyn FnOnce(&PresentedKey) -> SshResult<Zeroizing<String>> + Send>,
    pub finish_auth: Box<dyn FnOnce() + Send>,
    pub on_state: Box<dyn Fn(&str, Option<String>) + Send + Sync>,
}

impl SshTerminal {
    pub fn start(launch: ShellLaunch) -> SshResult<Arc<Self>> {
        let now = Instant::now();
        let mut flow = FlowState::new(FlowConfig::default(), now);
        flow.subscribe(now);
        let shared = Arc::new(OutputShared {
            state: Mutex::new(OutputState {
                flow,
                sink: Some(launch.sink.clone()),
                status: TerminalStatus::Starting,
                pending_status: VecDeque::from([TerminalStatus::Starting]),
                pending_errors: VecDeque::new(),
                exited: false,
                disposed: false,
            }),
            cv: Condvar::new(),
        });
        let (input_tx, input_rx) = mpsc::sync_channel(32);
        let resize = Arc::new(Mutex::new(None));
        let terminal = Arc::new(Self {
            id: launch.terminal_id.clone(),
            config_id: launch.config_id.clone(),
            shared: shared.clone(),
            input_tx: Mutex::new(Some(input_tx)),
            resize: resize.clone(),
            cancel: launch.cancel.clone(),
            thread: Mutex::new(None),
        });
        let worker_shared = shared.clone();
        let worker_resize = resize.clone();
        let thread = thread::Builder::new()
            .name(format!("ssh-{}", &launch.terminal_id[..8.min(launch.terminal_id.len())]))
            .spawn(move || worker(launch, worker_shared, input_rx, worker_resize))
            .map_err(|_| SshError::Message("无法启动 SSH 线程".into()))?;
        *terminal.thread.lock().unwrap_or_else(|err| err.into_inner()) = Some(thread);
        thread::Builder::new()
            .name(format!("ssh-out-{}", &terminal.id[..8.min(terminal.id.len())]))
            .spawn({
                let shared = terminal.shared.clone();
                move || sender_loop(shared)
            })
            .map_err(|_| SshError::Message("无法启动 SSH 输出线程".into()))?;
        Ok(terminal)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn cancel_token(&self) -> Arc<CancelToken> {
        self.cancel.clone()
    }
}

impl SessionRuntime for SshTerminal {
    fn write(&self, data: Vec<u8>) -> TerminalResult<()> {
        if self.shared.lock().exited {
            return Err(orbyte_terminal::TerminalError::NotRunning);
        }
        let guard = self.input_tx.lock().unwrap_or_else(|err| err.into_inner());
        let Some(tx) = guard.as_ref() else {
            return Err(orbyte_terminal::TerminalError::NotRunning);
        };
        tx.try_send(data).map_err(|_| orbyte_terminal::TerminalError::Io("输入队列已满".into()))
    }

    fn resize(&self, size: TerminalSize) -> TerminalResult<()> {
        let size = size.validate()?;
        *self.resize.lock().unwrap_or_else(|err| err.into_inner()) = Some((size, false));
        Ok(())
    }

    fn redraw(&self, size: TerminalSize) -> TerminalResult<()> {
        let size = size.validate()?;
        *self.resize.lock().unwrap_or_else(|err| err.into_inner()) = Some((size, true));
        Ok(())
    }

    fn stop(&self) -> TerminalResult<Option<u32>> {
        self.cancel.cancel();
        set_status(&self.shared, TerminalStatus::Exited { exit_code: None });
        Ok(None)
    }

    fn subscribe(&self, sink: SharedSink) -> TerminalResult<()> {
        let mut state = self.shared.lock();
        state.sink = Some(sink);
        state.flow.subscribe(Instant::now());
        let status = state.status.clone();
        state.pending_status.clear();
        state.pending_status.push_back(status);
        drop(state);
        self.shared.cv.notify_all();
        Ok(())
    }

    fn ack(&self, seq: u64) -> TerminalResult<()> {
        self.shared.lock().flow.ack(seq, Instant::now());
        self.shared.cv.notify_all();
        Ok(())
    }

    fn info(&self) -> TerminalSessionInfo {
        let state = self.shared.lock();
        TerminalSessionInfo {
            terminal_id: self.id.clone(),
            terminal_config_id: Some(self.config_id.clone()),
            status: state.status.clone(),
            output: orbyte_terminal::OutputHealth {
                paused: state.flow.is_paused(),
                dropped_bytes: state.flow.dropped_bytes(),
                subscribed: state.flow.is_subscribed(),
                inflight_bytes: state.flow.inflight_bytes(),
                buffered_bytes: state.flow.buffered_len(),
                ack_idle_ms: state.flow.ack_idle_ms(Instant::now()),
            },
        }
    }

    fn supports_native_paste(&self) -> bool {
        false
    }

    fn pause_output(&self) -> TerminalResult<()> {
        self.shared.lock().flow.pause();
        self.shared.cv.notify_all();
        log::info!(target: "orbyte_terminal", "output_paused session={}", self.id);
        Ok(())
    }

    fn supports_process_activation(&self) -> bool {
        false
    }
}

fn worker(launch: ShellLaunch, shared: Arc<OutputShared>, input_rx: Receiver<Vec<u8>>, resize: Arc<Mutex<Option<(TerminalSize, bool)>>>) {
    let ShellLaunch { host, port, saved_algorithms, username, credential, cols, rows, cancel, prepare_auth, finish_auth, on_state, .. } = launch;
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
        let mut channel = open_shell(&mut connected, cols, rows, &cancel)?;
        on_state("ready", None);
        set_status(&shared, TerminalStatus::Running);
        shell_loop(&mut channel, &connected.session, &connected.stream, &shared, &input_rx, &resize, &cancel)?;
        Ok(())
    })();
    if let Err(error) = result {
        let message = error.to_string();
        on_state("failed", Some(message.clone()));
        push_error(&shared, message.clone());
        set_status(&shared, TerminalStatus::Failed { message });
    } else {
        on_state("closed", None);
        set_status(&shared, TerminalStatus::Exited { exit_code: None });
    }
    shared.lock().exited = true;
    shared.cv.notify_all();
}

fn open_shell(connected: &mut connect::Connected, cols: u16, rows: u16, cancel: &CancelToken) -> SshResult<Channel> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut channel = connect::drive(&connected.stream, &mut connected.session, cancel, deadline, |session| {
        session.channel_session()
    })?;
    let cols = u32::from(cols.max(1));
    let rows = u32::from(rows.max(1));
    connect::drive(&connected.stream, &mut connected.session, cancel, deadline, |_| {
        channel.request_pty("xterm-256color", None, Some((cols, rows, 0, 0)))
    })?;
    connect::drive(&connected.stream, &mut connected.session, cancel, deadline, |_| channel.shell())?;
    Ok(channel)
}

fn shell_loop(
    channel: &mut Channel,
    session: &Session,
    stream: &std::net::TcpStream,
    shared: &OutputShared,
    input_rx: &Receiver<Vec<u8>>,
    resize: &Mutex<Option<(TerminalSize, bool)>>,
    cancel: &CancelToken,
) -> SshResult<()> {
    let mut buffered = VecDeque::new();
    let mut keepalive = Keepalive::default();
    let mut probing = false;
    let mut probe_written = false;
    let mut scratch = [0u8; READ_CHUNK];
    let mut pending_sizes = VecDeque::new();
    // libssh2 treats an interval of 1 as 2 and does not wait for the reply itself.
    // The caller sends at most one probe and treats later inbound traffic as the answer.
    session.set_keepalive(true, 1);
    loop {
        if cancel.is_cancelled() {
            return Err(SshError::Cancelled);
        }
        let now = Instant::now();
        match keepalive.on_tick(now) {
            KeepaliveAction::Failed => return Err(SshError::Message("连接保活没有收到应答".into())),
            KeepaliveAction::SendProbe => {
                keepalive.note_probe_sent(now);
                probing = true;
                probe_written = false;
            }
            KeepaliveAction::Wait => {}
        }
        if probing && !probe_written {
            match session.keepalive_send() {
                Err(error) if connect_eagain(&error) => {}
                Ok(_) => probe_written = true,
                Err(_) if cancel.is_cancelled() => return Err(SshError::Cancelled),
                Err(_) => return Err(SshError::Message("连接已断开".into())),
            }
        }
        // A continuously readable peer must still yield to input and resize requests.
        for _ in 0..4 {
            let inbound = readable_now(stream);
            match channel.read(&mut scratch) {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    keepalive.note_reply();
                    probing = false;
                    probe_written = false;
                    push_output(shared, &scratch[..n], cancel)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    // A readable socket with no channel bytes means libssh2 consumed the keepalive reply.
                    if probing && probe_written && inbound {
                        keepalive.note_reply();
                        probing = false;
                        probe_written = false;
                    }
                    break;
                }
                Err(_) if cancel.is_cancelled() => return Err(SshError::Cancelled),
                Err(_) => return Err(SshError::Message("连接已断开".into())),
            }
        }
        while buffered.len() < INPUT_CAP {
            match input_rx.try_recv() {
                Ok(chunk) => buffered.extend(chunk),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        while !buffered.is_empty() {
            let data: Vec<u8> = buffered.iter().copied().take(READ_CHUNK).collect();
            match channel.write(&data) {
                Ok(0) => break,
                Ok(n) => {
                    buffered.drain(..n);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return Err(SshError::Message("连接已断开".into())),
            }
        }
        if let Some((size, redraw)) = resize.lock().unwrap_or_else(|err| err.into_inner()).take() {
            pending_sizes.clear();
            if redraw {
                pending_sizes.push_back(TerminalSize { cols: if size.cols == u16::MAX { size.cols - 1 } else { size.cols + 1 }, rows: size.rows });
            }
            pending_sizes.push_back(size);
        }
        while let Some(size) = pending_sizes.front() {
            match channel.request_pty_size(u32::from(size.cols), u32::from(size.rows), None, None) {
                Ok(()) => { pending_sizes.pop_front(); }
                Err(error) if connect_eagain(&error) => break,
                Err(error) => return Err(SshError::Message(error.to_string())),
            }
        }
        wait_session(stream, session, Duration::from_millis(200))?;
    }
}

fn connect_eagain(error: &ssh2::Error) -> bool {
    crate::error::is_eagain(error)
}

fn push_output(shared: &OutputShared, data: &[u8], cancel: &CancelToken) -> SshResult<()> {
    let mut state = shared.lock();
    loop {
        if cancel.is_cancelled() {
            return Err(SshError::Cancelled);
        }
        if state.flow.push(data, Instant::now()) != PushOutcome::MustWait {
            break;
        }
        let (guard, _) = shared.cv.wait_timeout(state, Duration::from_millis(200)).unwrap_or_else(|err| err.into_inner());
        state = guard;
    }
    drop(state);
    shared.cv.notify_all();
    Ok(())
}

fn set_status(shared: &OutputShared, status: TerminalStatus) {
    let mut state = shared.lock();
    state.status = status.clone();
    state.pending_status.push_back(status);
    drop(state);
    shared.cv.notify_all();
}

fn push_error(shared: &OutputShared, message: String) {
    shared.lock().pending_errors.push_back(message);
    shared.cv.notify_all();
}

struct Defer(Option<Box<dyn FnOnce() + Send>>);
impl Drop for Defer {
    fn drop(&mut self) {
        if let Some(finish) = self.0.take() { finish(); }
    }
}

fn sender_loop(shared: Arc<OutputShared>) {
    loop {
        let packet = {
            let mut state = shared.lock();
            loop {
                if state.disposed && state.pending_status.is_empty() && state.pending_errors.is_empty() && state.flow.buffered_len() == 0 {
                    return;
                }
                let frames = collect(&mut state);
                if !frames.is_empty() {
                    if let Some(sink) = state.sink.clone() {
                        break Some((sink, frames));
                    }
                }
                if state.exited && state.flow.buffered_len() == 0 && state.pending_status.is_empty() {
                    state.disposed = true;
                }
                state = shared.cv.wait_timeout(state, Duration::from_millis(200)).unwrap_or_else(|err| err.into_inner()).0;
            }
        };
        let Some((sink, frames)) = packet else { continue };
        for frame in frames {
            if !sink.send(frame) {
                let mut state = shared.lock();
                state.sink = None;
                state.flow.unsubscribe();
                break;
            }
        }
        shared.cv.notify_all();
    }
}

fn collect(state: &mut OutputState) -> Vec<Vec<u8>> {
    if state.sink.is_none() || !state.flow.is_subscribed() {
        return Vec::new();
    }
    let mut frames = Vec::new();
    while let Some(status) = state.pending_status.pop_front() {
        let payload = serde_json::to_vec(&status).unwrap_or_default();
        frames.push(encode_frame(FRAME_STATUS, state.flow.alloc_seq(), &payload));
    }
    while let Some(message) = state.pending_errors.pop_front() {
        let payload = serde_json::json!({ "message": message }).to_string();
        frames.push(encode_frame(FRAME_ERROR, state.flow.alloc_seq(), payload.as_bytes()));
    }
    let now = Instant::now();
    while frames.len() < 8 {
        match state.flow.take_frame(now) {
            Some(frame) => frames.push(encode_frame(frame.kind, frame.seq, &frame.payload)),
            None => break,
        }
    }
    frames
}
