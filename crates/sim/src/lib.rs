//! Network simulation for comparing block propagation strategies.
//!
//! Provides a discrete-event simulator that runs three strategies
//! (gossipsub baseline, RLNC+Pedersen, RLNC+BFKW) over a random
//! D-regular mesh topology and measures bandwidth, propagation time,
//! and redundancy.
//!
//! # Crate structure
//!
//! - [`config`] -- environment-driven simulation parameters
//! - [`topology`] -- random D-regular mesh generation
//! - [`strategy`] -- [`PropagationMechanism`] wrapper
//! - [`engine`] -- discrete-event simulation loop
//! - [`metrics`] -- bandwidth and propagation metrics

pub mod config;
pub mod engine;
pub mod metrics;
pub mod strategy;
pub mod topology;

pub use config::SimConfig;
pub use metrics::{CumulativeMetrics, SimMetrics, compare};
pub use strategy::PropagationMechanism;
pub use topology::Topology;
