//! One SSH shell on one operating-system thread, with the shared terminal output protocol.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use orbyte_terminal::output::{encode_frame, FlowConfig, FlowState, PushOutcome, FRAME_ERROR, FRAME_STATUS, FRAME_SUDO_PROMPT};
use orbyte_terminal::{SessionRuntime, SharedSink, TerminalResult, TerminalSessionInfo, TerminalSize, TerminalStatus};
use ssh2::{Channel, Session};

use crate::cancel::CancelToken;
use zeroize::Zeroizing;

use crate::connect::{self, Credential};
use crate::hostkey::PresentedKey;
use crate::error::{SshError, SshResult};
use crate::io_wait::{readable_now, wait_session};
use crate::keepalive::{Keepalive, KeepaliveAction};
use crate::sudo::{SudoDetector, SudoPrompt};

pub type SavedPasswordReader = Arc<dyn Fn() -> Result<Option<Zeroizing<String>>, String> + Send + Sync>;

enum InputRequest {
    Data(Vec<u8>),
    SavedPassword { prompt_id: String, reply: mpsc::Sender<Result<(), String>>, cancelled: Arc<AtomicBool> },
}

struct SecretWrite {
    bytes: Zeroizing<Vec<u8>>,
    offset: usize,
    reply: mpsc::Sender<Result<(), String>>,
    cancelled: Arc<AtomicBool>,
}

const INPUT_CAP: usize = 256 * 1024;
const READ_CHUNK: usize = 64 * 1024;
// Socket polling cannot be interrupted by an input queue notification.
const INTERACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SAVED_PASSWORD_TIMEOUT: Duration = Duration::from_secs(5);
const SAVED_PASSWORD_EXPIRED: &str = "The saved password request expired. Enter the password manually.";

struct OutputState {
    flow: FlowState,
    sink: Option<SharedSink>,
    status: TerminalStatus,
    pending_status: VecDeque<TerminalStatus>,
    pending_errors: VecDeque<String>,
    exited: bool,
    disposed: bool,
    sudo: SudoDetector,
    sudo_prompt: SudoPrompt,
    sudo_dirty: bool,
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
    input_tx: Mutex<Option<SyncSender<InputRequest>>>,
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
    pub saved_password: Option<SavedPasswordReader>,
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
                sudo: SudoDetector::default(),
                sudo_prompt: SudoPrompt::default(),
                sudo_dirty: true,
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

    /// Wait on a blocking task; the SSH worker validates the prompt before reading credentials.
    pub fn submit_saved_password(&self, prompt_id: String) -> Result<(), String> {
        let (reply, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        {
            let state = self.shared.lock();
            if state.exited || state.sudo_prompt.prompt_id.as_ref() != Some(&prompt_id) {
                return Err("The sudo prompt is no longer available. Enter the password manually.".into());
            }
            let guard = self.input_tx.lock().unwrap_or_else(|err| err.into_inner());
            let tx = guard.as_ref().ok_or("The SSH session has ended.")?;
            tx.try_send(InputRequest::SavedPassword { prompt_id, reply, cancelled: cancelled.clone() })
                .map_err(|_| "The terminal input queue is full. Enter the password manually.")?;
        }
        match result.recv_timeout(SAVED_PASSWORD_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::SeqCst);
                Err("Timed out sending the saved password. Enter the password manually.".into())
            }
            Err(RecvTimeoutError::Disconnected) => Err("The SSH session has ended. Enter the password manually.".into()),
        }
    }
}

impl SessionRuntime for SshTerminal {
    fn write(&self, data: Vec<u8>) -> TerminalResult<()> {
        let state = self.shared.lock();
        if state.exited {
            return Err(orbyte_terminal::TerminalError::NotRunning);
        }
        let guard = self.input_tx.lock().unwrap_or_else(|err| err.into_inner());
        let Some(tx) = guard.as_ref() else {
            return Err(orbyte_terminal::TerminalError::NotRunning);
        };
        // The worker applies this input to its sudo detector in queue order.
        tx.try_send(InputRequest::Data(data)).map_err(|_| orbyte_terminal::TerminalError::Io("输入队列已满".into()))?;
        drop(state);
        self.shared.cv.notify_all();
        Ok(())
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
        state.sudo_dirty = true;
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

fn worker(launch: ShellLaunch, shared: Arc<OutputShared>, input_rx: Receiver<InputRequest>, resize: Arc<Mutex<Option<(TerminalSize, bool)>>>) {
    let ShellLaunch { host, port, saved_algorithms, username, credential, cols, rows, cancel, prepare_auth, finish_auth, on_state, saved_password, .. } = launch;
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
        let initialization = directory_initialization(&mut connected, &cancel);
        let mut channel = open_shell(&mut connected, cols, rows, &cancel)?;
        on_state("ready", None);
        set_status(&shared, TerminalStatus::Running);
        shell_loop(&mut channel, &connected.session, &connected.stream, &shared, &input_rx, &resize, &cancel, initialization.as_bytes(), saved_password.as_ref())?;
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

fn directory_initialization(connected: &mut connect::Connected, cancel: &CancelToken) -> String {
    // Probe on a separate channel so unsupported shells never receive shell syntax as keyboard input.
    let deadline = Instant::now() + Duration::from_secs(2);
    let probe = (|| -> SshResult<Vec<u8>> {
        let mut channel = connect::drive(&connected.stream, &mut connected.session, cancel, deadline, |session| session.channel_session())?;
        connect::drive(&connected.stream, &mut connected.session, cancel, deadline, |_| {
            channel.exec("printf '\\036orbyte-shell:%s\\037' \"$0\"")
        })?;
        let mut output = Vec::new();
        let mut buffer = [0u8; 1024];
        while Instant::now() < deadline && !cancel.is_cancelled() && output.len() < 4096 {
            match channel.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    output.extend_from_slice(&buffer[..n]);
                    if output.contains(&0x1f) { break; }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    wait_session(&connected.stream, &connected.session, Duration::from_millis(50))?;
                }
                Err(_) => break,
            }
        }
        Ok(output)
    })();
    directory_initialization_command(&probe.unwrap_or_default())
}

// The injected line always starts with this text, which survives wrapped redraws.
const INITIALIZATION_ECHO_PREFIX: &[u8] = b" eval $'";
// Printed by the injected line after the directory hook is installed.
const INITIALIZATION_READY: &[u8] = b"\x1b]6973;D\x07";
const INITIALIZATION_DROP_LIMIT: usize = 16 * 1024;

fn directory_initialization_command(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let shell = text.split_once('\u{1e}').and_then(|(_, value)| value.strip_prefix("orbyte-shell:"))
        .and_then(|value| value.split_once('\u{1f}')).map(|(value, _)| value.rsplit('/').next().unwrap_or(value).trim_start_matches('-'));
    if !matches!(shell, Some("bash" | "zsh")) { return String::new(); }
    // A single eval line keeps multiline initialization out of the interactive line editor.
    let script = include_str!("directory-integration.sh").replace('\r', "");
    format!(
        " eval $'{}';command printf '\\033]6973;D\\007'\r",
        script.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n")
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EchoState {
    Searching,
    Dropping,
    Suppressed,
}

// The shell echoes the injected line before running it, and line editors can
// redraw wrapped text, so the echo is not one contiguous copy of the command.
// Drop output from the start of the command until the line reports that the
// hook is installed, then pass the prompt and its directory report through.
struct InitializationEcho {
    state: EchoState,
    pending: Vec<u8>,
    dropped: usize,
}

impl InitializationEcho {
    fn new(enabled: bool) -> Self {
        Self {
            state: if enabled { EchoState::Searching } else { EchoState::Suppressed },
            pending: Vec::new(),
            dropped: 0,
        }
    }

    fn filter<'a>(&mut self, data: &'a [u8]) -> Cow<'a, [u8]> {
        if self.state == EchoState::Suppressed {
            return Cow::Borrowed(data);
        }
        self.pending.extend_from_slice(data);
        if self.state == EchoState::Searching {
            if let Some(start) = find(&self.pending, INITIALIZATION_ECHO_PREFIX) {
                let mut output = self.pending[..start].to_vec();
                match find(&self.pending[start..], INITIALIZATION_READY) {
                    Some(offset) => {
                        let end = start + offset + INITIALIZATION_READY.len();
                        output.extend_from_slice(&self.pending[end..]);
                        self.pending.clear();
                        self.state = EchoState::Suppressed;
                    }
                    None => {
                        // Keep a marker that was split by this read before dropping the echo.
                        let retained = matching_suffix(&self.pending, INITIALIZATION_READY);
                        let discarded = self.pending.len() - retained;
                        self.pending.drain(..discarded);
                        self.dropped = discarded;
                        self.state = EchoState::Dropping;
                    }
                }
                return Cow::Owned(output);
            }
            if let Some(start) = find(&self.pending, INITIALIZATION_READY) {
                // The server disabled echo, so the hook runs without an echoed command.
                let mut output = self.pending[..start].to_vec();
                output.extend_from_slice(&self.pending[start + INITIALIZATION_READY.len()..]);
                self.pending.clear();
                self.state = EchoState::Suppressed;
                return Cow::Owned(output);
            }
            let retained = matching_suffix(&self.pending, INITIALIZATION_ECHO_PREFIX)
                .max(matching_suffix(&self.pending, INITIALIZATION_READY));
            let emitted = self.pending.len() - retained;
            return Cow::Owned(self.pending.drain(..emitted).collect());
        }
        if let Some(start) = find(&self.pending, INITIALIZATION_READY) {
            let output = self.pending[start + INITIALIZATION_READY.len()..].to_vec();
            self.pending.clear();
            self.state = EchoState::Suppressed;
            return Cow::Owned(output);
        }
        let retained = matching_suffix(&self.pending, INITIALIZATION_READY);
        let discarded = self.pending.len() - retained;
        self.pending.drain(..discarded);
        self.dropped += discarded;
        if self.dropped > INITIALIZATION_DROP_LIMIT {
            // The line never reported ready; stop discarding shell output.
            log::warn!(
                target: "orbyte_terminal",
                "shell_initialization_ready_missing dropped={}",
                self.dropped
            );
            self.pending.clear();
            self.state = EchoState::Suppressed;
        }
        Cow::Borrowed(&[])
    }

    fn finish(&mut self) -> Vec<u8> {
        let pending = std::mem::take(&mut self.pending);
        let searching = self.state == EchoState::Searching;
        self.state = EchoState::Suppressed;
        if searching { pending } else { Vec::new() }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn matching_suffix(haystack: &[u8], needle: &[u8]) -> usize {
    let max = needle.len().saturating_sub(1).min(haystack.len());
    (1..=max).rev().find(|&len| haystack.ends_with(&needle[..len])).unwrap_or(0)
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
    input_rx: &Receiver<InputRequest>,
    resize: &Mutex<Option<(TerminalSize, bool)>>,
    cancel: &CancelToken,
    initialization: &[u8],
    saved_password: Option<&SavedPasswordReader>,
) -> SshResult<()> {
    let mut buffered = VecDeque::from(initialization.to_vec());
    let mut pending_request = None;
    let mut secret_write: Option<SecretWrite> = None;
    let mut initialization_echo = InitializationEcho::new(!initialization.is_empty());
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
        let mut output_drained = false;
        for _ in 0..4 {
            let inbound = readable_now(stream);
            match channel.read(&mut scratch) {
                Ok(0) => {
                    let output = initialization_echo.finish();
                    if !output.is_empty() {
                        push_output(shared, &output, cancel)?;
                    }
                    return Ok(());
                }
                Ok(n) => {
                    keepalive.note_reply();
                    probing = false;
                    probe_written = false;
                    let output = initialization_echo.filter(&scratch[..n]);
                    if !output.is_empty() {
                        let mut state = shared.lock();
                        state.sudo.output(&output);
                        update_sudo(&mut state);
                        drop(state);
                        push_output(shared, &output, cancel)?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    // A readable socket with no channel bytes means libssh2 consumed the keepalive reply.
                    if probing && probe_written && inbound {
                        keepalive.note_reply();
                        probing = false;
                        probe_written = false;
                    }
                    output_drained = true;
                    break;
                }
                Err(_) if cancel.is_cancelled() => return Err(SshError::Cancelled),
                Err(_) => return Err(SshError::Message("连接已断开".into())),
            }
        }
        let candidate = {
            let mut state = shared.lock();
            let candidate = (output_drained && state.sudo.settle()).then(|| state.sudo.prompt());
            if candidate.is_none() {
                update_sudo(&mut state);
            }
            candidate
        };
        if let Some(candidate) = candidate {
            // Only advertise a candidate that still exists in the OS credential store.
            // The read can enter the OS credential vault, so keep it outside the shared lock.
            let available = saved_password_usable(saved_password);
            let mut state = shared.lock();
            if state.sudo.prompt().prompt_id == candidate.prompt_id && !available {
                state.sudo.disable();
            }
            update_sudo(&mut state);
        }
        shared.cv.notify_all();
        while secret_write.is_none() && buffered.len() < INPUT_CAP {
            match pending_request.take().map(Ok).unwrap_or_else(|| input_rx.try_recv()) {
                Ok(InputRequest::Data(chunk)) => {
                    let mut state = shared.lock();
                    state.sudo.input(&chunk);
                    update_sudo(&mut state);
                    buffered.extend(chunk);
                }
                Ok(InputRequest::SavedPassword { prompt_id, reply, cancelled }) => {
                    // A secret action must never overtake previously accepted bytes.
                    if !buffered.is_empty() || !output_drained {
                        pending_request = Some(InputRequest::SavedPassword { prompt_id, reply, cancelled });
                        break;
                    }
                    if cancelled.load(Ordering::SeqCst) {
                        let _ = reply.send(Err(SAVED_PASSWORD_EXPIRED.into()));
                        continue;
                    }
                    match prepare_saved_password(shared, saved_password, &prompt_id) {
                        Ok(bytes) => secret_write = Some(SecretWrite { bytes, offset: 0, reply, cancelled }),
                        Err(message) => { let _ = reply.send(Err(message)); }
                    }
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        while !buffered.is_empty() {
            let data = Zeroizing::new(buffered.iter().copied().take(READ_CHUNK).collect::<Vec<u8>>());
            match channel.write(&data) {
                Ok(0) => break,
                Ok(n) => {
                    buffered.drain(..n);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return Err(SshError::Message("连接已断开".into())),
            }
        }
        if let Some(secret) = secret_write.as_mut() {
            // A request that already timed out must not inject any password bytes.
            if secret.offset == 0 && secret.cancelled.load(Ordering::SeqCst) {
                let _ = secret.reply.send(Err(SAVED_PASSWORD_EXPIRED.into()));
                secret_write = None;
            } else {
                match channel.write(&secret.bytes[secret.offset..]) {
                    Ok(n) => {
                        secret.offset += n;
                        if secret.offset == secret.bytes.len() {
                            let _ = secret.reply.send(Ok(()));
                            secret_write = None;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => return Err(SshError::Message("连接已断开".into())),
                }
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
        wait_session(stream, session, INTERACTIVE_POLL_INTERVAL)?;
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
    if !matches!(status, TerminalStatus::Running) {
        state.sudo.disable();
        update_sudo(&mut state);
    }
    state.status = status.clone();
    state.pending_status.push_back(status);
    drop(state);
    shared.cv.notify_all();
}

fn update_sudo(state: &mut OutputState) {
    let prompt = state.sudo.prompt();
    if prompt != state.sudo_prompt {
        state.sudo_prompt = prompt;
        state.sudo_dirty = true;
    }
}

fn acceptable_saved_password(password: &str) -> bool {
    !password.is_empty() && !password.chars().any(char::is_control)
}

fn saved_password_usable(reader: Option<&SavedPasswordReader>) -> bool {
    reader.and_then(|read| read().ok().flatten())
        .is_some_and(|password| acceptable_saved_password(&password))
}

fn prepare_saved_password(shared: &OutputShared, reader: Option<&SavedPasswordReader>, id: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let stale = "The sudo prompt has changed. Enter the password manually.";
    {
        let state = shared.lock();
        if !matches!(state.status, TerminalStatus::Running) || state.sudo.prompt().prompt_id.as_deref() != Some(id) {
            return Err(stale.into());
        }
    }
    let result = (|| {
        let reader = reader.ok_or("No saved password is available. Enter the password manually.")?;
        let password = reader().map_err(|_| "Unable to read the saved password. Enter the password manually.")?
            .ok_or("No saved password is available. Enter the password manually.")?;
        // Terminal line discipline must not interpret password bytes as extra commands or signals.
        if !acceptable_saved_password(&password) {
            return Err("This saved password cannot be entered automatically. Enter the password manually.");
        }
        let mut state = shared.lock();
        if !matches!(state.status, TerminalStatus::Running) || !state.sudo.consume(id) { return Err(stale); }
        update_sudo(&mut state);
        let mut bytes = Zeroizing::new(Vec::with_capacity(password.len() + 1));
        bytes.extend_from_slice(password.as_bytes());
        bytes.push(b'\r');
        Ok(bytes)
    })();
    if result.is_err() {
        let mut state = shared.lock();
        if state.sudo.prompt().prompt_id.as_deref() == Some(id) {
            state.sudo.disable();
            update_sudo(&mut state);
        }
    }
    shared.cv.notify_all();
    result.map_err(String::from)
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
    if state.sudo_dirty {
        state.sudo_dirty = false;
        let payload = serde_json::to_vec(&state.sudo_prompt).unwrap_or_default();
        frames.push(encode_frame(FRAME_SUDO_PROMPT, state.flow.alloc_seq(), &payload));
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

#[cfg(test)]
mod sudo_tests {
    use super::*;

    fn fixture() -> (Arc<OutputShared>, String) {
        let mut flow = FlowState::new(FlowConfig::default(), Instant::now());
        flow.subscribe(Instant::now());
        let mut sudo = SudoDetector::default();
        sudo.output(b"[sudo] password for user: ");
        assert!(sudo.settle());
        let sudo_prompt = sudo.prompt();
        let id = sudo_prompt.prompt_id.clone().unwrap();
        let shared = Arc::new(OutputShared {
            state: Mutex::new(OutputState {
                flow, sink: Some(Arc::new(Discard)), status: TerminalStatus::Running,
                pending_status: VecDeque::new(), pending_errors: VecDeque::new(),
                exited: false, disposed: false, sudo, sudo_prompt, sudo_dirty: true,
            }),
            cv: Condvar::new(),
        });
        (shared, id)
    }

    struct Discard;
    impl orbyte_terminal::FrameSink for Discard {
        fn send(&self, _: Vec<u8>) -> bool { true }
    }

    #[test]
    fn saved_password_is_read_per_session_and_consumed_once() {
        for expected in ["first password", "another password 中文"] {
            let (shared, id) = fixture();
            let value = expected.to_string();
            let reader: SavedPasswordReader = Arc::new(move || Ok(Some(Zeroizing::new(value.clone()))));
            let bytes = prepare_saved_password(&shared, Some(&reader), &id).unwrap();
            assert_eq!(bytes.as_slice(), format!("{expected}\r").as_bytes());
            assert!(prepare_saved_password(&shared, Some(&reader), &id).is_err());
            assert!(shared.lock().sudo_prompt.prompt_id.is_none());
        }
    }

    #[test]
    fn stale_or_manually_used_prompts_never_read_credentials() {
        let (shared, id) = fixture();
        let reader: SavedPasswordReader = Arc::new(|| panic!("must not read credentials"));
        assert!(prepare_saved_password(&shared, Some(&reader), "stale").is_err());
        shared.lock().sudo.input(b"manual");
        assert!(prepare_saved_password(&shared, Some(&reader), &id).is_err());
    }

    #[test]
    fn credential_removal_read_errors_and_control_characters_require_manual_input() {
        let readers: Vec<SavedPasswordReader> = vec![
            Arc::new(|| Ok(None)),
            Arc::new(|| Err("sensitive store error".into())),
            Arc::new(|| Ok(Some(Zeroizing::new("secret\rcommand".into())))),
        ];
        for reader in readers {
            let (shared, id) = fixture();
            let error = prepare_saved_password(&shared, Some(&reader), &id).unwrap_err();
            assert!(!error.contains("sensitive"));
            assert!(!error.contains("secret"));
            assert!(shared.lock().sudo_prompt.prompt_id.is_none());
            assert!(!shared.lock().sudo.settle());
        }
    }

    #[test]
    fn hint_requires_a_usable_saved_password() {
        let usable: SavedPasswordReader = Arc::new(|| Ok(Some(Zeroizing::new("secret".into()))));
        assert!(saved_password_usable(Some(&usable)));
        assert!(!saved_password_usable(None));
        let readers: Vec<SavedPasswordReader> = vec![
            Arc::new(|| Ok(None)),
            Arc::new(|| Err("sensitive store error".into())),
            Arc::new(|| Ok(Some(Zeroizing::new(String::new())))),
            Arc::new(|| Ok(Some(Zeroizing::new("secret\rcommand".into())))),
        ];
        for reader in readers {
            assert!(!saved_password_usable(Some(&reader)));
        }
    }

    #[test]
    fn prompt_is_revalidated_after_reading_the_store() {
        let (shared, id) = fixture();
        let changed = shared.clone();
        let reader: SavedPasswordReader = Arc::new(move || {
            changed.lock().sudo.input(&[3]);
            Ok(Some(Zeroizing::new("secret".into())))
        });
        assert!(prepare_saved_password(&shared, Some(&reader), &id).is_err());
    }

    #[test]
    fn resubscription_sends_only_the_current_prompt_and_exit_clears_it() {
        let (shared, id) = fixture();
        let terminal = SshTerminal {
            id: "terminal".into(), config_id: "config".into(), shared: shared.clone(),
            input_tx: Mutex::new(None), resize: Arc::new(Mutex::new(None)),
            cancel: CancelToken::new(), thread: Mutex::new(None),
        };
        let metadata = |state: &mut OutputState| {
            collect(state).into_iter().find(|frame| frame[0] == FRAME_SUDO_PROMPT)
                .map(|frame| serde_json::from_slice::<serde_json::Value>(&frame[17..]).unwrap())
        };
        assert_eq!(metadata(&mut shared.lock()).unwrap()["promptId"], id);
        assert!(metadata(&mut shared.lock()).is_none());
        terminal.subscribe(Arc::new(Discard)).unwrap();
        assert_eq!(metadata(&mut shared.lock()).unwrap()["promptId"], id);
        set_status(&shared, TerminalStatus::Exited { exit_code: None });
        assert!(metadata(&mut shared.lock()).unwrap()["promptId"].is_null());
    }
}

#[cfg(test)]
mod directory_tests {
    use super::{directory_initialization_command, InitializationEcho};

    #[test]
    fn directory_initialization_requires_a_supported_shell_marker() {
        for output in ["", "bash", "\x1eorbyte-shell:/bin/fish\x1f", "\x1eorbyte-shell:/bin/bash", "\x1eorbyte-shell:cmd.exe\x1f"] {
            assert!(directory_initialization_command(output.as_bytes()).is_empty());
        }
        for shell in ["bash", "-bash", "/bin/bash", "/usr/bin/zsh", "-zsh"] {
            let output = format!("Welcome\n\x1eorbyte-shell:{shell}\x1f");
            assert!(!directory_initialization_command(output.as_bytes()).is_empty());
        }
    }

    #[test]
    fn initialization_is_one_quoted_line_with_no_literal_escape_sequences() {
        let command = directory_initialization_command(b"\x1eorbyte-shell:/bin/bash\x1f");
        assert!(command.starts_with(" eval $'"));
        assert!(command.ends_with("';command printf '\\033]6973;D\\007'\r"));
        assert!(!command.contains(['\n', '\x1b', '\x07']));
        // Decode the ANSI-C quoted payload to verify quotes, newlines and printf escapes survive.
        let payload = command.strip_prefix(" eval $'").unwrap()
            .strip_suffix("';command printf '\\033]6973;D\\007'\r").unwrap();
        let mut decoded = String::new();
        let mut chars = payload.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next().unwrap() {
                    'n' => decoded.push('\n'),
                    value @ ('\\' | '\'') => decoded.push(value),
                    value => panic!("unexpected escape {value}"),
                }
            } else { decoded.push(ch); }
        }
        assert_eq!(decoded, include_str!("directory-integration.sh").replace('\r', ""));
    }

    #[test]
    fn initialization_echo_is_dropped_even_when_the_line_editor_redraws_it() {
        let initialization = directory_initialization_command(b"\x1eorbyte-shell:/bin/bash\x1f");
        let echo = initialization.strip_suffix('\r').unwrap();
        // Line editors wrap long lines with explicit newlines and may redraw them.
        let redrawn = echo.replace("    __orbyte_report_directory", "\r\n    __orbyte_report_directory");
        let input = [
            b"Welcome\r\n$ ".as_slice(),
            redrawn.as_bytes(),
            b"\r\n".as_slice(),
            super::INITIALIZATION_READY,
            b"\x1b]9;9;/home/user\x07$ ".as_slice(),
        ].concat();
        let expected = b"Welcome\r\n$ \x1b]9;9;/home/user\x07$ ";
        let mut filter = InitializationEcho::new(true);
        let mut output = Vec::new();
        for byte in input.chunks(1) {
            output.extend_from_slice(filter.filter(byte).as_ref());
        }
        output.extend(filter.finish());
        assert_eq!(output, expected);
    }

    #[test]
    fn ready_marker_split_right_after_the_echo_is_still_detected() {
        let split = 4;
        let head = [
            b"$ ".as_slice(),
            super::INITIALIZATION_ECHO_PREFIX,
            b"echo\r\n".as_slice(),
            &super::INITIALIZATION_READY[..split],
        ].concat();
        let tail = [
            &super::INITIALIZATION_READY[split..],
            b"\x1b]9;9;/tmp\x07$ ".as_slice(),
        ].concat();
        let mut filter = InitializationEcho::new(true);
        let mut output = filter.filter(&head).into_owned();
        output.extend_from_slice(filter.filter(&tail).as_ref());
        output.extend(filter.finish());
        assert_eq!(output, b"$ \x1b]9;9;/tmp\x07$ ".as_slice());
    }

    #[test]
    fn output_without_initialization_echo_keeps_everything_but_the_ready_marker() {
        let input = b"prompt$ \x1b]6973;D\x07\r\nmore";
        let expected = b"prompt$ \r\nmore";
        let mut filter = InitializationEcho::new(true);
        let mut output = Vec::new();
        for chunk in input.chunks(3) {
            output.extend_from_slice(filter.filter(chunk).as_ref());
        }
        output.extend(filter.finish());
        assert_eq!(output, expected);
    }

    #[test]
    #[ignore = "requires an isolated Bash or Zsh SSH server and ORBYTE_SSH_PROBE=1"]
    fn ssh_shell_reports_directory_changes() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        use orbyte_terminal::{FrameSink, SessionRuntime};
        use super::{CancelToken, Credential, ShellLaunch, SshTerminal, Zeroizing};

        assert_eq!(std::env::var("ORBYTE_SSH_PROBE").as_deref(), Ok("1"));
        #[derive(Default)]
        struct Output(Mutex<Vec<u8>>);
        impl FrameSink for Output {
            fn send(&self, frame: Vec<u8>) -> bool {
                if frame[0] == orbyte_terminal::output::FRAME_OUTPUT {
                    self.0.lock().unwrap().extend_from_slice(&frame[orbyte_terminal::output::FRAME_HEADER_LEN..]);
                }
                true
            }
        }
        struct Stop(Arc<SshTerminal>);
        impl Drop for Stop { fn drop(&mut self) { let _ = self.0.stop(); } }
        let output = Arc::new(Output::default());
        let secret = Zeroizing::new(std::env::var("ORBYTE_SSH_PASSWORD").expect("test password"));
        let username = std::env::var("ORBYTE_SSH_USER").unwrap_or_else(|_| "orbyte".into());
        let initial = format!("/home/{username}");
        let terminal = SshTerminal::start(ShellLaunch {
            terminal_id: "directory-probe".into(), config_id: "directory-probe".into(),
            host: std::env::var("ORBYTE_SSH_HOST").expect("isolated test host"),
            port: std::env::var("ORBYTE_SSH_PORT").expect("isolated test port").parse().unwrap(),
            saved_algorithms: Vec::new(), username, credential: Credential::Password,
            cols: 120, rows: 30, sink: output.clone(), cancel: CancelToken::new(),
            prepare_auth: Box::new(move |_| Ok(secret)), finish_auth: Box::new(|| {}), on_state: Box::new(|_, _| {}),
            saved_password: None,
        }).unwrap();
        let _stop = Stop(terminal.clone());
        let wait = |path: &str| {
            let report = format!("\x1b]9;9;{path}\x07");
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if output.0.lock().unwrap().windows(report.len()).any(|window| window == report.as_bytes()) { return; }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("missing directory report for {path}; session status: {:?}", terminal.info().status);
        };
        wait(&initial);
        for (command, path) in [("cd /tmp\r", "/tmp"), ("cd /\r", "/"), ("cd - >/dev/null\r", "/tmp"), ("cd /__orbyte_missing_directory__\r", "/tmp")] {
            output.0.lock().unwrap().clear();
            terminal.write(command.as_bytes().to_vec()).unwrap();
            wait(path);
        }
    }
}
