//! Multi-block comparison binary.
//!
//! Runs all three propagation strategies over a configurable number
//! of randomly generated blocks and prints a summary comparison table.

use alloy_primitives::B256;
use commonware_cryptography::bls12381;
use commonware_cryptography::bls12381::primitives::group::G1;
use p2p_baseline::BaselineStrategy;
use p2p_bfkw::{BfkwStrategy, Bls12381};
use p2p_pedersen::PedersenStrategy;
use p2p_primitives_types::Block;
use p2p_sim::PropagationMechanism;
use p2p_sim::config::SimConfig;
use p2p_sim::metrics::{CumulativeMetrics, compare};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::thread;
use tracing::info;

/// Number of original chunks per block (fixed).
const N: usize = 10;

/// Generate a random block of approximately `size_bytes` of
/// transaction data.
fn generate_random_block(size_bytes: usize, rng: &mut ChaCha20Rng) -> Block {
    let transactions: Vec<u8> = (0..size_bytes).map(|_| rng.r#gen()).collect();
    let mut parent = [0u8; 32];
    rng.fill(&mut parent);
    let mut state_root = [0u8; 32];
    rng.fill(&mut state_root);
    Block::new(
        B256::from(parent),
        rng.r#gen(),
        1_700_000_000,
        15_000_000,
        30_000_000,
        B256::from(state_root),
        transactions,
    )
}

/// Run the simulation with computed m and fixed N.
fn run_simulation(config: &SimConfig, m: usize) {
    let mut rng = ChaCha20Rng::seed_from_u64(config.seed);

    // Create strategies wrapped in propagation mechanisms.
    let baseline = PropagationMechanism::new(
        config.num_nodes,
        config.baseline_degree,
        config.hop_latency,
        config.bandwidth,
        BaselineStrategy,
    );
    let pedersen = PropagationMechanism::new(
        config.num_nodes,
        config.rlnc_degree,
        config.hop_latency,
        config.bandwidth,
        PedersenStrategy::<G1, bls12381::PrivateKey, N>::new(m, &mut rng),
    );
    let bfkw = PropagationMechanism::new(
        config.num_nodes,
        config.rlnc_degree,
        config.hop_latency,
        config.bandwidth,
        BfkwStrategy::<Bls12381, N>::new(m, &mut rng),
    );

    // Create cumulative metrics accumulators.
    let mut metrics_baseline = CumulativeMetrics::new("Baseline");
    let mut metrics_pedersen = CumulativeMetrics::new("Pedersen");
    let mut metrics_bfkw = CumulativeMetrics::new("BFKW");

    // Generate and propagate blocks (strategies run in parallel).
    for block_num in 0..config.num_blocks {
        let block = generate_random_block(config.block_size, &mut rng);

        // Derive independent RNGs for each strategy to preserve
        // determinism while enabling parallel execution.
        let mut rng_baseline = ChaCha20Rng::seed_from_u64(rng.r#gen());
        let mut rng_pedersen = ChaCha20Rng::seed_from_u64(rng.r#gen());
        let mut rng_bfkw = ChaCha20Rng::seed_from_u64(rng.r#gen());

        let (r1, r2, r3) = thread::scope(|s| {
            let h1 = s.spawn(|| baseline.simulate(&block, &mut rng_baseline));
            let h2 = s.spawn(|| pedersen.simulate(&block, &mut rng_pedersen));
            let r3 = bfkw.simulate(&block, &mut rng_bfkw);
            let r1 = h1.join().expect("baseline thread panicked");
            let r2 = h2.join().expect("pedersen thread panicked");
            (r1, r2, r3)
        });

        metrics_baseline.accumulate(r1);
        metrics_pedersen.accumulate(r2);
        metrics_bfkw.accumulate(r3);

        info!(
            block = block_num + 1,
            total = config.num_blocks,
            "block propagated"
        );
    }

    // Display comparison.
    compare(&[&metrics_baseline, &metrics_pedersen, &metrics_bfkw]);
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = SimConfig::from_env();
    let m = config.compute_m(N);

    info!(
        nodes = config.num_nodes,
        blocks = config.num_blocks,
        block_size = config.block_size,
        baseline_degree = config.baseline_degree,
        rlnc_degree = config.rlnc_degree,
        hop_latency_ms = config.hop_latency.as_millis(),
        bandwidth_mbps = config.bandwidth / 1_000_000.0,
        n = N,
        m = m,
        "simulation starting"
    );

    run_simulation(&config, m);
}
