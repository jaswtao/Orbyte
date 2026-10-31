//! TCP connect, handshake, algorithm check, and authentication.
//!
//! The session loop runs on a dedicated OS thread. Credentials are used only
//! after the host key has been accepted, then dropped by the caller.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ssh2::Session;

use crate::algorithms;
use crate::cancel::CancelToken;
use crate::error::{is_eagain, SshError, SshResult};
use crate::hostkey::PresentedKey;
use crate::io_wait::wait_session;

const STEP_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Connected {
    pub session: Session,
    pub stream: TcpStream,
    pub presented: PresentedKey,
}

#[derive(Clone)]
pub enum Credential {
    Password,
    PrivateKey { path: String },
}

pub fn connect_transport(
    host: &str,
    port: u16,
    saved_hostkeys: &[String],
    cancel: &CancelToken,
) -> SshResult<Connected> {
    if cancel.is_cancelled() {
        return Err(SshError::Cancelled);
    }
    let addresses = resolve(host, port, cancel)?;
    if cancel.is_cancelled() {
        return Err(SshError::Cancelled);
    }
    let mut stream = None;
    let mut last_error = SshError::Timeout;
    for address in addresses {
        if cancel.is_cancelled() {
            return Err(SshError::Cancelled);
        }
        match TcpStream::connect_timeout(&address, STEP_TIMEOUT) {
            Ok(tcp) => {
                stream = Some(tcp);
                break;
            }
            Err(error) => last_error = SshError::from(error),
        }
    }
    let stream = stream.ok_or(last_error)?;
    if cancel.is_cancelled() {
        return Err(SshError::Cancelled);
    }
    stream.set_nodelay(true).ok();
    stream.set_nonblocking(true).map_err(SshError::from)?;
    cancel.attach_socket(&stream);
    let poll = stream.try_clone().map_err(SshError::from)?;
    let mut session = Session::new().map_err(|_| SshError::Message("无法创建 SSH 会话".into()))?;
    session.set_tcp_stream(stream);
    algorithms::apply_preferences(&session, saved_hostkeys)?;
    session.set_blocking(false);
    let deadline = Instant::now() + STEP_TIMEOUT;
    drive(&poll, &mut session, cancel, deadline, |session| session.handshake())?;
    if cancel.is_cancelled() {
        return Err(SshError::Cancelled);
    }
    let negotiated = algorithms::negotiated_names(&session).ok_or(SshError::WeakAlgorithm)?;
    if !negotiated.acceptable() {
        let _ = session.disconnect(None, "algorithm rejected", None);
        return Err(SshError::WeakAlgorithm);
    }
    let (public_key, _) = session.host_key().ok_or(SshError::Message("服务器没有提供主机密钥".into()))?;
    let presented = PresentedKey::new(negotiated.hostkey.clone(), public_key.to_vec());
    Ok(Connected { session, stream: poll, presented })
}

pub fn authenticate(connected: &mut Connected, cancel: &CancelToken, username: &str, credential: &Credential, secret: &str) -> SshResult<()> {
    if cancel.is_cancelled() {
        return Err(SshError::Cancelled);
    }
    let methods = drive(&connected.stream, &mut connected.session, cancel, Instant::now() + STEP_TIMEOUT, |session| {
        session.auth_methods(username).map(str::to_owned)
    })?;
    let offered: Vec<&str> = methods.split(',').map(str::trim).filter(|method| !method.is_empty()).collect();
    match credential {
        Credential::Password => {
            if !offered.iter().any(|method| *method == "password") {
                if offered.iter().any(|method| *method == "keyboard-interactive") {
                    return Err(SshError::KeyboardInteractiveUnsupported);
                }
                return Err(SshError::AuthMethodMissing);
            }
            drive(&connected.stream, &mut connected.session, cancel, Instant::now() + STEP_TIMEOUT, |session| {
                session.userauth_password(username, secret)
            })
            .map_err(|error| match error {
                SshError::Message(_) | SshError::Timeout => SshError::AuthenticationFailed,
                other => other,
            })?;
        }
        Credential::PrivateKey { path } => {
            if !offered.iter().any(|method| *method == "publickey") {
                return Err(SshError::AuthMethodMissing);
            }
            let key = Path::new(path);
            let passphrase = if secret.is_empty() { None } else { Some(secret) };
            drive(&connected.stream, &mut connected.session, cancel, Instant::now() + STEP_TIMEOUT, |session| {
                session.userauth_pubkey_file(username, None, key, passphrase)
            })
            .map_err(|error| match error {
                SshError::Message(_) | SshError::Timeout => SshError::AuthenticationFailed,
                other => other,
            })?;
        }
    }
    if !connected.session.authenticated() {
        return Err(SshError::AuthenticationFailed);
    }
    Ok(())
}

fn resolve(host: &str, port: u16, cancel: &CancelToken) -> SshResult<Vec<std::net::SocketAddr>> {
    let host = host.to_owned();
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("ssh-resolve".into())
        .spawn(move || {
            let _ = tx.send((host.as_str(), port).to_socket_addrs());
        })
        .map_err(|_| SshError::Message("无法解析主机".into()))?;
    match rx.recv_timeout(STEP_TIMEOUT) {
        Ok(Ok(addresses)) => {
            if cancel.is_cancelled() {
                return Err(SshError::Cancelled);
            }
            let addresses: Vec<_> = addresses.collect();
            if addresses.is_empty() { Err(SshError::Message("无法解析主机".into())) } else { Ok(addresses) }
        }
        Ok(Err(_)) => Err(SshError::Message("无法解析主机".into())),
        Err(_) => Err(SshError::Timeout),
    }
}

pub fn drive<T>(
    stream: &TcpStream,
    session: &mut Session,
    cancel: &CancelToken,
    deadline: Instant,
    mut operation: impl FnMut(&mut Session) -> Result<T, ssh2::Error>,
) -> SshResult<T> {
    loop {
        if cancel.is_cancelled() {
            return Err(SshError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(SshError::Timeout);
        }
        match operation(session) {
            Ok(value) => return Ok(value),
            Err(error) if is_eagain(&error) || io_would_block(&error) => {
                let remaining = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(200));
                wait_session(stream, session, remaining)?;
            }
            Err(error) => {
                if cancel.is_cancelled() {
                    return Err(SshError::Cancelled);
                }
                log::info!("ssh_step_failed code={:?}", error.code());
                return Err(SshError::Message("SSH 连接失败".into()));
            }
        }
    }
}

fn io_would_block(error: &ssh2::Error) -> bool {
    let text = error.to_string();
    text.contains("would block") || text.contains("WouldBlock")
}

