//! Minimal Ethereum-like block for network propagation simulation.
//!
//! [`Block`] provides the shared block representation used by all
//! three propagation strategies. It supports:
//!
//! - **Serialization** via `commonware-codec` `Write` / `Read`.
//! - **Hashing** via `keccak256`.
//! - **Chunking** via [`as_chunks`](Block::as_chunks) and
//!   [`from_chunks`](Block::from_chunks) for RLNC encoding/decoding.
//! - **RLNC encoding** via [`encode_chunks`](Block::encode_chunks)
//!   for proposer-side coded chunk generation.

use alloy_primitives::{B256, keccak256};
use bytes::{Buf, BufMut};
use commonware_codec::{
    Encode, EncodeSize, Error, FixedSize, Read as CodecRead, Write as CodecWrite,
};
use p2p_primitives_math::Random;

use crate::coding::{Chunk, CodedChunk};
use crate::util::{PackableField, bytes_to_field_elements, field_elements_to_bytes};

/// Maximum transaction payload size accepted during deserialization
/// (16 MiB). Sufficient for simulation purposes.
const MAX_TRANSACTIONS_BYTES: usize = 16 * 1024 * 1024;

/// Minimal Ethereum-like block for network propagation simulation.
///
/// The block hash is computed via `keccak256` over the canonical
/// codec encoding.
#[derive(Clone, Debug)]
pub struct Block {
    /// Hash of the parent block.
    parent_hash: B256,
    /// Block number (height).
    number: u64,
    /// Unix timestamp.
    timestamp: u64,
    /// Gas consumed by this block's execution.
    gas_used: u64,
    /// Maximum gas allowed in this block.
    gas_limit: u64,
    /// Post-execution state trie root.
    state_root: B256,
    /// Opaque transaction payload bytes.
    transactions: Vec<u8>,
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        self.parent_hash == other.parent_hash
            && self.number == other.number
            && self.timestamp == other.timestamp
            && self.gas_used == other.gas_used
            && self.gas_limit == other.gas_limit
            && self.state_root == other.state_root
            && self.transactions == other.transactions
    }
}

impl Eq for Block {}

impl CodecWrite for Block {
    fn write(&self, buf: &mut impl BufMut) {
        buf.put_slice(self.parent_hash.as_ref());
        self.number.write(buf);
        self.timestamp.write(buf);
        self.gas_used.write(buf);
        self.gas_limit.write(buf);
        buf.put_slice(self.state_root.as_ref());
        self.transactions.as_slice().write(buf);
    }
}

impl EncodeSize for Block {
    fn encode_size(&self) -> usize {
        32 // parent_hash
        + u64::SIZE // number
        + u64::SIZE // timestamp
        + u64::SIZE // gas_used
        + u64::SIZE // gas_limit
        + 32 // state_root
        + self.transactions.as_slice().encode_size() // length-prefixed txs
    }
}

impl CodecRead for Block {
    type Cfg = ();

    fn read_cfg(buf: &mut impl Buf, _cfg: &()) -> Result<Self, Error> {
        let mut parent_hash = [0u8; 32];
        if buf.remaining() < 32 {
            return Err(Error::EndOfBuffer);
        }
        buf.copy_to_slice(&mut parent_hash);

        let number = u64::read_cfg(buf, &())?;
        let timestamp = u64::read_cfg(buf, &())?;
        let gas_used = u64::read_cfg(buf, &())?;
        let gas_limit = u64::read_cfg(buf, &())?;

        let mut state_root = [0u8; 32];
        if buf.remaining() < 32 {
            return Err(Error::EndOfBuffer);
        }
        buf.copy_to_slice(&mut state_root);

        let range = commonware_codec::RangeCfg::new(0..=MAX_TRANSACTIONS_BYTES);
        let transactions = Vec::<u8>::read_cfg(buf, &(range, ()))?;

        Ok(Self {
            parent_hash: B256::from(parent_hash),
            number,
            timestamp,
            gas_used,
            gas_limit,
            state_root: B256::from(state_root),
            transactions,
        })
    }
}

impl Block {
    /// Create a new block.
    pub const fn new(
        parent_hash: B256,
        number: u64,
        timestamp: u64,
        gas_used: u64,
        gas_limit: u64,
        state_root: B256,
        transactions: Vec<u8>,
    ) -> Self {
        Self {
            parent_hash,
            number,
            timestamp,
            gas_used,
            gas_limit,
            state_root,
            transactions,
        }
    }

    /// The block number (height).
    pub const fn number(&self) -> u64 {
        self.number
    }

    /// Compute the block hash.
    ///
    /// The hash is `keccak256` of the canonical codec encoding.
    pub fn hash(&self) -> B256 {
        let encoded = Encode::encode(self);
        keccak256(&encoded)
    }

    /// Split this block into $N$ chunks of $m$ field elements each.
    ///
    /// Returns the chunks $\mathbf{v}_1, \ldots, \mathbf{v}_N$ and
    /// the serialized byte length (needed for lossless reconstruction
    /// via [`from_chunks`](Block::from_chunks)).
    ///
    /// # Panics
    ///
    /// Panics if the serialized block requires more than $N \times m$
    /// field elements.
    pub fn as_chunks<F: PackableField, const N: usize>(&self, m: usize) -> (Vec<Chunk<F>>, usize) {
        let encoded = self.serialize();
        let byte_len = encoded.len();
        let elements = bytes_to_field_elements::<F>(&encoded);

        let total_slots = N * m;
        assert!(
            elements.len() <= total_slots,
            "block requires {} field elements but N*m = {total_slots}",
            elements.len(),
        );

        // Pad to exactly N*m elements, then split into N chunks of m.
        let mut padded = elements;
        padded.resize(total_slots, F::zero());

        let chunks: Vec<Chunk<F>> = padded.chunks_exact(m).map(Chunk::new).collect();

        (chunks, byte_len)
    }

    /// Encode this block into `count` coded chunks for network
    /// transmission.
    ///
    /// This is the **proposer's primary API**: serializes the block,
    /// splits into $N$ original chunks, and produces `count` coded
    /// chunks with random coefficient vectors $\mathbf{b}$.
    ///
    /// Each coded chunk satisfies
    /// $\mathbf{w} = \sum_i b_i \cdot \mathbf{v}_i$.
    ///
    /// Returns the coded chunks and the serialized byte length
    /// (needed by receivers for [`from_chunks`](Block::from_chunks)
    /// reconstruction).
    pub fn encode_chunks<F: PackableField + Random, const N: usize>(
        &self,
        m: usize,
        count: usize,
        rng: &mut (impl rand::CryptoRng + rand::RngCore),
    ) -> (Vec<CodedChunk<F, N>>, usize) {
        let (original_chunks, byte_len) = self.as_chunks::<F, N>(m);

        let chunk_refs: Vec<&[F]> = original_chunks.iter().map(Chunk::v).collect();
        let coded: Vec<CodedChunk<F, N>> = (0..count)
            .map(|_| {
                let coeffs = p2p_primitives_math::linalg::random_coefficients(N, &mut *rng);
                let coded_data =
                    p2p_primitives_math::linalg::linear_combination(&chunk_refs, &coeffs);
                CodedChunk::new(Chunk::new(&coded_data), coeffs)
            })
            .collect();

        (coded, byte_len)
    }

    /// Reconstruct a block from $N$ decoded chunks and the original
    /// serialized byte length.
    ///
    /// This is the inverse of [`as_chunks`](Block::as_chunks): the
    /// chunks are flattened to field elements, converted back to
    /// bytes, and decoded via the codec.
    ///
    /// # Panics
    ///
    /// Panics if `chunks.len() != N` or if the byte stream does not
    /// decode to a valid [`Block`].
    pub fn from_chunks<F: PackableField, const N: usize>(
        chunks: &[Chunk<F>],
        byte_len: usize,
    ) -> Self {
        assert_eq!(
            chunks.len(),
            N,
            "expected N={N} chunks, got {}",
            chunks.len()
        );

        let elements: Vec<F> = chunks.iter().flat_map(|c| c.v().iter().cloned()).collect();
        let all_bytes = field_elements_to_bytes(&elements);
        let trimmed = &all_bytes[..byte_len];

        let mut cursor = trimmed;
        <Self as CodecRead>::read_cfg(&mut cursor, &())
            .expect("decoded chunks must produce a valid Block")
    }

    /// Serialize this block using the codec and return raw bytes.
    ///
    /// Convenience wrapper around [`Encode::encode`] that avoids
    /// name collisions with [`encode_chunks`](Block::encode_chunks).
    fn serialize(&self) -> bytes::Bytes {
        Encode::encode(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonware_codec::DecodeExt;
    use p2p_primitives_math::Scalar;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn sample_block() -> Block {
        Block::new(
            B256::repeat_byte(0xAA),
            42,
            1_700_000_000,
            15_000_000,
            30_000_000,
            B256::repeat_byte(0xBB),
            (0..200).map(|i| (i % 256) as u8).collect(),
        )
    }

    #[test]
    fn codec_roundtrip() {
        let block = sample_block();
        let encoded = Encode::encode(&block);
        let decoded = Block::decode(encoded).unwrap();
        assert_eq!(block, decoded);
    }

    #[test]
    fn as_chunks_from_chunks_roundtrip() {
        let block = sample_block();
        let (chunks, byte_len) = block.as_chunks::<Scalar, 8>(16);
        assert_eq!(chunks.len(), 8);
        for chunk in &chunks {
            assert_eq!(chunk.len(), 16);
        }
        let recovered = Block::from_chunks::<Scalar, 8>(&chunks, byte_len);
        assert_eq!(block, recovered);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let block = sample_block();
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        const M: usize = 16;
        const N: usize = 8;
        let k = 12;

        let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(M, k, &mut rng);
        assert_eq!(coded_chunks.len(), k);

        let mut decoder = crate::coding::BlockDecoder::<Scalar, N>::new();
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

    #[test]
    fn hash_deterministic() {
        let block = sample_block();
        let h1 = block.hash();
        let h2 = block.hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn empty_transactions() {
        let block = Block::new(B256::ZERO, 0, 0, 0, 0, B256::ZERO, vec![]);
        let encoded = Encode::encode(&block);
        let decoded = Block::decode(encoded).unwrap();
        assert_eq!(block, decoded);

        let (chunks, byte_len) = block.as_chunks::<Scalar, 4>(16);
        let recovered = Block::from_chunks::<Scalar, 4>(&chunks, byte_len);
        assert_eq!(block, recovered);
    }
}
