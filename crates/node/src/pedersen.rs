//! RLNC + Pedersen commitment protocol for real P2P networking.
//!
//! Nodes forward coded chunks on every rank increase until decode.
//! Each packet carries N Pedersen commitments and a BLS signature.

use commonware_codec::{FixedSize, Read as CodecRead, Write as CodecWrite};
use commonware_cryptography::Signer;
use commonware_cryptography::bls12381::primitives::group::Scalar;
use commonware_math::algebra::{Additive, CryptoGroup, HashToGroup, Ring};
use p2p_pedersen::{CommitmentKey, PedersenContext, PedersenProof, PedersenScheme, PedersenSigner};
use p2p_primitives_types::{Block, BlockDecoder, Chunk, CodedChunk};
use p2p_strategy_core::{ForwardCondition, SignedPacket, coded_forward, coded_receive};
use rand::{CryptoRng, Rng};
use tracing::{debug, trace};

use crate::protocol::{Channel, Proposal, Protocol};

/// Concrete Pedersen scheme type.
type Scheme<G, S> = PedersenScheme<G, S>;
/// Wire packet type.
type Packet<G, S, const N: usize> = SignedPacket<<G as CryptoGroup>::Scalar, N, Scheme<G, S>>;

/// RLNC + Pedersen commitment protocol, generic over group `G`,
/// signer `S`, and chunk count `N`.
pub struct PedersenProtocol<G, S, const N: usize>
where
    G: HashToGroup,
    S: Signer,
{
    /// Proposer's signer (Some for proposer, None for receivers).
    signer: Option<PedersenSigner<S>>,
    /// Verification context (commitment key + proposer public key).
    ctx: PedersenContext<G, S>,
    /// Chunk dimension.
    m: usize,

    // --- Per-block mutable state (cleared on reset) ---
    decoder: BlockDecoder<G::Scalar, N>,
    proofs: Vec<PedersenProof<G, S>>,
    byte_len: usize,
}

impl<G, S, const N: usize> PedersenProtocol<G, S, N>
where
    G: HashToGroup,
    S: Signer,
{
    /// Create a new Pedersen protocol.
    ///
    /// - `m`: chunk dimension (number of field elements per chunk).
    /// - `proposer_pk`: public key of the proposer.
    /// - `signer`: `Some` for the proposer node, `None` for receivers.
    pub fn new(m: usize, proposer_pk: S::PublicKey, signer: Option<PedersenSigner<S>>) -> Self {
        let ck = CommitmentKey::<G>::canonical(m);
        let ctx = PedersenContext::new(ck, proposer_pk);
        Self {
            signer,
            ctx,
            m,
            decoder: BlockDecoder::new(),
            proofs: Vec::new(),
            byte_len: 0,
        }
    }
}

impl<G, S, const N: usize> Protocol for PedersenProtocol<G, S, N>
where
    G: HashToGroup<Scalar = Scalar>
        + FixedSize
        + CodecWrite
        + CodecRead<Cfg = ()>
        + Additive
        + Send
        + Sync
        + 'static,
    S: Signer,
    S::Signature: FixedSize + CodecWrite + CodecRead<Cfg = ()>,
{
    fn name(&self) -> &'static str {
        "Pedersen"
    }

    fn forward_condition(&self) -> ForwardCondition {
        ForwardCondition::UntilDecode
    }

    fn propose<R: Rng + CryptoRng>(
        &mut self,
        block: &Block,
        num_targets: usize,
        rng: &mut R,
    ) -> Proposal {
        let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(self.m, num_targets, rng);
        let (original_chunks, _) = block.as_chunks::<Scalar, N>(self.m);
        let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();

        let signer = self
            .signer
            .as_ref()
            .expect("propose called on non-proposer");
        let proof = signer.sign_block::<G>(self.ctx.key(), &chunk_refs);

        trace!(
            m = self.m,
            num_targets,
            coded_chunks = coded_chunks.len(),
            "pedersen proposer initialized"
        );

        // Serialize packets.
        let packets: Vec<Vec<u8>> = coded_chunks
            .into_iter()
            .map(|chunk| {
                let pkt = SignedPacket::<Scalar, N, Scheme<G, S>>::new(chunk, proof.clone());
                pkt.serialize()
            })
            .collect();

        // Initialize proposer's decoder at full rank.
        self.decoder = BlockDecoder::new();
        let mut proposer_proofs = Vec::new();
        for (i, orig) in original_chunks.iter().enumerate() {
            let mut b = vec![Scalar::zero(); N];
            b[i] = Scalar::one();
            let coded = CodedChunk::new(orig.clone(), b);
            self.decoder.add(coded);
            proposer_proofs.push(proof.clone());
        }
        self.proofs = proposer_proofs;
        self.byte_len = byte_len;

        let announcement = (byte_len as u64).to_be_bytes().to_vec();
        Proposal {
            announcement,
            packets,
        }
    }

    fn ingest(&mut self, channel: Channel, _block_num: u64, data: &[u8]) -> Result<bool, String> {
        match channel {
            Channel::Announce => {
                if data.len() < 8 {
                    return Err("announcement too short".into());
                }
                #[allow(clippy::cast_possible_truncation)]
                {
                    self.byte_len = u64::from_be_bytes(data[..8].try_into().unwrap()) as usize;
                }
                trace!(byte_len = self.byte_len, "pedersen announcement received");
                Ok(true)
            },
            Channel::Data => {
                let packet = Packet::<G, S, N>::deserialize(data, self.m)?;
                coded_receive::<Scheme<G, S>, Scalar, N>(
                    &mut self.decoder,
                    &mut self.proofs,
                    &self.ctx,
                    &packet,
                )
            },
        }
    }

    fn is_complete(&self) -> bool {
        self.decoder.is_complete()
    }

    fn recode<R: Rng + CryptoRng>(&self, num_targets: usize, rng: &mut R) -> Vec<Vec<u8>> {
        let packets = coded_forward::<Scheme<G, S>, Scalar, R, N>(
            &self.decoder,
            &self.proofs,
            num_targets,
            rng,
        );
        packets.into_iter().map(|pkt| pkt.serialize()).collect()
    }

    fn decode(&mut self) -> Block {
        let decoded_chunks = self.decoder.decode();
        let block = Block::from_chunks::<Scalar, N>(&decoded_chunks, self.byte_len);
        debug!("pedersen block decoded");
        block
    }

    fn reset(&mut self) {
        self.decoder.reset();
        self.proofs.clear();
        self.byte_len = 0;
    }

    fn packet_wire_size(&self) -> usize {
        <G::Scalar as FixedSize>::SIZE * self.m
            + <G::Scalar as FixedSize>::SIZE * N
            + G::SIZE * N
            + <S::Signature as FixedSize>::SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;
    use commonware_cryptography::bls12381;
    use commonware_cryptography::bls12381::primitives::group::G1;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    const N: usize = 10;
    const M: usize = 16;

    type TestProtocol = PedersenProtocol<G1, bls12381::PrivateKey, N>;

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

        let signer = PedersenSigner::<bls12381::PrivateKey>::generate(&mut rng);
        let pk = signer.public_key();

        let mut proposer = TestProtocol::new(M, pk.clone(), Some(signer));
        let proposal = proposer.propose(&block, 15, &mut rng);

        // Receiver.
        let mut receiver = TestProtocol::new(M, pk, None);
        receiver
            .ingest(Channel::Announce, 0, &proposal.announcement)
            .unwrap();

        for pkt in &proposal.packets {
            let result = receiver.ingest(Channel::Data, 0, pkt);
            assert!(result.is_ok(), "ingest failed: {result:?}");
            if receiver.is_complete() {
                break;
            }
        }
        assert!(receiver.is_complete());

        let decoded = receiver.decode();
        assert_eq!(block, decoded);
    }

    #[test]
    fn recode_and_decode() {
        let mut rng = ChaCha20Rng::seed_from_u64(99);
        let block = sample_block(1);

        let signer = PedersenSigner::<bls12381::PrivateKey>::generate(&mut rng);
        let pk = signer.public_key();

        let mut proposer = TestProtocol::new(M, pk.clone(), Some(signer));
        let proposal = proposer.propose(&block, 15, &mut rng);

        // Intermediate node: receive 3 packets.
        let mut intermediate = TestProtocol::new(M, pk.clone(), None);
        intermediate
            .ingest(Channel::Announce, 1, &proposal.announcement)
            .unwrap();
        for pkt in &proposal.packets[..3] {
            intermediate.ingest(Channel::Data, 1, pkt).unwrap();
        }
        assert!(!intermediate.is_complete());

        // Re-encode for a second receiver.
        let recoded = intermediate.recode(15, &mut rng);

        // Second receiver: decode from recoded + remaining.
        let mut receiver = TestProtocol::new(M, pk, None);
        receiver
            .ingest(Channel::Announce, 1, &proposal.announcement)
            .unwrap();
        for pkt in &recoded {
            receiver.ingest(Channel::Data, 1, pkt).unwrap();
            if receiver.is_complete() {
                break;
            }
        }
        if !receiver.is_complete() {
            for pkt in &proposal.packets[3..] {
                receiver.ingest(Channel::Data, 1, pkt).unwrap();
                if receiver.is_complete() {
                    break;
                }
            }
        }
        assert!(receiver.is_complete());

        let decoded = receiver.decode();
        assert_eq!(block, decoded);
    }

    #[test]
    fn multi_block_with_reset() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);

        let signer = PedersenSigner::<bls12381::PrivateKey>::generate(&mut rng);
        let pk = signer.public_key();
        let mut proposer = TestProtocol::new(M, pk.clone(), Some(signer));
        let mut receiver = TestProtocol::new(M, pk, None);

        for block_num in 0..3 {
            let block = sample_block(block_num);
            let proposal = proposer.propose(&block, 15, &mut rng);

            receiver
                .ingest(Channel::Announce, block_num, &proposal.announcement)
                .unwrap();
            for pkt in &proposal.packets {
                receiver.ingest(Channel::Data, block_num, pkt).unwrap();
                if receiver.is_complete() {
                    break;
                }
            }
            assert!(receiver.is_complete());
            let decoded = receiver.decode();
            assert_eq!(block, decoded);

            proposer.reset();
            receiver.reset();
        }
    }
}
