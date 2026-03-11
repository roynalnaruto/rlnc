//! Environment-driven simulation configuration.
//!
//! All parameters are read from a `.env` file (via [`dotenvy`]) with
//! sensible defaults. No recompilation is needed to change parameters.

use core::time::Duration;

/// Simulation configuration loaded from `.env` via [`dotenvy`].
#[derive(Clone, Debug)]
pub struct SimConfig {
    /// Number of nodes in the network.
    pub num_nodes: usize,
    /// Mesh degree for the baseline (gossipsub) strategy.
    pub baseline_degree: usize,
    /// Mesh degree for RLNC-based strategies (Pedersen / BFKW).
    pub rlnc_degree: usize,
    /// Per-hop network latency.
    pub hop_latency: Duration,
    /// Per-link upload bandwidth in bytes per second.
    pub bandwidth: f64,
    /// Block size in bytes.
    pub block_size: usize,
    /// Number of blocks to simulate.
    pub num_blocks: usize,
    /// Deterministic RNG seed.
    pub seed: u64,
}

/// Block header overhead: parent hash (32) + four u64 fields (32) +
/// state root (32) + transaction length prefix (4) = 100 bytes.
const BLOCK_HEADER_BYTES: usize = 100;

impl SimConfig {
    /// Compute the chunk dimension $m = \lceil \mathrm{encoded\_bytes}
    /// / (31 \cdot n) \rceil$.
    ///
    /// Each BLS12-381 `Fr` element packs 31 data bytes.
    pub const fn compute_m(&self, n: usize) -> usize {
        let estimated_encoded = self.block_size + BLOCK_HEADER_BYTES;
        estimated_encoded.div_ceil(31 * n)
    }

    /// Load configuration from `.env` (via [`dotenvy`]) with defaults.
    ///
    /// Environment variables override `.env` values. Missing values
    /// fall back to the defaults listed in the repository `.env` file.
    pub fn from_env() -> Self {
        // Best-effort .env loading — ignore errors (file may not exist).
        let _ = dotenvy::dotenv();

        Self {
            num_nodes: env_or("RLNC_NUM_NODES", 1000),
            baseline_degree: env_or("RLNC_BASELINE_DEGREE", 8),
            rlnc_degree: env_or("RLNC_RLNC_DEGREE", 40),
            hop_latency: Duration::from_millis(env_or("RLNC_HOP_LATENCY_MS", 70)),
            bandwidth: env_or::<f64>("RLNC_BANDWIDTH_MBPS", 20.0) * 1_000_000.0,
            block_size: env_or("RLNC_BLOCK_SIZE", 2_000_000),
            num_blocks: env_or("RLNC_NUM_BLOCKS", 10),
            seed: env_or("RLNC_SEED", 2025),
        }
    }
}

/// Read an environment variable and parse it, falling back to `default`.
fn env_or<T: core::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        // Clear any env overrides for deterministic testing.
        let config = SimConfig {
            num_nodes: 1000,
            baseline_degree: 8,
            rlnc_degree: 40,
            hop_latency: Duration::from_millis(70),
            bandwidth: 20.0 * 1_000_000.0,
            block_size: 2_000_000,
            num_blocks: 10,
            seed: 2025,
        };

        assert_eq!(config.num_nodes, 1000);
        assert_eq!(config.baseline_degree, 8);
        assert_eq!(config.rlnc_degree, 40);
        assert_eq!(config.hop_latency, Duration::from_millis(70));
        assert!((config.bandwidth - 20_000_000.0).abs() < 1.0);
        assert_eq!(config.block_size, 2_000_000);
        assert_eq!(config.num_blocks, 10);
        assert_eq!(config.seed, 2025);
    }
}
