//! Full-pipeline integration tests for the BFKW signature scheme.
//!
//! Exercises the complete path:
//! Block → encode → sign → verify → (re-encode + combine) → verify → decode → Block

use alloy_primitives::B256;
use p2p_bfkw::{BfkwError, BfkwScheme, BfkwSigner, Bls12381};
use p2p_primitives_math::linalg::random_coefficients;
use p2p_primitives_types::{Block, BlockDecoder, Chunk, CodedChunk, Scalar};
use p2p_strategy_core::IntegrityScheme;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

const M: usize = 16;
const N: usize = 4;
const K: usize = 8;

fn sample_block() -> Block {
    Block::new(
        B256::repeat_byte(0xAA),
        42,
        1_700_000_000,
        15_000_000,
        30_000_000,
        B256::repeat_byte(0xBB),
        (0..200u8).collect(),
    )
}

type Scheme = BfkwScheme<Bls12381>;

/// Full proposer → receiver decode path using BFKW.
#[test]
fn bfkw_direct_pipeline() {
    let mut rng = ChaCha20Rng::seed_from_u64(42);
    let block = sample_block();

    // Encode block into coded chunks.
    let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(M, K, &mut rng);

    // Proposer setup.
    let (original_chunks, _) = block.as_chunks::<Scalar, N>(M);
    let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();

    let signer = BfkwSigner::<Bls12381>::generate(&mut rng);
    let block_id = [0xCCu8; 32];
    let ctx = signer.setup_context::<N>(&block_id, M);

    // Receiver: sign, verify, decode each coded chunk.
    let mut decoder = BlockDecoder::<Scalar, N>::new();
    for coded in &coded_chunks {
        let proof = signer.sign_coded::<N>(&ctx, &chunk_refs, coded.b());
        let result = Scheme::verify(&ctx, coded, &proof);
        assert!(result.is_ok(), "verification failed: {result:?}");
        decoder.add(coded.clone());
        if decoder.is_complete() {
            break;
        }
    }
    assert!(decoder.is_complete());

    // Decode and reconstruct.
    let decoded_chunks = decoder.decode();
    let recovered = Block::from_chunks::<Scalar, N>(&decoded_chunks, byte_len);
    assert_eq!(block, recovered);
}

/// Proposer → intermediate node re-encodes → second receiver decodes.
#[test]
fn bfkw_reencode_pipeline() {
    let mut rng = ChaCha20Rng::seed_from_u64(99);
    let block = sample_block();

    // Proposer encodes and signs.
    let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(M, K, &mut rng);
    let (original_chunks, _) = block.as_chunks::<Scalar, N>(M);
    let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();

    let signer = BfkwSigner::<Bls12381>::generate(&mut rng);
    let block_id = [0xCCu8; 32];
    let ctx = signer.setup_context::<N>(&block_id, M);

    // Intermediate node: receive first 2 coded chunks with their proofs.
    let mut decoder1 = BlockDecoder::<Scalar, N>::new();
    let mut received_proofs = Vec::new();
    for coded in &coded_chunks[..2] {
        let proof = signer.sign_coded::<N>(&ctx, &chunk_refs, coded.b());
        assert!(Scheme::verify(&ctx, coded, &proof).is_ok());
        decoder1.add(coded.clone());
        received_proofs.push(proof);
    }
    assert!(!decoder1.is_complete());

    // Re-encode 4 new coded chunks from the partial decoder.
    let mut reencoded = Vec::new();
    for _ in 0..4 {
        let alphas = random_coefficients(decoder1.rank(), &mut rng);
        let new_chunk = decoder1.reencode_with(&alphas);

        // Combine proofs (BFKW: true homomorphic MSM in G2).
        let new_proof = <Scheme as IntegrityScheme<Scalar, N>>::combine(&received_proofs, &alphas);

        // The re-encoded chunk must verify.
        assert!(
            Scheme::verify(&ctx, &new_chunk, &new_proof).is_ok(),
            "re-encoded chunk failed verification"
        );
        reencoded.push(new_chunk);
    }

    // Second receiver: decode using re-encoded chunks + remaining originals.
    let mut decoder2 = BlockDecoder::<Scalar, N>::new();
    for coded in &reencoded {
        decoder2.add(coded.clone());
        if decoder2.is_complete() {
            break;
        }
    }
    if !decoder2.is_complete() {
        for coded in &coded_chunks[2..] {
            let proof = signer.sign_coded::<N>(&ctx, &chunk_refs, coded.b());
            assert!(Scheme::verify(&ctx, coded, &proof).is_ok());
            decoder2.add(coded.clone());
            if decoder2.is_complete() {
                break;
            }
        }
    }
    assert!(decoder2.is_complete());

    let decoded_chunks = decoder2.decode();
    let recovered = Block::from_chunks::<Scalar, N>(&decoded_chunks, byte_len);
    assert_eq!(block, recovered);
}

/// Verify that a tampered coded chunk fails proof verification.
#[test]
fn bfkw_rejects_tampered_block() {
    let mut rng = ChaCha20Rng::seed_from_u64(42);
    let block = sample_block();

    let (coded_chunks, _) = block.encode_chunks::<Scalar, N>(M, K, &mut rng);
    let (original_chunks, _) = block.as_chunks::<Scalar, N>(M);
    let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();

    let signer = BfkwSigner::<Bls12381>::generate(&mut rng);
    let block_id = [0xCCu8; 32];
    let ctx = signer.setup_context::<N>(&block_id, M);

    // Sign the first coded chunk.
    let coded = coded_chunks[0].clone();
    let proof = signer.sign_coded::<N>(&ctx, &chunk_refs, coded.b());

    // Tamper with w[0].
    let (w, b) = coded.into_parts();
    let mut tampered = w.into_inner();
    tampered[0] += &Scalar::from_u64(1);
    let tampered_chunk: CodedChunk<Scalar, N> = CodedChunk::new(Chunk::new(&tampered), b);

    let result = Scheme::verify(&ctx, &tampered_chunk, &proof);
    assert!(
        matches!(result, Err(BfkwError::PairingMismatch)),
        "expected PairingMismatch, got {result:?}"
    );
}
