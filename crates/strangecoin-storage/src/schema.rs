//! Key encoding + schema versioning for the ADR-0024 column families.
//!
//! Pure byte functions only — no I/O, no clocks, no RNG (Stage 3
//! determinism checklist, V-21 extension). This module intentionally does
//! **not** use `strangecoin-core` types: it encodes key *structure*
//! (heights, hashes, addresses, slots, nonces); value codecs for `Block` /
//! `AccountState` / `Transaction` stay at the monolith edge (ADR-0024
//! §Decision "Values"). Storage→core as a crate-level dependency is
//! allowed for the snapshot (S3-P13) / witness (S3-P11) modules; the
//! reverse (core→storage) is forbidden (ADR-0024 §Decision "Values").
//!
//! # Layout rules (ADR-0024)
//!
//! - **Primary record keys are bare**, exactly as the ADR's key column
//!   spells them: `meta` name keys, `height: u64` big-endian, address
//!   UTF-8 bytes, `sender ‖ nonce(u64 BE)`, raw `txid`,
//!   `address ‖ slot`. Fixed-width big-endian components keep
//!   lexicographic key order == numeric order (heights in `blocks` /
//!   `state_history`, nonces in `mempool` — iteration order is a
//!   determinism requirement, Stage 3 checklist #3).
//! - **Index sub-keyspaces carry a leading [`KeyPrefix`] tag byte**: the
//!   `blocks` hash→height index (ADR-0024 calls it "index sub-keyspace")
//!   and the `transactions` txid→(height, index) index (ADR-0024: "raw +
//!   index sub-keys"). Without a tag the two `transactions` sub-keys
//!   would be byte-identical (same 32-byte txid, same CF) — the prompt
//!   requires two separate constructors, so the tag is what keeps them
//!   disjoint (33 B index vs 32 B raw).
//! - ROADMAP3's CF-spanning letter tags (`c/`, `s/`, `b/` …) were
//!   **rejected** by ADR-0024 §Alternatives (Alternative 1): column
//!   families do the separation. `KeyPrefix` exists only *within* a CF.
//!
//! Values are opaque bytes at the engine boundary; this module owns KEY
//! structure only. Decoders exist where the framing is unambiguous
//! (`schema_version`, block height, `state_history`, `mempool` — keys are
//! split at a fixed offset). `state_key`/`meta_key` are identity
//! encodings and `contract_storage_key` (`address ‖ slot`) has no
//! length-prefixed framing, so there is nothing to decode without
//! already knowing the address.

/// On-disk schema version written to `meta` CF under
/// [`META_SCHEMA_VERSION`].
///
/// `1` = this per-block/per-account layout (ADR-0024). `0` is reserved
/// for the legacy LevelDB blob schema (`chain`/`balances` blobs —
/// ADR-0024 §Context), which only `migrations.rs` (S3-P08) ever reads.
/// The guard itself lives in `RocksDbEngine::open` (S3-P06): fresh DB →
/// initialize to `1`, existing DB with a different value →
/// `StorageError::SchemaVersion` (fail-closed, no auto-upgrade).
pub const SCHEMA_VERSION: u32 = 1;

/// Known `meta` CF name keys (ADR-0024 §Column families → `meta`).
///
/// [`meta_key`] takes the name as `&str` (the key *is* the UTF-8 name);
/// these constants exist so call sites never spell magic strings.
pub const META_SCHEMA_VERSION: &str = "schema_version";
/// Chain tip height (`u64` BE value).
pub const META_TIP_HEIGHT: &str = "tip_height";
/// Chain tip block hash (32-byte value).
pub const META_TIP_HASH: &str = "tip_hash";
/// Current difficulty target (value encoding owned by the monolith edge).
pub const META_DIFFICULTY: &str = "difficulty";
/// Network/chain id (value encoding owned by the monolith edge).
pub const META_CHAIN_ID: &str = "chain_id";

/// Leading tag byte for the index sub-keyspaces that live in the *same*
/// column family as another key space (ADR-0024 "index sub-keyspace" /
/// "raw + index sub-keys").
///
/// The primary record keys of every CF stay bare (see module docs); only
/// index records are tagged, and only where two constructors would
/// otherwise produce identical bytes in one CF:
///
/// | Variant | CF | Key | Value |
/// |---|---|---|---|
/// | [`KeyPrefix::BlockHashIndex`] | `blocks` | `0x00 ‖ hash` | height `u64` BE |
/// | [`KeyPrefix::TxIndex`] | `transactions` | `0x01 ‖ txid` | `(height, tx_index)` |
///
/// Tag byte values carry no ordering contract (storage is an
/// implementation detail, not consensus — ADR-0024): both CFs are read
/// by point lookup, and no prefix iteration spans the two sub-spaces of
/// either CF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyPrefix {
    /// `blocks` CF: block hash → height index sub-space.
    BlockHashIndex = 0x00,
    /// `transactions` CF: txid → `(height, tx_index)` index sub-space.
    TxIndex = 0x01,
}

impl KeyPrefix {
    /// The raw leading byte this prefix writes into keys.
    pub const fn byte(self) -> u8 {
        self as u8
    }
}

/// `meta` CF key: the UTF-8 name itself (ADR-0024 key column).
///
/// Known names: [`META_SCHEMA_VERSION`], [`META_TIP_HEIGHT`],
/// [`META_TIP_HASH`], [`META_DIFFICULTY`], [`META_CHAIN_ID`].
pub fn meta_key(name: &str) -> Vec<u8> {
    name.as_bytes().to_vec()
}

/// `blocks` CF primary key: block height as big-endian `u64`
/// (lexicographic order == numeric order). Value: canonical block bytes.
pub fn block_by_height_key(height: u64) -> Vec<u8> {
    height.to_be_bytes().to_vec()
}

/// `blocks` CF index key: block hash → height
/// (`KeyPrefix::BlockHashIndex ‖ hash`; value: height `u64` BE).
///
/// Disjoint from [`block_by_height_key`] by tag + length (33 B vs 8 B).
pub fn block_hash_index_key(hash: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + hash.len());
    key.push(KeyPrefix::BlockHashIndex.byte());
    key.extend_from_slice(hash);
    key
}

/// `state` CF key: account address as UTF-8 bytes (identity encoding —
/// no framing to decode back). Value: canonical `AccountState` bytes.
pub fn state_key(address: &str) -> Vec<u8> {
    address.as_bytes().to_vec()
}

/// `state_history` CF key: `height: u64 BE ‖ address` — ADR-0024.
///
/// The fixed 8-byte height prefix makes `iterate_prefix` over
/// `height.to_be_bytes()` return every account snapshot written for that
/// height, ordered by address (ADR-0025 pruning walks this).
pub fn state_history_key(height: u64, address: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(8 + address.len());
    key.extend_from_slice(&height.to_be_bytes());
    key.extend_from_slice(address.as_bytes());
    key
}

/// `transactions` CF primary key: the bare 32-byte txid (ADR-0024 key
/// column). Value: canonical raw transaction bytes.
///
/// Disjoint from [`tx_index_key`] (32 B vs 33 B — tag + length).
pub fn tx_raw_key(txid: &[u8; 32]) -> Vec<u8> {
    txid.to_vec()
}

/// `transactions` CF index key: txid → `(height, tx_index)`
/// (`KeyPrefix::TxIndex ‖ txid`; value: `height BE ‖ tx_index BE`).
///
/// The tag is what separates this sub-space from [`tx_raw_key`]: both
/// constructors are keyed by the same txid in the same CF (ADR-0024
/// "raw + index sub-keys").
pub fn tx_index_key(txid: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + txid.len());
    key.push(KeyPrefix::TxIndex.byte());
    key.extend_from_slice(txid);
    key
}

/// `contract_storage` CF key: `address ‖ slot_key` — ADR-0024.
///
/// Uniqueness under the fixed-length bech32 address invariant of the
/// chain (same HRP ⇒ same address length ⇒ the address/slot boundary is
/// implicit). No decoder: splitting arbitrary `address ‖ slot` bytes
/// would need the address length, which the caller already owns.
pub fn contract_storage_key(address: &str, slot: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(address.len() + slot.len());
    key.extend_from_slice(address.as_bytes());
    key.extend_from_slice(slot);
    key
}

/// `mempool` CF key: `sender ‖ nonce: u64 BE` — ADR-0024.
///
/// Big-endian nonce ⇒ iterating
/// `iterate_prefix(Mempool, mempool_sender_prefix(sender))` returns one
/// sender's transactions in ascending numeric nonce order. Same
/// fixed-length bech32 sender assumption as [`contract_storage_key`].
pub fn mempool_key(sender: &str, nonce: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(sender.len() + 8);
    key.extend_from_slice(sender.as_bytes());
    key.extend_from_slice(&nonce.to_be_bytes());
    key
}

/// `mempool` CF prefix covering every queued transaction of `sender`
/// ([`mempool_sender_prefix`] = the sender bytes; the nonce tail sorts
/// them). The Stage 1 full-DB-scan load path dies here (ADR-0024
/// §Consequences).
pub fn mempool_sender_prefix(sender: &str) -> Vec<u8> {
    sender.as_bytes().to_vec()
}

/// `meta` CF value for [`META_SCHEMA_VERSION`]: `u32` big-endian,
/// 4 bytes (ADR-0024 §Column families → `meta`).
pub fn encode_schema_version(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// Inverse of [`encode_schema_version`]; `None` on wrong length (the
/// caller maps that to [`crate::StorageError::Corrupt`] at open — the
/// engine-side guard is S3-P06).
pub fn decode_schema_version(bytes: &[u8]) -> Option<u32> {
    let arr: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_be_bytes(arr))
}

/// Inverse of [`block_by_height_key`]; `None` unless the key is exactly
/// the 8-byte height form (a tagged hash-index key decodes to `None`).
pub fn decode_block_height_key(key: &[u8]) -> Option<u64> {
    let arr: [u8; 8] = key.try_into().ok()?;
    Some(u64::from_be_bytes(arr))
}

/// Inverse of [`state_history_key`]: split at the fixed 8-byte height
/// offset; `None` on short keys or non-UTF-8 addresses.
pub fn decode_state_history_key(key: &[u8]) -> Option<(u64, &str)> {
    if key.len() < 8 {
        return None;
    }
    let height = u64::from_be_bytes(key[..8].try_into().ok()?);
    Some((height, std::str::from_utf8(&key[8..]).ok()?))
}

/// Inverse of [`mempool_key`]: split at the fixed 8-byte nonce tail;
/// `None` on short keys or non-UTF-8 senders.
pub fn decode_mempool_key(key: &[u8]) -> Option<(&str, u64)> {
    let split = key.len().checked_sub(8)?;
    let nonce = u64::from_be_bytes(key[split..].try_into().ok()?);
    Some((std::str::from_utf8(&key[..split]).ok()?, nonce))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    /// Bech32-like fixed-length addresses (35 bytes for the `sc1…` HRP
    /// shape): the length invariant is what makes the `address ‖ …`
    /// framings unambiguous, so property tests pin it.
    fn address_strategy() -> impl Strategy<Value = String> {
        proptest::string::string_regex("[0-9a-z]{35}").expect("valid regex")
    }

    /// Fixed-length test address matching [`address_strategy`] (3 + 32).
    fn fixed_addr(fill: &str) -> String {
        format!("sc1{}", fill.repeat(32))
    }

    #[test]
    fn schema_version_constant_and_roundtrip() {
        assert_eq!(SCHEMA_VERSION, 1, "ADR-0024 fixes schema v1");

        assert_eq!(encode_schema_version(SCHEMA_VERSION), [0, 0, 0, 1]);
        assert_eq!(encode_schema_version(0), [0, 0, 0, 0]);
        assert_eq!(encode_schema_version(u32::MAX), [0xff, 0xff, 0xff, 0xff]);

        for v in [0u32, 1, 42, u32::MAX] {
            assert_eq!(decode_schema_version(&encode_schema_version(v)), Some(v));
        }
        assert_eq!(decode_schema_version(&[0, 0, 1]), None, "wrong length");
        assert_eq!(decode_schema_version(&[0, 0, 0, 1, 0]), None, "too long");
        assert_eq!(decode_schema_version(&[]), None);
    }

    #[test]
    fn meta_keys_are_the_names_and_distinct() {
        let names = [
            META_SCHEMA_VERSION,
            META_TIP_HEIGHT,
            META_TIP_HASH,
            META_DIFFICULTY,
            META_CHAIN_ID,
        ];
        let keys: BTreeSet<Vec<u8>> = names.iter().map(|n| meta_key(n)).collect();
        assert_eq!(keys.len(), names.len(), "known meta names must not collide");

        assert_eq!(meta_key(META_TIP_HEIGHT), b"tip_height".to_vec());
        assert_eq!(meta_key(META_SCHEMA_VERSION), b"schema_version".to_vec());
        assert_eq!(meta_key(META_TIP_HASH), b"tip_hash".to_vec());
        assert_eq!(meta_key(META_DIFFICULTY), b"difficulty".to_vec());
        assert_eq!(meta_key(META_CHAIN_ID), b"chain_id".to_vec());
    }

    #[test]
    fn height_keys_order_numerically() {
        // Golden bytes: big-endian, fixed 8 bytes.
        assert_eq!(block_by_height_key(0), vec![0u8; 8]);
        assert_eq!(
            block_by_height_key(0x0102_0304_0506_0708),
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        );

        // The headline order invariant (КГ): lexicographic == numeric,
        // including the 255/256 and 65535/65536 byte-carry boundaries.
        let heights = [
            0u64,
            1,
            255,
            256,
            65_535,
            65_536,
            u64::from(u32::MAX),
            u64::from(u32::MAX) + 1,
            u64::MAX,
        ];
        for pair in heights.windows(2) {
            assert!(
                block_by_height_key(pair[0]) < block_by_height_key(pair[1]),
                "key({}) < key({})",
                pair[0],
                pair[1]
            );
        }

        assert_eq!(decode_block_height_key(&block_by_height_key(42)), Some(42));
        assert_eq!(decode_block_height_key(&[0u8; 7]), None, "wrong length");
    }

    proptest! {
        #[test]
        fn height_key_order_matches_numeric_order(a: u64, b: u64) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(block_by_height_key(lo) <= block_by_height_key(hi));
        }

        // `state_history` iterates by 8-byte height prefix (ADR-0025
        // pruning): keys of lower heights must always sort first,
        // whatever addresses ride behind them.
        #[test]
        fn state_history_orders_by_height_first(
            h1: u64,
            h2: u64,
            a1 in address_strategy(),
            a2 in address_strategy(),
        ) {
            prop_assume!(h1 < h2);
            prop_assert!(state_history_key(h1, &a1) < state_history_key(h2, &a2));
        }
    }

    #[test]
    fn state_history_height_prefix_groups_accounts() {
        let addr = fixed_addr("q");
        for h in [1u64, 2, 255, 256, u64::MAX] {
            assert!(
                state_history_key(h, &addr).starts_with(&h.to_be_bytes()),
                "8-byte BE height is the prefix of every key at h={h}"
            );
        }

        // Same-height group: both accounts share the height prefix, so a
        // prefix iterate on `h.to_be_bytes()` covers exactly that height.
        let h = 7u64;
        let a1 = fixed_addr("a");
        let a2 = fixed_addr("b");
        let prefix = h.to_be_bytes();
        assert!(state_history_key(h, &a1).starts_with(&prefix));
        assert!(state_history_key(h, &a2).starts_with(&prefix));

        // Round-trip at the fixed split offset.
        let key = state_history_key(1_234_567, &a1);
        assert_eq!(
            decode_state_history_key(&key),
            Some((1_234_567, a1.as_str()))
        );
        assert_eq!(decode_state_history_key(&[0u8; 7]), None, "short key");
        assert_eq!(
            decode_state_history_key(&[0u8; 8]),
            Some((0, "")),
            "empty address still decodes at the fixed offset"
        );
    }

    #[test]
    fn mempool_sender_prefix_is_contiguous_and_numeric() {
        let s1 = fixed_addr("x");
        let s2 = fixed_addr("y");
        // Equal-length senders: neither is a prefix of the other (the
        // fixed-length bech32 invariant).
        assert_eq!(s1.len(), s2.len());

        // Deliberately shuffled insertion order incl. byte-carry nonces.
        let mut keys: Vec<Vec<u8>> = [5u64, 0, 1, 256, 1 << 32, u64::MAX]
            .into_iter()
            .map(|n| mempool_key(&s1, n))
            .chain([7u64, 1].into_iter().map(|n| mempool_key(&s2, n)))
            .collect();
        keys.sort();

        let p1 = mempool_sender_prefix(&s1);
        let p2 = mempool_sender_prefix(&s2);
        assert_eq!(p1, s1.as_bytes().to_vec());
        // Equal-length distinct senders: each key matches exactly one
        // sender prefix (the prefix classifies senders unambiguously).
        for k in &keys {
            assert!(
                k.starts_with(&p1) ^ k.starts_with(&p2),
                "exactly one sender prefix matches"
            );
        }

        // Contiguity (КГ): every key of s1 occupies one unbroken slice of
        // the sorted keyspace.
        let idxs: Vec<usize> = keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.starts_with(&p1))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(idxs.len(), 6, "all six s1 nonces found by prefix");
        assert_eq!(
            idxs.last().unwrap() - idxs.first().unwrap() + 1,
            idxs.len(),
            "s1 keys must be contiguous in sorted order"
        );

        // Numeric (not lexicographic-string) nonce order inside the block.
        let nonces: Vec<u64> = idxs
            .iter()
            .map(|&i| decode_mempool_key(&keys[i]).unwrap().1)
            .collect();
        assert_eq!(nonces, vec![0, 1, 5, 256, 1 << 32, u64::MAX]);

        // Round-trip.
        let key = mempool_key(&s1, 99);
        assert_eq!(decode_mempool_key(&key), Some((s1.as_str(), 99)));
        assert_eq!(decode_mempool_key(&[0u8; 7]), None, "short key");
    }

    #[test]
    fn index_subspaces_never_collide_in_their_cf() {
        // transactions CF: raw (32 B, bare) vs index (33 B, tagged) —
        // strongest case first: a txid whose first byte equals the tag.
        let txid = [KeyPrefix::TxIndex.byte(); 32];
        let other = [0xAAu8; 32];
        assert_eq!(tx_raw_key(&txid), txid.to_vec());
        assert_eq!(tx_index_key(&txid)[0], KeyPrefix::TxIndex.byte());
        assert_eq!(&tx_index_key(&txid)[1..], &txid[..]);
        assert_ne!(tx_raw_key(&txid), tx_index_key(&txid));
        assert_ne!(tx_raw_key(&txid), tx_index_key(&other));

        // blocks CF: height (8 B, bare) vs hash index (33 B, tagged) —
        // strongest case: all-zero hash, zero heights.
        let hash = [0u8; 32];
        assert_eq!(
            block_hash_index_key(&hash)[0],
            KeyPrefix::BlockHashIndex.byte()
        );
        assert_eq!(&block_hash_index_key(&hash)[1..], &hash[..]);
        for h in [0u64, 1, u64::MAX] {
            let hk = block_by_height_key(h);
            assert_ne!(hk, block_hash_index_key(&hash));
            assert_eq!(hk.len(), 8);
        }
        assert_eq!(block_hash_index_key(&hash).len(), 33);
    }

    proptest! {
        // Cross-pair property: no raw-txid key ever equals any
        // tx-index key (same-CF collisions are the only ones CF
        // isolation cannot hide — cross-CF byte equality is harmless
        // because the CFs are separate key spaces).
        #[test]
        fn tx_subspaces_never_collide(
            t1 in proptest::array::uniform32(any::<u8>()),
            t2 in proptest::array::uniform32(any::<u8>()),
        ) {
            prop_assert_ne!(tx_raw_key(&t1), tx_index_key(&t2));
        }

        // Injectivity per codec inside its own CF (fuzz-style, КГ):
        // distinct inputs ⇒ distinct keys, fixed-length addresses.
        #[test]
        fn address_key_codecs_are_injective(
            a1 in address_strategy(),
            a2 in address_strategy(),
            h1: u64,
            h2: u64,
            n1: u64,
            n2: u64,
            s1 in proptest::collection::vec(any::<u8>(), 0..64),
            s2 in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            if a1 != a2 {
                prop_assert_ne!(state_key(&a1), state_key(&a2));
            }
            if h1 != h2 || a1 != a2 {
                prop_assert_ne!(state_history_key(h1, &a1), state_history_key(h2, &a2));
            }
            if n1 != n2 || a1 != a2 {
                prop_assert_ne!(mempool_key(&a1, n1), mempool_key(&a2, n2));
            }
            if s1 != s2 || a1 != a2 {
                prop_assert_ne!(contract_storage_key(&a1, &s1), contract_storage_key(&a2, &s2));
            }
        }
    }

    #[test]
    fn contract_storage_key_frames_address_then_slot() {
        let addr = fixed_addr("c");
        let slot = [7u8; 32];
        let key = contract_storage_key(&addr, &slot);
        assert_eq!(&key[..addr.len()], addr.as_bytes());
        assert_eq!(&key[addr.len()..], &slot[..]);

        // Prefix-iterate by contract address works for the caller that
        // already knows the (fixed-length) address.
        let key2 = contract_storage_key(&addr, &[8u8; 32]);
        assert!(key.starts_with(addr.as_bytes()) && key2.starts_with(addr.as_bytes()));
        assert_ne!(key, key2);
    }

    #[test]
    fn key_codecs_are_deterministic() {
        // Stage 3 determinism checklist #1/#5: same input ⇒ same bytes;
        // pure functions, no clock/RNG/io surfaces.
        let txid = [9u8; 32];
        let hash = [3u8; 32];
        let addr = fixed_addr("d");

        for _ in 0..2 {
            assert_eq!(meta_key(META_TIP_HEIGHT), meta_key(META_TIP_HEIGHT));
            assert_eq!(block_by_height_key(77), block_by_height_key(77));
            assert_eq!(block_hash_index_key(&hash), block_hash_index_key(&hash));
            assert_eq!(state_key(&addr), state_key(&addr));
            assert_eq!(state_history_key(77, &addr), state_history_key(77, &addr));
            assert_eq!(tx_raw_key(&txid), tx_raw_key(&txid));
            assert_eq!(tx_index_key(&txid), tx_index_key(&txid));
            assert_eq!(
                contract_storage_key(&addr, &[1, 2, 3]),
                contract_storage_key(&addr, &[1, 2, 3])
            );
            assert_eq!(mempool_key(&addr, 77), mempool_key(&addr, 77));
            assert_eq!(mempool_sender_prefix(&addr), mempool_sender_prefix(&addr));
            assert_eq!(
                encode_schema_version(SCHEMA_VERSION),
                encode_schema_version(SCHEMA_VERSION)
            );
        }
    }
}
