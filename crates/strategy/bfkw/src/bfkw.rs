//! BFKW linearly-homomorphic signature scheme for RLNC.
//!
//! Implements the BFKW construction (Boneh–Freeman–Katz–Waters) over
//! a generic [`PairingGroup`]. Each coded chunk carries a **single**
//! $\mathbb{G}_2$ signature $\sigma$ verified via the pairing equation:
//!
//! $$e(G_1, \sigma) \stackrel{?}{=}
//! e(\mathrm{pk}, \mathrm{MSM}(\mathbf{H}, [\mathbf{w} \| \mathbf{b}]))$$
//!
//! The "basis vector trick" binds both coded data $\mathbf{w}$ and
//! encoding coefficients $\mathbf{b}$ into a single signature, making
//! the scheme linearly homomorphic: intermediate nodes combine proofs
//! via $\sigma' = \mathrm{MSM}([\sigma_1 \ldots \sigma_L], \alpha)$.

use commonware_codec::{FixedSize, Read as CodecRead, Write as CodecWrite};
use commonware_math::algebra::{Additive, CryptoGroup, HashToGroup, Ring, Space};
use commonware_parallel::Sequential;
use core::marker::PhantomData;
use p2p_primitives_math::Random;
use p2p_primitives_types::CodedChunk;
use p2p_strategy_core::{IntegrityProof, IntegrityScheme};

use crate::PairingGroup;

/// Domain separator for BFKW hash-to-G2 derivation.
pub const BFKW_DST: &[u8] = b"RLNC-BFKW-H2G-v1";

/// Verification errors for the BFKW scheme.
#[derive(Debug)]
pub enum BfkwError {
    /// The pairing check $e(G_1, \sigma) = e(\mathrm{pk}, P)$ failed.
    PairingMismatch,
}

impl core::fmt::Display for BfkwError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PairingMismatch => write!(f, "BFKW pairing check failed"),
        }
    }
}

/// Proof material for a BFKW-signed coded chunk: a single
/// $\mathbb{G}_2$ signature.
#[derive(Clone, Debug)]
pub struct BfkwProof<C: PairingGroup> {
    sigma: C::G2,
}

impl<C: PairingGroup> BfkwProof<C> {
    /// Create a proof from a signature point.
    pub const fn new(sigma: C::G2) -> Self {
        Self { sigma }
    }

    /// Borrow the signature point.
    pub const fn sigma(&self) -> &C::G2 {
        &self.sigma
    }
}

impl<C> IntegrityProof for BfkwProof<C>
where
    C: PairingGroup,
    C::G2: FixedSize + CodecWrite + CodecRead<Cfg = ()>,
{
    fn serialize_proof(&self, buf: &mut Vec<u8>) {
        self.sigma.write(buf);
    }

    fn deserialize_proof(buf: &mut &[u8]) -> Result<Self, String> {
        let sigma = C::G2::read_cfg(buf, &()).map_err(|e| format!("sigma: {e}"))?;
        Ok(Self::new(sigma))
    }

    fn proof_wire_size(&self) -> usize {
        <C::G2 as FixedSize>::SIZE
    }
}

/// Per-block verification context for BFKW (no secret material).
///
/// Contains the $m + N$ hash points $H(\mathrm{id}, s)$ in
/// $\mathbb{G}_2$ and the proposer's public key in $\mathbb{G}_1$.
#[derive(Clone, Debug)]
pub struct BfkwContext<C: PairingGroup> {
    hash_points: Vec<C::G2>,
    public_key: C::G1,
}

impl<C: PairingGroup> BfkwContext<C> {
    /// Create a new context.
    pub const fn new(hash_points: Vec<C::G2>, public_key: C::G1) -> Self {
        Self {
            hash_points,
            public_key,
        }
    }

    /// The $m + N$ hash points $H(\mathrm{id}, s)$.
    pub fn hash_points(&self) -> &[C::G2] {
        &self.hash_points
    }

    /// The proposer's public key.
    pub const fn public_key(&self) -> &C::G1 {
        &self.public_key
    }
}

/// BFKW integrity scheme, generic over a [`PairingGroup`].
///
/// This is the only scheme that requires bilinear pairings.
/// Verification checks:
///
/// $$e(G_1, \sigma) = e(\mathrm{pk},
/// \mathrm{MSM}(\mathbf{H}, [\mathbf{w} \| \mathbf{b}]))$$
#[derive(Clone, Debug)]
pub struct BfkwScheme<C: PairingGroup>(PhantomData<C>);

impl<C, const N: usize> IntegrityScheme<C::Scalar, N> for BfkwScheme<C>
where
    C: PairingGroup,
{
    type Proof = BfkwProof<C>;
    type Context = BfkwContext<C>;
    type Error = BfkwError;

    /// Verify the BFKW pairing equation for a received coded chunk.
    ///
    /// 1. Build the augmented message $[\mathbf{w} \| \mathbf{b}]$
    ///    ($m + N$ scalars).
    /// 2. Compute $P = \mathrm{MSM}(\mathbf{H}, [\mathbf{w} \|
    ///    \mathbf{b}])$ in $\mathbb{G}_2$.
    /// 3. Multi-pairing check $e(G_1, \sigma) = e(\mathrm{pk}, P)$.
    ///
    /// # Errors
    ///
    /// Returns [`BfkwError::PairingMismatch`] if the pairing check
    /// fails.
    fn verify(
        ctx: &Self::Context,
        chunk: &CodedChunk<C::Scalar, N>,
        proof: &Self::Proof,
    ) -> Result<(), BfkwError> {
        // 1. Build augmented message: [w || b].
        let m = ctx
            .hash_points
            .len()
            .checked_sub(N)
            .expect("hash_points must have at least N elements");
        let mut msg = Vec::with_capacity(m + N);
        msg.extend_from_slice(chunk.w().v());
        msg.extend_from_slice(chunk.b());

        // 2. P = MSM(hash_points, [w || b]).
        let p = C::G2::msm(&ctx.hash_points, &msg, &Sequential);

        // 3. Multi-pairing check: e(G1_gen, σ) == e(pk, P).
        C::pairing_check(&ctx.public_key, &proof.sigma, &p)
            .then_some(())
            .ok_or(BfkwError::PairingMismatch)
    }

    /// Combine $L$ BFKW proofs into one for a re-encoded chunk.
    ///
    /// $\sigma' = \mathrm{MSM}([\sigma_1, \ldots, \sigma_L], \alpha)$
    ///
    /// This is the linearly-homomorphic property of BFKW.
    fn combine(proofs: &[Self::Proof], alphas: &[C::Scalar]) -> Self::Proof {
        let sigmas: Vec<C::G2> = proofs.iter().map(|p| p.sigma.clone()).collect();
        let sigma = C::G2::msm(&sigmas, alphas, &Sequential);
        BfkwProof { sigma }
    }
}

/// Derive $m + N$ hash points for a given block:
/// $H(\mathrm{id}, s) = \mathrm{hash\_to\_group}(\text{BFKW\_DST},
/// \mathrm{id} \| s)$ for $s \in [0, m + N)$.
///
/// `block_id` can be a block hash (32 bytes) or any other unique
/// identifier (e.g. `block_num.to_be_bytes()`).
pub fn derive_hash_points<C: PairingGroup, const N: usize>(
    block_id: &[u8],
    m: usize,
) -> Vec<C::G2> {
    (0..(m + N))
        .map(|s| {
            let mut msg = Vec::with_capacity(block_id.len() + 8);
            msg.extend_from_slice(block_id);
            msg.extend_from_slice(&(s as u64).to_le_bytes());
            C::G2::hash_to_group(BFKW_DST, &msg)
        })
        .collect()
}

/// Proposer-side signer for the BFKW scheme.
///
/// Holds a secret scalar $\mathrm{sk}$ and the corresponding public
/// key $\mathrm{pk} = \mathrm{sk} \cdot G_1$.
pub struct BfkwSigner<C: PairingGroup> {
    sk: C::Scalar,
    pk: C::G1,
}

impl<C: PairingGroup> BfkwSigner<C> {
    /// The proposer's public key.
    pub const fn public_key(&self) -> &C::G1 {
        &self.pk
    }

    /// Generate a fresh signer from randomness.
    #[must_use]
    pub fn generate(rng: &mut (impl rand::CryptoRng + rand::RngCore)) -> Self {
        let sk = C::Scalar::random(rng);
        let pk = C::G1::generator() * &sk;
        Self { sk, pk }
    }

    /// Build a per-block verification context.
    pub fn setup_context<const N: usize>(&self, block_id: &[u8], m: usize) -> BfkwContext<C> {
        let hash_points = derive_hash_points::<C, N>(block_id, m);
        BfkwContext::new(hash_points, self.pk.clone())
    }

    /// Sign a single original chunk (augmented with its basis vector).
    ///
    /// Computes $\mathbf{m}' = [\mathbf{v}_i \| \mathbf{e}_i]$
    /// ($m + N$ scalars), then
    /// $P = \mathrm{MSM}(\mathbf{H}, \mathbf{m}')$, then
    /// $\sigma_i = \mathrm{sk} \cdot P$.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_index >= N`.
    #[must_use]
    pub fn sign<const N: usize>(
        &self,
        ctx: &BfkwContext<C>,
        chunk_index: usize,
        v_i: &[C::Scalar],
    ) -> BfkwProof<C> {
        let m = ctx
            .hash_points
            .len()
            .checked_sub(N)
            .expect("hash_points must have at least N elements");
        assert_eq!(
            v_i.len(),
            m,
            "chunk must have m={m} elements, got {}",
            v_i.len()
        );
        assert!(
            chunk_index < N,
            "chunk_index {chunk_index} out of range [0, {N})"
        );

        // Build augmented message: [v_i || e_i].
        let mut m_prime: Vec<C::Scalar> = Vec::with_capacity(m + N);
        m_prime.extend_from_slice(v_i);
        m_prime.extend((0..N).map(|j| {
            if j == chunk_index {
                C::Scalar::one()
            } else {
                C::Scalar::zero()
            }
        }));

        // P = MSM(hash_points, m').
        let p = C::G2::msm(&ctx.hash_points, &m_prime, &Sequential);

        // σ = sk * P.
        let sigma = p * &self.sk;
        BfkwProof { sigma }
    }

    /// Sign all original chunks and combine into a proof for a coded
    /// chunk with the given coefficient vector $\mathbf{b}$.
    ///
    /// This is a convenience for the proposer: sign each original
    /// chunk, then linearly combine the signatures with $\mathbf{b}$.
    ///
    /// # Panics
    ///
    /// Panics if `chunks.len() != N`.
    #[must_use]
    pub fn sign_coded<const N: usize>(
        &self,
        ctx: &BfkwContext<C>,
        chunks: &[&[C::Scalar]],
        b: &[C::Scalar],
    ) -> BfkwProof<C> {
        assert_eq!(
            chunks.len(),
            N,
            "expected N={N} chunks, got {}",
            chunks.len()
        );
        let individual: Vec<BfkwProof<C>> = chunks
            .iter()
            .enumerate()
            .map(|(i, v)| self.sign::<N>(ctx, i, v))
            .collect();
        <BfkwScheme<C> as IntegrityScheme<C::Scalar, N>>::combine(&individual, b)
    }
}

#[cfg(test)]
mod tests {
    use commonware_math::algebra::{Additive, Ring};
    use p2p_primitives_math::{Random, Scalar};
    use p2p_primitives_types::{Chunk, CodedChunk};
    use p2p_strategy_core::SignedPacket;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    use crate::pairing::Bls12381;

    use super::*;

    const M: usize = 4;
    const N: usize = 3;

    type Scheme = BfkwScheme<Bls12381>;

    /// Build random original chunks, derive context, sign each chunk.
    fn setup(
        rng: &mut ChaCha20Rng,
    ) -> (
        BfkwSigner<Bls12381>,
        BfkwContext<Bls12381>,
        Vec<Vec<Scalar>>,
    ) {
        let signer = BfkwSigner::<Bls12381>::generate(rng);
        let block_id = [0xABu8; 32];
        let ctx = signer.setup_context::<N>(&block_id, M);

        let chunks: Vec<Vec<Scalar>> = (0..N)
            .map(|_| (0..M).map(|_| Scalar::random(&mut *rng)).collect())
            .collect();

        (signer, ctx, chunks)
    }

    /// Build a coded chunk from original chunks.
    fn encode_chunk(chunks: &[Vec<Scalar>], rng: &mut ChaCha20Rng) -> CodedChunk<Scalar, N> {
        let b: Vec<Scalar> = (0..N).map(|_| Scalar::random(&mut *rng)).collect();
        let mut w = vec![Scalar::zero(); M];
        for (bi, vi) in b.iter().zip(chunks) {
            for (wj, vj) in w.iter_mut().zip(vi) {
                *wj += &(vj.clone() * bi);
            }
        }
        CodedChunk::new(Chunk::new(&w), b)
    }

    #[test]
    fn sign_verify_roundtrip() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let (signer, ctx, chunks) = setup(&mut rng);

        let coded = encode_chunk(&chunks, &mut rng);
        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let proof = signer.sign_coded::<N>(&ctx, &chunk_refs, coded.b());

        assert!(Scheme::verify(&ctx, &coded, &proof).is_ok());
    }

    #[test]
    fn verify_rejects_tampered_data() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let (signer, ctx, chunks) = setup(&mut rng);

        let coded = encode_chunk(&chunks, &mut rng);
        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let proof = signer.sign_coded::<N>(&ctx, &chunk_refs, coded.b());

        // Tamper with w[0].
        let (w, b) = coded.into_parts();
        let mut tampered = w.into_inner();
        tampered[0] += &Scalar::from_u64(1);
        let coded: CodedChunk<Scalar, N> = CodedChunk::new(Chunk::new(&tampered), b);

        assert!(matches!(
            Scheme::verify(&ctx, &coded, &proof),
            Err(BfkwError::PairingMismatch)
        ));
    }

    #[test]
    fn combine_linearity() {
        let mut rng = ChaCha20Rng::seed_from_u64(55);
        let (signer, ctx, chunks) = setup(&mut rng);

        // Create two coded chunks with their proofs.
        let coded1 = encode_chunk(&chunks, &mut rng);
        let coded2 = encode_chunk(&chunks, &mut rng);
        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let proof1 = signer.sign_coded::<N>(&ctx, &chunk_refs, coded1.b());
        let proof2 = signer.sign_coded::<N>(&ctx, &chunk_refs, coded2.b());

        assert!(Scheme::verify(&ctx, &coded1, &proof1).is_ok());
        assert!(Scheme::verify(&ctx, &coded2, &proof2).is_ok());

        // Re-encode: combine the two coded chunks with random alphas.
        let alpha1 = Scalar::random(&mut rng);
        let alpha2 = Scalar::random(&mut rng);
        let alphas = vec![alpha1.clone(), alpha2.clone()];

        // New coded data.
        let mut new_w = vec![Scalar::zero(); M];
        for (wj, (w1j, w2j)) in new_w
            .iter_mut()
            .zip(coded1.w().v().iter().zip(coded2.w().v()))
        {
            *wj = w1j.clone() * &alpha1 + &(w2j.clone() * &alpha2);
        }
        let mut new_b = vec![Scalar::zero(); N];
        for (bj, (b1j, b2j)) in new_b.iter_mut().zip(coded1.b().iter().zip(coded2.b())) {
            *bj = b1j.clone() * &alpha1 + &(b2j.clone() * &alpha2);
        }
        let new_coded: CodedChunk<Scalar, N> = CodedChunk::new(Chunk::new(&new_w), new_b);

        // Combine proofs.
        let combined = <Scheme as IntegrityScheme<Scalar, N>>::combine(&[proof1, proof2], &alphas);

        // The combined proof should verify against the re-encoded chunk.
        assert!(Scheme::verify(&ctx, &new_coded, &combined).is_ok());
    }

    #[test]
    fn basis_vector_trick() {
        let mut rng = ChaCha20Rng::seed_from_u64(77);
        let (signer, ctx, chunks) = setup(&mut rng);

        // Sign each original chunk individually.
        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let individual_proofs: Vec<BfkwProof<Bls12381>> = (0..N)
            .map(|i| signer.sign::<N>(&ctx, i, &chunk_refs[i]))
            .collect();

        // Verify that each individual signature is valid for its
        // original chunk (b = e_i).
        for (i, proof) in individual_proofs.iter().enumerate() {
            let mut b = vec![Scalar::zero(); N];
            b[i] = Scalar::one();
            let coded: CodedChunk<Scalar, N> = CodedChunk::new(Chunk::new(chunks[i].as_slice()), b);
            assert!(Scheme::verify(&ctx, &coded, proof).is_ok());
        }

        // Combine with a coefficient vector to get a coded chunk proof.
        let b: Vec<Scalar> = (0..N).map(|_| Scalar::random(&mut rng)).collect();
        let combined = <Scheme as IntegrityScheme<Scalar, N>>::combine(&individual_proofs, &b);

        // Build the corresponding coded chunk.
        let mut w = vec![Scalar::zero(); M];
        for (bi, vi) in b.iter().zip(&chunks) {
            for (wj, vj) in w.iter_mut().zip(vi) {
                *wj += &(vj.clone() * bi);
            }
        }
        let coded: CodedChunk<Scalar, N> = CodedChunk::new(Chunk::new(&w), b);
        assert!(Scheme::verify(&ctx, &coded, &combined).is_ok());
    }

    #[test]
    fn signed_packet_roundtrip() {
        let mut rng = ChaCha20Rng::seed_from_u64(99);
        let (signer, ctx, chunks) = setup(&mut rng);

        let coded = encode_chunk(&chunks, &mut rng);
        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let proof = signer.sign_coded::<N>(&ctx, &chunk_refs, coded.b());

        let pkt = SignedPacket::<Scalar, N, Scheme>::new(coded, proof);
        assert!(pkt.verify(&ctx).is_ok());

        let (recovered, _) = pkt.into_parts();
        assert_eq!(recovered.w().len(), M);
        assert_eq!(recovered.b().len(), N);
    }
}
