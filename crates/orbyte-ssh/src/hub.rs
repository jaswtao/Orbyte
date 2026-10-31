//! In-memory SSH attempts, host-key challenges, and transfer queue.
//!
//! Passwords live only inside a batch until every attempt in that batch has finished authenticating.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use orbyte_terminal::{SessionRuntime, SharedSink, TerminalSessionInfo};
use serde::Serialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::algorithms::allows_negotiated_hostkey;
use crate::cancel::CancelToken;
use crate::connect::Credential;
use crate::error::{SshError, SshResult};
use crate::hostkey::{self, PresentedKey};
use crate::sftp::{self, DirBatch, SftpJob};
use crate::shell::{ShellLaunch, SshTerminal};
use crate::transfer::{self, Choice, Direction, Snapshot, TransferSpec};

pub type HostLookup = Arc<dyn Fn(&str, u16) -> Result<Vec<StoredHostKey>, String> + Send + Sync>;
pub type HostSave = Arc<dyn Fn(NewHostKey) -> Result<(), String> + Send + Sync>;
pub type UiSink = Arc<dyn Fn(SshUiEvent) + Send + Sync>;

#[derive(Clone)]
pub struct StoredHostKey {
    pub algorithm: String,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

#[derive(Clone)]
pub struct NewHostKey {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SshUiEvent {
    Challenge { challenge_id: String, host: String, port: u16, algorithm: String, fingerprint: String },
    CredentialRequired { batch_id: String, auth_type: String },
    Attempt { attempt_id: String, terminal_id: Option<String>, state: String, message: Option<String> },
}

#[derive(Clone)]
pub enum LocalGrantKind { Upload, Download }

pub struct LocalGrant {
    pub token: String,
    pub workspace_id: String,
    pub kind: LocalGrantKind,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: String,
    pub client_request_id: String,
    pub workspace_id: String,
    pub direction: String,
    pub state: String,
    pub transferred: u64,
    pub total: Option<u64>,
    pub bytes_per_second: u64,
    pub message: Option<String>,
    pub files: Vec<transfer::FileRow>,
    pub revision: u64,
}

pub struct SshHub {
    inner: Mutex<Inner>,
}

struct Inner {
    accepting: bool,
    lookup: Option<HostLookup>,
    save: Option<HostSave>,
    batches: HashMap<String, Arc<Batch>>,
    attempts: HashMap<String, Attempt>,
    terminals: HashMap<String, Arc<SshTerminal>>,
    sftp: HashMap<String, SftpLink>,
    grants: HashMap<String, LocalGrant>,
    transfers: Vec<TransferRecord>,
    conflicts: HashMap<String, Arc<ConflictSlot>>,
    listeners: Vec<Arc<dyn Fn(Vec<TransferView>) + Send + Sync>>,
    revision: u64,
    sticky: HashMap<String, Choice>,
    challenges: HashMap<String, Arc<Challenge>>,
    browses: HashMap<String, Arc<AtomicBool>>,
}

struct Attempt {
    batch_id: String,
    cancel: Arc<CancelToken>,
}

struct SftpLink {
    tx: std::sync::mpsc::SyncSender<SftpJob>,
    cancel: Arc<CancelToken>,
    busy: bool,
    waiting: VecDeque<SftpJob>,
}

struct TransferRecord {
    id: String,
    client_request_id: String,
    workspace_id: String,
    direction: String,
    cancelled: Arc<AtomicBool>,
    snapshot: Snapshot,
}

struct Batch {
    state: Mutex<BatchState>,
    cv: Condvar,
    sink: UiSink,
    auth_type: String,
    connection_id: String,
}

struct BatchState {
    secret: Option<Zeroizing<String>>,
    cancelled: bool,
    pending: usize,
    notified: bool,
    closed: bool,
    sealed: bool,
}

struct Challenge {
    state: Mutex<ChallengeState>,
    cv: Condvar,
}

struct ChallengeState {
    decision: Option<Trust>,
    waiters: usize,
    expires: Instant,
    host: String,
    port: u16,
    algorithm: String,
    public_key: Vec<u8>,
}

#[derive(Clone, Copy)]
enum Trust { Once, Save, Cancel }

struct ConflictSlot {
    state: Mutex<ConflictState>,
    cv: Condvar,
}

struct ConflictState {
    open: bool,
    decision: Option<Choice>,
    generation: u64,
}

impl SshHub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { inner: Mutex::new(Inner {
            accepting: true,
            lookup: None,
            save: None,
            batches: HashMap::new(),
            attempts: HashMap::new(),
            terminals: HashMap::new(),
            sftp: HashMap::new(),
            grants: HashMap::new(),
            transfers: Vec::new(),
            conflicts: HashMap::new(),
            listeners: Vec::new(),
            revision: 0,
            sticky: HashMap::new(),
            challenges: HashMap::new(),
            browses: HashMap::new(),
        }) })
    }

    pub fn set_host_store(&self, lookup: HostLookup, save: HostSave) {
        let mut inner = self.lock();
        inner.lookup = Some(lookup);
        inner.save = Some(save);
    }

    pub fn set_accepting(&self, accepting: bool) {
        self.lock().accepting = accepting;
    }

    pub fn is_accepting(&self) -> bool {
        self.lock().accepting
    }

    pub fn active_transfer_count(&self) -> usize {
        self.lock().transfers.iter().filter(|transfer| is_active(&transfer.snapshot.state)).count()
    }

    pub fn batch_connection(&self, batch_id: &str) -> SshResult<String> {
        Ok(self.batch(batch_id)?.connection_id.clone())
    }

    pub fn check_batch_connection(&self, batch_id: &str, connection_id: &str) -> SshResult<()> {
        if self.batch_connection(batch_id)? != connection_id { return Err(SshError::ChallengeRejected); }
        let batch = self.batch(batch_id)?;
        let state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        if state.closed || state.cancelled || state.sealed { return Err(SshError::ChallengeRejected); }
        Ok(())
    }

    pub fn cancel_connection_batches(&self, connection_id: &str) {
        let ids: Vec<_> = self.lock().batches.iter()
            .filter(|(_, batch)| batch.connection_id == connection_id
                && !batch.state.lock().unwrap_or_else(|err| err.into_inner()).closed)
            .map(|(id, _)| id.clone()).collect();
        for id in ids { let _ = self.cancel_batch(&id); }
    }

    /// Keep the secret until every request has been registered and authenticated.
    pub fn seal_batch(&self, batch_id: &str) -> SshResult<()> {
        let batch = self.batch(batch_id)?;
        let mut state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        state.sealed = true;
        if state.pending == 0 {
            state.secret = None;
            state.closed = true;
        }
        Ok(())
    }

    pub fn validate_secret_submission(&self, batch_id: &str) -> SshResult<()> {
        let batch = self.batch(batch_id)?;
        let state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        if state.cancelled || state.closed || state.pending == 0 { return Err(SshError::ChallengeRejected); }
        Ok(())
    }

    pub fn begin_connection_batch(&self, connection_id: String, auth_type: String, secret: Option<Zeroizing<String>>, sink: UiSink) -> SshResult<String> {
        let mut inner = self.lock();
        if !inner.accepting { return Err(SshError::ShuttingDown); }
        let id = Uuid::new_v4().to_string();
        inner.batches.insert(id.clone(), Arc::new(Batch {
            state: Mutex::new(BatchState { secret, cancelled: false, pending: 0, notified: false, closed: false, sealed: false }),
            cv: Condvar::new(),
            sink,
            auth_type,
            connection_id,
        }));
        Ok(id)
    }

    pub fn submit_secret(&self, batch_id: &str, secret: String) -> SshResult<()> {
        let secret = Zeroizing::new(secret);
        let batch = self.batch(batch_id)?;
        let mut state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        if state.cancelled || state.closed || state.pending == 0 {
            return Err(SshError::ChallengeRejected);
        }
        state.secret = Some(secret);
        drop(state);
        batch.cv.notify_all();
        Ok(())
    }

    pub fn answer_challenge(&self, challenge_id: &str, decision: &str) -> SshResult<()> {
        if !self.is_accepting() { return Err(SshError::ShuttingDown); }
        let trust = match decision {
            "once" => Trust::Once,
            "save" => Trust::Save,
            "cancel" => Trust::Cancel,
            _ => return Err(SshError::ChallengeRejected),
        };
        let challenge = {
            let inner = self.lock();
            inner.challenges.get(challenge_id).cloned().ok_or(SshError::ChallengeRejected)?
        };
        let mut state = challenge.state.lock().unwrap_or_else(|err| err.into_inner());
        if state.waiters == 0 || Instant::now() >= state.expires || state.decision.is_some() {
            return Err(SshError::ChallengeRejected);
        }
        state.decision = Some(trust);
        drop(state);
        challenge.cv.notify_all();
        Ok(())
    }

    pub fn cancel_attempt(&self, attempt_id: &str) -> SshResult<()> {
        if let Some(batch_id) = attempt_id.strip_prefix("batch:") {
            return self.cancel_batch(batch_id);
        }
        let Some(cancel) = self.lock().attempts.get(attempt_id).map(|attempt| attempt.cancel.clone()) else {
            return Err(SshError::NotRunning);
        };
        cancel.cancel();
        Ok(())
    }

    pub fn connect_shell(self: &Arc<Self>, request: ShellRequest) -> SshResult<Arc<SshTerminal>> {
        if !self.is_accepting() { return Err(SshError::ShuttingDown); }
        let batch = self.batch(&request.batch_id)?;
        batch.state.lock().unwrap_or_else(|err| err.into_inner()).pending += 1;
        let cancel = CancelToken::new();
        let auth_cancel = cancel.clone();
        let attempt_id = request.attempt_id.clone();
        self.lock().attempts.insert(attempt_id.clone(), Attempt { batch_id: request.batch_id.clone(), cancel: cancel.clone() });
        let hub = self.clone();
        let host = request.host.clone();
        let port = request.port;
        let batch_id = request.batch_id.clone();
        let error_batch = batch_id.clone();
        let saved = self.saved_algorithms(&host, port)?;
        let on_state = self.attempt_callback(&attempt_id, Some(request.terminal_id.clone()), batch.sink.clone());
        let launch = ShellLaunch {
            terminal_id: request.terminal_id.clone(),
            config_id: request.config_id.clone(),
            host: request.host,
            port: request.port,
            saved_algorithms: saved,
            username: request.username,
            credential: request.credential,
            cols: request.cols,
            rows: request.rows,
            sink: request.sink,
            cancel: cancel.clone(),
            prepare_auth: Box::new(move |key| hub.prepare(&batch_id, &host, port, &auth_cancel, key)),
            finish_auth: {
                let hub = self.clone();
                let batch_id = request.batch_id.clone();
                Box::new(move || hub.finish_auth(&batch_id))
            },
            on_state,
        };
        let terminal = match SshTerminal::start(launch) {
            Ok(terminal) => terminal,
            Err(error) => {
                self.finish_auth(&error_batch);
                return Err(error);
            }
        };
        self.lock().terminals.insert(request.terminal_id, terminal.clone());
        Ok(terminal)
    }

    pub fn connect_sftp(self: &Arc<Self>, request: SftpRequest) -> SshResult<()> {
        if !self.is_accepting() { return Err(SshError::ShuttingDown); }
        let batch = self.batch(&request.batch_id)?;
        batch.state.lock().unwrap_or_else(|err| err.into_inner()).pending += 1;
        let cancel = CancelToken::new();
        let auth_cancel = cancel.clone();
        let hub = self.clone();
        let host = request.host.clone();
        let port = request.port;
        let batch_id = request.batch_id.clone();
        let error_batch = batch_id.clone();
        let saved = self.saved_algorithms(&host, port)?;
        let on_state = self.attempt_callback(&request.attempt_id, None, batch.sink.clone());
        let tx = match sftp::start(sftp::SftpLaunch {
            host: request.host,
            port,
            saved_algorithms: saved,
            username: request.username,
            credential: request.credential,
            cancel: cancel.clone(),
            prepare_auth: Box::new(move |key| hub.prepare(&batch_id, &host, port, &auth_cancel, key)),
            finish_auth: {
                let hub = self.clone();
                let batch_id = request.batch_id.clone();
                Box::new(move || hub.finish_auth(&batch_id))
            },
            on_state,
        }) {
            Ok(tx) => tx,
            Err(error) => {
                self.finish_auth(&error_batch);
                return Err(error);
            }
        };
        self.lock().attempts.insert(request.attempt_id, Attempt { batch_id: request.batch_id, cancel: cancel.clone() });
        self.lock().sftp.insert(request.workspace_id, SftpLink { tx, cancel, busy: false, waiting: VecDeque::new() });
        Ok(())
    }

    pub fn terminal(&self, id: &str) -> Option<Arc<SshTerminal>> {
        self.lock().terminals.get(id).cloned()
    }

    pub fn disconnect_sftp(&self, workspace_id: &str) {
        if let Some(link) = self.lock().sftp.remove(workspace_id) {
            link.cancel.cancel();
            let _ = link.tx.try_send(SftpJob::Shutdown);
        }
    }

    pub fn list_terminals(&self) -> Vec<TerminalSessionInfo> {
        self.lock().terminals.values().map(|terminal| terminal.info()).collect()
    }

    pub fn stop_terminal(&self, id: &str) -> SshResult<()> {
        let terminal = self.terminal(id).ok_or(SshError::NotRunning)?;
        terminal.stop().map_err(|error| SshError::Message(error.to_string()))?;
        Ok(())
    }

    pub fn insert_grant(&self, grant: LocalGrant) {
        self.lock().grants.insert(grant.token.clone(), grant);
    }

    pub fn list_directory(&self, workspace_id: &str, path: &str, request_id: &str, emit: Arc<dyn Fn(DirBatch) + Send + Sync>) -> SshResult<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        let tx = self.sftp_tx(workspace_id)?;
        self.lock().browses.insert(request_id.into(), cancel.clone());
        tx.try_send(SftpJob::List { path: path.into(), request_id: request_id.into(), cancel, emit }).map_err(|_| SshError::SftpUnavailable)?;
        Ok(())
    }

    pub fn cancel_browse(&self, request_id: &str) {
        if let Some(cancel) = self.lock().browses.get(request_id) {
            cancel.store(true, Ordering::Release);
        }
    }

    pub fn create_directory(&self, workspace_id: &str, path: &str) -> SshResult<()> {
        self.sftp_roundtrip(workspace_id, |done| SftpJob::Mkdir { path: path.into(), done })
    }

    pub fn canonicalize(&self, workspace_id: &str, path: &str) -> SshResult<String> {
        let (tx, rx) = mpsc::channel();
        self.sftp_tx(workspace_id)?.try_send(SftpJob::Realpath { path: path.into(), done: tx }).map_err(|_| SshError::SftpUnavailable)?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(|_| SshError::Timeout)?
    }

    pub fn enqueue_transfer(self: &Arc<Self>, request: EnqueueRequest) -> SshResult<String> {
        if !self.is_accepting() { return Err(SshError::ShuttingDown); }
        let mut inner = self.lock();
        if let Some(existing) = inner.transfers.iter().find(|transfer| transfer.client_request_id == request.client_request_id) {
            return Ok(existing.id.clone());
        }
        if inner.transfers.iter().filter(|transfer| is_active(&transfer.snapshot.state)).count() >= 100 {
            return Err(SshError::TransferQueueFull);
        }
        let grant = inner.grants.get(&request.token).ok_or_else(|| SshError::Message("本地路径授权已失效，请重新选择".into()))?;
        if grant.workspace_id != request.workspace_id {
            return Err(SshError::Message("本地路径授权不属于这个远程工作区".into()));
        }
        let (direction, names, local_root) = match grant.kind {
            LocalGrantKind::Upload => (Direction::Upload, grant.paths.iter().map(|path| path.display().to_string()).collect(), PathBuf::new()),
            LocalGrantKind::Download => (Direction::Download, request.remote_names.clone(), grant.paths.first().cloned().unwrap_or_default()),
        };
        let remote_directory = request.remote_directory.clone();
        let id = Uuid::new_v4().to_string();
        let cancelled = Arc::new(AtomicBool::new(false));
        inner.transfers.push(TransferRecord {
            id: id.clone(),
            client_request_id: request.client_request_id,
            workspace_id: request.workspace_id.clone(),
            direction: match direction { Direction::Upload => "upload", Direction::Download => "download" }.into(),
            cancelled: cancelled.clone(),
            snapshot: Snapshot { state: "queued".into(), transferred: 0, total: None, bytes_per_second: 0, files: Vec::new(), message: None },
        });
        inner.conflicts.insert(id.clone(), Arc::new(ConflictSlot { state: Mutex::new(ConflictState { open: false, decision: None, generation: 0 }), cv: Condvar::new() }));
        let spec = TransferSpec { direction, remote_directory, local_root, names };
        let (done_tx, done_rx) = mpsc::channel();
        let progress = {
            let hub = self.clone();
            let transfer_id = id.clone();
            Arc::new(move |snapshot: Snapshot| hub.on_progress(&transfer_id, snapshot))
        };
        let conflict = {
            let hub = self.clone();
            let transfer_id = id.clone();
            Arc::new(move |relative: &str, size, mtime| hub.wait_conflict(&transfer_id, relative, size, mtime))
        };
        let job = SftpJob::Transfer { spec, cancelled, progress, conflict, done: done_tx };
        let workspace_id = request.workspace_id.clone();
        {
            let link = inner.sftp.get_mut(&workspace_id).ok_or(SshError::SftpUnavailable)?;
            if link.busy {
                link.waiting.push_back(job);
            } else {
                link.tx.try_send(job).map_err(|_| SshError::TransferQueueFull)?;
                link.busy = true;
            }
        }
        let hub = self.clone();
        let transfer_id = id.clone();
        std::thread::spawn(move || {
            if let Ok(snapshot) = done_rx.recv() {
                hub.on_progress(&transfer_id, snapshot);
            }
            hub.finish_transfer(&workspace_id);
        });
        publish(inner);
        Ok(id)
    }

    pub fn list_transfers(&self) -> (u64, Vec<TransferView>) {
        let inner = self.lock();
        (inner.revision, inner.views())
    }

    pub fn subscribe_transfers(&self, listener: Arc<dyn Fn(Vec<TransferView>) + Send + Sync>) {
        self.lock().listeners.push(listener);
    }

    pub fn cancel_transfer(&self, id: &str) -> SshResult<()> {
        let mut inner = self.lock();
        let transfer = inner.transfers.iter_mut().find(|transfer| transfer.id == id).ok_or(SshError::TransferNotFound)?;
        transfer.cancelled.store(true, Ordering::Release);
        transfer.snapshot.state = "cancelling".into();
        if let Some(slot) = inner.conflicts.get(id) {
            slot.state.lock().unwrap_or_else(|err| err.into_inner()).decision = Some(Choice::Cancel);
            slot.cv.notify_all();
        }
        publish(inner);
        Ok(())
    }

    pub fn resolve_conflict(&self, transfer_id: &str, choice: &str, apply_all: bool) -> SshResult<()> {
        let choice = match choice {
            "overwrite" => Choice::Overwrite,
            "skip" => Choice::Skip,
            "cancel" => Choice::Cancel,
            _ => return Err(SshError::ConflictExpired),
        };
        let slot = self.lock().conflicts.get(transfer_id).cloned().ok_or(SshError::TransferNotFound)?;
        let mut state = slot.state.lock().unwrap_or_else(|err| err.into_inner());
        if !state.open { return Err(SshError::ConflictExpired); }
        state.decision = Some(choice);
        state.open = false;
        drop(state);
        slot.cv.notify_all();
        if apply_all { self.lock().sticky.insert(transfer_id.into(), choice); }
        Ok(())
    }

    pub fn shutdown(&self) {
        self.set_accepting(false);
        let mut inner = self.lock();
        for attempt in inner.attempts.values() { attempt.cancel.cancel(); }
        for link in inner.sftp.values_mut() {
            link.cancel.cancel();
            while let Some(job) = link.waiting.pop_front() {
                if let SftpJob::Transfer { done, .. } = job {
                    let _ = done.send(terminal_snapshot("cancelled", "传输已取消"));
                }
            }
            let _ = link.tx.try_send(SftpJob::Shutdown);
        }
        for transfer in &inner.transfers { transfer.cancelled.store(true, Ordering::Release); }
        for terminal in inner.terminals.values() { let _ = terminal.stop(); }
        for batch in inner.batches.values() {
            batch.state.lock().unwrap_or_else(|err| err.into_inner()).cancelled = true;
            batch.state.lock().unwrap_or_else(|err| err.into_inner()).secret = None;
            batch.cv.notify_all();
        }
    }

    fn prepare(self: &Arc<Self>, batch_id: &str, host: &str, port: u16, cancel: &CancelToken, key: &PresentedKey) -> SshResult<Zeroizing<String>> {
        self.authorize(batch_id, host, port, key, cancel)?;
        self.wait_secret(batch_id, cancel)
    }

    fn authorize(self: &Arc<Self>, batch_id: &str, host: &str, port: u16, key: &PresentedKey, cancel: &CancelToken) -> SshResult<()> {
        let lookup = self.lock().lookup.clone().ok_or_else(|| SshError::Message("主机记录还不可用".into()))?;
        let stored = lookup(host, port).map_err(SshError::Message)?;
        let saved: Vec<_> = stored.iter().map(|item| item.algorithm.clone()).collect();
        allows_negotiated_hostkey(&saved, &key.algorithm)?;
        if let Some(existing) = stored.iter().find(|item| item.algorithm == key.algorithm) {
            return match hostkey::judge(Some(&existing.public_key), &key.public_key) {
                hostkey::HostKeyVerdict::Trusted => Ok(()),
                hostkey::HostKeyVerdict::Mismatch => Err(hostkey::mismatch(existing.fingerprint.clone(), key.fingerprint.clone())),
                hostkey::HostKeyVerdict::FirstUse => Ok(()),
            };
        }
        if cancel.is_cancelled() { return Err(SshError::Cancelled); }
        if let Some(existing) = self.pending_challenge(host, port, key) {
            return self.wait_shared_challenge(&existing, cancel, host, port, key);
        }
        let challenge_id = Uuid::new_v4().to_string();
        let challenge = Arc::new(Challenge {
            state: Mutex::new(ChallengeState {
                decision: None,
                waiters: 1,
                expires: Instant::now() + Duration::from_secs(120),
                host: host.into(),
                port,
                algorithm: key.algorithm.clone(),
                public_key: key.public_key.clone(),
            }),
            cv: Condvar::new(),
        });
        {
            let mut inner = self.lock();
            if let Some(existing) = find_challenge(&inner, host, port, key) {
                drop(inner);
                return self.wait_shared_challenge(&existing, cancel, host, port, key);
            }
            inner.challenges.insert(challenge_id.clone(), challenge.clone());
        }
        self.emit_challenge(batch_id, &challenge_id, host, port, &key.algorithm, &key.fingerprint);
        let decision = self.wait_challenge(&challenge, cancel)?;
        {
            let mut state = challenge.state.lock().unwrap_or_else(|err| err.into_inner());
            state.waiters = state.waiters.saturating_sub(1);
        }
        match decision {
            Trust::Cancel => Err(SshError::ChallengeRejected),
            Trust::Once => Ok(()),
            Trust::Save => {
                let save = self.lock().save.clone().ok_or_else(|| SshError::Message("无法保存主机密钥".into()))?;
                save(NewHostKey { host: host.into(), port, algorithm: key.algorithm.clone(), public_key: key.public_key.clone(), fingerprint: key.fingerprint.clone() })
                    .map_err(SshError::Message)
            }
        }
    }

    fn wait_challenge(&self, challenge: &Challenge, cancel: &CancelToken) -> SshResult<Trust> {
        let mut state = challenge.state.lock().unwrap_or_else(|err| err.into_inner());
        loop {
            if cancel.is_cancelled() { return Err(SshError::Cancelled); }
            if let Some(decision) = state.decision { return Ok(decision); }
            let now = Instant::now();
            if now >= state.expires { return Err(SshError::ChallengeRejected); }
            let remaining = state.expires.saturating_duration_since(now);
            let (guard, timeout) = challenge.cv.wait_timeout(state, remaining).unwrap_or_else(|err| err.into_inner());
            state = guard;
            if timeout.timed_out() && state.decision.is_none() { return Err(SshError::ChallengeRejected); }
        }
    }

    fn wait_secret(&self, batch_id: &str, cancel: &CancelToken) -> SshResult<Zeroizing<String>> {
        let batch = self.batch(batch_id)?;
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        if !state.notified && state.secret.is_none() && !state.cancelled {
            state.notified = true;
            let event = SshUiEvent::CredentialRequired { batch_id: batch_id.into(), auth_type: batch.auth_type.clone() };
            let sink = batch.sink.clone();
            drop(state);
            sink(event);
            state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        }
        loop {
            if cancel.is_cancelled() || state.cancelled { return Err(SshError::Cancelled); }
            if let Some(secret) = state.secret.clone() { return Ok(secret); }
            if Instant::now() >= deadline { return Err(SshError::Timeout); }
            let (guard, _) = batch.cv.wait_timeout(state, Duration::from_millis(200)).unwrap_or_else(|err| err.into_inner());
            state = guard;
        }
    }

    fn finish_auth(&self, batch_id: &str) {
        let Ok(batch) = self.batch(batch_id) else { return };
        let mut state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        state.pending = state.pending.saturating_sub(1);
        if state.pending == 0 && state.sealed {
            state.secret = None;
            state.closed = true;
        }
    }

    fn on_progress(&self, id: &str, snapshot: Snapshot) {
        let mut inner = self.lock();
        if let Some(transfer) = inner.transfers.iter_mut().find(|transfer| transfer.id == id) {
            transfer.snapshot = snapshot;
        }
        publish(inner);
    }

    fn wait_conflict(&self, transfer_id: &str, relative: &str, size: Option<u64>, mtime: Option<u64>) -> SshResult<Choice> {
        if let Some(choice) = self.lock().sticky.get(transfer_id).copied() { return Ok(choice); }
        if self.lock().transfers.iter().any(|transfer| transfer.id == transfer_id && transfer.cancelled.load(Ordering::Acquire)) {
            return Err(SshError::Cancelled);
        }
        let slot = self.lock().conflicts.get(transfer_id).cloned().ok_or(SshError::TransferNotFound)?;
        {
            let mut state = slot.state.lock().unwrap_or_else(|err| err.into_inner());
            state.open = true;
            state.decision = None;
            state.generation += 1;
        }
        {
            let mut inner = self.lock();
            if let Some(transfer) = inner.transfers.iter_mut().find(|transfer| transfer.id == transfer_id) {
                transfer.snapshot.state = "awaitingConflict".into();
                transfer.snapshot.message = Some(format!("{relative} size={size:?} mtime={mtime:?}"));
                if let Some(file) = transfer.snapshot.files.iter_mut().find(|file| file.relative == relative) {
                    file.status = "awaitingConflict".into();
                }
            }
            publish(inner);
        }
        loop {
            if self.lock().transfers.iter().any(|transfer| transfer.id == transfer_id && transfer.cancelled.load(Ordering::Acquire)) {
                return Err(SshError::Cancelled);
            }
            let state = slot.state.lock().unwrap_or_else(|err| err.into_inner());
            if let Some(decision) = state.decision {
                return Ok(decision);
            }
            let (guard, _) = slot.cv.wait_timeout(state, Duration::from_millis(200)).unwrap_or_else(|err| err.into_inner());
            drop(guard);
        }
    }

    fn saved_algorithms(&self, host: &str, port: u16) -> SshResult<Vec<String>> {
        let Some(lookup) = self.lock().lookup.clone() else { return Ok(Vec::new()); };
        Ok(lookup(host, port).map_err(SshError::Message)?.into_iter().map(|item| item.algorithm).collect())
    }

    fn attempt_callback(&self, attempt_id: &str, terminal_id: Option<String>, sink: UiSink) -> Box<dyn Fn(&str, Option<String>) + Send + Sync> {
        let attempt_id = attempt_id.to_owned();
        Box::new(move |state, message| {
            sink(SshUiEvent::Attempt { attempt_id: attempt_id.clone(), terminal_id: terminal_id.clone(), state: state.into(), message });
        })
    }

    fn emit_challenge(&self, batch_id: &str, challenge_id: &str, host: &str, port: u16, algorithm: &str, fingerprint: &str) {
        let Ok(batch) = self.batch(batch_id) else { return };
        (batch.sink)(SshUiEvent::Challenge { challenge_id: challenge_id.into(), host: host.into(), port, algorithm: algorithm.into(), fingerprint: fingerprint.into() });
    }

    fn batch(&self, id: &str) -> SshResult<Arc<Batch>> {
        self.lock().batches.get(id).cloned().ok_or(SshError::NotRunning)
    }

    fn pending_challenge(&self, host: &str, port: u16, key: &PresentedKey) -> Option<Arc<Challenge>> {
        find_challenge(&self.lock(), host, port, key)
    }

    fn wait_shared_challenge(&self, challenge: &Challenge, cancel: &CancelToken, _host: &str, _port: u16, _key: &PresentedKey) -> SshResult<()> {
        {
            let mut state = challenge.state.lock().unwrap_or_else(|err| err.into_inner());
            state.waiters += 1;
        }
        let decision = self.wait_challenge(challenge, cancel)?;
        {
            let mut state = challenge.state.lock().unwrap_or_else(|err| err.into_inner());
            state.waiters = state.waiters.saturating_sub(1);
        }
        match decision {
            Trust::Cancel => Err(SshError::ChallengeRejected),
            Trust::Once | Trust::Save => Ok(()),
        }
    }

    fn finish_transfer(&self, workspace_id: &str) {
        let mut inner = self.lock();
        let Some(link) = inner.sftp.get_mut(workspace_id) else { return };
        link.busy = false;
        let Some(job) = link.waiting.pop_front() else { return };
        match link.tx.try_send(job) {
            Ok(()) => link.busy = true,
            Err(mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job)) => {
                if let SftpJob::Transfer { done, .. } = job {
                    let _ = done.send(terminal_snapshot("failed", "SFTP 不可用"));
                }
            }
        }
    }

    fn sftp_tx(&self, workspace_id: &str) -> SshResult<std::sync::mpsc::SyncSender<SftpJob>> {
        self.lock().sftp.get(workspace_id).map(|link| link.tx.clone()).ok_or(SshError::SftpUnavailable)
    }

    fn sftp_roundtrip(&self, workspace_id: &str, job: impl FnOnce(mpsc::Sender<SshResult<()>>) -> SftpJob) -> SshResult<()> {
        let (tx, rx) = mpsc::channel();
        self.sftp_tx(workspace_id)?.try_send(job(tx)).map_err(|_| SshError::SftpUnavailable)?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(|_| SshError::Timeout)?
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn cancel_batch(&self, batch_id: &str) -> SshResult<()> {
        let batch = self.batch(batch_id)?;
        let mut state = batch.state.lock().unwrap_or_else(|err| err.into_inner());
        state.cancelled = true;
        state.secret = None;
        drop(state);
        batch.cv.notify_all();
        let cancels: Vec<_> = self.lock().attempts.values().filter(|attempt| attempt.batch_id == batch_id).map(|attempt| attempt.cancel.clone()).collect();
        for cancel in cancels { cancel.cancel(); }
        Ok(())
    }
}

impl Inner {
    fn views(&self) -> Vec<TransferView> {
        self.transfers.iter().map(|transfer| TransferView {
            id: transfer.id.clone(),
            client_request_id: transfer.client_request_id.clone(),
            workspace_id: transfer.workspace_id.clone(),
            direction: transfer.direction.clone(),
            state: transfer.snapshot.state.clone(),
            transferred: transfer.snapshot.transferred,
            total: transfer.snapshot.total,
            bytes_per_second: transfer.snapshot.bytes_per_second,
            message: transfer.snapshot.message.clone(),
            files: transfer.snapshot.files.clone(),
            revision: self.revision,
        }).collect()
    }

}

fn find_challenge(inner: &Inner, host: &str, port: u16, key: &PresentedKey) -> Option<Arc<Challenge>> {
    inner.challenges.values().find(|challenge| {
        let state = challenge.state.lock().unwrap_or_else(|err| err.into_inner());
        state.host == host
            && state.port == port
            && state.algorithm == key.algorithm
            && state.public_key == key.public_key
            && Instant::now() < state.expires
    }).cloned()
}

fn terminal_snapshot(state: &str, message: &str) -> Snapshot {
    Snapshot {
        state: state.into(),
        transferred: 0,
        total: None,
        bytes_per_second: 0,
        files: Vec::new(),
        message: Some(message.into()),
    }
}

fn publish(mut inner: std::sync::MutexGuard<'_, Inner>) {
    inner.revision += 1;
    let views = inner.views();
    let listeners = inner.listeners.clone();
    drop(inner);
    for listener in listeners {
        listener(views.clone());
    }
}

fn is_active(state: &str) -> bool {
    matches!(state, "queued" | "scanning" | "running" | "awaitingConflict" | "cancelling")
}

pub struct ShellRequest {
    pub batch_id: String,
    pub attempt_id: String,
    pub terminal_id: String,
    pub config_id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credential: Credential,
    pub cols: u16,
    pub rows: u16,
    pub sink: SharedSink,
}

pub struct SftpRequest {
    pub batch_id: String,
    pub attempt_id: String,
    pub workspace_id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credential: Credential,
}

pub struct EnqueueRequest {
    pub workspace_id: String,
    pub token: String,
    pub client_request_id: String,
    pub remote_directory: String,
    pub remote_names: Vec<String>,
}

// Silence unused import in case the runtime trait is used by callers through SshTerminal.
#[allow(dead_code)]
fn _runtime(terminal: &SshTerminal) -> &dyn SessionRuntime { terminal }

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ui_events_use_camel_case_fields() {
        let events = [
            (SshUiEvent::Challenge {
                challenge_id: "challenge".into(), host: "host".into(), port: 22,
                algorithm: "ssh-ed25519".into(), fingerprint: "SHA256:key".into(),
            }, json!({
                "type": "challenge", "challengeId": "challenge", "host": "host", "port": 22,
                "algorithm": "ssh-ed25519", "fingerprint": "SHA256:key",
            })),
            (SshUiEvent::CredentialRequired { batch_id: "batch".into(), auth_type: "privateKey".into() },
                json!({ "type": "credentialRequired", "batchId": "batch", "authType": "privateKey" })),
            (SshUiEvent::Attempt {
                attempt_id: "attempt".into(), terminal_id: Some("terminal".into()), state: "ready".into(), message: None,
            }, json!({ "type": "attempt", "attemptId": "attempt", "terminalId": "terminal", "state": "ready", "message": null })),
        ];
        for (event, expected) in events {
            assert_eq!(serde_json::to_value(event).unwrap(), expected);
        }
    }

    #[test]
    fn batch_secret_is_cleared_when_the_last_authentication_finishes() {
        let hub = SshHub::new();
        let id = hub.begin_connection_batch("connection".into(), "password".into(), None, Arc::new(|_| {})).unwrap();
        let batch = hub.batch(&id).unwrap();
        batch.state.lock().unwrap().pending = 2;
        hub.seal_batch(&id).unwrap();
        hub.submit_secret(&id, "secret".into()).unwrap();
        hub.finish_auth(&id);
        assert!(batch.state.lock().unwrap().secret.is_some());
        hub.finish_auth(&id);
        let state = batch.state.lock().unwrap();
        assert!(state.secret.is_none());
        assert!(state.closed);
        drop(state);
        assert!(hub.submit_secret(&id, "late secret".into()).is_err());
    }

    #[test]
    fn saved_password_waits_for_registration_and_never_requests_frontend_credentials() {
        let hub = SshHub::new();
        let id = hub.begin_connection_batch("one".into(), "password".into(), Some(Zeroizing::new(" saved ".into())), Arc::new(|_| panic!("saved password must stay in the backend"))).unwrap();
        assert!(hub.check_batch_connection(&id, "two").is_err());
        hub.check_batch_connection(&id, "one").unwrap();
        let batch = hub.batch(&id).unwrap();
        batch.state.lock().unwrap().pending = 1;
        assert_eq!(hub.wait_secret(&id, &CancelToken::new()).unwrap().as_str(), " saved ");
        hub.finish_auth(&id);
        assert!(batch.state.lock().unwrap().secret.is_some());
        // A fast first authentication must not clear the password before SFTP registers.
        batch.state.lock().unwrap().pending += 1;
        hub.seal_batch(&id).unwrap();
        assert!(hub.check_batch_connection(&id, "one").is_err());
        assert_eq!(hub.wait_secret(&id, &CancelToken::new()).unwrap().as_str(), " saved ");
        hub.finish_auth(&id);
        let state = batch.state.lock().unwrap();
        assert!(state.secret.is_none());
        assert!(state.closed);
    }

    #[test]
    fn cancelling_connection_batches_clears_only_their_credentials() {
        let hub = SshHub::new();
        let first = hub.begin_connection_batch("one".into(), "password".into(), Some(Zeroizing::new("first".into())), Arc::new(|_| {})).unwrap();
        let second = hub.begin_connection_batch("two".into(), "password".into(), Some(Zeroizing::new("second".into())), Arc::new(|_| {})).unwrap();
        hub.cancel_connection_batches("one");
        assert!(hub.batch(&first).unwrap().state.lock().unwrap().secret.is_none());
        assert!(hub.check_batch_connection(&first, "one").is_err());
        hub.check_batch_connection(&second, "two").unwrap();
        assert!(hub.batch(&second).unwrap().state.lock().unwrap().secret.is_some());
        hub.seal_batch(&second).unwrap();
        assert!(hub.batch(&second).unwrap().state.lock().unwrap().secret.is_none());
    }
}
