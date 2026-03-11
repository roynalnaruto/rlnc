//! Node configuration loaded from YAML files.

use serde::Deserialize;

/// Default block size in bytes (~2 MB, similar to Ethereum block).
pub const DEFAULT_BLOCK_SIZE: usize = 2_000_000;
/// Default number of blocks to propagate.
pub const DEFAULT_NUM_BLOCKS: u64 = 10;
/// Default interval between blocks in seconds.
pub const DEFAULT_BLOCK_INTERVAL: u64 = 5;
/// Default startup delay before first proposal in seconds.
pub const DEFAULT_STARTUP_DELAY: u64 = 10;
/// Default mesh degree (number of peers to forward to).
pub const DEFAULT_MESH_DEGREE: usize = 8;
/// Default log level.
pub const DEFAULT_LOG_LEVEL: &str = "info";
/// Default to local P2P configuration.
pub const DEFAULT_LOCAL: bool = true;
/// Default node storage directory.
pub const DEFAULT_DIRECTORY: &str = "./data/node";
/// Default RNG seed for reproducible runs.
pub const DEFAULT_SEED: u64 = 42;

// Serde default functions — thin wrappers over the constants above.
const fn default_block_size() -> usize { DEFAULT_BLOCK_SIZE }
const fn default_num_blocks() -> u64 { DEFAULT_NUM_BLOCKS }
const fn default_block_interval() -> u64 { DEFAULT_BLOCK_INTERVAL }
const fn default_startup_delay() -> u64 { DEFAULT_STARTUP_DELAY }
const fn default_mesh_degree() -> usize { DEFAULT_MESH_DEGREE }
fn default_log_level() -> String { DEFAULT_LOG_LEVEL.into() }
const fn default_local() -> bool { DEFAULT_LOCAL }
fn default_directory() -> String { DEFAULT_DIRECTORY.into() }
const fn default_seed() -> u64 { DEFAULT_SEED }

/// Per-node configuration, loaded from a YAML file.
#[derive(Clone, Debug, Deserialize)]
pub struct NodeConfig {
    /// Ed25519 private key hex (for P2P identity).
    pub private_key: String,
    /// P2P listen port.
    pub port: u16,
    /// Prometheus metrics port.
    pub metrics_port: u16,
    /// Strategy: baseline | pedersen | bfkw.
    pub strategy: String,
    /// Role: proposer | receiver.
    pub role: String,

    /// BLS12-381 secret scalar hex (only for proposer).
    #[serde(default)]
    pub proposer_signing_key: Option<String>,

    /// Proposer's BLS12-381 G1 public key hex (all nodes know this).
    pub proposer_public_key: String,

    /// Block size in bytes (proposer only).
    #[serde(default = "default_block_size")]
    pub block_size: usize,
    /// Number of blocks to propagate (proposer only).
    #[serde(default = "default_num_blocks")]
    pub num_blocks: u64,
    /// Seconds between blocks (proposer only).
    #[serde(default = "default_block_interval")]
    pub block_interval_secs: u64,

    /// Wait before first proposal (seconds).
    #[serde(default = "default_startup_delay")]
    pub startup_delay_secs: u64,
    /// Peers to forward to.
    #[serde(default = "default_mesh_degree")]
    pub mesh_degree: usize,
    /// Log level.
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Use `Config::local()` vs `Config::recommended()`.
    #[serde(default = "default_local")]
    pub local: bool,
    /// Storage directory.
    #[serde(default = "default_directory")]
    pub directory: String,
    /// RNG seed.
    #[serde(default = "default_seed")]
    pub seed: u64,
}

/// Peer address entry in `peers.yaml`.
#[derive(Clone, Debug, Deserialize)]
pub struct PeerEntry {
    /// Ed25519 public key hex.
    pub public_key: String,
    /// IP:port address.
    pub address: String,
}

/// CLI arguments.
#[derive(clap::Parser, Debug)]
#[command(name = "node", about = "RLNC P2P node")]
pub struct CliArgs {
    /// Path to the node config YAML file.
    #[arg(long)]
    pub config: String,
    /// Path to the peers YAML file.
    #[arg(long)]
    pub peers: String,
}

impl NodeConfig {
    /// Load from a YAML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: &str) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read config: {e}"))?;
        serde_yaml::from_str(&contents)
            .map_err(|e| format!("failed to parse config: {e}"))
    }
}

/// Load peer entries from a YAML file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_peers(path: &str) -> Result<Vec<PeerEntry>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read peers file: {e}"))?;
    serde_yaml::from_str(&contents)
        .map_err(|e| format!("failed to parse peers file: {e}"))
}

/// Compute chunk dimension $m$ from block size and $N$.
///
/// Formula: `m = ceil((block_size + 100) / (31 * N))`
pub const fn compute_m(block_size: usize, n: usize) -> usize {
    (block_size + 100).div_ceil(31 * n)
}
