//! BUG-S1-016 — U256 limb-order invariant.
//!
//! `u256_from_bytes`/`u256_to_bytes`/`u256_from_u64` must present values to
//! the LE-limb arithmetic helpers (`u256_mul`, `u256_div`, `u256_gt`, ...)
//! without reversing them. The previous BE-limb converters made
//! `mul(from_u64(3), from_u64(4))` overflow to zero and inverted `hash <=
//! target` ordering; these tests pin the numeric semantics directly.

use strangecoin_core::consensus::{
    u256_add, u256_div, u256_from_bytes, u256_from_u64, u256_gt, u256_le, u256_mul, u256_to_bytes,
    work_from_target,
};

fn bytes_of_pow2(bit: usize) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31 - bit / 8] = 1 << (bit % 8);
    bytes
}

#[test]
fn from_bytes_places_value_in_numeric_limbs() {
    // 2^200 spans the most-significant byte of the BE encoding but must land
    // in words[3] (bits 192..256) for the LE-limb arithmetic.
    let bytes = bytes_of_pow2(200);
    assert_eq!(u256_from_bytes(&bytes), [0, 0, 0, 1 << 8]);
    // 2^64 is exactly words[1].
    let bytes = bytes_of_pow2(64);
    assert_eq!(u256_from_bytes(&bytes), [0, 1, 0, 0]);
}

#[test]
fn bytes_roundtrip_preserves_patterns() {
    for bits in [0usize, 1, 63, 64, 127, 192, 199, 200, 255] {
        let bytes = bytes_of_pow2(bits);
        assert_eq!(u256_to_bytes(u256_from_bytes(&bytes)), bytes, "bit {bits}");
    }
    let ff = [0xffu8; 32];
    assert_eq!(u256_to_bytes(u256_from_bytes(&ff)), ff);
    let zero = [0u8; 32];
    assert_eq!(u256_to_bytes(u256_from_bytes(&zero)), zero);
}

#[test]
fn from_u64_agrees_with_from_bytes() {
    for v in [0u64, 1, 4, 1 << 8, u64::MAX] {
        let mut bytes = [0u8; 32];
        bytes[24..].copy_from_slice(&v.to_be_bytes());
        assert_eq!(u256_from_u64(v), u256_from_bytes(&bytes), "v={v}");
    }
}

#[test]
fn mul_of_small_scalars_does_not_overflow() {
    // With BE-limb converters from_u64(3) landed in words[3] = 3 * 2^192 and
    // the product 12 * 2^384 truncated to zero — retarget never scaled.
    assert_eq!(
        u256_mul(u256_from_u64(3), u256_from_u64(4)),
        u256_from_u64(12)
    );
    assert_eq!(
        u256_mul(u256_from_u64(0), u256_from_u64(4)),
        u256_from_u64(0)
    );
}

#[test]
fn ordering_follows_numeric_value() {
    let two_pow_200 = u256_from_bytes(&bytes_of_pow2(200));
    let one = u256_from_u64(1);
    assert!(u256_gt(two_pow_200, one), "2^200 must exceed 1");
    assert!(u256_le(one, two_pow_200));
    assert!(!u256_gt(one, two_pow_200));

    let two_pow_64 = u256_from_bytes(&bytes_of_pow2(64));
    let two_pow_63 = u256_from_bytes(&bytes_of_pow2(63));
    assert!(u256_gt(two_pow_64, two_pow_63));
}

#[test]
fn add_and_div_follow_numeric_value() {
    let sum = u256_add(u256_from_bytes(&bytes_of_pow2(200)), u256_from_u64(1));
    let mut expected = bytes_of_pow2(200);
    expected[31] = 1;
    assert_eq!(u256_to_bytes(sum), expected);

    // (2^256 - 1) / 2^200 = 2^56 - 1 exactly.
    let max = [u64::MAX; 4];
    let work = u256_div(max, u256_from_bytes(&bytes_of_pow2(200)));
    assert_eq!(work, u256_from_u64((1u64 << 56) - 1));
}

#[test]
fn work_from_target_matches_numeric_difficulty() {
    // Easier target (larger value) → less work; harder → more.
    let easy = work_from_target(&[0xffu8; 32]);
    assert_eq!(easy, u256_from_u64(1));
    let hard = work_from_target(&bytes_of_pow2(200));
    assert_eq!(hard, u256_from_u64((1u64 << 56) - 1));
    assert!(u256_gt(hard, easy));
    let zero_work = work_from_target(&[0u8; 32]);
    assert_eq!(zero_work, [0, 0, 0, 0]);
}
