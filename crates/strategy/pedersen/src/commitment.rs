//! Pedersen commitment key: reusable generator set for RLNC.
//!
//! A [`CommitmentKey`] holds $m$ generator points derived
//! deterministically via [`HashToGroup`]. These are **protocol
//! constants** — they do not change per block and can be cached as a
//! singleton via [`LazyLock`](std::sync::LazyLock).
//!
//! The key provides:
//! - [`commit`](CommitmentKey::commit): MSM to produce a single group
//!   element $C = \sum_j v_j \cdot G_j$.
//! - [`verify`](CommitmentKey::verify): check that $C(\mathbf{w})$
//!   equals $\sum_i b_i \cdot C_i$ (coded-chunk integrity).

use commonware_math::algebra::{CryptoGroup, HashToGroup};
use commonware_parallel::Sequential;

/// Domain separator for the canonical Pedersen generator derivation.
pub const PEDERSEN_GEN_DST: &[u8] = b"RLNC-PEDERSEN-GEN-v1";

/// Reusable Pedersen commitment key: $m$ generator points
/// $G_1, \ldots, G_m$ derived via hash-to-group.
///
/// Generators are protocol constants — they do not change per block.
/// Use [`canonical`](Self::canonical) for the standard derivation or
/// [`deterministic`](Self::deterministic) for a custom DST.
#[derive(Clone, Debug)]
pub struct CommitmentKey<G: CryptoGroup> {
    generators: Vec<G>,
}

impl<G: HashToGroup> CommitmentKey<G> {
    /// Derive $m$ generators deterministically:
    /// $G_j = \mathrm{hash\_to\_group}(\text{DST}, j)$ for
    /// $j \in [0, m)$.
    pub fn deterministic(m: usize, dst: &[u8]) -> Self {
        let generators = (0..m)
            .map(|j| {
                let index_bytes = (j as u64).to_le_bytes();
                G::hash_to_group(dst, &index_bytes)
            })
            .collect();
        Self { generators }
    }

    /// Create with the canonical DST and the given chunk dimension.
    pub fn canonical(m: usize) -> Self {
        Self::deterministic(m, PEDERSEN_GEN_DST)
    }

    /// Commit to a slice of $m$ scalars:
    /// $C = \mathrm{MSM}(\mathbf{G}, \mathbf{v})$.
    ///
    /// # Panics
    ///
    /// Panics if `scalars.len() != m` (the number of generators).
    #[must_use]
    pub fn commit(&self, scalars: &[G::Scalar]) -> G {
        let m = self.generators.len();
        assert_eq!(
            scalars.len(),
            m,
            "commit expects exactly m={m} scalars, got {}",
            scalars.len()
        );
        G::msm(&self.generators, scalars, &Sequential)
    }

    /// Verify that a coded chunk's data matches its coefficient vector
    /// against the original chunk commitments.
    ///
    /// Checks: $\mathrm{MSM}(\mathbf{G}, \mathbf{w})
    /// \stackrel{?}{=} \mathrm{MSM}(\mathbf{C}, \mathbf{b})$
    ///
    /// where $\mathbf{C} = [C_1, \ldots, C_N]$ are the commitments to
    /// the original chunks.
    #[must_use]
    pub fn verify(&self, w: &[G::Scalar], coefficients: &[G::Scalar], commitments: &[G]) -> bool {
        let lhs = self.commit(w);
        let rhs = G::msm(commitments, coefficients, &Sequential);
        lhs == rhs
    }

    /// Borrow the underlying generator points.
    pub fn generators(&self) -> &[G] {
        &self.generators
    }

    /// Number of generators (the chunk dimension $m$).
    pub fn m(&self) -> usize {
        self.generators.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonware_cryptography::bls12381::primitives::group::{G1, Scalar};
    use commonware_math::algebra::Additive;
    use p2p_primitives_math::Random;

    #[test]
    fn deterministic_generators_are_stable() {
        let ck1 = CommitmentKey::<G1>::canonical(4);
        let ck2 = CommitmentKey::<G1>::canonical(4);
        assert_eq!(ck1.generators(), ck2.generators());
    }

    #[test]
    fn generators_are_not_identity() {
        let ck = CommitmentKey::<G1>::canonical(8);
        for g in ck.generators() {
            assert_ne!(*g, G1::zero(), "generator must not be identity");
        }
    }

    #[test]
    fn commit_roundtrip() {
        let ck = CommitmentKey::<G1>::canonical(4);
        let scalars = vec![
            Scalar::from_u64(1),
            Scalar::from_u64(2),
            Scalar::from_u64(3),
            Scalar::from_u64(4),
        ];
        let c = ck.commit(&scalars);
        // Recomputing must give the same result.
        assert_eq!(c, ck.commit(&scalars));
    }

    #[test]
    fn verify_honest_coded_chunk() {
        use rand::SeedableRng;
        use rand_chacha::ChaCha20Rng;

        let mut rng = ChaCha20Rng::seed_from_u64(42);
        const M: usize = 4;
        const N: usize = 3;

        let ck = CommitmentKey::<G1>::canonical(M);

        // Original chunks.
        let chunks: Vec<Vec<Scalar>> = (0..N)
            .map(|_| (0..M).map(|_| Scalar::random(&mut rng)).collect())
            .collect();

        // Commit each original chunk.
        let commitments: Vec<G1> = chunks.iter().map(|v| ck.commit(v)).collect();

        // Build a coded chunk: w = sum_i b_i * v_i.
        let b: Vec<Scalar> = (0..N).map(|_| Scalar::random(&mut rng)).collect();
        let mut w = vec![Scalar::zero(); M];
        for (bi, vi) in b.iter().zip(&chunks) {
            for (wj, vj) in w.iter_mut().zip(vi) {
                *wj += &(vj.clone() * bi);
            }
        }

        assert!(ck.verify(&w, &b, &commitments));
    }

    #[test]
    fn verify_detects_tampered_data() {
        use rand::SeedableRng;
        use rand_chacha::ChaCha20Rng;

        let mut rng = ChaCha20Rng::seed_from_u64(99);
        const M: usize = 4;
        const N: usize = 2;

        let ck = CommitmentKey::<G1>::canonical(M);

        let chunks: Vec<Vec<Scalar>> = (0..N)
            .map(|_| (0..M).map(|_| Scalar::random(&mut rng)).collect())
            .collect();
        let commitments: Vec<G1> = chunks.iter().map(|v| ck.commit(v)).collect();

        let b: Vec<Scalar> = (0..N).map(|_| Scalar::random(&mut rng)).collect();
        let mut w = vec![Scalar::zero(); M];
        for (bi, vi) in b.iter().zip(&chunks) {
            for (wj, vj) in w.iter_mut().zip(vi) {
                *wj += &(vj.clone() * bi);
            }
        }

        // Tamper with w[0].
        w[0] += &Scalar::from_u64(1);
        assert!(!ck.verify(&w, &b, &commitments));
    }
}
