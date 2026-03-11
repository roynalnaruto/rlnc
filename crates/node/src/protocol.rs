//! Protocol trait for real P2P block propagation.
//!
//! Each implementation encapsulates encoding, integrity verification,
//! re-encoding, wire serialization, and mutable protocol state.
//! The node actor owns one instance and drives the protocol lifecycle.

use p2p_primitives_types::Block;
use p2p_strategy_core::ForwardCondition;
use rand::{CryptoRng, Rng};

/// Discriminates the P2P channel a message arrived on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Block announcement: per-block metadata (e.g. `byte_len`).
    Announce,
    /// Data packet: coded chunk + proof (or full block for baseline).
    Data,
}

/// Output of [`Protocol::propose`].
pub struct Proposal {
    /// Serialized announcement (broadcast on channel 0).
    /// Does NOT include the `block_num` prefix (actor prepends it).
    pub announcement: Vec<u8>,
    /// Serialized data packets (sent on channel 1, one per target).
    /// Do NOT include the `block_num` prefix (actor prepends it).
    pub packets: Vec<Vec<u8>>,
}

/// A block propagation protocol for real P2P networking.
///
/// Each implementation encapsulates encoding, integrity verification,
/// re-encoding, wire serialization, **and mutable protocol state**.
/// The node actor owns one instance and drives the protocol lifecycle.
///
/// The protocol processes one block at a time. Between blocks, the
/// actor calls [`reset`](Protocol::reset) to clear per-block state
/// while preserving long-lived parameters (keys, commitment
/// generators, m, etc.).
pub trait Protocol: Send + 'static {
    /// Strategy name (for logging and metrics).
    fn name(&self) -> &'static str;

    /// When the node should forward received data to peers.
    fn forward_condition(&self) -> ForwardCondition;

    /// Proposer: encode a block and produce wire-ready messages.
    ///
    /// Reads `block.number()` for the block number. Mutates self to
    /// initialize proposer state (decoder at full rank, proofs
    /// populated).
    fn propose<R: Rng + CryptoRng>(
        &mut self,
        block: &Block,
        num_targets: usize,
        rng: &mut R,
    ) -> Proposal;

    /// Process an incoming message from the given channel.
    ///
    /// `block_num` is extracted from the wire prefix by the actor and
    /// passed here. The `data` payload does NOT include the prefix.
    ///
    /// - `Channel::Announce`: initialize per-block state. Returns
    ///   `Ok(true)` on success.
    /// - `Channel::Data`: process a coded packet (or full block).
    ///   Returns `Ok(true)` if rank increased, `Ok(false)` if
    ///   redundant, `Err` on verification failure.
    ///
    /// # Errors
    ///
    /// Returns a descriptive error string on verification failure.
    fn ingest(
        &mut self,
        channel: Channel,
        block_num: u64,
        data: &[u8],
    ) -> Result<bool, String>;

    /// Whether this node has enough data to decode the block.
    fn is_complete(&self) -> bool;

    /// Re-encode stored chunks and produce wire-ready forwarding
    /// packets.
    fn recode<R: Rng + CryptoRng>(
        &self,
        num_targets: usize,
        rng: &mut R,
    ) -> Vec<Vec<u8>>;

    /// Decode the block from accumulated state.
    fn decode(&mut self) -> Block;

    /// Reset per-block state for the next block.
    ///
    /// Clears decoder, proofs, and per-block context while preserving
    /// long-lived parameters (keys, generators, m, public keys).
    fn reset(&mut self);

    /// Single packet wire size in bytes (for metrics/logging).
    fn packet_wire_size(&self) -> usize;
}
