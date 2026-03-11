//! Pedersen commitment-based integrity scheme for RLNC.
//!
//! Each coded chunk carries $N$ commitments $[C_1, \ldots, C_N]$ to
//! the original block chunks, plus a signature authenticating
//! those commitments. Verification is a pure MSM equality check (no
//! pairing needed):
//!
//! $$\mathrm{MSM}(\mathbf{G}, \mathbf{w})
//! \stackrel{?}{=} \mathrm{MSM}(\mathbf{C}, \mathbf{b})$$
//!
//! Since commitments are to the *original* chunks, they are invariant
//! under re-encoding — [`combine`](PedersenScheme) simply clones the
//! first proof.
//!
//! The scheme is generic over both the commitment group `G` and the
//! signature scheme `S: Signer`, allowing any commonware signer
//! (BLS12-381, Ed25519, etc.) to authenticate commitments.

use commonware_codec::{Encode, FixedSize, Read as CodecRead, Write};
use commonware_cryptography::{Signer, Verifier};
use commonware_math::algebra::{Additive, CryptoGroup, HashToGroup};
use core::marker::PhantomData;
use p2p_primitives_types::{CodedChunk, read_group};
use p2p_strategy_core::{IntegrityProof, IntegrityScheme};

use crate::commitment::CommitmentKey;

/// Signature namespace for Pedersen commitment authentication.
pub const PEDERSEN_SIG_NS: &[u8] = b"RLNC-PEDERSEN-SIG-v1";

/// Verification errors for the Pedersen scheme.
#[derive(Debug)]
pub enum PedersenError {
    /// Signature over the commitments is invalid.
    InvalidSignature,
    /// $\mathrm{MSM}(\mathbf{G}, \mathbf{w}) \neq
    /// \mathrm{MSM}(\mathbf{C}, \mathbf{b})$.
    CommitmentMismatch,
}

impl core::fmt::Display for PedersenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "invalid signature over commitments"),
            Self::CommitmentMismatch => write!(f, "coded chunk does not match commitments"),
        }
    }
}

/// Proof material attached to each coded chunk on the wire.
///
/// Contains the $N$ commitments to the original chunks (same in every
/// packet from a given block) and a signature authenticating them.
///
/// Generic over the commitment group `G` and the signer `S`.
#[derive(Clone, Debug)]
pub struct PedersenProof<G, S: Signer> {
    /// $[C_1, \ldots, C_N]$ — commitments to the original block chunks.
    commitments: Vec<G>,
    /// Signature $\sigma$ over the serialized commitments.
    signature: S::Signature,
}

impl<G, S: Signer> PedersenProof<G, S> {
    /// Create a proof from raw parts.
    pub const fn new(commitments: Vec<G>, signature: S::Signature) -> Self {
        Self {
            commitments,
            signature,
        }
    }

    /// Borrow the commitment vector.
    pub fn commitments(&self) -> &[G] {
        &self.commitments
    }

    /// Borrow the signature.
    pub const fn signature(&self) -> &S::Signature {
        &self.signature
    }
}

impl<G, S> IntegrityProof for PedersenProof<G, S>
where
    G: FixedSize + Write + CodecRead<Cfg = ()> + Additive + Clone + Send + Sync + 'static,
    S: Signer,
    S::Signature: FixedSize + Write + CodecRead<Cfg = ()>,
{
    fn serialize_proof(&self, buf: &mut Vec<u8>) {
        #[allow(clippy::cast_possible_truncation)]
        let count = self.commitments.len() as u32;
        count.write(buf);
        for c in &self.commitments {
            c.write(buf);
        }
        self.signature.write(buf);
    }

    fn deserialize_proof(buf: &mut &[u8]) -> Result<Self, String> {
        let n = u32::read_cfg(buf, &()).map_err(|e| format!("commitment count: {e}"))? as usize;
        if n > 1024 {
            return Err("too many commitments".into());
        }
        let mut commitments = Vec::with_capacity(n);
        for _ in 0..n {
            commitments.push(read_group::<G>(buf)?);
        }
        let signature = S::Signature::read_cfg(buf, &()).map_err(|e| format!("signature: {e}"))?;
        Ok(Self::new(commitments, signature))
    }

    fn proof_wire_size(&self) -> usize {
        4 + self.commitments.len() * G::SIZE + <S::Signature as FixedSize>::SIZE
    }
}

/// Per-block verification context (no secret material).
///
/// Holds the commitment key (generators) and the proposer's
/// public key for signature verification.
///
/// Generic over the commitment group `G` and the signer `S`.
#[derive(Clone, Debug)]
pub struct PedersenContext<G: CryptoGroup, S: Signer> {
    /// Reusable generator points for MSM.
    key: CommitmentKey<G>,
    /// Proposer's public key.
    public_key: S::PublicKey,
}

impl<G: CryptoGroup, S: Signer> PedersenContext<G, S> {
    /// Create a new context.
    pub const fn new(key: CommitmentKey<G>, public_key: S::PublicKey) -> Self {
        Self { key, public_key }
    }

    /// Borrow the commitment key.
    pub const fn key(&self) -> &CommitmentKey<G> {
        &self.key
    }

    /// Borrow the proposer's public key.
    pub const fn public_key(&self) -> &S::PublicKey {
        &self.public_key
    }
}

/// Pedersen commitment integrity scheme, generic over the commitment
/// group $\mathbb{G}$ and the signature scheme `S`.
///
/// In practice, $\mathbb{G} = \mathrm{G1}$ (BLS12-381). The scheme
/// does **not** require pairings — only MSM and hash-to-group.
#[derive(Clone, Debug)]
pub struct PedersenScheme<G, S: Signer>(PhantomData<(G, S)>);

impl<G, S, const N: usize> IntegrityScheme<G::Scalar, N> for PedersenScheme<G, S>
where
    G: HashToGroup + FixedSize + Write + Send + Sync + 'static,
    S: Signer,
{
    type Proof = PedersenProof<G, S>;
    type Context = PedersenContext<G, S>;
    type Error = PedersenError;

    /// Verify a Pedersen proof for a received coded chunk.
    ///
    /// 1. Check the signature over the serialized commitments.
    /// 2. Check $\mathrm{MSM}(\mathbf{G}, \mathbf{w}) =
    ///    \mathrm{MSM}(\mathbf{C}, \mathbf{b})$.
    ///
    /// # Errors
    ///
    /// Returns [`PedersenError::InvalidSignature`] if the
    /// signature check fails, or
    /// [`PedersenError::CommitmentMismatch`] if the MSM equality
    /// check fails.
    fn verify(
        ctx: &Self::Context,
        chunk: &CodedChunk<G::Scalar, N>,
        proof: &Self::Proof,
    ) -> Result<(), PedersenError> {
        // 1. Verify signature over commitments.
        let msg = serialize_commitments(&proof.commitments);
        if !ctx
            .public_key
            .verify(PEDERSEN_SIG_NS, &msg, &proof.signature)
        {
            return Err(PedersenError::InvalidSignature);
        }

        // 2. Check MSM equality.
        if !ctx.key.verify(chunk.w(), chunk.b(), &proof.commitments) {
            return Err(PedersenError::CommitmentMismatch);
        }

        Ok(())
    }

    /// Combine proofs for a re-encoded chunk.
    ///
    /// Commitments to the original chunks are invariant under
    /// re-encoding, so we simply clone the first proof.
    fn combine(proofs: &[Self::Proof], _alphas: &[G::Scalar]) -> Self::Proof {
        proofs[0].clone()
    }
}

/// Proposer-side signer that creates Pedersen proofs.
///
/// Holds a signing key used to sign the block's commitment
/// vector. The private key never leaves this struct.
///
/// Generic over the signer `S` (e.g., `bls12381::PrivateKey`,
/// `ed25519::PrivateKey`).
pub struct PedersenSigner<S: Signer> {
    signer: S,
}

impl<S: Signer> PedersenSigner<S> {
    /// Generate a fresh signer with a random keypair.
    pub fn generate(rng: &mut (impl rand::CryptoRng + rand::RngCore)) -> Self {
        let signer = S::random(rng);
        Self { signer }
    }

    /// The proposer's public key.
    pub fn public_key(&self) -> S::PublicKey {
        self.signer.public_key()
    }

    /// Build a verification context for receivers.
    pub fn context<G: CryptoGroup>(&self, key: CommitmentKey<G>) -> PedersenContext<G, S> {
        PedersenContext::new(key, self.signer.public_key())
    }

    /// Commit to each original chunk and sign the commitments.
    ///
    /// Returns a proof that can be attached to every coded chunk
    /// from this block.
    #[must_use]
    pub fn sign_block<G>(
        &self,
        key: &CommitmentKey<G>,
        chunks: &[&[G::Scalar]],
    ) -> PedersenProof<G, S>
    where
        G: HashToGroup + FixedSize + Write,
    {
        let commitments: Vec<G> = chunks.iter().map(|v| key.commit(v)).collect();
        let msg = serialize_commitments(&commitments);
        let signature = self.signer.sign(PEDERSEN_SIG_NS, &msg);
        PedersenProof {
            commitments,
            signature,
        }
    }
}

/// Serialize commitment points to a flat byte vector for signing.
fn serialize_commitments<G: Encode + FixedSize>(commitments: &[G]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(commitments.len() * G::SIZE);
    for c in commitments {
        msg.extend_from_slice(&c.encode());
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitment::CommitmentKey;
    use commonware_cryptography::bls12381;
    use commonware_cryptography::bls12381::primitives::group::{G1, Scalar};
    use commonware_math::algebra::Additive;
    use p2p_primitives_math::Random;
    use p2p_primitives_types::{Chunk, CodedChunk};
    use p2p_strategy_core::{IntegrityScheme, SignedPacket};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    const M: usize = 4;
    const N: usize = 3;

    /// Build random original chunks and commit them.
    fn setup(
        rng: &mut ChaCha20Rng,
    ) -> (
        PedersenSigner<bls12381::PrivateKey>,
        CommitmentKey<G1>,
        PedersenContext<G1, bls12381::PrivateKey>,
        Vec<Vec<Scalar>>,
        PedersenProof<G1, bls12381::PrivateKey>,
    ) {
        let signer = PedersenSigner::<bls12381::PrivateKey>::generate(rng);
        let ck = CommitmentKey::<G1>::canonical(M);

        let chunks: Vec<Vec<Scalar>> = (0..N)
            .map(|_| (0..M).map(|_| Scalar::random(&mut *rng)).collect())
            .collect();

        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let proof = signer.sign_block::<G1>(&ck, &chunk_refs);
        let ctx = signer.context::<G1>(ck.clone());

        (signer, ck, ctx, chunks, proof)
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
        let (_signer, _ck, ctx, chunks, proof) = setup(&mut rng);

        let coded = encode_chunk(&chunks, &mut rng);
        let result = PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, &coded, &proof);
        assert!(result.is_ok());
    }

    #[test]
    fn verify_rejects_tampered_data() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let (_signer, _ck, ctx, chunks, proof) = setup(&mut rng);

        let coded = encode_chunk(&chunks, &mut rng);
        // Tamper with w[0].
        let (w, b) = coded.into_parts();
        let mut tampered = w.into_inner();
        tampered[0] += &Scalar::from_u64(1);
        let coded: CodedChunk<Scalar, N> = CodedChunk::new(Chunk::new(&tampered), b);

        let result = PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, &coded, &proof);
        assert!(matches!(result, Err(PedersenError::CommitmentMismatch)));
    }

    #[test]
    fn verify_rejects_wrong_signature() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let (_signer, _ck, ctx, chunks, _proof) = setup(&mut rng);

        // Create a proof signed by a different key.
        let other_signer = PedersenSigner::<bls12381::PrivateKey>::generate(&mut rng);
        let ck2 = CommitmentKey::<G1>::canonical(M);
        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let bad_proof = other_signer.sign_block::<G1>(&ck2, &chunk_refs);

        let coded = encode_chunk(&chunks, &mut rng);
        let result = PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, &coded, &bad_proof);
        assert!(matches!(result, Err(PedersenError::InvalidSignature)));

        // But the same proof verifies against the other signer's context.
        let other_ctx = other_signer.context::<G1>(ck2);
        assert!(
            PedersenScheme::<G1, bls12381::PrivateKey>::verify(&other_ctx, &coded, &bad_proof)
                .is_ok()
        );
    }

    #[test]
    fn combine_preserves_commitments() {
        let mut rng = ChaCha20Rng::seed_from_u64(99);
        let (_signer, _ck, ctx, chunks, proof) = setup(&mut rng);

        // Create multiple coded chunks with the same proof.
        let coded1 = encode_chunk(&chunks, &mut rng);
        let coded2 = encode_chunk(&chunks, &mut rng);

        assert!(PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, &coded1, &proof).is_ok());
        assert!(PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, &coded2, &proof).is_ok());

        // Combine with arbitrary alphas — result is the same proof.
        let alphas = vec![Scalar::from_u64(3), Scalar::from_u64(7)];
        let combined =
            <PedersenScheme<G1, bls12381::PrivateKey> as IntegrityScheme<Scalar, N>>::combine(
                &[proof.clone(), proof.clone()],
                &alphas,
            );

        assert_eq!(combined.commitments(), proof.commitments());
        assert_eq!(combined.signature(), proof.signature());
    }

    #[test]
    fn signed_packet_roundtrip() {
        let mut rng = ChaCha20Rng::seed_from_u64(55);
        let (_signer, _ck, ctx, chunks, proof) = setup(&mut rng);

        let coded = encode_chunk(&chunks, &mut rng);
        let pkt =
            SignedPacket::<Scalar, N, PedersenScheme<G1, bls12381::PrivateKey>>::new(coded, proof);
        assert!(pkt.verify(&ctx).is_ok());

        let (recovered, _proof) = pkt.into_parts();
        assert_eq!(recovered.w().len(), M);
        assert_eq!(recovered.b().len(), N);
    }

    // --- Ed25519 cross-curve tests ---

    #[test]
    fn ed25519_sign_verify_roundtrip() {
        use commonware_cryptography::ed25519;

        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let signer = PedersenSigner::<ed25519::PrivateKey>::generate(&mut rng);
        let ck = CommitmentKey::<G1>::canonical(M);

        let chunks: Vec<Vec<Scalar>> = (0..N)
            .map(|_| (0..M).map(|_| Scalar::random(&mut rng)).collect())
            .collect();

        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        let proof = signer.sign_block::<G1>(&ck, &chunk_refs);
        let ctx = signer.context::<G1>(ck);

        let coded = encode_chunk(&chunks, &mut rng);
        let result = PedersenScheme::<G1, ed25519::PrivateKey>::verify(&ctx, &coded, &proof);
        assert!(result.is_ok());
    }

    #[test]
    fn ed25519_rejects_wrong_signer() {
        use commonware_cryptography::ed25519;

        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let signer1 = PedersenSigner::<ed25519::PrivateKey>::generate(&mut rng);
        let signer2 = PedersenSigner::<ed25519::PrivateKey>::generate(&mut rng);
        let ck = CommitmentKey::<G1>::canonical(M);

        let chunks: Vec<Vec<Scalar>> = (0..N)
            .map(|_| (0..M).map(|_| Scalar::random(&mut rng)).collect())
            .collect();

        let chunk_refs: Vec<&[Scalar]> = chunks.iter().map(Vec::as_slice).collect();
        // Sign with signer1, but verify with signer2's context.
        let proof = signer1.sign_block::<G1>(&ck, &chunk_refs);
        let ctx = signer2.context::<G1>(ck);

        let coded = encode_chunk(&chunks, &mut rng);
        let result = PedersenScheme::<G1, ed25519::PrivateKey>::verify(&ctx, &coded, &proof);
        assert!(matches!(result, Err(PedersenError::InvalidSignature)));
    }
}
