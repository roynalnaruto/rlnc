//! RLNC + BFKW linearly-homomorphic signature protocol.
//!
//! Nodes forward coded chunks on every rank increase until decode.
//! Each packet carries a single G2 signature verified via pairing.

use commonware_codec::{FixedSize, Read as CodecRead, Write as CodecWrite};
use commonware_cryptography::bls12381::primitives::group::Scalar;
use commonware_math::algebra::{Additive, Ring};
use p2p_bfkw::{
    BfkwContext, BfkwProof, BfkwScheme, BfkwSigner, PairingGroup, derive_hash_points,
};
use p2p_primitives_types::{Block, BlockDecoder, Chunk, CodedChunk};
use p2p_strategy_core::{ForwardCondition, SignedPacket, coded_forward, coded_receive};
use rand::{CryptoRng, Rng};
use tracing::{debug, trace};

use crate::protocol::{Channel, Proposal, Protocol};

/// Concrete BFKW scheme type.
type Scheme<C> = BfkwScheme<C>;
/// Wire packet type.
type Packet<C, const N: usize> =
    SignedPacket<<C as PairingGroup>::Scalar, N, Scheme<C>>;

/// RLNC + BFKW linearly-homomorphic signature protocol, generic over
/// pairing group `C` and chunk count `N`.
pub struct BfkwProtocol<C, const N: usize>
where
    C: PairingGroup,
{
    /// Proposer's signer (Some for proposer, None for receivers).
    signer: Option<BfkwSigner<C>>,
    /// Chunk dimension.
    m: usize,
    /// Proposer's G1 public key (from config).
    proposer_pk: C::G1,

    // --- Per-block mutable state (cleared on reset) ---
    decoder: BlockDecoder<C::Scalar, N>,
    proofs: Vec<BfkwProof<C>>,
    ctx: Option<BfkwContext<C>>,
    byte_len: usize,
}

impl<C, const N: usize> BfkwProtocol<C, N>
where
    C: PairingGroup,
{
    /// Create a new BFKW protocol.
    ///
    /// - `m`: chunk dimension (number of field elements per chunk).
    /// - `proposer_pk`: G1 public key of the proposer.
    /// - `signer`: `Some` for the proposer node, `None` for receivers.
    pub fn new(
        m: usize,
        proposer_pk: C::G1,
        signer: Option<BfkwSigner<C>>,
    ) -> Self {
        Self {
            signer,
            m,
            proposer_pk,
            decoder: BlockDecoder::new(),
            proofs: Vec::new(),
            ctx: None,
            byte_len: 0,
        }
    }
}

impl<C, const N: usize> Protocol for BfkwProtocol<C, N>
where
    C: PairingGroup<Scalar = Scalar>,
    C::Scalar: FixedSize + CodecWrite + CodecRead<Cfg = ()>,
    C::G2: FixedSize + CodecWrite + CodecRead<Cfg = ()>,
{
    fn name(&self) -> &'static str {
        "BFKW"
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
        let block_num = block.number();
        let block_id = block_num.to_be_bytes();

        let (coded_chunks, byte_len) =
            block.encode_chunks::<Scalar, N>(self.m, num_targets, rng);
        let (original_chunks, _) = block.as_chunks::<Scalar, N>(self.m);
        let chunk_refs: Vec<&[Scalar]> =
            original_chunks.iter().map(Chunk::v).collect();

        let signer = self
            .signer
            .as_ref()
            .expect("propose called on non-proposer");
        let ctx = signer.setup_context::<N>(&block_id, self.m);

        trace!(
            m = self.m,
            num_targets,
            coded_chunks = coded_chunks.len(),
            "bfkw proposer initialized"
        );

        // Serialize packets.
        let packets: Vec<Vec<u8>> = coded_chunks
            .into_iter()
            .map(|chunk| {
                let proof =
                    signer.sign_coded::<N>(&ctx, &chunk_refs, chunk.b());
                let pkt =
                    SignedPacket::<Scalar, N, Scheme<C>>::new(chunk, proof);
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
            let proof = signer.sign::<N>(&ctx, i, orig.v());
            self.decoder.add(coded);
            proposer_proofs.push(proof);
        }
        self.proofs = proposer_proofs;
        self.ctx = Some(ctx);
        self.byte_len = byte_len;

        let announcement = (byte_len as u64).to_be_bytes().to_vec();
        Proposal {
            announcement,
            packets,
        }
    }

    fn ingest(
        &mut self,
        channel: Channel,
        block_num: u64,
        data: &[u8],
    ) -> Result<bool, String> {
        match channel {
            Channel::Announce => {
                if data.len() < 8 {
                    return Err("announcement too short".into());
                }
                #[allow(clippy::cast_possible_truncation)]
                {
                    self.byte_len = u64::from_be_bytes(
                        data[..8].try_into().unwrap(),
                    ) as usize;
                }

                // Derive per-block context from block_num.
                let block_id = block_num.to_be_bytes();
                let hash_points =
                    derive_hash_points::<C, N>(&block_id, self.m);
                self.ctx = Some(BfkwContext::new(
                    hash_points,
                    self.proposer_pk.clone(),
                ));

                trace!(
                    byte_len = self.byte_len,
                    block_num,
                    "bfkw announcement received"
                );
                Ok(true)
            }
            Channel::Data => {
                let ctx = self
                    .ctx
                    .as_ref()
                    .ok_or("data received before announcement")?;
                let packet = Packet::<C, N>::deserialize(data, self.m)?;
                coded_receive::<Scheme<C>, Scalar, N>(
                    &mut self.decoder,
                    &mut self.proofs,
                    ctx,
                    &packet,
                )
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.decoder.is_complete()
    }

    fn recode<R: Rng + CryptoRng>(
        &self,
        num_targets: usize,
        rng: &mut R,
    ) -> Vec<Vec<u8>> {
        let packets = coded_forward::<Scheme<C>, Scalar, R, N>(
            &self.decoder,
            &self.proofs,
            num_targets,
            rng,
        );
        packets
            .into_iter()
            .map(|pkt| pkt.serialize())
            .collect()
    }

    fn decode(&mut self) -> Block {
        let decoded_chunks = self.decoder.decode();
        let block =
            Block::from_chunks::<Scalar, N>(&decoded_chunks, self.byte_len);
        debug!("bfkw block decoded");
        block
    }

    fn reset(&mut self) {
        self.decoder.reset();
        self.proofs.clear();
        self.ctx = None;
        self.byte_len = 0;
    }

    fn packet_wire_size(&self) -> usize {
        <C::Scalar as FixedSize>::SIZE * self.m
            + <C::Scalar as FixedSize>::SIZE * N
            + <C::G2 as FixedSize>::SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;
    use p2p_bfkw::Bls12381;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    const N: usize = 10;
    const M: usize = 16;

    type TestProtocol = BfkwProtocol<Bls12381, N>;

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

        let signer = BfkwSigner::<Bls12381>::generate(&mut rng);
        let pk = signer.public_key().clone();

        let mut proposer =
            TestProtocol::new(M, pk.clone(), Some(signer));
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

        let signer = BfkwSigner::<Bls12381>::generate(&mut rng);
        let pk = signer.public_key().clone();

        let mut proposer =
            TestProtocol::new(M, pk.clone(), Some(signer));
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

        // Second receiver.
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

        let signer = BfkwSigner::<Bls12381>::generate(&mut rng);
        let pk = signer.public_key().clone();
        let mut proposer =
            TestProtocol::new(M, pk.clone(), Some(signer));
        let mut receiver = TestProtocol::new(M, pk, None);

        for block_num in 0..3 {
            let block = sample_block(block_num);
            let proposal = proposer.propose(&block, 15, &mut rng);

            receiver
                .ingest(
                    Channel::Announce,
                    block_num,
                    &proposal.announcement,
                )
                .unwrap();
            for pkt in &proposal.packets {
                receiver
                    .ingest(Channel::Data, block_num, pkt)
                    .unwrap();
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
