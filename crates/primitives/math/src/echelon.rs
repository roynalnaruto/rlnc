//! Incremental row-echelon form maintenance for RLNC decoding.
//!
//! As coded chunks arrive one at a time, [`IncrementalEchelon`] tests
//! each coefficient vector for linear independence against previously
//! received vectors. Dependent rows are discarded. Once `N` independent
//! rows have been collected (full rank), the coefficient matrix can be
//! inverted to decode the original block chunks.
//!
//! The algorithm performs Gaussian elimination incrementally during
//! insertion and tracks an augmented transformation matrix so that
//! [`IncrementalEchelon::inverse`] only needs a back-substitution pass.

use commonware_math::algebra::Field;

/// Maintains a coefficient matrix in row-echelon form as rows arrive
/// one at a time. Used for RLNC decoding.
///
/// Tracks a transformation matrix so that the original coefficient
/// matrix can be inverted when full rank is reached.
#[derive(Clone, Debug)]
pub struct IncrementalEchelon<F: Field> {
    /// Number of columns (= number of original chunks N).
    n: usize,
    /// Rows in echelon form, each with pivot normalized to one.
    echelon: Vec<Vec<F>>,
    /// Pivot column index for each echelon row.
    pivots: Vec<usize>,
    /// Transformation matrix tracking row operations applied to derive
    /// each echelon row from the original input rows.
    transform: Vec<Vec<F>>,
}

impl<F: Field> IncrementalEchelon<F> {
    /// Create a new echelon tracker for `n`-column coefficient vectors.
    pub fn new(n: usize) -> Self {
        Self {
            n,
            echelon: Vec::with_capacity(n),
            pivots: Vec::with_capacity(n),
            transform: Vec::with_capacity(n),
        }
    }

    /// Insert a new coefficient row. Returns `true` if the row is
    /// linearly independent (inserted), `false` if dependent (discarded).
    ///
    /// # Panics
    ///
    /// Panics if `row.len() != n` or if the matrix is already full rank.
    pub fn add_row(&mut self, row: &[F]) -> bool {
        assert_eq!(row.len(), self.n, "row length must equal n");
        assert!(!self.is_full(), "matrix is already full rank");

        let rank = self.echelon.len();
        let mut row = row.to_vec();

        // Transformation row: starts as e_{rank} (unit vector for this
        // input's position among the independent rows collected so far).
        let mut trow = vec![F::zero(); self.n];
        trow[rank] = F::one();

        // Reduce against existing echelon rows.
        for j in 0..rank {
            let pivot_col = self.pivots[j];
            if row[pivot_col] == F::zero() {
                continue;
            }
            let factor = row[pivot_col].clone();
            for (r, e) in row.iter_mut().zip(&self.echelon[j]) {
                let sub = e.clone() * &factor;
                *r -= &sub;
            }
            for (t, e) in trow.iter_mut().zip(&self.transform[j]) {
                let sub = e.clone() * &factor;
                *t -= &sub;
            }
        }

        // Find first non-zero entry — that becomes the pivot.
        let Some(pivot) = row.iter().position(|x| *x != F::zero()) else {
            return false;
        };

        // Normalize so the pivot entry is one.
        let inv = row[pivot].inv();
        for x in &mut row {
            *x *= &inv;
        }
        for x in &mut trow {
            *x *= &inv;
        }

        self.echelon.push(row);
        self.pivots.push(pivot);
        self.transform.push(trow);
        true
    }

    /// Number of independent rows collected so far.
    pub fn rank(&self) -> usize {
        self.echelon.len()
    }

    /// Whether `n` independent rows have been collected (ready to decode).
    pub fn is_full(&self) -> bool {
        self.echelon.len() == self.n
    }

    /// Compute the inverse of the coefficient matrix formed by the `n`
    /// independent rows received so far.
    ///
    /// # Panics
    ///
    /// Panics if the matrix is not full rank ([`is_full`](Self::is_full)
    /// returns `false`).
    pub fn inverse(&self) -> Vec<Vec<F>> {
        assert!(self.is_full(), "cannot invert: matrix is not full rank");

        let n = self.n;
        let mut echelon = self.echelon.clone();
        let mut transform = self.transform.clone();

        // Back-substitution: clear entries above each pivot.
        for j in (0..n).rev() {
            let pivot_col = self.pivots[j];
            for i in 0..j {
                if echelon[i][pivot_col] == F::zero() {
                    continue;
                }
                let factor = echelon[i][pivot_col].clone();
                let (upper, lower) = echelon.split_at_mut(j);
                let ech_j = &lower[0];
                for (ei, ej) in upper[i].iter_mut().zip(ech_j) {
                    let sub = ej.clone() * &factor;
                    *ei -= &sub;
                }
                let (upper, lower) = transform.split_at_mut(j);
                let tr_j = &lower[0];
                for (ti, tj) in upper[i].iter_mut().zip(tr_j) {
                    let sub = tj.clone() * &factor;
                    *ti -= &sub;
                }
            }
        }

        // Reorder rows: echelon row j has its sole 1 at column pivots[j],
        // so it corresponds to row pivots[j] of the identity. Place
        // transform[j] at inverse row pivots[j].
        let mut inverse = vec![vec![F::zero(); n]; n];
        for j in 0..n {
            inverse[self.pivots[j]] = core::mem::take(&mut transform[j]);
        }
        inverse
    }

    /// Reset to empty state, ready to process the next block.
    pub fn reset(&mut self) {
        self.echelon.clear();
        self.pivots.clear();
        self.transform.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ring, Scalar};
    use commonware_math::algebra::Random;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    /// Multiply two square matrices represented as `Vec<Vec<F>>`.
    fn mat_mul<F: Field>(a: &[Vec<F>], b: &[Vec<F>]) -> Vec<Vec<F>> {
        let n = a.len();
        let mut result = vec![vec![F::zero(); n]; n];
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    let term = a[i][k].clone() * &b[k][j];
                    result[i][j] += &term;
                }
            }
        }
        result
    }

    /// Check whether a matrix is the identity.
    fn is_identity<F: Field>(m: &[Vec<F>]) -> bool {
        let n = m.len();
        for i in 0..n {
            for j in 0..n {
                let expected = if i == j { F::one() } else { F::zero() };
                if m[i][j] != expected {
                    return false;
                }
            }
        }
        true
    }

    fn random_matrix_impl(rng: &mut ChaCha20Rng, n: usize) -> Vec<Vec<Scalar>> {
        (0..n)
            .map(|_| (0..n).map(|_| Scalar::random(&mut *rng)).collect())
            .collect()
    }

    #[test]
    fn inverse_3x3_random() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let matrix = random_matrix_impl(&mut rng, 3);
        let mut ech = IncrementalEchelon::new(3);
        for row in &matrix {
            assert!(ech.add_row(row));
        }
        assert!(ech.is_full());

        let inv = ech.inverse();
        let product = mat_mul(&inv, &matrix);
        assert!(is_identity(&product));
    }

    #[test]
    fn dependent_row_rejected() {
        let mut rng = ChaCha20Rng::seed_from_u64(99);
        let matrix = random_matrix_impl(&mut rng, 3);
        let mut ech = IncrementalEchelon::new(3);
        assert!(ech.add_row(&matrix[0]));
        assert!(ech.add_row(&matrix[1]));

        // Linear combination of first two rows.
        let alpha = Scalar::random(&mut rng);
        let beta = Scalar::random(&mut rng);
        let dep: Vec<Scalar> = (0..3)
            .map(|j| {
                let a = matrix[0][j].clone() * &alpha;
                let b = matrix[1][j].clone() * &beta;
                a + &b
            })
            .collect();
        assert!(!ech.add_row(&dep));
        assert_eq!(ech.rank(), 2);
    }

    #[test]
    fn same_row_twice() {
        let mut rng = ChaCha20Rng::seed_from_u64(7);
        let row: Vec<Scalar> = (0..4).map(|_| Scalar::random(&mut rng)).collect();
        let mut ech = IncrementalEchelon::new(4);
        assert!(ech.add_row(&row));
        assert!(!ech.add_row(&row));
        assert_eq!(ech.rank(), 1);
    }

    #[test]
    fn inverse_10x10_random() {
        let mut rng = ChaCha20Rng::seed_from_u64(123);
        let n = 10;
        let matrix = random_matrix_impl(&mut rng, n);
        let mut ech = IncrementalEchelon::new(n);
        for row in &matrix {
            assert!(ech.add_row(row));
        }
        let inv = ech.inverse();
        let product = mat_mul(&inv, &matrix);
        assert!(is_identity(&product));
    }

    #[test]
    fn incremental_rank() {
        let mut rng = ChaCha20Rng::seed_from_u64(55);
        let n = 5;
        let matrix = random_matrix_impl(&mut rng, n);
        let mut ech = IncrementalEchelon::new(n);
        for (i, row) in matrix.iter().enumerate() {
            assert!(!ech.is_full());
            assert_eq!(ech.rank(), i);
            assert!(ech.add_row(row));
        }
        assert!(ech.is_full());
        assert_eq!(ech.rank(), n);
    }

    #[test]
    fn reset_clears_state() {
        let mut rng = ChaCha20Rng::seed_from_u64(77);
        let matrix = random_matrix_impl(&mut rng, 3);
        let mut ech = IncrementalEchelon::new(3);
        for row in &matrix {
            assert!(ech.add_row(row));
        }
        assert!(ech.is_full());

        ech.reset();
        assert_eq!(ech.rank(), 0);
        assert!(!ech.is_full());

        // Can reuse after reset.
        let matrix2 = random_matrix_impl(&mut rng, 3);
        for row in &matrix2 {
            assert!(ech.add_row(row));
        }
        assert!(ech.is_full());
        let inv = ech.inverse();
        let product = mat_mul(&inv, &matrix2);
        assert!(is_identity(&product));
    }

    #[test]
    fn edge_case_n1() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let s = Scalar::random(&mut rng);
        let mut ech = IncrementalEchelon::new(1);
        assert!(ech.add_row(&[s.clone()]));
        assert!(ech.is_full());

        let inv = ech.inverse();
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].len(), 1);
        // inv[0][0] * s == 1
        let product = inv[0][0].clone() * &s;
        assert_eq!(product, Scalar::one());
    }

    #[test]
    fn known_values_from_u64() {
        // 2x2 matrix with known scalars: [[2, 3], [1, 4]]
        // det = 8 - 3 = 5, inv = 1/5 * [[4, -3], [-1, 2]]
        let two = Scalar::from_u64(2);
        let three = Scalar::from_u64(3);
        let one = Scalar::from_u64(1);
        let four = Scalar::from_u64(4);

        let mut ech = IncrementalEchelon::new(2);
        assert!(ech.add_row(&[two.clone(), three.clone()]));
        assert!(ech.add_row(&[one, four.clone()]));
        assert!(ech.is_full());

        let inv = ech.inverse();
        // Verify inv * original = identity.
        let original = vec![vec![two, three], vec![Scalar::from_u64(1), four]];
        let product = mat_mul(&inv, &original);
        assert!(is_identity(&product));
    }
}
