//! Strategy trait and forwarding condition for block propagation.
//!
//! Defines [`Strategy`], the trait that each block propagation
//! strategy implements, and [`ForwardCondition`], which controls
//! when a node starts forwarding data to its peers.

pub mod helpers;
pub mod proof;

pub use helpers::{coded_forward, coded_receive};
pub use proof::{IntegrityProof, IntegrityScheme, NoopScheme, SignedPacket};

use p2p_primitives_types::Block;
use rand::{CryptoRng, Rng};

/// When a node should start forwarding received data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardCondition {
    /// Forward only after the full block is decoded (gossipsub).
    AfterDecode,
    /// Forward once on first useful receive, then stop.
    ///
    /// Matches the article's simulation model: each node re-encodes
    /// from whatever it has after the first useful packet.
    OneShot,
    /// Forward on every rank increase, including the decode-completing one.
    ///
    /// Matches the article's proposed production behavior. The node
    /// re-encodes on each useful receive. Once decoded, the node is
    /// marked done and future packets are skipped.
    UntilDecode,
}

/// Abstraction over block propagation strategies.
///
/// Each strategy encapsulates its cryptographic parameters (keys,
/// commitment generators, etc.) but knows nothing about the network
/// topology or simulation parameters. Those belong to the simulation
/// layer ([`PropagationMechanism`](crate) in `p2p-sim`).
pub trait Strategy {
    /// Per-node mutable state (decoder, proofs, etc.).
    type NodeState;

    /// Wire packet transmitted between peers.
    type Packet: Clone;

    /// Strategy name (for reporting).
    fn name(&self) -> &'static str;

    /// When should a node start forwarding?
    fn forward_condition(&self) -> ForwardCondition;

    /// Initialize the proposer node.
    ///
    /// Returns the proposer's node state, a packet for each peer
    /// (`len == num_peers`), and the original serialized byte length
    /// (needed for block reconstruction).
    fn init_proposer<R: Rng + CryptoRng>(
        &self,
        block: &Block,
        num_peers: usize,
        rng: &mut R,
    ) -> (Self::NodeState, Vec<Self::Packet>, usize);

    /// Initialize a receiver node (empty state).
    fn init_receiver(&self) -> Self::NodeState;

    /// Process a received packet.
    ///
    /// - `Ok(true)` -- new information gained (linearly independent /
    ///   first copy).
    /// - `Ok(false)` -- redundant (linearly dependent / duplicate).
    /// - `Err(_)` -- verification failure.
    ///
    /// # Errors
    ///
    /// Returns a descriptive error string if the packet fails
    /// integrity verification.
    fn receive(&self, state: &mut Self::NodeState, packet: &Self::Packet) -> Result<bool, String>;

    /// Whether this node has enough data to decode the block.
    fn can_decode(&self, state: &Self::NodeState) -> bool;

    /// Create forwarding packets for `num_peers` neighbors.
    ///
    /// Called each time the node's forwarding condition is met.
    fn forward<R: Rng + CryptoRng>(
        &self,
        state: &Self::NodeState,
        num_peers: usize,
        rng: &mut R,
    ) -> Vec<Self::Packet>;

    /// Size of a packet in bytes (for bandwidth accounting).
    fn packet_size(&self, packet: &Self::Packet) -> usize;

    /// Decode the block from accumulated state.
    fn decode(&self, state: Self::NodeState, byte_len: usize) -> Block;
}
