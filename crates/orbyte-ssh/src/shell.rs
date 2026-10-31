//! One SSH shell on one operating-system thread, with the shared terminal output protocol.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::borrow::Cow;
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
        let initialization = directory_initialization(&mut connected, &cancel);
        let mut channel = open_shell(&mut connected, cols, rows, &cancel)?;
        on_state("ready", None);
        set_status(&shared, TerminalStatus::Running);
        shell_loop(&mut channel, &connected.session, &connected.stream, &shared, &input_rx, &resize, &cancel, initialization.as_bytes())?;
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
    input_rx: &Receiver<Vec<u8>>,
    resize: &Mutex<Option<(TerminalSize, bool)>>,
    cancel: &CancelToken,
    initialization: &[u8],
) -> SshResult<()> {
    let mut buffered = VecDeque::from(initialization.to_vec());
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
