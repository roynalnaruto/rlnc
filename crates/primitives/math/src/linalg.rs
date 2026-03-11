//! Linear algebra helpers for RLNC encoding and re-encoding.
//!
//! - [`linear_combination`]: compute a linear combination of field-element
//!   vectors (used for encoding original chunks into coded chunks).
//! - [`random_coefficients`]: sample a random coefficient vector.
//! - [`reencode`]: re-encode received coded chunks into new coded chunks
//!   (used by intermediate nodes that combine received packets before
//!   forwarding).

use commonware_math::algebra::{Field, Random};

/// Compute a linear combination of field-element vectors:
///
/// `result[j] = sum_i coeffs[i] * vecs[i][j]`
///
/// # Panics
///
/// Panics if `vecs` is empty, if `vecs` and `coeffs` have different
/// lengths, or if the inner vectors have inconsistent lengths.
pub fn linear_combination<F: Field>(vecs: &[&[F]], coeffs: &[F]) -> Vec<F> {
    assert!(!vecs.is_empty(), "need at least one vector");
    assert_eq!(vecs.len(), coeffs.len(), "vecs and coeffs length mismatch");
    let len = vecs[0].len();
    debug_assert!(
        vecs.iter().all(|v| v.len() == len),
        "all vectors must have the same length"
    );

    let mut result = vec![F::zero(); len];
    for (vec, coeff) in vecs.iter().zip(coeffs) {
        for (r, v) in result.iter_mut().zip(vec.iter()) {
            let term = v.clone() * coeff;
            *r += &term;
        }
    }
    result
}

/// Sample a random coefficient vector of length `n`.
///
/// Uses the [`Random`] trait to generate each element, which for
/// [`Scalar`](crate::Scalar) produces non-zero random field elements.
pub fn random_coefficients<F: Field + Random>(
    n: usize,
    rng: &mut (impl rand::CryptoRng + rand::RngCore),
) -> Vec<F> {
    (0..n).map(|_| F::random(&mut *rng)).collect()
}

/// Re-encode `L` received (data, coefficient) pairs into a single new
/// coded chunk.
///
/// Given `L` coded chunks, each consisting of a data vector and its
/// RLNC coefficient vector, this function computes:
///
/// ```text
/// new_data[j]   = sum_l alphas[l] * data_vecs[l][j]
/// new_coeffs[i] = sum_l alphas[l] * coeff_vecs[l][i]
/// ```
///
/// The `alphas` are the re-encoding coefficients chosen by the
/// intermediate node.
///
/// # Panics
///
/// Panics if `data_vecs`, `coeff_vecs`, and `alphas` have inconsistent
/// lengths, or if any slice is empty.
pub fn reencode<F: Field>(
    data_vecs: &[&[F]],
    coeff_vecs: &[&[F]],
    alphas: &[F],
) -> (Vec<F>, Vec<F>) {
    let new_data = linear_combination(data_vecs, alphas);
    let new_coeffs = linear_combination(coeff_vecs, alphas);
    (new_data, new_coeffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Additive, Ring, Scalar};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn linear_combination_ones() {
        // With all-one coefficients, result is element-wise sum.
        let a = vec![
            Scalar::from_u64(1u64),
            Scalar::from_u64(2u64),
            Scalar::from_u64(3u64),
        ];
        let b = vec![
            Scalar::from_u64(4u64),
            Scalar::from_u64(5u64),
            Scalar::from_u64(6u64),
        ];
        let coeffs = vec![Scalar::one(), Scalar::one()];

        let result = linear_combination(&[&a, &b], &coeffs);
        assert_eq!(result[0], Scalar::from_u64(5u64));
        assert_eq!(result[1], Scalar::from_u64(7u64));
        assert_eq!(result[2], Scalar::from_u64(9u64));
    }

    #[test]
    fn linear_combination_weighted() {
        // 2 * [1, 0] + 3 * [0, 1] = [2, 3]
        let a = vec![Scalar::from_u64(1u64), Scalar::zero()];
        let b = vec![Scalar::zero(), Scalar::from_u64(1u64)];
        let coeffs = vec![Scalar::from_u64(2u64), Scalar::from_u64(3u64)];

        let result = linear_combination(&[&a, &b], &coeffs);
        assert_eq!(result[0], Scalar::from_u64(2u64));
        assert_eq!(result[1], Scalar::from_u64(3u64));
    }

    #[test]
    fn reencode_single_identity() {
        // Re-encoding a single chunk with alpha=1 returns the same vectors.
        let data = vec![Scalar::from_u64(10u64), Scalar::from_u64(20u64)];
        let coeff = vec![Scalar::from_u64(1u64), Scalar::zero()];
        let alphas = vec![Scalar::one()];

        let (new_data, new_coeff) = reencode(&[&data], &[&coeff], &alphas);
        assert_eq!(new_data, data);
        assert_eq!(new_coeff, coeff);
    }

    #[test]
    fn reencode_valid_combination() {
        // Re-encoding two chunks and verifying the result is a valid
        // linear combination of the originals.
        let mut rng = ChaCha20Rng::seed_from_u64(42);

        let d0: Vec<Scalar> = (0..4).map(|_| Scalar::random(&mut rng)).collect();
        let d1: Vec<Scalar> = (0..4).map(|_| Scalar::random(&mut rng)).collect();
        let c0 = vec![Scalar::one(), Scalar::zero()];
        let c1 = vec![Scalar::zero(), Scalar::one()];

        let alpha = Scalar::from_u64(3u64);
        let beta = Scalar::from_u64(5u64);
        let alphas = vec![alpha.clone(), beta.clone()];

        let (new_data, new_coeff) = reencode(&[&d0, &d1], &[&c0, &c1], &alphas);

        // new_coeff should be [alpha, beta] = [3, 5]
        assert_eq!(new_coeff[0], alpha);
        assert_eq!(new_coeff[1], beta);

        // new_data[j] = alpha * d0[j] + beta * d1[j]
        for j in 0..4 {
            let expected = d0[j].clone() * &alpha + &(d1[j].clone() * &beta);
            assert_eq!(new_data[j], expected);
        }
    }

    #[test]
    fn random_coefficients_nonzero() {
        // Scalar::random() returns non-zero scalars. Verify over many samples.
        let mut rng = ChaCha20Rng::seed_from_u64(0);
        let coeffs: Vec<Scalar> = random_coefficients(100, &mut rng);
        assert_eq!(coeffs.len(), 100);
        for c in &coeffs {
            assert_ne!(*c, Scalar::zero());
        }
    }
}
