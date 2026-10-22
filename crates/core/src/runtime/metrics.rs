//! Lightweight in-process counters.
//!
//! The base runtime does not ship a metrics exporter. Counters are logged at shutdown and are
//! cheap enough to increment on every message, giving later instrumentation a stable foundation.

use std::sync::atomic::AtomicU64;

#[derive(Debug, Default)]
pub struct RuntimeMetrics {
    pub inbound_received: AtomicU64,
    pub inbound_dropped: AtomicU64,
    pub handled_ok: AtomicU64,
    pub handled_failed: AtomicU64,
    pub outbound_sent: AtomicU64,
    pub outbound_failed: AtomicU64,
    pub task_failures: AtomicU64,
}

impl RuntimeMetrics {
    pub fn snapshot(&self) -> MetricsSnapshot {
        use std::sync::atomic::Ordering::Relaxed;

        MetricsSnapshot {
            inbound_received: self.inbound_received.load(Relaxed),
            inbound_dropped: self.inbound_dropped.load(Relaxed),
            handled_ok: self.handled_ok.load(Relaxed),
            handled_failed: self.handled_failed.load(Relaxed),
            outbound_sent: self.outbound_sent.load(Relaxed),
            outbound_failed: self.outbound_failed.load(Relaxed),
            task_failures: self.task_failures.load(Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub inbound_received: u64,
    pub inbound_dropped: u64,
    pub handled_ok: u64,
    pub handled_failed: u64,
    pub outbound_sent: u64,
    pub outbound_failed: u64,
    pub task_failures: u64,
}
