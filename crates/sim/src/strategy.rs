//! Network-layer wrapper around a coding [`Strategy`].
//!
//! [`PropagationMechanism`] holds simulation parameters (mesh degree,
//! latency, bandwidth) and delegates all encoding/decoding/verification
//! to the inner strategy.

use core::time::Duration;

use p2p_primitives_types::Block;
use p2p_strategy_core::Strategy;
use rand_chacha::ChaCha20Rng;

pub use p2p_strategy_core::ForwardCondition;

use crate::engine;
use crate::metrics::SimMetrics;

/// Network-layer wrapper around a coding [`Strategy`].
///
/// Holds simulation parameters (mesh degree, latency, bandwidth) and
/// delegates all encoding/decoding/verification to the inner strategy.
pub struct PropagationMechanism<S> {
    num_nodes: usize,
    mesh_degree: usize,
    hop_latency: Duration,
    bandwidth: f64,
    strategy: S,
}

impl<S> PropagationMechanism<S> {
    /// Create a new mechanism wrapping the given strategy.
    pub const fn new(
        num_nodes: usize,
        mesh_degree: usize,
        hop_latency: Duration,
        bandwidth: f64,
        strategy: S,
    ) -> Self {
        Self {
            num_nodes,
            mesh_degree,
            hop_latency,
            bandwidth,
            strategy,
        }
    }

    /// Number of nodes in the network.
    pub const fn num_nodes(&self) -> usize {
        self.num_nodes
    }

    /// Mesh degree for this strategy.
    pub const fn mesh_degree(&self) -> usize {
        self.mesh_degree
    }

    /// Per-hop network latency.
    pub const fn hop_latency(&self) -> Duration {
        self.hop_latency
    }

    /// Per-link upload bandwidth (bytes/sec).
    pub const fn bandwidth(&self) -> f64 {
        self.bandwidth
    }

    /// Borrow the inner strategy.
    pub const fn strategy(&self) -> &S {
        &self.strategy
    }
}

impl<S: Strategy> PropagationMechanism<S> {
    /// Run the full simulation for one block propagation.
    pub fn simulate(&self, block: &Block, rng: &mut ChaCha20Rng) -> SimMetrics {
        engine::run(self, block, rng)
    }
}
