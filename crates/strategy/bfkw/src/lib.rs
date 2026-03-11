//! Linearly-Homomorphic Signature Scheme (LHSS) for RLNC.
//!
//! Implements the BFKW construction (Boneh–Freeman–Katz–Waters) for
//! integrity verification of coded chunks. Each coded chunk carries a
//! single $\mathbb{G}_2$ signature verified via a bilinear pairing
//! check.
//!
//! - [`BfkwScheme`] — [`IntegrityScheme`](p2p_strategy_core::IntegrityScheme)
//!   implementation using pairings.
//! - [`BfkwSigner`] — proposer-side proof creation.
//! - [`derive_hash_points`] — per-block hash-to-G2 derivation.

pub mod bfkw;
pub mod pairing;
pub mod strategy;

pub use bfkw::{BfkwContext, BfkwError, BfkwProof, BfkwScheme, BfkwSigner, derive_hash_points};
/// Re-export the pairing group trait and BLS12-381 implementation.
pub use pairing::{Bls12381, PairingGroup};
pub use strategy::{BfkwNodeState, BfkwStrategy};
