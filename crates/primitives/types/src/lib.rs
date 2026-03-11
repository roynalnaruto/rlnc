//! Shared data types for Pedersen / BFKW block propagation simulation.
//!
//! This crate provides the common types used by all three propagation
//! strategies (`p2p-baseline`, `p2p-pedersen`, `p2p-bfkw`):
//!
//! - [`Block`] — a minimal Ethereum-like block with codec support and
//!   keccak256 hashing.
//! - [`Chunk`] — an original block chunk ($\mathbf{v}_i \in \mathbb{F}^M$).
//! - [`CodedChunk`] — a coded chunk ($\mathbf{w}, \mathbf{b}$) for
//!   network transmission.
//! - [`BlockDecoder`] — stateful decoder that collects coded chunks
//!   and recovers the original block.
//! - [`PackableField`] — byte$\leftrightarrow$field-element conversion
//!   trait.
//!
//! The RLNC encoding/decoding protocol is shared between `p2p-pedersen`
//! and `p2p-bfkw`; only the integrity proof differs (Pedersen
//! commitments vs BFKW signatures).

pub mod block;
pub mod coding;
pub mod util;

pub use block::Block;
pub use coding::{BlockDecoder, Chunk, CodedChunk};
pub use util::{PackableField, read_field, read_group};

pub use p2p_primitives_math::{Additive, Field, Multiplicative, Random, Ring, Scalar};

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B256;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    /// Full pipeline integration test:
    ///
    /// `Block` → `block.encode_chunks()` → `BlockDecoder::add()` →
    /// `decode()` → `Block::from_chunks()` → equal to original.
    #[test]
    fn full_pipeline() {
        const M: usize = 16;
        const N: usize = 4;
        let k = 6;

        let block = Block::new(
            B256::repeat_byte(0x11),
            100,
            1_700_000_000,
            12_000_000,
            30_000_000,
            B256::repeat_byte(0x22),
            (0..300).map(|i| (i % 256) as u8).collect(),
        );

        let mut rng = ChaCha20Rng::seed_from_u64(2025);
        let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(M, k, &mut rng);

        let mut decoder = BlockDecoder::<Scalar, N>::new();
        for coded in coded_chunks {
            decoder.add(coded);
            if decoder.is_complete() {
                break;
            }
        }
        assert!(decoder.is_complete());

        let decoded_chunks = decoder.decode();
        let recovered = Block::from_chunks::<Scalar, N>(&decoded_chunks, byte_len);
        assert_eq!(block, recovered);
    }
}
