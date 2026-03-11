//! Full-pipeline integration tests for the Pedersen commitment scheme.
//!
//! Exercises the complete path:
//! Block → encode → sign → verify → (re-encode + combine) → verify → decode → Block

use alloy_primitives::B256;
use commonware_cryptography::bls12381;
use commonware_cryptography::bls12381::primitives::group::G1;
use p2p_pedersen::{CommitmentKey, PedersenError, PedersenScheme, PedersenSigner};
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

/// Full proposer → receiver decode path (no intermediate re-encoding).
#[test]
fn pedersen_direct_pipeline() {
    let mut rng = ChaCha20Rng::seed_from_u64(42);
    let block = sample_block();

    // Encode block into coded chunks.
    let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(M, K, &mut rng);

    // Proposer signs: commit to original chunks + signature.
    let (original_chunks, _) = block.as_chunks::<Scalar, N>(M);
    let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();

    let signer = PedersenSigner::<bls12381::PrivateKey>::generate(&mut rng);
    let ck = CommitmentKey::<G1>::canonical(M);
    let proof = signer.sign_block::<G1>(&ck, &chunk_refs);
    let ctx = signer.context::<G1>(ck);

    // Receiver: verify each coded chunk then feed to decoder.
    let mut decoder = BlockDecoder::<Scalar, N>::new();
    for coded in &coded_chunks {
        let result = PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, coded, &proof);
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
fn pedersen_reencode_pipeline() {
    let mut rng = ChaCha20Rng::seed_from_u64(99);
    let block = sample_block();

    // Proposer encodes and signs.
    let (coded_chunks, byte_len) = block.encode_chunks::<Scalar, N>(M, K, &mut rng);
    let (original_chunks, _) = block.as_chunks::<Scalar, N>(M);
    let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();

    let signer = PedersenSigner::<bls12381::PrivateKey>::generate(&mut rng);
    let ck = CommitmentKey::<G1>::canonical(M);
    let proof = signer.sign_block::<G1>(&ck, &chunk_refs);
    let ctx = signer.context::<G1>(ck);

    // Intermediate node: receive first 2 coded chunks (partial rank).
    let mut decoder1 = BlockDecoder::<Scalar, N>::new();
    for coded in &coded_chunks[..2] {
        assert!(PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, coded, &proof).is_ok());
        decoder1.add(coded.clone());
    }
    assert!(!decoder1.is_complete());

    // Re-encode 4 new coded chunks from the partial decoder.
    let mut reencoded: Vec<CodedChunk<Scalar, N>> = Vec::new();
    for _ in 0..4 {
        let alphas = random_coefficients(decoder1.rank(), &mut rng);
        let new_chunk = decoder1.reencode_with(&alphas);

        // Combine proof (Pedersen: clones first proof since commitments
        // are invariant under re-encoding).
        let new_proof =
            <PedersenScheme<G1, bls12381::PrivateKey> as IntegrityScheme<Scalar, N>>::combine(
                &[proof.clone(), proof.clone()],
                &alphas,
            );

        // The re-encoded chunk must verify.
        assert!(
            PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, &new_chunk, &new_proof)
                .is_ok(),
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
            assert!(
                PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, coded, &proof).is_ok()
            );
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
fn pedersen_rejects_tampered_block() {
    let mut rng = ChaCha20Rng::seed_from_u64(42);
    let block = sample_block();

    let (coded_chunks, _) = block.encode_chunks::<Scalar, N>(M, K, &mut rng);
    let (original_chunks, _) = block.as_chunks::<Scalar, N>(M);
    let chunk_refs: Vec<&[Scalar]> = original_chunks.iter().map(Chunk::v).collect();

    let signer = PedersenSigner::<bls12381::PrivateKey>::generate(&mut rng);
    let ck = CommitmentKey::<G1>::canonical(M);
    let proof = signer.sign_block::<G1>(&ck, &chunk_refs);
    let ctx = signer.context::<G1>(ck);

    // Tamper with the first coded chunk.
    let coded = coded_chunks[0].clone();
    let (w, b) = coded.into_parts();
    let mut tampered = w.into_inner();
    tampered[0] += &Scalar::from_u64(1);
    let tampered_chunk: CodedChunk<Scalar, N> = CodedChunk::new(Chunk::new(&tampered), b);

    let result = PedersenScheme::<G1, bls12381::PrivateKey>::verify(&ctx, &tampered_chunk, &proof);
    assert!(
        matches!(result, Err(PedersenError::CommitmentMismatch)),
        "expected CommitmentMismatch, got {result:?}"
    );
}
