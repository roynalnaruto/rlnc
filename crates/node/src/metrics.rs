//! Prometheus metrics for the RLNC node.
//!
//! Follows the same pattern as commonware-p2p's metrics modules:
//! a `Metrics` struct with an `init()` constructor that registers
//! each metric with the runtime context.

use commonware_runtime::{Metrics as RuntimeMetrics, telemetry::metrics::histogram};
use prometheus_client::metrics::{counter::Counter, gauge::Gauge, histogram::Histogram};

/// Per-node Prometheus metrics for RLNC block propagation.
#[derive(Clone)]
pub struct Metrics {
    // -- Bandwidth --
    /// Total bytes uploaded (announcements + data packets).
    pub bytes_uploaded: Counter,
    /// Total bytes downloaded (announcements + data packets).
    pub bytes_downloaded: Counter,

    // -- Announcements --
    /// Total announcements sent (including re-broadcasts).
    pub announcements_sent: Counter,
    /// Total announcements received.
    pub announcements_received: Counter,

    // -- Data packets --
    /// Total data packets sent (initial + forwarded).
    pub packets_sent: Counter,
    /// Total data packets received.
    pub packets_received: Counter,
    /// Packets that increased decoder rank.
    pub packets_useful: Counter,
    /// Packets that were linearly dependent (redundant).
    pub packets_redundant: Counter,
    /// Packets that failed integrity verification.
    pub packets_failed: Counter,

    // -- Blocks --
    /// Total blocks proposed by this node.
    pub blocks_proposed: Counter,
    /// Total blocks successfully decoded by this node.
    pub blocks_decoded: Counter,
    /// Histogram of block decode durations (seconds).
    pub decode_duration_seconds: Histogram,

    // -- Liveness --
    /// Current block number being processed.
    pub current_block: Gauge,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            bytes_uploaded: Counter::default(),
            bytes_downloaded: Counter::default(),
            announcements_sent: Counter::default(),
            announcements_received: Counter::default(),
            packets_sent: Counter::default(),
            packets_received: Counter::default(),
            packets_useful: Counter::default(),
            packets_redundant: Counter::default(),
            packets_failed: Counter::default(),
            blocks_proposed: Counter::default(),
            blocks_decoded: Counter::default(),
            decode_duration_seconds: Histogram::new(histogram::Buckets::NETWORK),
            current_block: Gauge::default(),
        }
    }
}

impl Metrics {
    /// Create and return a new set of metrics, registered with the given
    /// context.
    #[allow(clippy::needless_pass_by_value)]
    pub fn init<E: RuntimeMetrics>(context: E) -> Self {
        let metrics = Self::default();
        context.register(
            "bytes_uploaded",
            "Total bytes uploaded (announcements + data packets)",
            metrics.bytes_uploaded.clone(),
        );
        context.register(
            "bytes_downloaded",
            "Total bytes downloaded (announcements + data packets)",
            metrics.bytes_downloaded.clone(),
        );
        context.register(
            "announcements_sent",
            "Total announcements sent (including re-broadcasts)",
            metrics.announcements_sent.clone(),
        );
        context.register(
            "announcements_received",
            "Total announcements received",
            metrics.announcements_received.clone(),
        );
        context.register(
            "packets_sent",
            "Total data packets sent (initial + forwarded)",
            metrics.packets_sent.clone(),
        );
        context.register(
            "packets_received",
            "Total data packets received",
            metrics.packets_received.clone(),
        );
        context.register(
            "packets_useful",
            "Packets that increased decoder rank",
            metrics.packets_useful.clone(),
        );
        context.register(
            "packets_redundant",
            "Packets that were linearly dependent (redundant)",
            metrics.packets_redundant.clone(),
        );
        context.register(
            "packets_failed",
            "Packets that failed integrity verification",
            metrics.packets_failed.clone(),
        );
        context.register(
            "blocks_proposed",
            "Total blocks proposed by this node",
            metrics.blocks_proposed.clone(),
        );
        context.register(
            "blocks_decoded",
            "Total blocks successfully decoded by this node",
            metrics.blocks_decoded.clone(),
        );
        context.register(
            "decode_duration_seconds",
            "Histogram of block decode durations in seconds",
            metrics.decode_duration_seconds.clone(),
        );
        context.register(
            "current_block",
            "Current block number being processed",
            metrics.current_block.clone(),
        );
        metrics
    }
}
