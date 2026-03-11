//! Shared receive/forward logic for coded strategies.
//!
//! Both Pedersen and BFKW strategies follow the same pattern for
//! receiving and forwarding coded chunks. These helpers eliminate
//! the duplication.

use p2p_primitives_math::{Field, Random};
use p2p_primitives_types::{BlockDecoder, CodedChunk};
use rand::{CryptoRng, Rng};
use tracing::trace;

use crate::proof::{IntegrityScheme, SignedPacket};

/// Process a received coded packet: verify, then add to decoder.
///
/// Returns `Ok(true)` if the chunk was linearly independent (new info),
/// `Ok(false)` if redundant, or `Err` on verification failure.
///
/// # Errors
///
/// Returns a descriptive error string if the packet fails
/// integrity verification.
pub fn coded_receive<S, F, const N: usize>(
    decoder: &mut BlockDecoder<F, N>,
    proofs: &mut Vec<S::Proof>,
    ctx: &S::Context,
    packet: &SignedPacket<F, N, S>,
) -> Result<bool, String>
where
    F: Field,
    S: IntegrityScheme<F, N>,
    S::Error: core::fmt::Display,
{
    S::verify(ctx, packet.chunk(), packet.proof()).map_err(|e| format!("{e}"))?;
    trace!("integrity check passed");

    if decoder.add(packet.chunk().clone()) {
        proofs.push(packet.proof().clone());
        trace!(rank = decoder.rank(), "rank increased");
        Ok(true)
    } else {
        trace!("chunk redundant, no rank increase");
        Ok(false)
    }
}

/// Create forwarding packets for `num_peers` neighbors by re-encoding.
///
/// Each packet gets a fresh random linear combination of the collected
/// coded chunks, with the proof combined accordingly.
pub fn coded_forward<S, F, R, const N: usize>(
    decoder: &BlockDecoder<F, N>,
    proofs: &[S::Proof],
    num_peers: usize,
    rng: &mut R,
) -> Vec<SignedPacket<F, N, S>>
where
    F: Field + Random,
    S: IntegrityScheme<F, N>,
    R: Rng + CryptoRng,
{
    trace!(rank = decoder.rank(), num_peers, "re-encoding for forward");
    (0..num_peers)
        .map(|_| {
            let alphas = p2p_primitives_math::linalg::random_coefficients(decoder.rank(), rng);
            let new_chunk: CodedChunk<F, N> = decoder.reencode_with(&alphas);
            let new_proof = S::combine(proofs, &alphas);
            SignedPacket::new(new_chunk, new_proof)
        })
        .collect()
}
