//! Bilinear pairing group abstraction.
//!
//! Defines the [`PairingGroup`] trait for pairing-based cryptography and
//! provides a concrete [`Bls12381`] implementation backed by commonware's
//! `blst`-based BLS12-381 primitives.
//!
//! Only the LHSS (BFKW) strategy requires pairings. The Pedersen scheme
//! uses only [`HashToGroup`] and [`commonware_math::algebra::CryptoGroup`]
//! — no pairing.
//!
//! This trait exists so that both strategies can be generic over the curve.

use commonware_math::algebra::{Field, HashToGroup, Random};

/// A bilinear group suitable for pairing-based cryptography.
///
/// Bundles the scalar field and two source groups $\mathbb{G}_1$ and
/// $\mathbb{G}_2$ (both supporting MSM and hash-to-group), plus a
/// multi-pairing check that shares the final exponentiation for
/// ~25% speedup over two independent pairing calls.
pub trait PairingGroup: Clone + Send + Sync + 'static {
    /// Scalar field shared by both source groups.
    type Scalar: Field + Random;

    /// First source group ($\mathbb{G}_1$) — used for public keys
    /// and Pedersen commitment points.
    type G1: HashToGroup<Scalar = Self::Scalar>;

    /// Second source group ($\mathbb{G}_2$) — used for BFKW
    /// signatures and hash points $H(\mathrm{id}, s)$.
    type G2: HashToGroup<Scalar = Self::Scalar>;

    /// Multi-pairing check: $e(G_1, \sigma) \stackrel{?}{=} e(\mathrm{pk}, P)$.
    ///
    /// Uses a single shared final exponentiation when the backend supports it,
    /// which is ~25% faster than two independent pairing calls.
    fn pairing_check(pk: &Self::G1, sigma: &Self::G2, p: &Self::G2) -> bool;
}

// ---------------------------------------------------------------------------
// BLS12-381 implementation
// ---------------------------------------------------------------------------

use commonware_cryptography::bls12381::primitives::{
    group::{G1, G2, Scalar},
    variant::{MinPk, Variant},
};

/// BLS12-381 pairing group using commonware's `blst`-backed primitives.
///
/// Uses the [`MinPk`] variant: public keys live in $\mathbb{G}_1$
/// (48 bytes) and signatures in $\mathbb{G}_2$ (96 bytes).
#[derive(Clone, Debug)]
pub struct Bls12381;

impl PairingGroup for Bls12381 {
    type Scalar = Scalar;
    type G1 = G1;
    type G2 = G2;

    fn pairing_check(pk: &G1, sigma: &G2, p: &G2) -> bool {
        // MinPk::verify(pk, hm=p, sig=σ) internally calls multi_pairing_check,
        // checking: e(P, pk) == e(σ, G1_gen)  ≡  e(G1_gen, σ) == e(pk, P).
        MinPk::verify(pk, p, sigma).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonware_math::algebra::CryptoGroup;

    /// `pairing_check` returns true when e(G1_gen, σ) == e(pk, P) and false otherwise.
    #[test]
    fn pairing_check_valid() {
        // σ = sk · P  ⇒  e(G1, sk·P) = e(sk·G1, P) = e(pk, P)  ✓
        let sk = Scalar::from_u64(7);
        let pk = G1::generator() * &sk;
        let p = G2::generator() * &Scalar::from_u64(13);
        let sigma = p.clone() * &sk;
        assert!(Bls12381::pairing_check(&pk, &sigma, &p));
    }

    #[test]
    fn pairing_check_rejects_mismatch() {
        let sk = Scalar::from_u64(7);
        let pk = G1::generator() * &sk;
        let p = G2::generator() * &Scalar::from_u64(13);
        // Wrong sigma (not sk·P).
        let sigma = G2::generator() * &Scalar::from_u64(99);
        assert!(!Bls12381::pairing_check(&pk, &sigma, &p));
    }
}
