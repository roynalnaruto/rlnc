//! Random Linear Network Coding (RLNC) primitives.
//!
//! Provides the Pedersen commitment-based integrity scheme for coded
//! chunks, including:
//!
//! - [`CommitmentKey`] — reusable generator set derived via
//!   hash-to-group.
//! - [`PedersenScheme`] — [`IntegrityScheme`](p2p_strategy_core::IntegrityScheme)
//!   implementation using MSM equality checks.
//! - [`PedersenSigner`] — proposer-side proof creation, generic
//!   over any [`Signer`](commonware_cryptography::Signer).

pub mod commitment;
pub mod pedersen;
pub mod strategy;

pub use commitment::CommitmentKey;
pub use pedersen::{PedersenContext, PedersenError, PedersenProof, PedersenScheme, PedersenSigner};
pub use strategy::{PedersenNodeState, PedersenStrategy};
