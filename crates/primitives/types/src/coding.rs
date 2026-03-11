//! RLNC chunk types and block decoder.
//!
//! A block is split into $N$ original chunks
//! $\mathbf{v}_1, \ldots, \mathbf{v}_N \in \mathbb{F}^M$.
//! Coded chunks $\mathbf{w} = \sum_i b_i \cdot \mathbf{v}_i$
//! are transmitted alongside their coefficient vectors
//! $\mathbf{b} \in \mathbb{F}^N$.
//!
//! [`BlockDecoder`] collects coded chunks from the network, tracks
//! linear independence of their encoding vectors, and recovers the
//! original $N$ block chunks once full rank is reached.

use core::ops::Deref;
use p2p_primitives_math::echelon::IncrementalEchelon;
use p2p_primitives_math::linalg;
use p2p_primitives_math::{Field, Random};

/// An original chunk of block data: $m$ field elements.
///
/// Matches the presentation notation: $\mathbf{v}_i \in \mathbb{F}^m$.
/// A block is split into $N$ such chunks. Implements
/// [`Deref<Target = [F]>`] for seamless interop with
/// `p2p-primitives-math` functions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk<F: Field> {
    /// The chunk data vector ($\mathbf{v}_i$).
    v: Vec<F>,
}

impl<F: Field> Chunk<F> {
    /// Create a new chunk from `m` field elements.
    ///
    /// # Panics
    ///
    /// Panics if `v` is empty.
    pub fn new(v: &[F]) -> Self {
        assert!(!v.is_empty(), "chunk must not be empty");
        Self { v: v.to_vec() }
    }

    /// Create a new chunk, asserting that `v.len() == m`.
    ///
    /// # Panics
    ///
    /// Panics if `v.len() != m`.
    pub fn new_checked(v: &[F], m: usize) -> Self {
        assert_eq!(
            v.len(),
            m,
            "chunk must contain exactly m={m} elements, got {}",
            v.len()
        );
        Self { v: v.to_vec() }
    }

    /// Borrow the underlying field-element slice $\mathbf{v}_i$.
    pub fn v(&self) -> &[F] {
        &self.v
    }

    /// Consume the chunk and return the inner vector.
    pub fn into_inner(self) -> Vec<F> {
        self.v
    }
}

impl<F: Field> Deref for Chunk<F> {
    type Target = [F];

    fn deref(&self) -> &Self::Target {
        &self.v
    }
}

/// A coded chunk for network transmission.
///
/// Contains $m$ coded field elements
/// $\mathbf{w} \in \mathbb{F}^m$ (a linear combination of original
/// chunks: $\mathbf{w} = \sum_i b_i \cdot \mathbf{v}_i$) and $N$
/// encoding coefficients $\mathbf{b} \in \mathbb{F}^N$.
///
/// This is the common data unit shared by both RLNC and LHSS — each
/// strategy wraps it with its own integrity proof (Pedersen
/// commitments or BFKW signature).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodedChunk<F: Field, const N: usize> {
    /// Coded data vector
    /// ($\mathbf{w} = \sum_i b_i \cdot \mathbf{v}_i$, length $m$).
    w: Chunk<F>,
    /// Encoding coefficient vector ($\mathbf{b} \in \mathbb{F}^N$).
    b: Vec<F>,
}

impl<F: Field, const N: usize> CodedChunk<F, N> {
    /// Create from pre-computed coded data and coefficients.
    ///
    /// # Panics
    ///
    /// Panics if `b.len() != N`.
    pub fn new(w: Chunk<F>, b: Vec<F>) -> Self {
        assert_eq!(
            b.len(),
            N,
            "coefficient vector must have length N={N}, got {}",
            b.len()
        );
        Self { w, b }
    }

    /// The coded data vector $\mathbf{w}$ ($m$ field elements).
    pub const fn w(&self) -> &Chunk<F> {
        &self.w
    }

    /// The encoding coefficient vector $\mathbf{b}$ ($N$ elements).
    pub fn b(&self) -> &[F] {
        &self.b
    }

    /// Consume and return $(\mathbf{w}, \mathbf{b})$.
    pub fn into_parts(self) -> (Chunk<F>, Vec<F>) {
        (self.w, self.b)
    }
}

/// Stateful decoder that collects coded chunks and recovers the
/// original $N$ block chunks.
///
/// Tracks linear independence via [`IncrementalEchelon`]. Coded
/// chunks whose encoding vectors $\mathbf{b}$ are linearly dependent
/// on previously received ones are discarded.
///
/// Used by receiving nodes. Strategy-specific integrity verification
/// (Pedersen or BFKW) should be performed **before** adding coded
/// chunks to the decoder.
#[derive(Clone, Debug)]
pub struct BlockDecoder<F: Field, const N: usize> {
    /// Coded chunks accepted so far (linearly independent only).
    coded_chunks: Vec<CodedChunk<F, N>>,
    /// Echelon tracker for encoding coefficient independence.
    echelon: IncrementalEchelon<F>,
}

impl<F: Field, const N: usize> BlockDecoder<F, N> {
    /// Create a new decoder expecting $N$ independent coded chunks.
    pub fn new() -> Self {
        Self {
            coded_chunks: Vec::with_capacity(N),
            echelon: IncrementalEchelon::new(N),
        }
    }

    /// Add a coded chunk. Returns `true` if its encoding vector
    /// $\mathbf{b}$ is linearly independent (accepted), `false` if
    /// dependent (discarded).
    pub fn add(&mut self, coded: CodedChunk<F, N>) -> bool {
        if self.echelon.is_full() {
            return false;
        }
        if self.echelon.add_row(coded.b()) {
            self.coded_chunks.push(coded);
            true
        } else {
            false
        }
    }

    /// Number of independent coded chunks collected.
    pub fn rank(&self) -> usize {
        self.echelon.rank()
    }

    /// Whether $N$ independent coded chunks have been collected
    /// (ready to decode).
    pub fn is_complete(&self) -> bool {
        self.echelon.is_full()
    }

    /// Decode and return the $N$ original chunks
    /// $\mathbf{v}_1, \ldots, \mathbf{v}_N$.
    ///
    /// Computes $\mathbf{V} = B^{-1} \cdot \mathbf{W}$ where $B$ is
    /// the $N \times N$ coefficient matrix and $\mathbf{W}$ is the
    /// matrix of coded data vectors.
    ///
    /// # Panics
    ///
    /// Panics if [`is_complete`](Self::is_complete) returns `false`.
    pub fn decode(&self) -> Vec<Chunk<F>> {
        assert!(
            self.is_complete(),
            "cannot decode: need N={N} independent chunks"
        );

        let inv = self.echelon.inverse();
        let data_refs: Vec<&[F]> = self.coded_chunks.iter().map(|c| c.w().v()).collect();

        (0..N)
            .map(|i| {
                let recovered = linalg::linear_combination(&data_refs, &inv[i]);
                Chunk::new(&recovered)
            })
            .collect()
    }

    /// Re-encode with caller-provided mixing coefficients.
    ///
    /// Like [`reencode`](Self::reencode) but accepts pre-generated
    /// $\alpha_i$ so the caller can pass the same coefficients to
    /// `IntegrityScheme::combine`
    /// for proof re-combination.
    ///
    /// # Panics
    ///
    /// Panics if `alphas.len()` does not equal the number of collected
    /// coded chunks, or if no coded chunks have been collected.
    pub fn reencode_with(&self, alphas: &[F]) -> CodedChunk<F, N> {
        assert!(
            !self.coded_chunks.is_empty(),
            "cannot reencode: no coded chunks"
        );
        assert_eq!(
            alphas.len(),
            self.coded_chunks.len(),
            "alphas length ({}) must equal collected chunk count ({})",
            alphas.len(),
            self.coded_chunks.len()
        );

        let data_refs: Vec<&[F]> = self.coded_chunks.iter().map(|c| c.w().v()).collect();
        let coeff_refs: Vec<&[F]> = self.coded_chunks.iter().map(CodedChunk::b).collect();

        let (new_data, new_coeffs) = linalg::reencode(&data_refs, &coeff_refs, alphas);
        CodedChunk::new(Chunk::new(&new_data), new_coeffs)
    }

    /// Re-encode from collected coded chunks to produce a fresh coded
    /// chunk for forwarding to peers.
    ///
    /// Samples random mixing coefficients $\alpha_i$ and computes:
    ///
    /// $$\mathbf{w}' = \sum_i \alpha_i \cdot \mathbf{w}_i, \quad
    /// \mathbf{b}' = \sum_i \alpha_i \cdot \mathbf{b}_i$$
    ///
    /// Used by intermediate nodes to combine received coded chunks
    /// into a new coded chunk without decoding.
    ///
    /// # Panics
    ///
    /// Panics if no coded chunks have been collected.
    pub fn reencode(&self, rng: &mut (impl rand::CryptoRng + rand::RngCore)) -> CodedChunk<F, N>
    where
        F: Random,
    {
        let l = self.coded_chunks.len();
        let alphas = linalg::random_coefficients(l, rng);
        self.reencode_with(&alphas)
    }

    /// Reset for the next block, clearing all collected chunks.
    pub fn reset(&mut self) {
        self.coded_chunks.clear();
        self.echelon.reset();
    }
}

impl<F: Field, const N: usize> Default for BlockDecoder<F, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p2p_primitives_math::{Additive, Ring, Scalar};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    /// Build N random original chunks of M elements each.
    fn random_chunks<const N: usize>(m: usize, rng: &mut ChaCha20Rng) -> Vec<Chunk<Scalar>> {
        (0..N)
            .map(|_| {
                Chunk::new(
                    &(0..m)
                        .map(|_| Scalar::random(&mut *rng))
                        .collect::<Vec<Scalar>>(),
                )
            })
            .collect()
    }

    /// Encode one coded chunk from original chunks with random coefficients.
    fn encode_one<const N: usize>(
        chunks: &[Chunk<Scalar>],
        rng: &mut ChaCha20Rng,
    ) -> CodedChunk<Scalar, N> {
        let coeffs = linalg::random_coefficients(N, rng);
        let refs: Vec<&[Scalar]> = chunks.iter().map(|c| c.v()).collect();
        let coded_data = linalg::linear_combination(&refs, &coeffs);
        CodedChunk::new(Chunk::new(&coded_data), coeffs)
    }

    #[test]
    fn chunk_construction() {
        let v: Vec<Scalar> = (0..4).map(|i| Scalar::from_u64(i)).collect();
        let chunk: Chunk<Scalar> = Chunk::new(&v);
        assert_eq!(chunk.v(), &v[..]);
        assert_eq!(chunk.len(), 4);
    }

    #[test]
    #[should_panic(expected = "chunk must contain exactly m=4 elements")]
    fn chunk_wrong_length() {
        let v: Vec<Scalar> = vec![Scalar::zero(); 3];
        Chunk::<Scalar>::new_checked(&v, 4);
    }

    #[test]
    fn chunk_deref() {
        let v: Vec<Scalar> = (0..4).map(|i| Scalar::from_u64(i)).collect();
        let chunk: Chunk<Scalar> = Chunk::new(&v);
        let slice: &[Scalar] = &chunk;
        assert_eq!(slice, &v[..]);
    }

    #[test]
    fn coded_chunk_fields() {
        let w = Chunk::<Scalar>::new(&[Scalar::from_u64(10), Scalar::from_u64(20)]);
        let b = vec![Scalar::one(), Scalar::zero(), Scalar::from_u64(3)];
        let coded = CodedChunk::<Scalar, 3>::new(w.clone(), b.clone());
        assert_eq!(coded.w(), &w);
        assert_eq!(coded.b(), &b[..]);
    }

    #[test]
    #[should_panic(expected = "coefficient vector must have length N=3")]
    fn coded_chunk_wrong_b_length() {
        let w = Chunk::<Scalar>::new(vec![Scalar::zero(); 2].as_slice());
        let b = vec![Scalar::zero(); 2]; // should be 3
        CodedChunk::<Scalar, 3>::new(w, b);
    }

    #[test]
    fn decoder_lifecycle() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        const M: usize = 8;
        const N: usize = 4;

        let originals = random_chunks::<N>(M, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        assert_eq!(decoder.rank(), 0);
        assert!(!decoder.is_complete());

        for _ in 0..N {
            let coded = encode_one::<N>(&originals, &mut rng);
            assert!(decoder.add(coded));
        }
        assert!(decoder.is_complete());
        assert_eq!(decoder.rank(), N);

        let recovered = decoder.decode();
        assert_eq!(recovered.len(), N);
        for (orig, rec) in originals.iter().zip(&recovered) {
            assert_eq!(orig, rec);
        }
    }

    #[test]
    fn decoder_dependent_rejection() {
        let mut rng = ChaCha20Rng::seed_from_u64(99);
        const M: usize = 4;
        const N: usize = 3;

        let originals = random_chunks::<N>(M, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        let coded0 = encode_one::<N>(&originals, &mut rng);
        assert!(decoder.add(coded0.clone()));

        // Add a dependent chunk (same coefficients → same row).
        assert!(!decoder.add(coded0));
        assert_eq!(decoder.rank(), 1);
    }

    #[test]
    fn decoder_excess_chunks() {
        let mut rng = ChaCha20Rng::seed_from_u64(77);
        const M: usize = 4;
        const N: usize = 3;

        let originals = random_chunks::<N>(M, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        let mut added = 0;
        for _ in 0..10 {
            let coded = encode_one::<N>(&originals, &mut rng);
            if decoder.add(coded) {
                added += 1;
            }
            if decoder.is_complete() {
                break;
            }
        }
        assert_eq!(added, N);
        assert!(decoder.is_complete());

        let recovered = decoder.decode();
        for (orig, rec) in originals.iter().zip(&recovered) {
            assert_eq!(orig, rec);
        }
    }

    #[test]
    fn decoder_reencode() {
        let mut rng = ChaCha20Rng::seed_from_u64(55);
        const M: usize = 8;
        const N: usize = 4;

        let originals = random_chunks::<N>(M, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        for _ in 0..2 {
            let coded = encode_one::<N>(&originals, &mut rng);
            assert!(decoder.add(coded));
        }
        assert_eq!(decoder.rank(), 2);

        let reencoded = decoder.reencode(&mut rng);
        assert_eq!(reencoded.w().len(), M);
        assert_eq!(reencoded.b().len(), N);

        // Verify w' = sum_i b'_i * v_i.
        let orig_refs: Vec<&[Scalar]> = originals.iter().map(|c| c.v()).collect();
        let expected_w = linalg::linear_combination(&orig_refs, reencoded.b());
        assert_eq!(reencoded.w().v(), &expected_w[..]);
    }

    #[test]
    fn decoder_reencode_with_matches_reencode() {
        let mut rng = ChaCha20Rng::seed_from_u64(55);
        const M: usize = 8;
        const N: usize = 4;

        let originals = random_chunks::<N>(M, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        for _ in 0..2 {
            let coded = encode_one::<N>(&originals, &mut rng);
            assert!(decoder.add(coded));
        }

        // Generate alphas, then compare reencode_with vs manual computation.
        let alphas = linalg::random_coefficients(2, &mut rng);
        let reencoded = decoder.reencode_with(&alphas);

        // Verify result is a valid linear combination of originals.
        let orig_refs: Vec<&[Scalar]> = originals.iter().map(|c| c.v()).collect();
        let expected_w = linalg::linear_combination(&orig_refs, reencoded.b());
        assert_eq!(reencoded.w().v(), &expected_w[..]);
    }

    #[test]
    #[should_panic(expected = "alphas length")]
    fn decoder_reencode_with_wrong_alpha_len() {
        let mut rng = ChaCha20Rng::seed_from_u64(77);
        const M: usize = 4;
        const N: usize = 3;

        let originals = random_chunks::<N>(M, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        let coded = encode_one::<N>(&originals, &mut rng);
        assert!(decoder.add(coded));

        // Wrong number of alphas — should panic.
        let alphas = linalg::random_coefficients(5, &mut rng);
        let _ = decoder.reencode_with(&alphas);
    }

    #[test]
    #[should_panic(expected = "cannot reencode: no coded chunks")]
    fn decoder_reencode_with_empty() {
        let decoder = BlockDecoder::<Scalar, 3>::new();
        let _ = decoder.reencode_with(&[]);
    }

    #[test]
    fn decoder_reset() {
        let mut rng = ChaCha20Rng::seed_from_u64(11);
        const M: usize = 4;
        const N: usize = 2;

        let originals = random_chunks::<N>(M, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        for _ in 0..N {
            let coded = encode_one::<N>(&originals, &mut rng);
            assert!(decoder.add(coded));
        }
        assert!(decoder.is_complete());

        decoder.reset();
        assert_eq!(decoder.rank(), 0);
        assert!(!decoder.is_complete());

        let originals2 = random_chunks::<N>(M, &mut rng);
        for _ in 0..N {
            let coded = encode_one::<N>(&originals2, &mut rng);
            assert!(decoder.add(coded));
        }
        assert!(decoder.is_complete());
        let recovered = decoder.decode();
        for (orig, rec) in originals2.iter().zip(&recovered) {
            assert_eq!(orig, rec);
        }
    }
}
