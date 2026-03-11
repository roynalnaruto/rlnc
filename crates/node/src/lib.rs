//! RLNC P2P node — protocol implementations and actor.
//!
//! This crate provides the [`Protocol`] trait and three concrete
//! implementations for real P2P block propagation:
//!
//! - [`BaselineProtocol`](baseline::BaselineProtocol) — full-block
//!   gossipsub.
//! - [`PedersenProtocol`](pedersen::PedersenProtocol) — RLNC +
//!   Pedersen commitments.
//! - [`BfkwProtocol`](bfkw::BfkwProtocol) — RLNC + BFKW linearly-
//!   homomorphic signatures.
//!
//! The [`NodeActor`](actor::NodeActor) drives a protocol instance
//! over `commonware-p2p` channels.

pub mod actor;
pub mod baseline;
pub mod bfkw;
pub mod config;
pub mod metrics;
pub mod pedersen;
pub mod protocol;
