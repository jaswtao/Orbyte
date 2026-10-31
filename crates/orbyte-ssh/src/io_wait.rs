//! Wait until a non-blocking SSH socket can make progress.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::io;
use std::net::TcpStream;
use std::time::Duration;

use ssh2::{BlockDirections, Session};

use crate::error::{SshError, SshResult};

pub fn wait_session(stream: &TcpStream, session: &Session, timeout: Duration) -> SshResult<()> {
    let (read, write) = match session.block_directions() {
        BlockDirections::None => {
            std::thread::sleep(timeout.min(Duration::from_millis(50)));
            return Ok(());
        }
        BlockDirections::Inbound => (true, false),
        BlockDirections::Outbound => (false, true),
        BlockDirections::Both => (true, true),
    };
    poll_socket(stream, read, write, timeout).map(|_| ())
}

/// True when the socket already has data to read. Used to notice a keepalive reply
/// that libssh2 consumes without delivering channel bytes.
pub fn readable_now(stream: &TcpStream) -> bool {
    matches!(poll_socket(stream, true, false, Duration::ZERO), Ok(true))
}

pub fn poll_socket(stream: &TcpStream, read: bool, write: bool, timeout: Duration) -> SshResult<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{WSAPoll, POLLIN, POLLOUT, SOCKET_ERROR, WSAPOLLFD};
        let mut fd = WSAPOLLFD {
            fd: stream.as_raw_socket() as usize,
            events: (if read { POLLIN } else { 0 }) | (if write { POLLOUT } else { 0 }),
            revents: 0,
        };
        let milliseconds = timeout.as_millis().min(i32::MAX as u128) as i32;
        let result = unsafe { WSAPoll(&mut fd, 1u32, milliseconds) };
        if result == SOCKET_ERROR {
            return Err(SshError::from(io::Error::last_os_error()));
        }
        Ok(result > 0)
    }
    #[cfg(not(windows))]
    {
        let _ = (stream, read, write);
        std::thread::sleep(timeout.min(Duration::from_millis(50)));
        Ok(false)
    }
}
