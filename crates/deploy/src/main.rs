//! Deploy configuration generator for the RLNC P2P network.
//!
//! Generates per-node YAML configs, peer files, and optionally
//! `commonware-deployer` configs for AWS deployment.

use clap::{Parser, Subcommand};
use commonware_codec::Write as CodecWrite;
use commonware_cryptography::{Signer, ed25519};
use commonware_math::algebra::Random;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// Deploy CLI.
#[derive(Parser)]
#[command(name = "deploy", about = "RLNC P2P deployment config generator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Generate deployment configs.
    Generate {
        #[command(subcommand)]
        mode: GenerateMode,
    },
}

/// Generation mode.
#[derive(Subcommand)]
enum GenerateMode {
    /// Generate configs for local multi-process testing.
    Local {
        /// Number of nodes.
        #[arg(long, default_value = "5")]
        nodes: usize,
        /// Strategy: baseline | pedersen | bfkw.
        #[arg(long, default_value = "pedersen")]
        strategy: String,
        /// Block size in bytes.
        #[arg(long, default_value = "2000000")]
        block_size: usize,
        /// Mesh degree.
        #[arg(long, default_value = "8")]
        mesh_degree: usize,
        /// Number of blocks.
        #[arg(long, default_value = "5")]
        num_blocks: u64,
        /// Starting port.
        #[arg(long, default_value = "3000")]
        start_port: u16,
        /// Output directory.
        #[arg(long, default_value = "/tmp/rlnc")]
        output: String,
        /// RNG seed for key generation.
        #[arg(long, default_value = "42")]
        seed: u64,
    },
}

/// Node config for serialization.
#[derive(Serialize)]
struct NodeConfigYaml {
    private_key: String,
    port: u16,
    metrics_port: u16,
    strategy: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    proposer_signing_key: Option<String>,
    proposer_public_key: String,
    block_size: usize,
    num_blocks: u64,
    block_interval_secs: u64,
    startup_delay_secs: u64,
    mesh_degree: usize,
    log_level: String,
    local: bool,
    directory: String,
    seed: u64,
}

/// Peer entry for serialization.
#[derive(Serialize)]
struct PeerEntryYaml {
    public_key: String,
    address: String,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate { mode } => match mode {
            GenerateMode::Local {
                nodes,
                strategy,
                block_size,
                mesh_degree,
                num_blocks,
                start_port,
                output,
                seed,
            } => {
                generate_local(
                    nodes,
                    &strategy,
                    block_size,
                    mesh_degree,
                    num_blocks,
                    start_port,
                    &output,
                    seed,
                );
            }
        },
    }
}

#[allow(clippy::too_many_arguments, clippy::cast_possible_truncation)]
fn generate_local(
    nodes: usize,
    strategy: &str,
    block_size: usize,
    mesh_degree: usize,
    num_blocks: u64,
    start_port: u16,
    output: &str,
    seed: u64,
) {
    // Create output directory.
    fs::create_dir_all(output).expect("failed to create output directory");

    // Generate ed25519 keypairs.
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut keys: Vec<(ed25519::PrivateKey, ed25519::PublicKey)> = (0..nodes)
        .map(|_| {
            let sk = ed25519::PrivateKey::random(&mut rng);
            let pk = sk.public_key();
            (sk, pk)
        })
        .collect();

    // Sort by public key for determinism.
    keys.sort_by(|a, b| a.1.as_ref().cmp(b.1.as_ref()));

    // Generate BLS12-381 keypair for the proposer (node 0).
    // We use a deterministic seed derived from the main seed.
    let proposer_seed: [u8; 32] = {
        let mut s = [0u8; 32];
        s[..8].copy_from_slice(&(seed + 1000).to_le_bytes());
        s
    };
    let proposer_signing_key_hex = format!("0x{}", hex::encode(proposer_seed));

    // Compute proposer's BLS public key by generating from the same seed.
    let (proposer_public_key_hex,) = {
        use commonware_cryptography::bls12381;
        let mut bls_rng = ChaCha20Rng::from_seed(proposer_seed);
        let bls_sk = bls12381::PrivateKey::random(&mut bls_rng);
        let pk = bls_sk.public_key();
        (format!("0x{}", hex::encode(AsRef::<[u8]>::as_ref(&pk))),)
    };
    // Generate peers.yaml.
    let mut peer_entries = Vec::new();
    for (i, (_, pk)) in keys.iter().enumerate() {
        let port = start_port + (i as u16) * 2;
        peer_entries.push(PeerEntryYaml {
            public_key: format!("0x{}", hex::encode(pk.as_ref())),
            address: format!("127.0.0.1:{port}"),
        });
    }

    let peers_path = Path::new(output).join("peers.yaml");
    let peers_yaml =
        serde_yaml::to_string(&peer_entries).expect("failed to serialize peers");
    fs::write(&peers_path, &peers_yaml).expect("failed to write peers.yaml");

    // Generate per-node configs.
    for (i, (sk, _pk)) in keys.iter().enumerate() {
        let port = start_port + (i as u16) * 2;
        let metrics_port = start_port + (i as u16) * 2 + 1;
        let role = if i == 0 { "proposer" } else { "receiver" };

        let mut sk_buf = Vec::new();
        sk.write(&mut sk_buf);
        let node_config = NodeConfigYaml {
            private_key: format!("0x{}", hex::encode(&sk_buf)),
            port,
            metrics_port,
            strategy: strategy.to_string(),
            role: role.to_string(),
            proposer_signing_key: if i == 0 {
                Some(proposer_signing_key_hex.clone())
            } else {
                None
            },
            proposer_public_key: proposer_public_key_hex.clone(),
            block_size,
            num_blocks,
            block_interval_secs: 5,
            startup_delay_secs: 10,
            mesh_degree,
            log_level: "info".to_string(),
            local: true,
            directory: format!("{output}/data/node-{i}"),
            seed: seed + i as u64,
        };

        let config_path = Path::new(output).join(format!("node-{i}.yaml"));
        let config_yaml = serde_yaml::to_string(&node_config)
            .expect("failed to serialize node config");
        fs::write(&config_path, &config_yaml)
            .expect("failed to write node config");
    }

    // Print start commands.
    println!("Generated configs in {output}/");
    println!();
    println!("Start commands:");
    for i in 0..nodes {
        println!(
            "  cargo run --release --bin node -- --peers {output}/peers.yaml --config {output}/node-{i}.yaml"
        );
    }
}
