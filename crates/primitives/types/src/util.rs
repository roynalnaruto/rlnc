//! Byte$\leftrightarrow$field-element conversion via [`PackableField`].
//!
//! Block data (opaque bytes) must be split into field elements for RLNC
//! encoding. [`PackableField`] abstracts the fixed-width packing scheme:
//! each field element carries [`PackableField::DATA_BYTES`] bytes of
//! payload, and the conversion is lossless in both directions.

use bytes::Buf;
use commonware_codec::{EncodeSize, FixedSize, Read as CodecRead, Write as CodecWrite};
use p2p_primitives_math::{Additive, Field};

/// A field element that can be losslessly packed from / unpacked to
/// raw data bytes for block$\leftrightarrow$chunk conversion.
///
/// # Safety contract
///
/// Any byte window of length $\leq$ [`DATA_BYTES`](PackableField::DATA_BYTES) must produce a
/// valid, canonical field element via [`pack`](PackableField::pack),
/// and [`unpack`](PackableField::unpack) must recover those bytes
/// exactly (right-padded with zeros when the window was short).
pub trait PackableField: Field + Sized {
    /// Number of data bytes each field element carries.
    const DATA_BYTES: usize;

    /// Pack a byte window ($\leq$ [`DATA_BYTES`](PackableField::DATA_BYTES) bytes) into a field
    /// element.
    ///
    /// If `window.len() < DATA_BYTES` the remaining positions are
    /// treated as zero (right-padding).
    ///
    /// # Panics
    ///
    /// Panics if `window.len() > DATA_BYTES`.
    fn pack(window: &[u8]) -> Self;

    /// Unpack a field element to exactly [`DATA_BYTES`](PackableField::DATA_BYTES) raw bytes.
    ///
    /// This is the inverse of [`pack`](PackableField::pack). Callers
    /// must track the original byte length externally and truncate
    /// the final element's output to recover the exact payload.
    fn unpack(&self) -> Vec<u8>;
}

/// BLS12-381 scalar: 31 data bytes per element.
///
/// Any 31-byte value is numerically $< 2^{248} < r$ (the BLS12-381
/// scalar field modulus), so the packing is always lossless. The
/// leading byte of the 32-byte big-endian representation is always
/// zero for packed data, which guarantees a valid canonical encoding.
impl PackableField for p2p_primitives_math::Scalar {
    const DATA_BYTES: usize = 31;

    fn pack(window: &[u8]) -> Self {
        assert!(
            window.len() <= Self::DATA_BYTES,
            "window length {} exceeds DATA_BYTES {}",
            window.len(),
            Self::DATA_BYTES,
        );

        // Fast path: all-zero window maps to the additive identity.
        if window.iter().all(|&b| b == 0) {
            return Self::zero();
        }

        // Build a 32-byte big-endian buffer: [0x00 | window | padding].
        let mut be = [0u8; 32];
        be[1..=window.len()].copy_from_slice(window);

        // The value is < 2^248 < r and non-zero, so codec Read succeeds.
        let mut cursor = &be[..];
        <Self as CodecRead>::read_cfg(&mut cursor, &())
            .expect("31-byte value is always a valid non-zero BLS12-381 scalar")
    }

    fn unpack(&self) -> Vec<u8> {
        // Serialize to 32 big-endian bytes via the codec Write impl.
        let size = <Self as EncodeSize>::encode_size(self);
        let mut buf = Vec::with_capacity(size);
        <Self as CodecWrite>::write(self, &mut buf);

        // Strip the leading byte (always 0x00 for packed data) and
        // return the 31 data bytes.
        buf.split_off(1)
    }
}

/// Convert a byte slice to a vector of field elements.
///
/// The input is split into consecutive windows of
/// [`PackableField::DATA_BYTES`] bytes. The last window may be
/// shorter and is right-padded with zeros internally.
pub fn bytes_to_field_elements<F: PackableField>(data: &[u8]) -> Vec<F> {
    if data.is_empty() {
        return vec![F::zero()];
    }
    data.chunks(F::DATA_BYTES).map(F::pack).collect()
}

/// Convert field elements back to raw bytes.
///
/// Each element produces exactly [`PackableField::DATA_BYTES`] bytes.
/// Callers must truncate the result to the original byte length to
/// strip trailing padding from the last element.
pub fn field_elements_to_bytes<F: PackableField>(elements: &[F]) -> Vec<u8> {
    let mut out = Vec::with_capacity(elements.len() * F::DATA_BYTES);
    for elem in elements {
        out.extend_from_slice(&elem.unpack());
    }
    out
}

/// Read a field element from a byte buffer.
///
/// The standard `read_cfg` for BLS12-381 scalars rejects zero (it uses
/// `blst_sk_check` which is designed for secret keys). This helper
/// handles zero elements correctly, which is needed for coefficient
/// vectors and coded chunks.
///
/// # Errors
///
/// Returns an error if the buffer is too short or the bytes don't
/// represent a valid field element.
pub fn read_field<F>(buf: &mut &[u8]) -> Result<F, String>
where
    F: FixedSize + CodecRead<Cfg = ()> + Additive,
{
    if buf.remaining() < F::SIZE {
        return Err("not enough bytes for field element".into());
    }
    if buf[..F::SIZE].iter().all(|&b| b == 0) {
        buf.advance(F::SIZE);
        return Ok(F::zero());
    }
    F::read_cfg(buf, &()).map_err(|e| format!("invalid field element: {e}"))
}

/// Read a group element from a byte buffer.
///
/// The standard `read_cfg` for BLS12-381 G1 points rejects the identity
/// element (infinity point). This helper handles identity correctly,
/// which is needed for Pedersen commitments of all-zero chunks.
///
/// # Errors
///
/// Returns an error if the buffer is too short or the bytes don't
/// represent a valid group element.
pub fn read_group<G>(buf: &mut &[u8]) -> Result<G, String>
where
    G: FixedSize + CodecRead<Cfg = ()> + Additive + CodecWrite,
{
    if buf.remaining() < G::SIZE {
        return Err("not enough bytes for group element".into());
    }
    // Serialize the identity to get the canonical encoding, then compare.
    let mut identity_bytes = Vec::with_capacity(G::SIZE);
    G::zero().write(&mut identity_bytes);
    if buf[..G::SIZE] == identity_bytes[..] {
        buf.advance(G::SIZE);
        return Ok(G::zero());
    }
    G::read_cfg(buf, &()).map_err(|e| format!("invalid group element: {e}"))
}

#[cfg(test)]
mod tests {
    use commonware_cryptography::bls12381::primitives::group::{G1, G2};
    use commonware_math::algebra::CryptoGroup;
    use p2p_primitives_math::{Additive, Scalar};

    use super::*;

    #[test]
    fn roundtrip_exact_multiple() {
        // 62 bytes = exactly 2 windows of 31 bytes.
        let data: Vec<u8> = (0..62).collect();
        let elems = bytes_to_field_elements::<Scalar>(&data);
        assert_eq!(elems.len(), 2);
        let recovered = field_elements_to_bytes(&elems);
        assert_eq!(&recovered[..data.len()], &data);
    }

    #[test]
    fn roundtrip_non_multiple() {
        // 50 bytes = 1 full window (31) + 1 short window (19).
        let data: Vec<u8> = (100..150).collect();
        let elems = bytes_to_field_elements::<Scalar>(&data);
        assert_eq!(elems.len(), 2);
        let recovered = field_elements_to_bytes(&elems);
        assert_eq!(&recovered[..data.len()], &data);
    }

    #[test]
    fn zero_bytes_roundtrip() {
        let data = vec![0u8; 31];
        let elems = bytes_to_field_elements::<Scalar>(&data);
        assert_eq!(elems.len(), 1);
        assert_eq!(elems[0], Scalar::zero());
        let recovered = field_elements_to_bytes(&elems);
        assert_eq!(&recovered[..31], &data[..]);
    }

    #[test]
    fn empty_input() {
        let elems = bytes_to_field_elements::<Scalar>(&[]);
        assert_eq!(elems.len(), 1);
        assert_eq!(elems[0], Scalar::zero());
    }

    #[test]
    fn single_byte() {
        let data = [0x42];
        let elems = bytes_to_field_elements::<Scalar>(&data);
        assert_eq!(elems.len(), 1);
        let recovered = field_elements_to_bytes(&elems);
        assert_eq!(recovered[0], 0x42);
    }

    #[test]
    fn short_final_window() {
        // 33 bytes: first window 31 bytes, second window 2 bytes.
        let data: Vec<u8> = (1..=33).collect();
        let elems = bytes_to_field_elements::<Scalar>(&data);
        assert_eq!(elems.len(), 2);
        let recovered = field_elements_to_bytes(&elems);
        assert_eq!(&recovered[..data.len()], &data[..]);
    }

    #[test]
    fn large_roundtrip() {
        // Simulate a realistic payload.
        let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let elems = bytes_to_field_elements::<Scalar>(&data);
        let expected_count = data.len().div_ceil(31);
        assert_eq!(elems.len(), expected_count);
        let recovered = field_elements_to_bytes(&elems);
        assert_eq!(&recovered[..data.len()], &data[..]);
    }

    #[test]
    fn read_field_zero_scalar() {
        let zero = Scalar::zero();
        let mut buf = Vec::new();
        <Scalar as CodecWrite>::write(&zero, &mut buf);
        let mut cursor: &[u8] = &buf;
        let result: Scalar = read_field(&mut cursor).unwrap();
        assert_eq!(result, Scalar::zero());
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_field_nonzero_scalar() {
        let val = Scalar::from_u64(42);
        let mut buf = Vec::new();
        <Scalar as CodecWrite>::write(&val, &mut buf);
        let mut cursor: &[u8] = &buf;
        let result: Scalar = read_field(&mut cursor).unwrap();
        assert_eq!(result, val);
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_field_buffer_too_short() {
        let short = [0u8; 16];
        let mut cursor: &[u8] = &short;
        let result: Result<Scalar, _> = read_field(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn read_field_multiple_scalars() {
        let a = Scalar::from_u64(7);
        let b = Scalar::zero();
        let mut buf = Vec::new();
        <Scalar as CodecWrite>::write(&a, &mut buf);
        <Scalar as CodecWrite>::write(&b, &mut buf);
        let mut cursor: &[u8] = &buf;
        let ra: Scalar = read_field(&mut cursor).unwrap();
        let rb: Scalar = read_field(&mut cursor).unwrap();
        assert_eq!(ra, a);
        assert_eq!(rb, b);
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_group_identity_g1() {
        let id = G1::zero();
        let mut buf = Vec::new();
        <G1 as CodecWrite>::write(&id, &mut buf);
        let mut cursor: &[u8] = &buf;
        let result: G1 = read_group(&mut cursor).unwrap();
        assert_eq!(result, G1::zero());
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_group_nonzero_g1() {
        let g = G1::generator();
        let mut buf = Vec::new();
        <G1 as CodecWrite>::write(&g, &mut buf);
        let mut cursor: &[u8] = &buf;
        let result: G1 = read_group(&mut cursor).unwrap();
        assert_eq!(result, g);
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_group_identity_g2() {
        let id = G2::zero();
        let mut buf = Vec::new();
        <G2 as CodecWrite>::write(&id, &mut buf);
        let mut cursor: &[u8] = &buf;
        let result: G2 = read_group(&mut cursor).unwrap();
        assert_eq!(result, G2::zero());
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_group_nonzero_g2() {
        let g = G2::generator();
        let mut buf = Vec::new();
        <G2 as CodecWrite>::write(&g, &mut buf);
        let mut cursor: &[u8] = &buf;
        let result: G2 = read_group(&mut cursor).unwrap();
        assert_eq!(result, g);
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_group_buffer_too_short() {
        let short = [0u8; 16];
        let mut cursor: &[u8] = &short;
        let result: Result<G1, _> = read_group(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn read_group_multiple_g1() {
        let a = G1::generator();
        let b = G1::zero();
        let mut buf = Vec::new();
        <G1 as CodecWrite>::write(&a, &mut buf);
        <G1 as CodecWrite>::write(&b, &mut buf);
        let mut cursor: &[u8] = &buf;
        let ra: G1 = read_group(&mut cursor).unwrap();
        let rb: G1 = read_group(&mut cursor).unwrap();
        assert_eq!(ra, a);
        assert_eq!(rb, b);
        assert!(cursor.is_empty());
    }
}
