//! RLNC + BFKW linearly-homomorphic signature strategy.
//!
//! Nodes forward coded chunks on every rank increase until decode.
//! Each packet carries a single G2 signature verified via a pairing
//! check. Re-encoding combines signatures homomorphically via MSM.

use core::cell::RefCell;

use commonware_codec::FixedSize;
use p2p_primitives_types::{Additive, Block, BlockDecoder, Chunk, CodedChunk, Ring, Scalar};
use p2p_strategy_core::{ForwardCondition, SignedPacket, Strategy, coded_forward, coded_receive};
use rand::{CryptoRng, Rng};
use tracing::{debug, trace};

use crate::{BfkwContext, BfkwProof, BfkwScheme, BfkwSigner, PairingGroup};

/// RLNC + BFKW signature strategy.
///
/// Holds only cryptographic parameters (signer, cached context).
/// Network parameters live in the simulation layer.
pub struct BfkwStrategy<C: PairingGroup, const N: usize> {
    signer: BfkwSigner<C>,
    m: usize,
    /// Cached verification context, updated each block in `init_proposer`.
    ctx_cache: RefCell<Option<BfkwContext<C>>>,
}

impl<C: PairingGroup, const N: usize> BfkwStrategy<C, N> {
    /// Create with a fresh random signer.
    pub fn new(m: usize, rng: &mut (impl Rng + CryptoRng)) -> Self {
        Self {
            signer: BfkwSigner::<C>::generate(rng),
            m,
            ctx_cache: RefCell::new(None),
        }
    }
}

/// Per-node state for the BFKW strategy.
pub struct BfkwNodeState<C: PairingGroup, const N: usize> {
    decoder: BlockDecoder<Scalar, N>,
    proofs: Vec<BfkwProof<C>>,
    ctx: BfkwContext<C>,
}

/// Wire packet: coded chunk + BFKW proof.
type BfkwPacket<C, const N: usize> = SignedPacket<Scalar, N, BfkwScheme<C>>;

impl<C, const N: usize> Strategy for BfkwStrategy<C, N>
where
    C: PairingGroup<Scalar = Scalar>,
    C::G2: FixedSize,
{
    type NodeState = BfkwNodeState<C, N>;
    type Packet = BfkwPacket<C, N>;

    fn name(&self) -> &'static str {
        "BFKW"
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
        let block_id = block.hash();

        let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(self.m, num_peers, rng);
        let (original_chunks, _) = block.as_chunks::<Scalar, N>(self.m);
        let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();
        let ctx = self.signer.setup_context::<N>(block_id.as_slice(), self.m);

        trace!(
            m = self.m,
            num_peers,
            coded_chunks = coded_chunks.len(),
            "bfkw proposer initialized"
        );

        // Cache the context so init_receiver can use it.
        self.ctx_cache.replace(Some(ctx.clone()));

        let packets: Vec<BfkwPacket<C, N>> = coded_chunks
            .into_iter()
            .map(|chunk| {
                let proof = self.signer.sign_coded::<N>(&ctx, &chunk_refs, chunk.b());
                SignedPacket::new(chunk, proof)
            })
            .collect();

        // Proposer's decoder is already full.
        let mut decoder = BlockDecoder::<Scalar, N>::new();
        let mut proposer_proofs = Vec::new();
        for (i, orig) in original_chunks.iter().enumerate() {
            let mut b = vec![Scalar::zero(); N];
            b[i] = Scalar::one();
            let coded = CodedChunk::new(orig.clone(), b);
            let proof = self.signer.sign::<N>(&ctx, i, orig.v());
            decoder.add(coded);
            proposer_proofs.push(proof);
        }

        let state = BfkwNodeState {
            decoder,
            proofs: proposer_proofs,
            ctx,
        };

        (state, packets, byte_len)
    }

    fn init_receiver(&self) -> Self::NodeState {
        let ctx = self
            .ctx_cache
            .borrow()
            .as_ref()
            .expect("init_proposer must be called before init_receiver")
            .clone();

        BfkwNodeState {
            decoder: BlockDecoder::new(),
            proofs: Vec::new(),
            ctx,
        }
    }

    fn receive(&self, state: &mut Self::NodeState, packet: &Self::Packet) -> Result<bool, String> {
        coded_receive::<BfkwScheme<C>, Scalar, N>(
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
        coded_forward::<BfkwScheme<C>, Scalar, R, N>(&state.decoder, &state.proofs, num_peers, rng)
    }

    fn packet_size(&self, _packet: &Self::Packet) -> usize {
        // m scalars (data w) + N scalars (coefficients b) + 1 G2 signature
        32 * self.m + 32 * N + <C::G2 as FixedSize>::SIZE
    }

    fn decode(&self, state: Self::NodeState, byte_len: usize) -> Block {
        let decoded_chunks = state.decoder.decode();
        let block = Block::from_chunks::<Scalar, N>(&decoded_chunks, byte_len);
        debug!("bfkw block decoded");
        block
    }
}
