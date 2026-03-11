//! RLNC P2P node binary.
//!
//! Loads configuration, sets up `commonware-p2p` authenticated
//! networking, and runs the node actor with the selected strategy.

use clap::Parser;
use commonware_codec::Read as CodecRead;
use commonware_cryptography::{Signer, ed25519};
use commonware_p2p::authenticated::discovery;
use commonware_p2p::Manager;
use commonware_runtime::{Runner, tokio};
use commonware_utils::NZU32;
use commonware_cryptography::bls12381;
use commonware_cryptography::bls12381::primitives::group::G1;
use p2p_bfkw::{BfkwSigner, Bls12381};
use p2p_node::actor::{NodeActor, Role};
use p2p_node::baseline::BaselineProtocol;
use p2p_node::bfkw::BfkwProtocol;
use p2p_node::config::{CliArgs, NodeConfig, compute_m, load_peers};
use p2p_node::pedersen::PedersenProtocol;
use p2p_pedersen::PedersenSigner;
use rand::SeedableRng;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::info;

/// P2P namespace for this application.
const NAMESPACE: &[u8] = b"RLNC-P2P-v1";

/// Maximum message size (16 MiB).
const MAX_MSG_SIZE: u32 = 16 * 1024 * 1024;

/// Announcement channel ID.
const ANNOUNCE_CH: u64 = 0;
/// Data channel ID.
const DATA_CH: u64 = 1;

/// Number of original chunks per block.
const N: usize = 10;

#[allow(clippy::too_many_lines)]
fn main() {
    let args = CliArgs::parse();
    let config = NodeConfig::load(&args.config).expect("failed to load config");
    let peer_entries = load_peers(&args.peers).expect("failed to load peers");

    // Compute chunk dimension.
    let m = compute_m(config.block_size, N);

    // Create tokio runtime.
    let rt_config = tokio::Config::new()
        .with_storage_directory(&config.directory);
    let executor = tokio::Runner::new(rt_config);

    executor.start(|context| async move {
        // Init telemetry.
        let metrics_addr: SocketAddr =
            format!("0.0.0.0:{}", config.metrics_port).parse().unwrap();
        let logging = tokio::telemetry::Logging {
            level: config.log_level.parse().unwrap_or(tracing::Level::INFO),
            json: false,
        };
        tokio::telemetry::init(
            context.clone(),
            logging,
            Some(metrics_addr),
            None,
        );

        info!(
            strategy = config.strategy,
            role = config.role,
            port = config.port,
            metrics_port = config.metrics_port,
            m,
            "node starting"
        );

        // Parse our ed25519 key.
        let key_bytes = hex::decode(
            config.private_key.strip_prefix("0x").unwrap_or(&config.private_key),
        )
        .expect("invalid private key hex");
        let signer = ed25519::PrivateKey::read_cfg(
            &mut key_bytes.as_slice(), &(),
        )
        .expect("invalid private key");
        let my_pk = signer.public_key();

        // Parse peer set.
        let mut bootstrappers = Vec::new();
        let mut peer_pks = Vec::new();
        for entry in &peer_entries {
            let pk_bytes = hex::decode(
                entry.public_key.strip_prefix("0x").unwrap_or(&entry.public_key),
            )
            .expect("invalid peer public key hex");
            let pk = ed25519::PublicKey::read_cfg(
                &mut pk_bytes.as_slice(), &(),
            )
            .expect("invalid peer public key");

            if pk != my_pk {
                let addr: SocketAddr =
                    entry.address.parse().expect("invalid peer address");
                bootstrappers.push((pk.clone(), addr.into()));
                peer_pks.push(pk);
            }
        }

        // Create P2P network.
        let listen: SocketAddr =
            format!("0.0.0.0:{}", config.port).parse().unwrap();
        let dialable: SocketAddr =
            format!("127.0.0.1:{}", config.port).parse().unwrap();

        let p2p_cfg = if config.local {
            discovery::Config::local(
                signer,
                NAMESPACE,
                listen,
                dialable,
                bootstrappers,
                MAX_MSG_SIZE,
            )
        } else {
            discovery::Config::recommended(
                signer,
                NAMESPACE,
                listen,
                dialable,
                bootstrappers,
                MAX_MSG_SIZE,
            )
        };

        let (mut network, mut oracle) =
            discovery::Network::new(context.clone(), p2p_cfg);

        // Register channels.
        let ann_quota = commonware_runtime::Quota::per_second(NZU32!(10));
        let data_quota = commonware_runtime::Quota::per_second(NZU32!(256));

        let (ann_tx, ann_rx) = network.register(ANNOUNCE_CH, ann_quota, 128);
        let (data_tx, data_rx) = network.register(DATA_CH, data_quota, 4096);

        // Track peer set.
        let mut all_pks: Vec<ed25519::PublicKey> = peer_entries
            .iter()
            .map(|e| {
                let pk_bytes = hex::decode(
                    e.public_key.strip_prefix("0x").unwrap_or(&e.public_key),
                )
                .unwrap();
                ed25519::PublicKey::read_cfg(
                    &mut pk_bytes.as_slice(), &(),
                )
                .unwrap()
            })
            .collect();
        all_pks.sort();
        let peer_set = commonware_utils::ordered::Set::try_from(all_pks)
            .expect("duplicate peer public keys");
        oracle.track(0, peer_set).await;

        // Start P2P network.
        let _p2p_handle = network.start();

        // Determine role.
        let role = if config.role == "proposer" {
            Role::Proposer
        } else {
            Role::Receiver
        };

        // Parse proposer's BLS12-381 public key.
        let pk_hex = config
            .proposer_public_key
            .strip_prefix("0x")
            .unwrap_or(&config.proposer_public_key);
        let pk_bytes = hex::decode(pk_hex).expect("invalid proposer public key hex");

        let startup_delay = Duration::from_secs(config.startup_delay_secs);
        let block_interval = Duration::from_secs(config.block_interval_secs);

        // Create and run based on strategy.
        match config.strategy.as_str() {
            "baseline" => {
                let proto = BaselineProtocol::new();
                let mut actor = NodeActor::new(
                    proto, ann_tx, ann_rx, data_tx, data_rx,
                    peer_pks, config.mesh_degree, config.seed,
                );
                match role {
                    Role::Proposer => {
                        actor
                            .run_proposer(
                                &context,
                                config.num_blocks,
                                config.block_size,
                                block_interval,
                                startup_delay,
                            )
                            .await;
                    }
                    Role::Receiver => {
                        actor.run_receiver(config.num_blocks).await;
                    }
                }
            }
            "pedersen" => {
                // Parse proposer's BLS public key for Pedersen.
                let bls_pk = bls12381::PublicKey::read_cfg(
                    &mut pk_bytes.as_slice(), &(),
                )
                .expect("invalid BLS12-381 public key");

                let signer_opt = config.proposer_signing_key.as_ref().map(|sk_hex| {
                    let sk_hex = sk_hex.strip_prefix("0x").unwrap_or(sk_hex);
                    let sk_bytes = hex::decode(sk_hex).expect("invalid signing key hex");
                    let seed = <[u8; 32]>::try_from(sk_bytes.as_slice())
                        .expect("signing key must be 32 bytes");
                    let mut sk_rng =
                        rand_chacha::ChaCha20Rng::from_seed(seed);
                    PedersenSigner::<bls12381::PrivateKey>::generate(&mut sk_rng)
                });

                let proto = PedersenProtocol::<G1, bls12381::PrivateKey, N>::new(
                    m, bls_pk, signer_opt,
                );
                let mut actor = NodeActor::new(
                    proto, ann_tx, ann_rx, data_tx, data_rx,
                    peer_pks, config.mesh_degree, config.seed,
                );
                match role {
                    Role::Proposer => {
                        actor
                            .run_proposer(
                                &context,
                                config.num_blocks,
                                config.block_size,
                                block_interval,
                                startup_delay,
                            )
                            .await;
                    }
                    Role::Receiver => {
                        actor.run_receiver(config.num_blocks).await;
                    }
                }
            }
            "bfkw" => {
                // Parse proposer's G1 public key for BFKW.
                let g1_pk = G1::read_cfg(&mut pk_bytes.as_slice(), &())
                    .expect("invalid BLS12-381 G1 point");

                let signer_opt = config.proposer_signing_key.as_ref().map(|sk_hex| {
                    let sk_hex = sk_hex.strip_prefix("0x").unwrap_or(sk_hex);
                    let sk_bytes = hex::decode(sk_hex).expect("invalid signing key hex");
                    let seed = <[u8; 32]>::try_from(sk_bytes.as_slice())
                        .expect("signing key must be 32 bytes");
                    let mut sk_rng =
                        rand_chacha::ChaCha20Rng::from_seed(seed);
                    BfkwSigner::<Bls12381>::generate(&mut sk_rng)
                });

                let proto = BfkwProtocol::<Bls12381, N>::new(
                    m, g1_pk, signer_opt,
                );
                let mut actor = NodeActor::new(
                    proto, ann_tx, ann_rx, data_tx, data_rx,
                    peer_pks, config.mesh_degree, config.seed,
                );
                match role {
                    Role::Proposer => {
                        actor
                            .run_proposer(
                                &context,
                                config.num_blocks,
                                config.block_size,
                                block_interval,
                                startup_delay,
                            )
                            .await;
                    }
                    Role::Receiver => {
                        actor.run_receiver(config.num_blocks).await;
                    }
                }
            }
            other => panic!("unknown strategy: {other}"),
        }
    });
}
