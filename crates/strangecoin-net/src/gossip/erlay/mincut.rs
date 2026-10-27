//! S2-P15 — Erlay mincut sketches (ADR-0021): [`create_sketch`] +
//! [`decode_difference`].
//!
//! **TxId → element mapping** (ADR-0021 «TxId → element mapping»): a 32-byte
//! blake3 TxId is truncated to its first 8 bytes (big-endian) — a fixed,
//! salt-free, documented encoding over the GF(2^64) field the ADR selects.
//! The resulting `u64` cannot be inverted back to a 32-byte TxId, which is
//! why [`decode_difference`] returns short IDs and each peer resolves only
//! *its own* exclusive TxIds (see `reconcile.rs`); the complementary half of
//! the difference arrives from the peer as a regular INV.
//!
//! Two mapping edge cases (both documented, both harmless):
//!
//! * **zero** — libminisketch cannot store the element 0 (adding it is a
//!   no-op), so TxIds whose short ID is 0 are excluded from sketches; those
//!   txs keep flowing through the S2-P13 INV announce path (ADR-0021: Erlay
//!   never replaces INV);
//! * **collision** — two TxIds truncating to the same `u64` (probability
//!   ~n²/2^65 per set) behave as one element; a rare missed bulk-sync round
//!   is repaired by the INV announce path and the mandatory full-relay
//!   fallback.
//!
//! **Stub implementation** (ADR-0021 «Fallback dependency» escape hatch,
//! AGENTS.md Stage 2 Pitfalls): `minisketch-rs =0.1.9` is pinned optional in
//! `Cargo.toml` but default-off — its bindgen 0.49 build dependency requires
//! libclang, which is absent on this Windows host (verified 2026-10-27: no
//! LLVM, `LIBCLANG_PATH` unset). The active encoder therefore serializes the
//! sorted short-ID list directly. The *contract* is libminisketch-compatible:
//!
//! * decode fails when the symmetric difference exceeds the effective
//!   capacity (min of both envelopes) — the capacity bound real PinSketch
//!   decoding has, and the signal adaptive capacity growth keys on;
//! * malformed / unsorted / wrong-field-size / unknown-implementation
//!   envelopes fail with [`NetError::Protocol`] → mandatory full-relay
//!   fallback (ADR-0021, no tx loss);
//! * `impl_id` in the envelope reserves wire compatibility: a future
//!   SCIP-gated PinSketch payload uses `impl_id = 1`, and mixed stub/PinSketch
//!   peers fail cleanly into fallback instead of misparsing.
//!
//! **Determinism** (Stage 2 checklist, threat V-21 extension): pure functions
//! of (set, capacity) — sorting, two-pointer XOR, byte encoding only; no
//! `SystemTime`/`Instant`, no RNG, no I/O. Envelope elements are validated
//! strictly ascending on parse, so the same two sets + same capacity always
//! produce byte-identical sketches and identical decoded differences on both
//! peers.

use std::collections::HashSet;

use crate::error::NetError;

/// Field size of a sketch element (ADR-0021: GF(2^64)).
pub const SKETCH_BITS: u32 = 64;

/// Envelope implementation id: sorted-u64-list stub (the active impl).
pub const IMPL_STUB: u8 = 0;
/// Envelope implementation id reserved for the SCIP-gated minisketch-rs
/// PinSketch payload (never written today; parsing it fails into fallback).
pub const IMPL_PINSKETCH: u8 = 1;

const MAGIC: [u8; 4] = *b"ERS1";
/// `magic(4) + impl(1) + bits(2) + capacity(4) + count(4)`.
pub const ENVELOPE_HEADER_LEN: usize = 15;

/// A parsed sketch envelope.
struct Envelope {
    capacity: u64,
    elements: Vec<u64>,
}

/// Deterministic TxId → GF(2^64) element: first 8 bytes of the blake3 TxId,
/// big-endian. `None` for the reserved value 0 (libminisketch cannot store
/// element 0 — see the module docs).
pub fn short_id(txid: &[u8; 32]) -> Option<u64> {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&txid[..8]);
    let id = u64::from_be_bytes(buf);
    (id != 0).then_some(id)
}

/// Encode the known-tx set as a self-describing sketch envelope.
///
/// `capacity` bounds how many differences a later [`decode_difference`] can
/// recover (it does not bound the set size); it is clamped to `1..=u32::MAX`
/// (libminisketch rejects capacity 0 too). The element list is sorted and
/// deduplicated — set semantics, deterministic wire order.
pub fn create_sketch(set: &HashSet<[u8; 32]>, capacity: usize) -> Vec<u8> {
    let capacity = capacity.clamp(1, u32::MAX as usize) as u32;
    let mut elements: Vec<u64> = set.iter().filter_map(short_id).collect();
    elements.sort_unstable();
    elements.dedup();

    let mut out = Vec::with_capacity(ENVELOPE_HEADER_LEN + elements.len() * 8);
    out.extend_from_slice(&MAGIC);
    out.push(IMPL_STUB);
    out.extend_from_slice(&(SKETCH_BITS as u16).to_le_bytes());
    out.extend_from_slice(&capacity.to_le_bytes());
    let count = u32::try_from(elements.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_le_bytes());
    for element in &elements {
        out.extend_from_slice(&element.to_le_bytes());
    }
    out
}

/// Decode the set difference from two sketch envelopes.
///
/// Returns the symmetric difference as **short IDs** (sorted ascending) — the
/// prompt's literal `Vec<TxId>` return type is impossible for elements this
/// side does not hold (a 32-byte TxId is not invertible from a 64-bit field
/// element; deviation recorded in `erlay/mod.rs`). Each peer resolves the
/// half of the difference it possesses back to TxIds in `reconcile.rs`.
///
/// Errors (all `NetError::Protocol`, all → mandatory full-relay fallback):
/// malformed envelope, unknown `impl_id`, unsupported field size, unsorted
/// elements, or a difference larger than the effective capacity (min of both
/// envelope capacities — the libminisketch merge semantics).
pub fn decode_difference(local_sketch: &[u8], remote_sketch: &[u8]) -> Result<Vec<u64>, NetError> {
    let local = parse_envelope(local_sketch)?;
    let remote = parse_envelope(remote_sketch)?;
    let capacity = local.capacity.min(remote.capacity) as usize;
    let difference = xor_sorted(&local.elements, &remote.elements);
    if difference.len() > capacity {
        return Err(NetError::Protocol(format!(
            "erlay: difference {} exceeds capacity {capacity}",
            difference.len()
        )));
    }
    Ok(difference)
}

/// Parse + validate one envelope. Element count is derived from (and checked
/// against) the slice length before any allocation (invariant #16).
fn parse_envelope(bytes: &[u8]) -> Result<Envelope, NetError> {
    let malformed = |detail: &str| NetError::Protocol(format!("erlay: sketch {detail}"));
    if bytes.len() < ENVELOPE_HEADER_LEN {
        return Err(malformed("header truncated"));
    }
    if bytes[0..4] != MAGIC {
        return Err(malformed("bad magic"));
    }
    let impl_id = bytes[4];
    if impl_id != IMPL_STUB {
        return Err(NetError::Protocol(format!(
            "erlay: unsupported sketch implementation {impl_id}"
        )));
    }
    let bits = u16::from_le_bytes([bytes[5], bytes[6]]);
    if bits as u32 != SKETCH_BITS {
        return Err(NetError::Protocol(format!(
            "erlay: unsupported field size {bits} bits"
        )));
    }
    let capacity = u32::from_le_bytes([bytes[7], bytes[8], bytes[9], bytes[10]]) as u64;
    let count = u32::from_le_bytes([bytes[11], bytes[12], bytes[13], bytes[14]]) as usize;
    let payload = &bytes[ENVELOPE_HEADER_LEN..];
    if count * 8 != payload.len() {
        return Err(malformed("count exceeds payload length"));
    }
    let mut elements = Vec::with_capacity(count);
    let mut prev: Option<u64> = None;
    let (chunks, rest) = payload.as_chunks::<8>();
    debug_assert!(rest.is_empty(), "count * 8 checked above");
    for chunk in chunks {
        let element = u64::from_le_bytes(*chunk);
        if let Some(p) = prev {
            if element <= p {
                return Err(malformed("elements not strictly ascending"));
            }
        }
        prev = Some(element);
        elements.push(element);
    }
    Ok(Envelope { capacity, elements })
}

/// Symmetric difference of two ascending, deduplicated lists — the bitwise
/// XOR of the underlying sets, in sorted order.
fn xor_sorted(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txid_with_prefix(prefix: u8) -> [u8; 32] {
        let mut txid = [0u8; 32];
        txid[0] = prefix;
        txid
    }

    fn set_of(prefixes: &[u8]) -> HashSet<[u8; 32]> {
        prefixes.iter().map(|p| txid_with_prefix(*p)).collect()
    }

    #[test]
    fn short_id_is_first_eight_bytes_and_skips_zero() {
        let mut txid = [0u8; 32];
        txid[..8].copy_from_slice(&0x0102_0304_0506_0708u64.to_be_bytes());
        txid[8] = 0xFF;
        assert_eq!(short_id(&txid), Some(0x0102_0304_0506_0708));
        // Truncation must ignore everything past byte 8.
        assert_eq!(short_id(&[0u8; 32]), None, "reserved zero element");
    }

    #[test]
    fn create_sketch_is_deterministic_and_sorted() {
        let set = set_of(&[3, 1, 2]);
        let a = create_sketch(&set, 100);
        let b = create_sketch(&set, 100);
        assert_eq!(a, b, "same set + capacity → byte-identical sketch");
        let envelope = parse_envelope(&a).expect("parses");
        assert_eq!(envelope.capacity, 100);
        // txid[0] is the most-significant byte of the truncated u64.
        assert_eq!(
            envelope.elements,
            vec![1u64 << 56, 2u64 << 56, 3u64 << 56],
            "elements ascending"
        );
    }

    #[test]
    fn decode_returns_symmetric_difference_sorted() {
        // txid[0] == element high byte: shared {1,2}, local-only {3}, remote-only {4}.
        let local = set_of(&[1, 2, 3]);
        let remote = set_of(&[1, 2, 4]);
        let diff = decode_difference(&create_sketch(&local, 8), &create_sketch(&remote, 8))
            .expect("decodes");
        assert_eq!(diff, vec![3u64 << 56, 4u64 << 56]);
        // Identical sets → empty difference.
        assert!(
            decode_difference(&create_sketch(&local, 8), &create_sketch(&local, 8))
                .expect("decodes")
                .is_empty()
        );
    }

    #[test]
    fn decode_fails_when_difference_exceeds_capacity() {
        let local = set_of(&[1, 2, 3, 4, 5]);
        let remote = set_of(&[6, 7, 8, 9, 10]);
        let err = decode_difference(&create_sketch(&local, 4), &create_sketch(&remote, 4))
            .expect_err("diff 10 > capacity 4");
        assert!(err.to_string().contains("exceeds capacity"), "{err}");
        // Effective capacity = min(both): a generous local capacity cannot
        // rescue a small remote one (libminisketch merge semantics).
        let err = decode_difference(&create_sketch(&local, 100), &create_sketch(&remote, 4))
            .expect_err("min capacity 4");
        assert!(err.to_string().contains("exceeds capacity"), "{err}");
        // Same sets with adequate capacity succeed.
        assert_eq!(
            decode_difference(&create_sketch(&local, 16), &create_sketch(&remote, 16))
                .expect("decodes")
                .len(),
            10
        );
    }

    #[test]
    fn decode_rejects_malformed_envelopes() {
        let local = set_of(&[1]);
        let remote = set_of(&[2]);
        let good_local = create_sketch(&local, 8);
        let good_remote = create_sketch(&remote, 8);

        // Bad magic.
        let mut bad_magic = good_remote.clone();
        bad_magic[0] ^= 0xFF;
        assert!(decode_difference(&good_local, &bad_magic).is_err());

        // Unknown implementation (reserved PinSketch id until SCIP enables it).
        let mut bad_impl = good_remote.clone();
        bad_impl[4] = IMPL_PINSKETCH;
        let err = decode_difference(&good_local, &bad_impl).expect_err("impl id");
        assert!(err.to_string().contains("implementation"), "{err}");

        // Unsupported field size.
        let mut bad_bits = good_remote.clone();
        bad_bits[5..7].copy_from_slice(&32u16.to_le_bytes());
        assert!(decode_difference(&good_local, &bad_bits).is_err());

        // Truncated header.
        assert!(decode_difference(&good_local, &good_remote[..10]).is_err());

        // Element count inconsistent with payload length.
        let mut bad_count = good_remote.clone();
        bad_count[11..15].copy_from_slice(&9u32.to_le_bytes());
        assert!(decode_difference(&good_local, &bad_count).is_err());

        // Unsorted elements (would silently corrupt the two-pointer XOR).
        let mut unsorted = good_remote.clone();
        unsorted.truncate(ENVELOPE_HEADER_LEN);
        // two elements encoded descending where ascending is required:
        for element in [2u64, 1u64] {
            unsorted.extend_from_slice(&element.to_le_bytes());
        }
        unsorted[11..15].copy_from_slice(&2u32.to_le_bytes());
        assert!(decode_difference(&good_local, &unsorted).is_err());
    }

    #[test]
    fn zero_short_id_txs_are_excluded_from_sketches() {
        let mut zero_txid = [0u8; 32];
        zero_txid[8] = 7; // non-zero txid, but short_id() == 0
        let mut set = set_of(&[1]);
        set.insert(zero_txid);
        let envelope = parse_envelope(&create_sketch(&set, 8)).expect("parses");
        assert_eq!(envelope.elements.len(), 1, "zero element excluded");
    }

    #[test]
    fn duplicate_short_ids_collapse_to_one_element() {
        // Two distinct TxIds sharing the same first 8 bytes.
        let mut a = [0u8; 32];
        a[..8].copy_from_slice(&7u64.to_be_bytes());
        a[31] = 1;
        let mut b = a;
        b[31] = 2;
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b);
        let envelope = parse_envelope(&create_sketch(&set, 8)).expect("parses");
        assert_eq!(envelope.elements.len(), 1, "set semantics, no cancel-out");
    }
}
