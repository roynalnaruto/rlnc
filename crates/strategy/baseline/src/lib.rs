//! Gossipsub-style full-block forwarding strategy.
//!
//! Every node that receives the complete block forwards it to all
//! mesh peers. This is the baseline Ethereum gossipsub model.

use commonware_codec::{DecodeExt, Encode};
use p2p_primitives_types::Block;
use p2p_strategy_core::{ForwardCondition, Strategy};
use rand::{CryptoRng, Rng};
use tracing::{debug, trace};

/// Gossipsub full-block forwarding strategy.
///
/// A unit struct — all network parameters (mesh degree, latency,
/// bandwidth) live in the simulation layer's `PropagationMechanism`.
pub struct BaselineStrategy;

/// Node state: `None` until the block is received.
pub struct BaselineNodeState {
    block_bytes: Option<Vec<u8>>,
}

impl Strategy for BaselineStrategy {
    type NodeState = BaselineNodeState;
    type Packet = Vec<u8>;

    fn name(&self) -> &'static str {
        "Baseline"
    }

    fn forward_condition(&self) -> ForwardCondition {
        ForwardCondition::AfterDecode
    }

    fn init_proposer<R: Rng + CryptoRng>(
        &self,
        block: &Block,
        num_peers: usize,
        _rng: &mut R,
    ) -> (Self::NodeState, Vec<Self::Packet>, usize) {
        let bytes: Vec<u8> = Encode::encode(block).to_vec();
        let byte_len = bytes.len();
        let packets = vec![bytes.clone(); num_peers];
        let state = BaselineNodeState {
            block_bytes: Some(bytes),
        };
        (state, packets, byte_len)
    }

    fn init_receiver(&self) -> Self::NodeState {
        BaselineNodeState { block_bytes: None }
    }

    fn receive(&self, state: &mut Self::NodeState, packet: &Self::Packet) -> Result<bool, String> {
        if state.block_bytes.is_some() {
            trace!("duplicate block received, ignoring");
            Ok(false)
        } else {
            trace!(bytes = packet.len(), "full block received");
            state.block_bytes = Some(packet.clone());
            Ok(true)
        }
    }

    fn can_decode(&self, state: &Self::NodeState) -> bool {
        state.block_bytes.is_some()
    }

    fn forward<R: Rng + CryptoRng>(
        &self,
        state: &Self::NodeState,
        num_peers: usize,
        _rng: &mut R,
    ) -> Vec<Self::Packet> {
        let bytes = state
            .block_bytes
            .as_ref()
            .expect("forward called before decode");
        vec![bytes.clone(); num_peers]
    }

    fn packet_size(&self, packet: &Self::Packet) -> usize {
        packet.len()
    }

    fn decode(&self, state: Self::NodeState, _byte_len: usize) -> Block {
        let raw = state.block_bytes.expect("decode called before receive");
        let block = Block::decode(raw.as_slice()).expect("invalid block bytes");
        debug!("baseline block decoded");
        block
    }
}
