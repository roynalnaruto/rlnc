//! Shared mathematical foundations for RLNC and LHSS block propagation.
//!
//! This crate provides field-generic linear algebra and incremental
//! row-echelon maintenance over any type implementing [`Field`] from
//! `commonware-math`. The two modules are:
//!
//! - [`echelon`]: Incremental row-echelon form for RLNC decoding — tests
//!   linear independence as coded chunks arrive, then inverts once full
//!   rank is reached.
//! - [`linalg`]: Linear combination, random coefficient sampling, and
//!   re-encoding helpers for RLNC encoding and intermediate-node
//!   re-encoding.
//!
//! All operations are generic over `F: Field`. The concrete scalar used
//! downstream is the BLS12-381 scalar field, re-exported as [`Scalar`].

pub mod echelon;
pub mod linalg;

/// BLS12-381 scalar field element.
pub use commonware_cryptography::bls12381::primitives::group::Scalar;

/// Re-export commonware-math algebraic traits for downstream use.
pub use commonware_math::algebra::{Additive, Field, Multiplicative, Random, Ring};

#[cfg(test)]
mod tests {
    use echelon::IncrementalEchelon;
    use linalg::{linear_combination, random_coefficients};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    use super::*;

    /// Full RLNC encode → decode cycle (no commitments).
    ///
    /// 1. Generate a random "block" as N chunks of M field elements.
    /// 2. Encode K > N coded chunks via linear combination with random
    ///    coefficients.
    /// 3. Feed N coefficient vectors into `IncrementalEchelon`.
    /// 4. Invert and multiply to recover the original chunks.
    #[test]
    fn rlnc_encode_decode_cycle() {
        let mut rng = ChaCha20Rng::seed_from_u64(2025);
        let n = 8; // number of original chunks
        let m = 16; // elements per chunk
        let k = 12; // number of coded chunks (> n)

        // Step 1: random block.
        let block: Vec<Vec<Scalar>> = (0..n)
            .map(|_| (0..m).map(|_| Scalar::random(&mut rng)).collect())
            .collect();

        // Step 2: encode K coded chunks.
        let mut coded_data: Vec<Vec<Scalar>> = Vec::with_capacity(k);
        let mut coded_coeffs: Vec<Vec<Scalar>> = Vec::with_capacity(k);
        for _ in 0..k {
            let coeffs: Vec<Scalar> = random_coefficients(n, &mut rng);
            let chunk_refs: Vec<&[Scalar]> = block.iter().map(Vec::as_slice).collect();
            let coded = linear_combination(&chunk_refs, &coeffs);
            coded_data.push(coded);
            coded_coeffs.push(coeffs);
        }

        // Step 3: feed first N coefficient vectors into echelon.
        let mut ech = IncrementalEchelon::new(n);
        let mut selected_data: Vec<Vec<Scalar>> = Vec::with_capacity(n);
        let mut count = 0;
        for i in 0..k {
            if ech.add_row(&coded_coeffs[i]) {
                selected_data.push(coded_data[i].clone());
                count += 1;
                if count == n {
                    break;
                }
            }
        }
        assert!(ech.is_full());

        // Step 4: decode — recovered = inverse * selected_data.
        let inv = ech.inverse();
        for i in 0..n {
            // recovered_chunk[i] = sum_j inv[i][j] * selected_data[j]
            let data_refs: Vec<&[Scalar]> = selected_data.iter().map(Vec::as_slice).collect();
            let recovered = linear_combination(&data_refs, &inv[i]);
            assert_eq!(recovered, block[i], "chunk {i} mismatch");
        }
    }
}
