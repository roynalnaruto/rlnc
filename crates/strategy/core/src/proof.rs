//! Integrity verification abstraction for coded chunks.
//!
//! Defines [`IntegrityScheme`], the trait that strategy crates implement
//! to attach and verify integrity proofs on [`CodedChunk`]s, and
//! [`SignedPacket`], the wire-level wrapper that bundles a coded chunk
//! with its proof.
//!
//! - **Pedersen** (`p2p-pedersen`): proof = $N$ G1 commitments + BLS
//!   signature. Verification is a pure MSM equality check (no pairing).
//! - **BFKW** (`p2p-bfkw`): proof = single G2 signature. Verification
//!   uses a bilinear pairing check.
//!
//! The proposer creates proofs via a strategy-specific `Signer` (which
//! holds the secret key). Intermediate nodes use
//! [`IntegrityScheme::combine`] to derive a proof for a re-encoded
//! chunk from the proofs of its constituents.

use commonware_codec::{FixedSize, Read as CodecRead, Write as CodecWrite};
use commonware_math::algebra::Additive;
use p2p_primitives_math::Field;
use p2p_primitives_types::{Chunk, CodedChunk, read_field};

/// Integrity verification scheme for coded chunks.
///
/// Generic over the scalar field `F` and $N$ (chunks per block).
///
/// Strategy crates implement this trait, parameterized by curve
/// types that supply the concrete group elements.
///
/// There is **no `prove` method** on this trait — the proposer needs
/// a secret key (not in [`Context`](Self::Context)). Each strategy
/// provides a separate `Signer` struct for proof creation.
/// Intermediate nodes use [`combine`](Self::combine).
pub trait IntegrityScheme<F: Field, const N: usize>: Sized + Clone + Send + Sync + 'static {
    /// Proof material attached to a [`CodedChunk`] on the wire.
    type Proof: Clone + Send + Sync + 'static;

    /// Per-block verification context (generators, public key, hash
    /// points, etc.). Distributed to all nodes; must **not** contain
    /// secret keys.
    type Context: Clone + Send + Sync + 'static;

    /// Verification error type.
    type Error: core::fmt::Debug + Send + Sync + 'static;

    /// Verify a proof for a received coded chunk.
    ///
    /// Must be called **before**
    /// [`BlockDecoder::add`](p2p_primitives_types::BlockDecoder::add).
    ///
    /// # Errors
    ///
    /// Returns `Self::Error` if the proof does not match the chunk
    /// under the given context (e.g. commitment mismatch, invalid
    /// signature, or failed pairing check).
    fn verify(
        ctx: &Self::Context,
        chunk: &CodedChunk<F, N>,
        proof: &Self::Proof,
    ) -> Result<(), Self::Error>;

    /// Combine $L$ proofs into one for a re-encoded chunk.
    ///
    /// `alphas` are the same mixing coefficients passed to
    /// [`BlockDecoder::reencode_with`](p2p_primitives_types::BlockDecoder::reencode_with).
    ///
    /// - **Pedersen**: clone first proof (commitments to originals are
    ///   invariant under re-encoding).
    /// - **BFKW**: $\sigma' = \mathrm{MSM}([\sigma_1 \ldots \sigma_L],
    ///   \alpha)$ — true homomorphic combination.
    fn combine(proofs: &[Self::Proof], alphas: &[F]) -> Self::Proof;
}

/// Wire serialization for integrity proofs.
///
/// Each [`IntegrityScheme::Proof`] implements this trait so that
/// [`SignedPacket`] can provide unified `serialize` / `deserialize`
/// methods. The coded-chunk portion (w + b scalars) is identical
/// across schemes; only the proof bytes differ.
pub trait IntegrityProof: Clone + Send + Sync + 'static {
    /// Append the proof's wire bytes to `buf`.
    fn serialize_proof(&self, buf: &mut Vec<u8>);

    /// Read a proof from `buf`, advancing past consumed bytes.
    ///
    /// # Errors
    ///
    /// Returns a descriptive error if the buffer is malformed.
    fn deserialize_proof(buf: &mut &[u8]) -> Result<Self, String>;

    /// Exact wire size in bytes (for buffer pre-allocation).
    fn proof_wire_size(&self) -> usize;
}

/// Trivial [`IntegrityProof`] for [`NoopScheme`] (`Proof = ()`).
impl IntegrityProof for () {
    fn serialize_proof(&self, _buf: &mut Vec<u8>) {}

    fn deserialize_proof(_buf: &mut &[u8]) -> Result<Self, String> {
        Ok(())
    }

    fn proof_wire_size(&self) -> usize {
        0
    }
}

/// A coded chunk bundled with its integrity proof for wire transmission.
///
/// This is the unit that travels the network. Receivers call
/// [`verify`](Self::verify) before feeding the inner
/// [`CodedChunk`] to the decoder.
#[derive(Clone, Debug)]
pub struct SignedPacket<F, const N: usize, S>
where
    F: Field,
    S: IntegrityScheme<F, N>,
{
    chunk: CodedChunk<F, N>,
    proof: S::Proof,
}

impl<F, const N: usize, S> SignedPacket<F, N, S>
where
    F: Field,
    S: IntegrityScheme<F, N>,
{
    /// Create a new signed packet from a coded chunk and its proof.
    pub const fn new(chunk: CodedChunk<F, N>, proof: S::Proof) -> Self {
        Self { chunk, proof }
    }

    /// Verify the integrity proof against the given context.
    ///
    /// # Errors
    ///
    /// Returns `S::Error` if the proof fails verification.
    pub fn verify(&self, ctx: &S::Context) -> Result<(), S::Error> {
        S::verify(ctx, &self.chunk, &self.proof)
    }

    /// Borrow the inner coded chunk.
    #[must_use]
    pub const fn chunk(&self) -> &CodedChunk<F, N> {
        &self.chunk
    }

    /// Borrow the proof.
    #[must_use]
    pub const fn proof(&self) -> &S::Proof {
        &self.proof
    }

    /// Consume and return `(CodedChunk, Proof)`.
    #[must_use]
    pub fn into_parts(self) -> (CodedChunk<F, N>, S::Proof) {
        (self.chunk, self.proof)
    }
}

/// Wire serialization for [`SignedPacket`] when the proof implements
/// [`IntegrityProof`].
impl<F, const N: usize, S> SignedPacket<F, N, S>
where
    F: Field + FixedSize + CodecWrite + CodecRead<Cfg = ()> + Additive,
    S: IntegrityScheme<F, N>,
    S::Proof: IntegrityProof,
{
    /// Serialize the packet to a byte vector.
    ///
    /// Layout: `[w (m scalars) | b (N scalars) | proof bytes]`.
    pub fn serialize(&self) -> Vec<u8> {
        let m = self.chunk.w().len();
        let total = F::SIZE * m + F::SIZE * N + self.proof.proof_wire_size();
        let mut buf = Vec::with_capacity(total);
        for elem in self.chunk.w().v() {
            elem.write(&mut buf);
        }
        for elem in self.chunk.b() {
            elem.write(&mut buf);
        }
        self.proof.serialize_proof(&mut buf);
        buf
    }

    /// Deserialize a packet from bytes.
    ///
    /// `m` is the chunk dimension (number of field elements per chunk).
    ///
    /// # Errors
    ///
    /// Returns an error if the buffer is malformed or too short.
    pub fn deserialize(data: &[u8], m: usize) -> Result<Self, String> {
        let mut buf = data;
        let mut w = Vec::with_capacity(m);
        for _ in 0..m {
            w.push(read_field::<F>(&mut buf)?);
        }
        let mut b = Vec::with_capacity(N);
        for _ in 0..N {
            b.push(read_field::<F>(&mut buf)?);
        }
        let proof = S::Proof::deserialize_proof(&mut buf)?;
        let chunk = CodedChunk::new(Chunk::new(&w), b);
        Ok(Self::new(chunk, proof))
    }

    /// Wire size in bytes (for bandwidth accounting).
    pub fn wire_size(&self) -> usize {
        let m = self.chunk.w().len();
        F::SIZE * m + F::SIZE * N + self.proof.proof_wire_size()
    }
}

/// A no-op integrity scheme that always verifies successfully.
///
/// Useful for testing coded pipelines without cryptographic overhead.
/// Generic over the field `F` and $N$.
#[derive(Clone, Debug)]
pub struct NoopScheme;

impl<F: Field, const N: usize> IntegrityScheme<F, N> for NoopScheme {
    type Proof = ();
    type Context = ();
    type Error = core::convert::Infallible;

    fn verify(
        _ctx: &(),
        _chunk: &CodedChunk<F, N>,
        _proof: &(),
    ) -> Result<(), core::convert::Infallible> {
        Ok(())
    }

    fn combine(_proofs: &[()], _alphas: &[F]) {}
}

#[cfg(test)]
mod tests {
    use p2p_primitives_math::Additive;
    use p2p_primitives_types::{Chunk, Scalar};

    use super::*;

    fn sample_coded_chunk() -> CodedChunk<Scalar, 2> {
        let w = Chunk::new(&[Scalar::from_u64(1), Scalar::from_u64(2)]);
        let b = vec![Scalar::from_u64(1), Scalar::zero()];
        CodedChunk::new(w, b)
    }

    #[test]
    fn signed_packet_verify_noop() {
        let chunk = sample_coded_chunk();
        let pkt = SignedPacket::<Scalar, 2, NoopScheme>::new(chunk, ());
        assert!(pkt.verify(&()).is_ok());
    }

    #[test]
    fn signed_packet_into_parts_roundtrip() {
        let chunk = sample_coded_chunk();
        let original_w = chunk.w().v().to_vec();
        let original_b = chunk.b().to_vec();
        let pkt = SignedPacket::<Scalar, 2, NoopScheme>::new(chunk, ());

        let (recovered_chunk, _proof) = pkt.into_parts();
        assert_eq!(recovered_chunk.w().v(), &original_w[..]);
        assert_eq!(recovered_chunk.b(), &original_b[..]);
    }

    #[test]
    fn signed_packet_accessors() {
        let chunk = sample_coded_chunk();
        let pkt = SignedPacket::<Scalar, 2, NoopScheme>::new(chunk, ());
        assert_eq!(pkt.chunk().w().v()[0], Scalar::from_u64(1));
        assert_eq!(*pkt.proof(), ());
    }
}
