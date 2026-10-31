//! SSH and SFTP for Orbyte.
//!
//! Each shell or SFTP session owns one `ssh2::Session` on a dedicated thread.
//! Ordinary `cargo test` does not open a real server; the ignored probe does.
//!
//! Author: jaswtao
//! Date: 2026-10-31

mod algorithms;
mod cancel;
mod connect;
mod error;
mod hostkey;
mod hub;
mod io_wait;
mod keepalive;
mod path;
mod sftp;
mod shell;
mod transfer;

pub use connect::Credential;
pub use path::normalize_remote;
pub use error::SshError;
pub use hub::{EnqueueRequest, LocalGrant, LocalGrantKind, NewHostKey, SftpRequest, ShellRequest, SshHub, SshUiEvent, StoredHostKey, TransferView, UiSink};
pub use sftp::DirBatch;
pub use shell::SshTerminal;

/// Internal surface for the ignored real-server protocol probe.
/// Not part of the application API; enabled only by the `probe` feature.
#[cfg(feature = "probe")]
#[doc(hidden)]
pub mod probe {
    pub use crate::cancel::CancelToken;
    pub use crate::connect::{authenticate, connect_transport, Connected};
    pub use crate::error::{SshError, SshResult};
    pub use crate::transfer::{run_transfer, Choice, Direction, FileRow, Hooks, Snapshot, TransferSpec};
}

#[cfg(test)]
mod cancel_tests {
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::cancel::CancelToken;
    use crate::connect::connect_transport;
    use crate::error::SshError;

    #[test]
    fn cancelling_a_silent_peer_stops_the_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((socket, _)) = listener.accept() {
                thread::sleep(Duration::from_secs(30));
                drop(socket);
            }
        });
        let cancel = CancelToken::new();
        let cancel_later = cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            cancel_later.cancel();
        });
        let started = Instant::now();
        let error = match connect_transport("127.0.0.1", port, &[], &cancel) {
            Err(error) => error,
            Ok(_) => panic!("a silent peer must not finish the handshake"),
        };
        assert!(started.elapsed() < Duration::from_secs(3), "elapsed {:?}", started.elapsed());
        assert!(matches!(error, SshError::Cancelled), "{error}");
    }
}
