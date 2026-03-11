//! RLNC + Pedersen commitment strategy.
//!
//! Nodes forward coded chunks on every rank increase until decode.
//! Each packet carries N Pedersen commitments (to the original chunks)
//! and a BLS signature authenticating them. Verification is a pure
//! MSM equality check -- no pairing needed.

use commonware_codec::{FixedSize, Write};
use commonware_cryptography::Signer;
use commonware_math::algebra::{CryptoGroup, HashToGroup};
use p2p_primitives_types::{Additive, Block, BlockDecoder, Chunk, CodedChunk, Ring, Scalar};
use p2p_strategy_core::{ForwardCondition, SignedPacket, Strategy, coded_forward, coded_receive};
use rand::{CryptoRng, Rng};
use tracing::{debug, trace};

use crate::{CommitmentKey, PedersenContext, PedersenProof, PedersenScheme, PedersenSigner};

/// RLNC + Pedersen commitment strategy.
///
/// Holds only cryptographic parameters (commitment key, signer).
/// Network parameters live in the simulation layer.
pub struct PedersenStrategy<G: CryptoGroup, S: Signer, const N: usize> {
    ck: CommitmentKey<G>,
    signer: PedersenSigner<S>,
    m: usize,
}

impl<G, S, const N: usize> PedersenStrategy<G, S, N>
where
    G: HashToGroup,
    S: Signer,
{
    /// Create with a fresh random signer and canonical commitment key.
    pub fn new(m: usize, rng: &mut (impl Rng + CryptoRng)) -> Self {
        Self {
            ck: CommitmentKey::<G>::canonical(m),
            signer: PedersenSigner::<S>::generate(rng),
            m,
        }
    }

    /// Build the verification context for receivers.
    fn context(&self) -> PedersenContext<G, S> {
        self.signer.context::<G>(self.ck.clone())
    }
}

/// Per-node state for the Pedersen strategy.
pub struct PedersenNodeState<G: CryptoGroup, S: Signer, const N: usize> {
    decoder: BlockDecoder<Scalar, N>,
    proofs: Vec<PedersenProof<G, S>>,
    ctx: PedersenContext<G, S>,
}

/// Wire packet: coded chunk + Pedersen proof.
type PedersenPacket<G, S, const N: usize> = SignedPacket<Scalar, N, PedersenScheme<G, S>>;

impl<G, S, const N: usize> Strategy for PedersenStrategy<G, S, N>
where
    G: HashToGroup<Scalar = Scalar> + FixedSize + Write + Send + Sync + 'static,
    S: Signer,
{
    type NodeState = PedersenNodeState<G, S, N>;
    type Packet = PedersenPacket<G, S, N>;

    fn name(&self) -> &'static str {
        "Pedersen"
    }

    fn forward_condition(&self) -> ForwardCondition {
        ForwardCondition::UntilDecode
    }

    fn init_proposer<R: Rng + CryptoRng>(
        &self,
        block: &Block,
        num_peers: usize,
        rng: &mut R,
    ) -> (Self::NodeState, Vec<Self::Packet>, usize) {
        let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(self.m, num_peers, rng);
        let (original_chunks, _) = block.as_chunks::<Scalar, N>(self.m);
        let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();
        let proof = self.signer.sign_block::<G>(&self.ck, &chunk_refs);

        trace!(
            m = self.m,
            num_peers,
            coded_chunks = coded_chunks.len(),
            "pedersen proposer initialized"
        );

        let packets: Vec<PedersenPacket<G, S, N>> = coded_chunks
            .into_iter()
            .map(|chunk| SignedPacket::new(chunk, proof.clone()))
            .collect();

        // Proposer's decoder is already full (has all original chunks).
        let mut decoder = BlockDecoder::<Scalar, N>::new();
        for (i, orig) in original_chunks.iter().enumerate() {
            let mut b = vec![Scalar::zero(); N];
            b[i] = Scalar::one();
            let coded = CodedChunk::new(orig.clone(), b);
            decoder.add(coded);
        }

        let state = PedersenNodeState {
            decoder,
            proofs: vec![proof],
            ctx: self.context(),
        };

        (state, packets, byte_len)
    }

    fn init_receiver(&self) -> Self::NodeState {
        PedersenNodeState {
            decoder: BlockDecoder::new(),
            proofs: Vec::new(),
            ctx: self.context(),
        }
    }

    fn receive(&self, state: &mut Self::NodeState, packet: &Self::Packet) -> Result<bool, String> {
        coded_receive::<PedersenScheme<G, S>, Scalar, N>(
            &mut state.decoder,
            &mut state.proofs,
            &state.ctx,
            packet,
        )
    }

    fn can_decode(&self, state: &Self::NodeState) -> bool {
        state.decoder.is_complete()
    }

    fn forward<R: Rng + CryptoRng>(
        &self,
        state: &Self::NodeState,
        num_peers: usize,
        rng: &mut R,
    ) -> Vec<Self::Packet> {
        coded_forward::<PedersenScheme<G, S>, Scalar, R, N>(
            &state.decoder,
            &state.proofs,
            num_peers,
            rng,
        )
    }

    fn packet_size(&self, _packet: &Self::Packet) -> usize {
        // m scalars (data w) + N scalars (coefficients b)
        //   + N commitment group elements + 1 signature
        32 * self.m + 32 * N + G::SIZE * N + <S::Signature as FixedSize>::SIZE
    }

    fn decode(&self, state: Self::NodeState, byte_len: usize) -> Block {
        let decoded_chunks = state.decoder.decode();
        let block = Block::from_chunks::<Scalar, N>(&decoded_chunks, byte_len);
        debug!("pedersen block decoded");
        block
    }
}
