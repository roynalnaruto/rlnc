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
use std::fmt::Write as _;
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
    /// Generate configs for Docker Compose multi-container testing.
    Docker {
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
        /// P2P port (same inside each container).
        #[arg(long, default_value = "3000")]
        p2p_port: u16,
        /// Metrics port (same inside each container).
        #[arg(long, default_value = "9090")]
        metrics_port: u16,
        /// Output directory.
        #[arg(long, default_value = "./docker")]
        output: String,
        /// RNG seed for key generation.
        #[arg(long, default_value = "42")]
        seed: u64,
        /// Seconds between blocks (proposer only).
        #[arg(long, default_value = "5")]
        block_interval_secs: u64,
        /// Startup delay before first proposal (seconds).
        #[arg(long, default_value = "10")]
        startup_delay_secs: u64,
        /// Include Prometheus container for metrics collection.
        #[arg(long)]
        monitoring: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    dialable_address: Option<String>,
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
            },
            GenerateMode::Docker {
                nodes,
                strategy,
                block_size,
                mesh_degree,
                num_blocks,
                p2p_port,
                metrics_port,
                output,
                seed,
                block_interval_secs,
                startup_delay_secs,
                monitoring,
            } => {
                generate_docker(
                    nodes,
                    &strategy,
                    block_size,
                    mesh_degree,
                    num_blocks,
                    p2p_port,
                    metrics_port,
                    &output,
                    seed,
                    block_interval_secs,
                    startup_delay_secs,
                    monitoring,
                );
            },
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
    let peers_yaml = serde_yaml::to_string(&peer_entries).expect("failed to serialize peers");
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
            dialable_address: None,
        };

        let config_path = Path::new(output).join(format!("node-{i}.yaml"));
        let config_yaml =
            serde_yaml::to_string(&node_config).expect("failed to serialize node config");
        fs::write(&config_path, &config_yaml).expect("failed to write node config");
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

#[allow(clippy::too_many_arguments, clippy::cast_possible_truncation)]
fn generate_docker(
    nodes: usize,
    strategy: &str,
    block_size: usize,
    mesh_degree: usize,
    num_blocks: u64,
    p2p_port: u16,
    metrics_port: u16,
    output: &str,
    seed: u64,
    block_interval_secs: u64,
    startup_delay_secs: u64,
    monitoring: bool,
) {
    fs::create_dir_all(output).expect("failed to create output directory");

    // Remove stale node configs from previous runs with more nodes.
    let out = Path::new(output);
    for entry in fs::read_dir(out).expect("failed to read output directory") {
        let entry = entry.expect("failed to read directory entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("node-") && name.ends_with(".yaml") {
            fs::remove_file(entry.path()).expect("failed to remove stale config");
        }
    }

    // Generate ed25519 keypairs.
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut keys: Vec<(ed25519::PrivateKey, ed25519::PublicKey)> = (0..nodes)
        .map(|_| {
            let sk = ed25519::PrivateKey::random(&mut rng);
            let pk = sk.public_key();
            (sk, pk)
        })
        .collect();
    keys.sort_by(|a, b| a.1.as_ref().cmp(b.1.as_ref()));

    // Generate BLS12-381 keypair for the proposer.
    let proposer_seed: [u8; 32] = {
        let mut s = [0u8; 32];
        s[..8].copy_from_slice(&(seed + 1000).to_le_bytes());
        s
    };
    let proposer_signing_key_hex = format!("0x{}", hex::encode(proposer_seed));

    let proposer_public_key_hex = {
        use commonware_cryptography::bls12381;
        let mut bls_rng = ChaCha20Rng::from_seed(proposer_seed);
        let bls_sk = bls12381::PrivateKey::random(&mut bls_rng);
        let pk = bls_sk.public_key();
        format!("0x{}", hex::encode(AsRef::<[u8]>::as_ref(&pk)))
    };

    // Generate peers.yaml with Docker hostnames.
    let mut peer_entries = Vec::new();
    for (i, (_, pk)) in keys.iter().enumerate() {
        peer_entries.push(PeerEntryYaml {
            public_key: format!("0x{}", hex::encode(pk.as_ref())),
            address: format!("node-{i}:{p2p_port}"),
        });
    }

    let peers_path = Path::new(output).join("peers.yaml");
    let peers_yaml = serde_yaml::to_string(&peer_entries).expect("failed to serialize peers");
    fs::write(&peers_path, &peers_yaml).expect("failed to write peers.yaml");

    // Generate per-node configs.
    for (i, (sk, _pk)) in keys.iter().enumerate() {
        let role = if i == 0 { "proposer" } else { "receiver" };

        let mut sk_buf = Vec::new();
        sk.write(&mut sk_buf);
        let node_config = NodeConfigYaml {
            private_key: format!("0x{}", hex::encode(&sk_buf)),
            port: p2p_port,
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
            block_interval_secs,
            startup_delay_secs,
            mesh_degree,
            log_level: "info".to_string(),
            local: true,
            directory: "/data".to_string(),
            seed: seed + i as u64,
            dialable_address: Some(format!("node-{i}:{p2p_port}")),
        };

        let config_path = Path::new(output).join(format!("node-{i}.yaml"));
        let config_yaml =
            serde_yaml::to_string(&node_config).expect("failed to serialize node config");
        fs::write(&config_path, &config_yaml).expect("failed to write node config");
    }

    // Generate docker-compose.yml.
    let mut compose = String::from("services:\n");
    let log_level = "debug";
    for i in 0..nodes {
        let _ = write!(
            compose,
            "  node-{i}:\n\
             \x20   image: rlnc-node:latest\n\
             \x20   container_name: node-{i}\n\
             \x20   hostname: node-{i}\n\
             \x20   networks: [rlnc-net]\n\
             \x20   environment:\n\
             \x20     - RUST_LOG={log_level}\n\
             \x20   volumes:\n\
             \x20     - ./node-{i}.yaml:/config/node.yaml:ro\n\
             \x20     - ./peers.yaml:/config/peers.yaml:ro\n\
             \x20   command: [\"--config\", \"/config/node.yaml\", \
                      \"--peers\", \"/config/peers.yaml\"]\n\n"
        );
    }

    if monitoring {
        // Place Prometheus host port just above the last node's metrics port
        // to avoid collisions (e.g. 9090+N for N nodes).
        let prom_host_port = metrics_port + nodes as u16 + 1;
        let _ = write!(
            compose,
            "  prometheus:\n\
             \x20   image: prom/prometheus:v2.51.0\n\
             \x20   profiles: [monitoring]\n\
             \x20   container_name: prometheus\n\
             \x20   hostname: prometheus\n\
             \x20   networks: [rlnc-net]\n\
             \x20   volumes:\n\
             \x20     - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro\n\
             \x20   ports:\n\
             \x20     - \"{prom_host_port}:9090\"\n\n"
        );
    }

    compose.push_str("networks:\n  rlnc-net:\n    driver: bridge\n    enable_ipv6: true\n");

    let compose_path = Path::new(output).join("docker-compose.yml");
    fs::write(&compose_path, &compose).expect("failed to write docker-compose.yml");

    // Generate prometheus.yml if monitoring enabled.
    if monitoring {
        let targets: Vec<String> = (0..nodes)
            .map(|i| format!("'node-{i}:{metrics_port}'"))
            .collect();
        let prom_config = format!(
            "global:\n\
             \x20 scrape_interval: 5s\n\
             scrape_configs:\n\
             \x20 - job_name: rlnc-nodes\n\
             \x20   static_configs:\n\
             \x20     - targets: [{}]\n",
            targets.join(", ")
        );
        let prom_path = Path::new(output).join("prometheus.yml");
        fs::write(&prom_path, &prom_config).expect("failed to write prometheus.yml");
    }

    println!("Generated Docker configs in {output}/");
    println!();
    println!("Build:    docker build -t rlnc-node:latest .");
    println!("Run:      cd {output} && docker compose up");
    if monitoring {
        let prom_host_port = metrics_port + nodes as u16;
        println!("Monitor:  cd {output} && docker compose --profile monitoring up");
        println!("Prometheus UI: http://localhost:{prom_host_port}");
    }
    println!("Down:     cd {output} && docker compose down");
}
