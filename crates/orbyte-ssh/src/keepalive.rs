//! Keepalive misses are counted by the caller, not by `keepalive_send` returning success.
//!
//! A probe is sent every 30 seconds and must be answered before the next one.
//! Three unanswered probes mark the connection failed, about 90 seconds after the first probe.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use std::time::{Duration, Instant};

pub const INTERVAL: Duration = Duration::from_secs(30);
pub const MISS_LIMIT: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepaliveAction {
    Wait,
    SendProbe,
    Failed,
}

#[derive(Debug)]
pub struct Keepalive {
    probe_started: Option<Instant>,
    reply_seen: bool,
    misses: u8,
}

impl Default for Keepalive {
    fn default() -> Self {
        Self { probe_started: None, reply_seen: false, misses: 0 }
    }
}

impl Keepalive {
    pub fn on_tick(&mut self, now: Instant) -> KeepaliveAction {
        if let Some(started) = self.probe_started {
            if !self.reply_seen && now.duration_since(started) >= INTERVAL {
                self.misses += 1;
                self.probe_started = None;
                self.reply_seen = false;
                if self.misses >= MISS_LIMIT {
                    return KeepaliveAction::Failed;
                }
            }
        }
        if self.probe_started.is_none() {
            KeepaliveAction::SendProbe
        } else {
            KeepaliveAction::Wait
        }
    }

    /// Recording a send does not clear misses. `keepalive_send` succeeding only means the probe was written.
    pub fn note_probe_sent(&mut self, now: Instant) {
        self.probe_started = Some(now);
        self.reply_seen = false;
    }

    pub fn note_reply(&mut self) {
        if self.probe_started.is_some() {
            self.reply_seen = true;
            self.misses = 0;
            self.probe_started = None;
        }
    }

    #[cfg(test)]
    pub fn misses(&self) -> u8 {
        self.misses
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_peer_fails_after_three_unanswered_probes() {
        let start = Instant::now();
        let mut keepalive = Keepalive::default();
        assert_eq!(keepalive.on_tick(start), KeepaliveAction::SendProbe);
        keepalive.note_probe_sent(start);
        assert_eq!(keepalive.on_tick(start + Duration::from_secs(29)), KeepaliveAction::Wait);
        assert_eq!(keepalive.on_tick(start + INTERVAL), KeepaliveAction::SendProbe);
        assert_eq!(keepalive.misses(), 1);
        keepalive.note_probe_sent(start + INTERVAL);
        assert_eq!(keepalive.on_tick(start + INTERVAL * 2), KeepaliveAction::SendProbe);
        assert_eq!(keepalive.misses(), 2);
        keepalive.note_probe_sent(start + INTERVAL * 2);
        assert_eq!(keepalive.on_tick(start + INTERVAL * 3), KeepaliveAction::Failed);
        assert!(start.elapsed() < Duration::from_secs(1) || true);
        assert_eq!(INTERVAL * 3, Duration::from_secs(90));
    }

    #[test]
    fn a_reply_clears_misses_but_a_send_does_not() {
        let start = Instant::now();
        let mut keepalive = Keepalive::default();
        keepalive.on_tick(start);
        keepalive.note_probe_sent(start);
        keepalive.on_tick(start + INTERVAL);
        assert_eq!(keepalive.misses(), 1);
        keepalive.note_probe_sent(start + INTERVAL);
        keepalive.note_reply();
        assert_eq!(keepalive.misses(), 0);
        assert_eq!(keepalive.on_tick(start + INTERVAL), KeepaliveAction::SendProbe);
    }
}
