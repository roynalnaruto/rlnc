//! Discrete-event simulation engine.
//!
//! Drives block propagation over a random mesh topology. The engine
//! is generic over [`Strategy`] and reads network parameters (latency,
//! bandwidth, mesh degree) from the [`PropagationMechanism`] wrapper.

use core::cmp::Reverse;
use core::iter;
use core::time::Duration;
use std::collections::BinaryHeap;

use p2p_strategy_core::{ForwardCondition, Strategy};
use rand_chacha::ChaCha20Rng;
use tracing::{debug, trace};

use crate::metrics::SimMetrics;
use crate::strategy::PropagationMechanism;
use crate::topology::{NodeId, Topology};

/// A scheduled event in the simulation.
struct Event<P> {
    /// When this event fires.
    time: Duration,
    /// Receiver node.
    to: NodeId,
    /// The packet being transmitted.
    packet: P,
}

impl<P> PartialEq for Event<P> {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl<P> Eq for Event<P> {}

impl<P> PartialOrd for Event<P> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<P> Ord for Event<P> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

/// Compute transmission delay: latency + serialization time.
#[allow(clippy::cast_precision_loss)]
fn transmission_delay(hop_latency: Duration, packet_bytes: usize, bandwidth: f64) -> Duration {
    let serialization_secs = packet_bytes as f64 / bandwidth;
    hop_latency + Duration::from_secs_f64(serialization_secs)
}

/// Percentile index into a sorted slice (0-based).
#[allow(clippy::cast_precision_loss)]
fn percentile_idx(len: usize, pct: f64) -> usize {
    if len == 0 {
        return 0;
    }
    let idx = (len - 1) as f64 * pct;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let idx = idx as usize;
    idx.min(len - 1)
}

/// Run the discrete-event simulation for one block propagation.
///
/// This is the core simulation loop called by
/// [`PropagationMechanism::simulate`].
///
/// # Panics
///
/// Panics if `mechanism.num_nodes()` is zero.
#[allow(clippy::cast_precision_loss)]
pub fn run<S: Strategy>(
    mechanism: &PropagationMechanism<S>,
    block: &p2p_primitives_types::Block,
    rng: &mut ChaCha20Rng,
) -> SimMetrics {
    let strategy = mechanism.strategy();
    let n = mechanism.num_nodes();
    assert!(n > 0, "num_nodes must be > 0");
    let degree = mechanism.mesh_degree();

    // 1. Build topology.
    let topo = Topology::random_mesh(n, degree, rng);

    // 2. Choose node 0 as the proposer.
    let proposer: NodeId = 0;
    let proposer_neighbors = topo.neighbors(proposer);
    let (proposer_state, proposer_packets, byte_len) =
        strategy.init_proposer(block, proposer_neighbors.len(), rng);

    // Node states: proposer first, then receivers.
    let mut states: Vec<S::NodeState> = iter::once(proposer_state)
        .chain(iter::repeat_with(|| strategy.init_receiver()).take(n - 1))
        .collect();

    // Tracking arrays.
    let mut upload: Vec<u64> = vec![0; n];
    let mut download: Vec<u64> = vec![0; n];
    let mut decode_times: Vec<Option<Duration>> = vec![None; n];
    let mut has_forwarded: Vec<bool> = vec![false; n];
    let mut total_packets: u64 = 0;
    let mut useful_packets: u64 = 0;
    let mut redundant_packets: u64 = 0;

    // Proposer is already decoded at time 0.
    decode_times[proposer] = Some(Duration::ZERO);

    // 3. Schedule proposer's initial transmissions.
    let mut event_queue: BinaryHeap<Reverse<Event<S::Packet>>> = BinaryHeap::new();

    for (peer, packet) in proposer_neighbors.iter().copied().zip(proposer_packets) {
        let pkt_size = strategy.packet_size(&packet);
        let delay = transmission_delay(mechanism.hop_latency(), pkt_size, mechanism.bandwidth());

        upload[proposer] += pkt_size as u64;
        download[peer] += pkt_size as u64;
        total_packets += 1;

        event_queue.push(Reverse(Event {
            time: delay,
            to: peer,
            packet,
        }));
    }

    // 4. Process events.
    while let Some(Reverse(event)) = event_queue.pop() {
        let to = event.to;

        // Skip already-decoded nodes. This guarantees decode_times[to]
        // is None for all code below, so no is_none() guards are needed.
        if decode_times[to].is_some() {
            continue;
        }

        let state = &mut states[to];
        let rank_increased = match strategy.receive(state, &event.packet) {
            Ok(true) => {
                useful_packets += 1;
                trace!(node = to, "chunk accepted (rank increase)");
                true
            },
            Ok(false) => {
                redundant_packets += 1;
                trace!(node = to, "chunk redundant");
                false
            },
            Err(e) => {
                redundant_packets += 1;
                trace!(node = to, error = %e, "chunk verification failed");
                continue;
            },
        };

        let decodable = strategy.can_decode(state);

        // Check forwarding condition.
        let should_forward = match strategy.forward_condition() {
            ForwardCondition::AfterDecode => decodable,
            ForwardCondition::OneShot => rank_increased && !has_forwarded[to],
            ForwardCondition::UntilDecode => rank_increased,
        };

        if should_forward {
            has_forwarded[to] = true;
            let peers = topo.neighbors(to);
            trace!(node = to, num_peers = peers.len(), "forwarding");
            let packets = strategy.forward(state, peers.len(), rng);

            for (peer, packet) in peers.iter().copied().zip(packets) {
                let pkt_size = strategy.packet_size(&packet);
                let delay =
                    transmission_delay(mechanism.hop_latency(), pkt_size, mechanism.bandwidth());
                let arrival = event.time + delay;

                upload[to] += pkt_size as u64;
                download[peer] += pkt_size as u64;
                total_packets += 1;

                event_queue.push(Reverse(Event {
                    time: arrival,
                    to: peer,
                    packet,
                }));
            }
        }

        if decodable {
            decode_times[to] = Some(event.time);
            debug!(
                node = to,
                strategy = strategy.name(),
                time_ms = event.time.as_secs_f64() * 1000.0,
                "block decoded"
            );
        }
    }

    // 5. Compute metrics.
    let nodes_decoded = decode_times.iter().flatten().count();
    let total_bytes_sent: u64 = upload.iter().sum();
    let useful_bytes = byte_len as u64 * nodes_decoded as u64;
    let redundancy_ratio = if useful_bytes > 0 {
        total_bytes_sent as f64 / useful_bytes as f64
    } else {
        0.0
    };

    let mean_upload = total_bytes_sent as f64 / n as f64;
    let max_upload = upload.iter().copied().max().unwrap_or(0);

    // Propagation percentiles.
    let mut sorted_times: Vec<Duration> = decode_times.iter().copied().flatten().collect();
    sorted_times.sort_unstable();

    let propagation_time = sorted_times.last().copied().unwrap_or(Duration::ZERO);
    let p50_time = sorted_times
        .get(percentile_idx(sorted_times.len(), 0.5))
        .copied()
        .unwrap_or(Duration::ZERO);
    let p99_time = sorted_times
        .get(percentile_idx(sorted_times.len(), 0.99))
        .copied()
        .unwrap_or(Duration::ZERO);

    SimMetrics {
        strategy: strategy.name().to_string(),
        num_nodes: n,
        mesh_degree: degree,
        block_size: byte_len,
        total_bytes_sent,
        useful_bytes,
        redundancy_ratio,
        per_node_upload: upload,
        per_node_download: download,
        mean_upload,
        max_upload,
        decode_times,
        nodes_decoded,
        propagation_time,
        p50_time,
        p99_time,
        total_packets,
        useful_packets,
        redundant_packets,
    }
}
