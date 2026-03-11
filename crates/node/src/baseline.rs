//! Gossipsub-style full-block forwarding protocol.
//!
//! Every node that receives the complete block forwards it to all
//! mesh peers. This is the baseline Ethereum gossipsub model.

use commonware_codec::{DecodeExt, Encode};
use p2p_primitives_types::Block;
use p2p_strategy_core::ForwardCondition;
use rand::{CryptoRng, Rng};
use tracing::{debug, trace};

use crate::protocol::{Channel, Proposal, Protocol};

/// Full-block gossipsub protocol.
pub struct BaselineProtocol {
    /// The decoded block (once received).
    block: Option<Block>,
    /// Serialized block bytes (cached for forwarding).
    block_bytes: Option<Vec<u8>>,
    /// Expected byte length from announcement.
    byte_len: usize,
}

impl Default for BaselineProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl BaselineProtocol {
    /// Create a new baseline protocol instance.
    pub const fn new() -> Self {
        Self {
            block: None,
            block_bytes: None,
            byte_len: 0,
        }
    }
}

impl Protocol for BaselineProtocol {
    fn name(&self) -> &'static str {
        "Baseline"
    }

    fn forward_condition(&self) -> ForwardCondition {
        ForwardCondition::AfterDecode
    }

    fn propose<R: Rng + CryptoRng>(
        &mut self,
        block: &Block,
        num_targets: usize,
        _rng: &mut R,
    ) -> Proposal {
        let bytes: Vec<u8> = Encode::encode(block).to_vec();
        let byte_len = bytes.len();

        // Announcement = byte_len as u64 BE.
        let announcement = (byte_len as u64).to_be_bytes().to_vec();

        // Each target gets a copy of the full block.
        let packets = vec![bytes.clone(); num_targets];

        self.block = Some(block.clone());
        self.block_bytes = Some(bytes);
        self.byte_len = byte_len;

        trace!(byte_len, num_targets, "baseline proposer initialized");

        Proposal {
            announcement,
            packets,
        }
    }

    fn ingest(
        &mut self,
        channel: Channel,
        _block_num: u64,
        data: &[u8],
    ) -> Result<bool, String> {
        match channel {
            Channel::Announce => {
                if data.len() < 8 {
                    return Err("announcement too short".into());
                }
                #[allow(clippy::cast_possible_truncation)]
                let byte_len =
                    u64::from_be_bytes(data[..8].try_into().unwrap()) as usize;
                self.byte_len = byte_len;
                trace!(byte_len, "baseline announcement received");
                Ok(true)
            }
            Channel::Data => {
                if self.block.is_some() {
                    trace!("duplicate block received, ignoring");
                    return Ok(false);
                }
                let block = Block::decode(data)
                    .map_err(|e| format!("failed to decode block: {e}"))?;
                trace!(bytes = data.len(), "full block received");
                self.block_bytes = Some(data.to_vec());
                self.block = Some(block);
                Ok(true)
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.block.is_some()
    }

    fn recode<R: Rng + CryptoRng>(
        &self,
        num_targets: usize,
        _rng: &mut R,
    ) -> Vec<Vec<u8>> {
        let bytes = self
            .block_bytes
            .as_ref()
            .expect("recode called before decode");
        vec![bytes.clone(); num_targets]
    }

    fn decode(&mut self) -> Block {
        let block = self.block.take().expect("decode called before receive");
        debug!("baseline block decoded");
        block
    }

    fn reset(&mut self) {
        self.block = None;
        self.block_bytes = None;
        self.byte_len = 0;
    }

    fn packet_wire_size(&self) -> usize {
        self.byte_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn sample_block(num: u64) -> Block {
        Block::new(
            B256::repeat_byte(0xAA),
            num,
            1_700_000_000,
            15_000_000,
            30_000_000,
            B256::repeat_byte(0xBB),
            (0..200u8).collect(),
        )
    }

    #[test]
    fn propose_ingest_decode_roundtrip() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let block = sample_block(0);
        let mut proposer = BaselineProtocol::new();

        let proposal = proposer.propose(&block, 3, &mut rng);
        assert_eq!(proposal.packets.len(), 3);
        assert_eq!(proposal.announcement.len(), 8);

        // Receiver side.
        let mut receiver = BaselineProtocol::new();
        let result = receiver.ingest(
            Channel::Announce,
            0,
            &proposal.announcement,
        );
        assert!(result.unwrap());

        let result = receiver.ingest(Channel::Data, 0, &proposal.packets[0]);
        assert!(result.unwrap());
        assert!(receiver.is_complete());

        let decoded = receiver.decode();
        assert_eq!(block, decoded);
    }

    #[test]
    fn multi_block_with_reset() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);

        for block_num in 0..3 {
            let block = sample_block(block_num);
            let mut proposer = BaselineProtocol::new();
            let proposal = proposer.propose(&block, 1, &mut rng);

            let mut receiver = BaselineProtocol::new();
            receiver
                .ingest(Channel::Announce, block_num, &proposal.announcement)
                .unwrap();
            receiver
                .ingest(Channel::Data, block_num, &proposal.packets[0])
                .unwrap();

            let decoded = receiver.decode();
            assert_eq!(block, decoded);
            receiver.reset();
            assert!(!receiver.is_complete());
        }
    }

    #[test]
    fn duplicate_data_returns_false() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let block = sample_block(0);
        let mut proto = BaselineProtocol::new();
        let proposal = proto.propose(&block, 1, &mut rng);

        let mut receiver = BaselineProtocol::new();
        receiver
            .ingest(Channel::Announce, 0, &proposal.announcement)
            .unwrap();
        assert!(receiver
            .ingest(Channel::Data, 0, &proposal.packets[0])
            .unwrap());
        // Second time should return false.
        assert!(!receiver
            .ingest(Channel::Data, 0, &proposal.packets[0])
            .unwrap());
    }
}
