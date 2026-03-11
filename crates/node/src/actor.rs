//! Node actor: async task that drives a [`Protocol`] over P2P
//! channels.
//!
//! The actor owns the protocol instance and manages multi-block
//! sequencing with a state machine and per-block buffering.

use bytes::Buf;
use crate::protocol::{Channel, Protocol};
use commonware_cryptography::PublicKey;
use commonware_p2p::{Receiver, Recipients, Sender};
use commonware_runtime::Clock;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::collections::BTreeMap;
use tracing::{debug, info, trace, warn};

use p2p_primitives_types::Block;
use p2p_strategy_core::ForwardCondition;

/// Actor state machine for sequential block processing.
#[derive(Debug, Clone, Copy)]
enum ActorState {
    /// Waiting for announcement for block `block_num`.
    Init(u64),
    /// Actively decoding block `block_num`.
    Decoding(u64),
}

impl ActorState {
    const fn current_block(&self) -> u64 {
        match self {
            Self::Init(n) | Self::Decoding(n) => *n,
        }
    }
}

/// Role of this node in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Block proposer (node 0).
    Proposer,
    /// Receiver node.
    Receiver,
}

/// Node actor that drives a [`Protocol`] over P2P channels.
///
/// Generic over the protocol `P`, the P2P sender/receiver types,
/// and the public key type.
pub struct NodeActor<P, S, R, K>
where
    P: Protocol,
    S: Sender,
    R: Receiver,
    K: PublicKey,
{
    protocol: P,
    announce_sender: S,
    announce_receiver: R,
    data_sender: S,
    data_receiver: R,
    peers: Vec<K>,
    mesh_degree: usize,
    rng: ChaCha20Rng,

    // Multi-block state.
    state: ActorState,
    /// Buffered messages for future blocks (or data while in Init).
    buffer: BTreeMap<u64, Vec<(Channel, Vec<u8>)>>,
    /// Whether we've forwarded for the current block (for OneShot).
    forwarded: bool,
}

impl<P, S, R, K> NodeActor<P, S, R, K>
where
    P: Protocol,
    S: Sender<PublicKey = K>,
    R: Receiver<PublicKey = K>,
    K: PublicKey,
{
    /// Create a new node actor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        protocol: P,
        announce_sender: S,
        announce_receiver: R,
        data_sender: S,
        data_receiver: R,
        peers: Vec<K>,
        mesh_degree: usize,
        seed: u64,
    ) -> Self {
        Self {
            protocol,
            announce_sender,
            announce_receiver,
            data_sender,
            data_receiver,
            peers,
            mesh_degree,
            rng: ChaCha20Rng::seed_from_u64(seed),
            state: ActorState::Init(0),
            buffer: BTreeMap::new(),
            forwarded: false,
        }
    }

    /// Run the proposer loop: generate and propagate blocks.
    pub async fn run_proposer<C: Clock>(
        &mut self,
        context: &C,
        num_blocks: u64,
        block_size: usize,
        block_interval: std::time::Duration,
        startup_delay: std::time::Duration,
    ) {
        // Wait for P2P connections to establish.
        info!(
            delay_secs = startup_delay.as_secs(),
            "waiting for P2P connections"
        );
        context.sleep(startup_delay).await;

        for block_num in 0..num_blocks {
            if block_num > 0 {
                context.sleep(block_interval).await;
                self.protocol.reset();
                self.forwarded = false;
            }

            let block = generate_random_block(block_size, block_num, &mut self.rng);
            info!(
                block_num,
                block_size,
                strategy = self.protocol.name(),
                "proposing block"
            );

            let proposal = self.protocol.propose(
                &block,
                self.mesh_degree,
                &mut self.rng,
            );

            // State = Decoding (proposer already has full rank).
            self.state = ActorState::Decoding(block_num);

            // Send announcement to all peers.
            let mut ann_msg = block_num.to_be_bytes().to_vec();
            ann_msg.extend_from_slice(&proposal.announcement);
            let _ = self
                .announce_sender
                .send(Recipients::All, ann_msg, true)
                .await;

            // Send data packets to mesh_degree random peers.
            let targets = self.select_peers(None);
            for (i, pkt) in proposal.packets.iter().enumerate() {
                if i < targets.len() {
                    let mut msg = block_num.to_be_bytes().to_vec();
                    msg.extend_from_slice(pkt);
                    let _ = self
                        .data_sender
                        .send(Recipients::One(targets[i].clone()), msg, false)
                        .await;
                }
            }

            debug!(block_num, "block proposed and sent");
        }

        info!(num_blocks, "proposer finished");
    }

    /// Run the receiver loop: process incoming messages.
    ///
    /// # Panics
    ///
    /// Panics if received messages have malformed block number prefixes.
    pub async fn run_receiver(&mut self, num_blocks: u64) {
        let mut decoded_count = 0u64;

        loop {
            if decoded_count >= num_blocks {
                info!(decoded_count, "all blocks decoded, stopping");
                break;
            }

            // Select on both channels.
            commonware_macros::select! {
                result = self.announce_receiver.recv() => {
                    match result {
                        Ok((sender, msg)) => {
                            let msg_bytes: &[u8] = msg.chunk();
                            if msg_bytes.len() < 8 {
                                warn!("announcement too short");
                                continue;
                            }
                            let block_num = u64::from_be_bytes(
                                msg_bytes[..8].try_into().unwrap(),
                            );
                            let payload = msg_bytes[8..].to_vec();
                            self.route(
                                Channel::Announce,
                                block_num,
                                payload,
                                Some(&sender),
                                &mut decoded_count,
                            )
                            .await;
                        }
                        Err(e) => {
                            debug!(?e, "announce channel closed");
                            break;
                        }
                    }
                },
                result = self.data_receiver.recv() => {
                    match result {
                        Ok((sender, msg)) => {
                            let msg_bytes: &[u8] = msg.chunk();
                            if msg_bytes.len() < 8 {
                                warn!("data packet too short");
                                continue;
                            }
                            let block_num = u64::from_be_bytes(
                                msg_bytes[..8].try_into().unwrap(),
                            );
                            let payload = msg_bytes[8..].to_vec();
                            self.route(
                                Channel::Data,
                                block_num,
                                payload,
                                Some(&sender),
                                &mut decoded_count,
                            )
                            .await;
                        }
                        Err(e) => {
                            debug!(?e, "data channel closed");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Route and process a message, iteratively draining any buffered
    /// messages that become processable due to state transitions.
    ///
    /// Uses a work queue instead of recursion to avoid infinite async
    /// type sizes.
    #[allow(clippy::too_many_lines)]
    async fn route(
        &mut self,
        channel: Channel,
        block_num: u64,
        payload: Vec<u8>,
        sender: Option<&K>,
        decoded_count: &mut u64,
    ) {
        let current = self.state.current_block();

        if block_num < current {
            trace!(block_num, current, "discarding stale message");
            return;
        }

        if block_num > current {
            self.buffer
                .entry(block_num)
                .or_default()
                .push((channel, payload));
            return;
        }

        // Process this message and any buffered follow-ups iteratively.
        let mut work = std::collections::VecDeque::new();
        work.push_back((channel, block_num, payload));

        // The initial message carries the sender for peer exclusion;
        // buffered messages don't track senders.
        let mut is_first = true;

        while let Some((ch, bn, data)) = work.pop_front() {
            let cur = self.state.current_block();
            if bn != cur {
                continue;
            }

            let current_sender = if is_first { sender } else { None };
            is_first = false;

            match self.state {
                ActorState::Init(n) => match ch {
                    Channel::Announce => {
                        let result = self
                            .protocol
                            .ingest(Channel::Announce, n, &data);
                        match result {
                            Ok(true) => {
                                self.state = ActorState::Decoding(n);
                                debug!(
                                    block_num = n,
                                    "announcement received, decoding"
                                );

                                // Re-broadcast announcement.
                                let mut msg = n.to_be_bytes().to_vec();
                                msg.extend_from_slice(&data);
                                let _ = self
                                    .announce_sender
                                    .send(Recipients::All, msg, true)
                                    .await;

                                // Drain buffered data for this block
                                // into the work queue.
                                if let Some(msgs) = self.buffer.remove(&n)
                                {
                                    for (bch, pkt) in msgs {
                                        if bch == Channel::Data {
                                            work.push_back((
                                                Channel::Data,
                                                n,
                                                pkt,
                                            ));
                                        }
                                    }
                                }
                            }
                            Ok(false) => {}
                            Err(e) => {
                                warn!(
                                    block_num = n,
                                    error = %e,
                                    "announcement failed"
                                );
                            }
                        }
                    }
                    Channel::Data => {
                        // Buffer data until announcement arrives.
                        self.buffer
                            .entry(n)
                            .or_default()
                            .push((Channel::Data, data));
                    }
                },
                ActorState::Decoding(n) => match ch {
                    Channel::Announce => {
                        // Already processing this block, ignore.
                    }
                    Channel::Data => {
                        let result = self
                            .protocol
                            .ingest(Channel::Data, n, &data);
                        match result {
                            Ok(true) => {
                                let should_forward = match self
                                    .protocol
                                    .forward_condition()
                                {
                                    ForwardCondition::AfterDecode => {
                                        self.protocol.is_complete()
                                            && !self.forwarded
                                    }
                                    ForwardCondition::UntilDecode => {
                                        !self.protocol.is_complete()
                                    }
                                    ForwardCondition::OneShot => {
                                        !self.forwarded
                                    }
                                };

                                if should_forward {
                                    self.forwarded = true;
                                    let packets = self.protocol.recode(
                                        self.mesh_degree,
                                        &mut self.rng,
                                    );
                                    let targets = self
                                        .select_peers(current_sender);
                                    for (i, pkt) in
                                        packets.iter().enumerate()
                                    {
                                        if i < targets.len() {
                                            let mut msg =
                                                n.to_be_bytes().to_vec();
                                            msg.extend_from_slice(pkt);
                                            let _ = self
                                                .data_sender
                                                .send(
                                                    Recipients::One(
                                                        targets[i]
                                                            .clone(),
                                                    ),
                                                    msg,
                                                    false,
                                                )
                                                .await;
                                        }
                                    }
                                }

                                if self.protocol.is_complete() {
                                    let _block =
                                        self.protocol.decode();
                                    info!(
                                        block_num = n,
                                        strategy =
                                            self.protocol.name(),
                                        "block decoded"
                                    );
                                    *decoded_count += 1;

                                    // Advance to next block.
                                    let next = n + 1;
                                    self.protocol.reset();
                                    self.forwarded = false;
                                    self.state =
                                        ActorState::Init(next);

                                    // Drain buffered messages for
                                    // the next block into work
                                    // queue (announcements first).
                                    if let Some(msgs) =
                                        self.buffer.remove(&next)
                                    {
                                        let mut data_msgs =
                                            Vec::new();
                                        for (bch, pkt) in msgs {
                                            match bch {
                                                Channel::Announce => {
                                                    work.push_back((
                                                        Channel::Announce,
                                                        next,
                                                        pkt,
                                                    ));
                                                }
                                                Channel::Data => {
                                                    data_msgs
                                                        .push(pkt);
                                                }
                                            }
                                        }
                                        for pkt in data_msgs {
                                            work.push_back((
                                                Channel::Data,
                                                next,
                                                pkt,
                                            ));
                                        }
                                    }
                                }
                            }
                            Ok(false) => {
                                trace!(
                                    block_num = n,
                                    "redundant packet"
                                );
                            }
                            Err(e) => {
                                warn!(
                                    block_num = n,
                                    error = %e,
                                    "verification failed"
                                );
                            }
                        }
                    }
                },
            }
        }
    }

    /// Select `mesh_degree` random peers, optionally excluding sender.
    fn select_peers(&mut self, exclude: Option<&K>) -> Vec<K> {
        use rand::seq::SliceRandom;

        let mut candidates: Vec<&K> = self
            .peers
            .iter()
            .filter(|p| {
                exclude != Some(*p)
            })
            .collect();

        candidates.shuffle(&mut self.rng);
        candidates
            .into_iter()
            .take(self.mesh_degree)
            .cloned()
            .collect()
    }
}

/// Generate a random block of the given size.
fn generate_random_block(
    size_bytes: usize,
    block_num: u64,
    rng: &mut impl Rng,
) -> Block {
    use alloy_primitives::B256;

    let transactions: Vec<u8> = (0..size_bytes).map(|_| rng.r#gen()).collect();
    let mut parent = [0u8; 32];
    rng.fill(&mut parent);
    let mut state_root = [0u8; 32];
    rng.fill(&mut state_root);
    Block::new(
        B256::from(parent),
        block_num,
        1_700_000_000,
        15_000_000,
        30_000_000,
        B256::from(state_root),
        transactions,
    )
}
