//! Cooperative cancellation that also shuts the socket down.
//!
//! Closing the socket aborts a handshake without waiting for the 10 second limit.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::io::Write;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct CancelToken {
    flag: AtomicBool,
    generation: AtomicU64,
    socket: Mutex<Option<TcpStream>>,
}

impl CancelToken {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
        self.generation.fetch_add(1, Ordering::Release);
        self.shutdown_socket();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn attach_socket(&self, stream: &TcpStream) {
        if let Ok(cloned) = stream.try_clone() {
            if self.is_cancelled() {
                let _ = cloned.shutdown(Shutdown::Both);
            }
            *self.socket.lock().unwrap_or_else(|err| err.into_inner()) = Some(cloned);
            if self.is_cancelled() {
                self.shutdown_socket();
            }
        }
    }

    fn shutdown_socket(&self) {
        if let Some(socket) = self.socket.lock().unwrap_or_else(|err| err.into_inner()).as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
            let _ = socket.try_clone().map(|mut extra| extra.write_all(&[]));
        }
    }
}
