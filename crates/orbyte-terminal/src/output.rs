//! 终端输出帧编码与流控状态机。
//!
//! 帧格式：`[kind: u8][seq: u64 LE][timestamp_ms: u64 LE][payload]`。
//! 流控：有活跃订阅时按 ack 窗口限速、缓冲满则暂停读取；
//! 无订阅或订阅失联时继续读取，丢弃最旧数据并记录缺口。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const FRAME_OUTPUT: u8 = 1;
pub const FRAME_STATUS: u8 = 2;
pub const FRAME_ERROR: u8 = 3;
pub const FRAME_GAP: u8 = 4;

/// 帧头长度：kind + seq + timestamp。
pub const FRAME_HEADER_LEN: usize = 1 + 8 + 8;

/// 编码一帧。
pub fn encode_frame(kind: u8, seq: u64, payload: &[u8]) -> Vec<u8> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    frame.push(kind);
    frame.extend_from_slice(&seq.to_le_bytes());
    frame.extend_from_slice(&timestamp.to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// 流控参数。
#[derive(Debug, Clone)]
pub struct FlowConfig {
    /// 每会话环形缓冲上限。
    pub buffer_capacity: usize,
    /// 在途未 ack 窗口。
    pub window: usize,
    /// 有在途数据但超过该时长没有 ack 时视为订阅失联。
    pub stale_after: Duration,
    /// 单个输出帧的最大负载。
    pub max_batch: usize,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            buffer_capacity: 4 * 1024 * 1024,
            window: 1024 * 1024,
            stale_after: Duration::from_secs(5),
            max_batch: 64 * 1024,
        }
    }
}

/// 读取到的数据写入缓冲的结果。
#[derive(Debug, PartialEq, Eq)]
pub enum PushOutcome {
    /// 已写入。
    Stored,
    /// 有活跃订阅且缓冲已满，读取方需要等待（背压）。
    MustWait,
    /// 无订阅或订阅失联，丢弃了最旧的若干字节后写入。
    DroppedOldest(usize),
}

/// 待发送的一帧（未编码）。
#[derive(Debug, PartialEq, Eq)]
pub struct PendingFrame {
    pub kind: u8,
    pub seq: u64,
    pub payload: Vec<u8>,
}

/// 单个会话的流控状态。由会话持有并在锁内访问。
#[derive(Debug)]
pub struct FlowState {
    cfg: FlowConfig,
    buffer: VecDeque<u8>,
    subscribed: bool,
    closing: bool,
    next_seq: u64,
    inflight: VecDeque<(u64, usize)>,
    inflight_bytes: usize,
    last_ack: Instant,
    dropped_pending: u64,
}

impl FlowState {
    pub fn new(cfg: FlowConfig, now: Instant) -> Self {
        Self {
            buffer: VecDeque::with_capacity(cfg.max_batch),
            cfg,
            subscribed: false,
            closing: false,
            next_seq: 0,
            inflight: VecDeque::new(),
            inflight_bytes: 0,
            last_ack: now,
            dropped_pending: 0,
        }
    }

    pub fn config(&self) -> &FlowConfig {
        &self.cfg
    }

    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn inflight_bytes(&self) -> usize {
        self.inflight_bytes
    }

    /// 有在途数据且超过失联阈值没有 ack。
    pub fn is_stale(&self, now: Instant) -> bool {
        self.inflight_bytes > 0 && now.duration_since(self.last_ack) >= self.cfg.stale_after
    }

    /// 会话正在结束：读取方不再等待，持续读空管道。
    pub fn set_closing(&mut self) {
        self.closing = true;
    }

    fn reader_should_block(&self, now: Instant) -> bool {
        self.subscribed && !self.closing && !self.is_stale(now)
    }

    /// 把读取到的数据写入缓冲。
    pub fn push(&mut self, data: &[u8], now: Instant) -> PushOutcome {
        let cap = self.cfg.buffer_capacity;
        if self.buffer.len() + data.len() <= cap {
            self.buffer.extend(data);
            return PushOutcome::Stored;
        }
        if self.reader_should_block(now) {
            return PushOutcome::MustWait;
        }
        // 单次数据比整个缓冲还大时只保留尾部。
        let skipped = data.len().saturating_sub(cap);
        let data = &data[skipped..];
        let overflow = (self.buffer.len() + data.len()).saturating_sub(cap);
        self.buffer.drain(..overflow);
        self.buffer.extend(data);
        let dropped = overflow + skipped;
        self.dropped_pending += dropped as u64;
        PushOutcome::DroppedOldest(dropped)
    }

    /// 取出下一帧：先发缺口通知，再按窗口取输出。无订阅时不发送。
    pub fn take_frame(&mut self, now: Instant) -> Option<PendingFrame> {
        if !self.subscribed {
            return None;
        }
        if self.dropped_pending > 0 {
            let payload = format!("{{\"droppedBytes\":{}}}", self.dropped_pending).into_bytes();
            self.dropped_pending = 0;
            return Some(PendingFrame { kind: FRAME_GAP, seq: self.alloc_seq(), payload });
        }
        if self.buffer.is_empty() || self.inflight_bytes >= self.cfg.window {
            return None;
        }
        let n = self.buffer.len().min(self.cfg.max_batch);
        let payload: Vec<u8> = self.buffer.drain(..n).collect();
        let seq = self.alloc_seq();
        if self.inflight_bytes == 0 {
            self.last_ack = now;
        }
        self.inflight.push_back((seq, n));
        self.inflight_bytes += n;
        Some(PendingFrame { kind: FRAME_OUTPUT, seq, payload })
    }

    /// 分配一个序列号，用于状态帧、错误帧等非输出帧。
    pub fn alloc_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    /// 前端确认已处理到 `seq`（含）。
    pub fn ack(&mut self, seq: u64, now: Instant) {
        while let Some(&(s, n)) = self.inflight.front() {
            if s > seq {
                break;
            }
            self.inflight.pop_front();
            self.inflight_bytes -= n;
        }
        self.last_ack = now;
    }

    /// 建立新订阅：丢弃未发送的缓冲与在途记录，只推后续输出。缺口计数保留，下一帧先发缺口。
    pub fn subscribe(&mut self, now: Instant) {
        self.subscribed = true;
        self.buffer.clear();
        self.inflight.clear();
        self.inflight_bytes = 0;
        self.last_ack = now;
    }

    /// 订阅失效（例如发送失败）。
    pub fn unsubscribe(&mut self) {
        self.subscribed = false;
        self.inflight.clear();
        self.inflight_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_cfg() -> FlowConfig {
        FlowConfig {
            buffer_capacity: 16,
            window: 8,
            stale_after: Duration::from_secs(5),
            max_batch: 4,
        }
    }

    #[test]
    fn frame_layout() {
        let frame = encode_frame(FRAME_OUTPUT, 7, b"hi");
        assert_eq!(frame[0], FRAME_OUTPUT);
        assert_eq!(u64::from_le_bytes(frame[1..9].try_into().unwrap()), 7);
        assert_eq!(&frame[FRAME_HEADER_LEN..], b"hi");
    }

    #[test]
    fn window_limits_inflight_and_ack_releases() {
        let now = Instant::now();
        let mut flow = FlowState::new(small_cfg(), now);
        flow.subscribe(now);
        assert_eq!(flow.push(b"abcdefghijkl", now), PushOutcome::Stored);

        let f1 = flow.take_frame(now).unwrap();
        let f2 = flow.take_frame(now).unwrap();
        assert_eq!(f1.payload, b"abcd");
        assert_eq!(f2.payload, b"efgh");
        assert_eq!(flow.inflight_bytes(), 8);
        // 窗口已满。
        assert!(flow.take_frame(now).is_none());

        flow.ack(f1.seq, now);
        assert_eq!(flow.inflight_bytes(), 4);
        assert_eq!(flow.take_frame(now).unwrap().payload, b"ijkl");
    }

    #[test]
    fn full_buffer_blocks_reader_when_subscribed() {
        let now = Instant::now();
        let mut flow = FlowState::new(small_cfg(), now);
        flow.subscribe(now);
        assert_eq!(flow.push(&[1; 16], now), PushOutcome::Stored);
        assert_eq!(flow.push(&[2; 1], now), PushOutcome::MustWait);
    }

    #[test]
    fn unsubscribed_drops_oldest_and_reports_gap_on_subscribe() {
        let now = Instant::now();
        let mut flow = FlowState::new(small_cfg(), now);
        assert_eq!(flow.push(&[1; 12], now), PushOutcome::Stored);
        assert_eq!(flow.push(&[2; 10], now), PushOutcome::DroppedOldest(6));
        assert_eq!(flow.buffered_len(), 16);
        // 无订阅不发送。
        assert!(flow.take_frame(now).is_none());

        flow.subscribe(now);
        let gap = flow.take_frame(now).unwrap();
        assert_eq!(gap.kind, FRAME_GAP);
        assert_eq!(gap.payload, b"{\"droppedBytes\":6}");
        // 订阅时丢弃了未发送缓冲。
        assert!(flow.take_frame(now).is_none());
        assert_eq!(flow.push(b"xy", now), PushOutcome::Stored);
        assert_eq!(flow.take_frame(now).unwrap().payload, b"xy");
    }

    #[test]
    fn stale_subscription_stops_backpressure() {
        let start = Instant::now();
        let mut flow = FlowState::new(small_cfg(), start);
        flow.subscribe(start);
        flow.push(&[1; 16], start);
        flow.take_frame(start).unwrap();
        // 缓冲还有 12 字节，再写 8 字节会溢出。
        assert_eq!(flow.push(&[2; 8], start), PushOutcome::MustWait);
        let later = start + Duration::from_secs(6);
        assert!(flow.is_stale(later));
        assert_eq!(flow.push(&[2; 8], later), PushOutcome::DroppedOldest(4));
        // 恢复 ack 后重新进入背压。
        flow.ack(u64::MAX, later);
        assert!(!flow.is_stale(later));
    }

    #[test]
    fn closing_never_blocks_reader() {
        let now = Instant::now();
        let mut flow = FlowState::new(small_cfg(), now);
        flow.subscribe(now);
        flow.push(&[1; 16], now);
        flow.set_closing();
        assert_eq!(flow.push(&[2; 4], now), PushOutcome::DroppedOldest(4));
    }

    #[test]
    fn oversized_chunk_keeps_tail() {
        let now = Instant::now();
        let mut flow = FlowState::new(small_cfg(), now);
        let data: Vec<u8> = (0..20).collect();
        assert_eq!(flow.push(&data, now), PushOutcome::DroppedOldest(4));
        assert_eq!(flow.buffered_len(), 16);
    }

    #[test]
    fn seq_increases_across_frame_kinds() {
        let now = Instant::now();
        let mut flow = FlowState::new(small_cfg(), now);
        flow.subscribe(now);
        let s0 = flow.alloc_seq();
        flow.push(b"a", now);
        let f = flow.take_frame(now).unwrap();
        assert_eq!(f.seq, s0 + 1);
    }
}
