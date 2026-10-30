//! `StorageEngine` trait — engine-agnostic persistence contract (ADR-0024).
//!
//! The trait is deliberately small and byte-oriented: seven column families,
//! opaque `Vec<u8>` values, atomic batches, and **sorted** prefix iteration.
//! Sorting is a determinism requirement (Stage 3 determinism checklist, V-21
//! extension): the same key set must iterate in the same lexicographic order
//! on every engine implementation and every run — callers may not depend on
//! insertion order.
//!
//! Implementations in this crate:
//! - [`tests`-only] [`MemoryEngine`] — in-memory `BTreeMap` per CF for
//!   trait-level tests (S3-P04); inherently sorted, no I/O.
//! - `RocksDbEngine` (S3-P06, feature `rocksdb`) — production backend.
//!
//! `rocksdb::DB` is thread-safe, so the trait is `Send + Sync` and carries
//! no interior-mutex requirement: the monolith shares one engine across
//! tasks and the DB-lock level disappears from the lock ordering (ADR-0024
//! §Decision "Concurrency", documented for S3-P07).

use thiserror::Error;

/// One of the seven RocksDB column families fixed by ADR-0024.
///
/// The key codec for each CF lives in [`crate::schema`] (S3-P05); this enum
/// only names the sub-spaces at the engine boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cf {
    /// Node metadata: `schema_version`, tip, difficulty, chain id.
    Meta,
    /// Blocks by height + block-hash → height index.
    Blocks,
    /// Current account state (consensus state — never pruned).
    State,
    /// Transaction index + raw transactions.
    Transactions,
    /// Contract storage slots (`address ‖ slot` → value).
    ContractStorage,
    /// Pre-block account snapshots — the height-window-pruned history
    /// (ADR-0025).
    StateHistory,
    /// Mempool entries (`sender ‖ nonce` → raw tx), deleted on mine.
    Mempool,
}

impl Cf {
    /// All column families in stable declaration order.
    ///
    /// Opening code (S3-P06) iterates this to create CFs; the order is part
    /// of the crate's public surface only in that it must not change
    /// silently — adding a CF bumps the schema (ADR-0024 versioning).
    pub const ALL: [Cf; 7] = [
        Cf::Meta,
        Cf::Blocks,
        Cf::State,
        Cf::Transactions,
        Cf::ContractStorage,
        Cf::StateHistory,
        Cf::Mempool,
    ];

    /// Stable ordinal (index into [`Cf::ALL`]) — for array-backed engines.
    pub fn ordinal(self) -> usize {
        match self {
            Cf::Meta => 0,
            Cf::Blocks => 1,
            Cf::State => 2,
            Cf::Transactions => 3,
            Cf::ContractStorage => 4,
            Cf::StateHistory => 5,
            Cf::Mempool => 6,
        }
    }
}

/// A single mutation inside an atomic [`StorageEngine::write_batch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteBatchOp {
    /// Insert or overwrite `key` with `value` in `cf`.
    Put {
        cf: Cf,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    /// Remove `key` from `cf` (no-op if absent — delete is idempotent).
    Delete { cf: Cf, key: Vec<u8> },
}

/// Errors surfaced by every `StorageEngine` implementation.
///
/// Shape follows the `NetError` precedent (S2-P06): one crate-local enum,
/// no leakage of engine types through the trait boundary except the
/// feature-gated `From<rocksdb::Error>` convenience conversion.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Filesystem-level failure (directory creation, missing path, flush).
    #[error("storage i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// RocksDB backend failure (feature `rocksdb`).
    #[cfg(feature = "rocksdb")]
    #[error("rocksdb error: {0}")]
    Rocksdb(#[from] rocksdb::Error),

    /// Structurally broken bytes: truncated record, bad magic, failed
    /// checksum. Fail-closed — never "repair" silently.
    #[error("storage corrupt: {0}")]
    Corrupt(&'static str),

    /// On-disk `meta.schema_version` differs from this build's expectation;
    /// upgrades happen only through `migrations.rs` (S3-P08), never on open
    /// (ADR-0024 §Decision "Schema versioning").
    #[error("schema version mismatch: found {found}, expected {expected}")]
    SchemaVersion { found: u32, expected: u32 },
}

/// Engine-agnostic persistence contract over seven column families.
///
/// Contract highlights (ADR-0024 §Decision "Trait surface"):
/// - values are opaque bytes; consensus-type codecs live at the monolith edge;
/// - `write_batch` is **atomic**: all ops apply or none do;
/// - `iterate_prefix` returns entries **sorted by key** (bytewise
///   lexicographic — the same order RocksDB's default comparator uses);
/// - the raw DB handle never leaves the implementing crate.
pub trait StorageEngine: Send + Sync {
    /// Read `key` from `cf`. `Ok(None)` = absent (not an error).
    fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError>;

    /// Insert or overwrite. Convenience wrapper over a one-op batch.
    fn put(&self, cf: Cf, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        self.write_batch(vec![WriteBatchOp::Put {
            cf,
            key: key.to_vec(),
            value: value.to_vec(),
        }])
    }

    /// Remove `key`. Idempotent. Convenience wrapper over a one-op batch.
    fn delete(&self, cf: Cf, key: &[u8]) -> Result<(), StorageError> {
        self.write_batch(vec![WriteBatchOp::Delete {
            cf,
            key: key.to_vec(),
        }])
    }

    /// Apply all `ops` atomically (all-or-nothing).
    fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError>;

    /// All `(key, value)` pairs in `cf` whose key starts with `prefix`,
    /// **sorted by key**. Empty `prefix` = the whole CF.
    fn iterate_prefix(
        &self,
        cf: Cf,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError>;

    /// Drain memtables/WAL to stable storage (best effort for in-memory
    /// engines; a durability barrier for disk-backed ones).
    fn flush(&self) -> Result<(), StorageError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// In-memory [`StorageEngine`] for trait-level tests: one `BTreeMap`
    /// per CF behind a single mutex (sorted by construction — mirrors the
    /// trait's ordering contract without any I/O). The trait takes `&self`
    /// and is `Send + Sync` (ADR-0024 thread-safe design), so the mock
    /// needs interior mutability; one lock over the whole CF array also
    /// makes `write_batch` atomic with respect to concurrent readers.
    struct MemoryEngine {
        cfs: Mutex<[BTreeMap<Vec<u8>, Vec<u8>>; Cf::ALL.len()]>,
    }

    impl MemoryEngine {
        fn new() -> Self {
            Self {
                cfs: Mutex::new(std::array::from_fn(|_| BTreeMap::new())),
            }
        }
    }

    impl StorageEngine for MemoryEngine {
        fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
            let cfs = self.cfs.lock().expect("memory engine lock poisoned");
            Ok(cfs[cf.ordinal()].get(key).cloned())
        }

        fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError> {
            // Atomic with respect to other engine calls: one lock covers
            // the whole batch, and there is no failure path between
            // mutations.
            let mut cfs = self.cfs.lock().expect("memory engine lock poisoned");
            for op in ops {
                match op {
                    WriteBatchOp::Put { cf, key, value } => {
                        cfs[cf.ordinal()].insert(key, value);
                    }
                    WriteBatchOp::Delete { cf, key } => {
                        cfs[cf.ordinal()].remove(&key);
                    }
                }
            }
            Ok(())
        }

        fn iterate_prefix(
            &self,
            cf: Cf,
            prefix: &[u8],
        ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError> {
            let cfs = self.cfs.lock().expect("memory engine lock poisoned");
            Ok(cfs[cf.ordinal()]
                .range(prefix.to_vec()..)
                .take_while(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect())
        }

        fn flush(&self) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[test]
    fn put_get_roundtrip_and_delete_are_idempotent() {
        let engine = MemoryEngine::new();
        assert_eq!(engine.get(Cf::State, b"a").unwrap(), None);

        engine.put(Cf::State, b"a", b"1").unwrap();
        assert_eq!(engine.get(Cf::State, b"a").unwrap(), Some(b"1".to_vec()));

        // Overwrite.
        engine.put(Cf::State, b"a", b"2").unwrap();
        assert_eq!(engine.get(Cf::State, b"a").unwrap(), Some(b"2".to_vec()));

        engine.delete(Cf::State, b"a").unwrap();
        assert_eq!(engine.get(Cf::State, b"a").unwrap(), None);
        engine.delete(Cf::State, b"a").unwrap();
        assert_eq!(engine.get(Cf::State, b"a").unwrap(), None);
    }

    #[test]
    fn cfs_are_isolated_from_each_other() {
        let engine = MemoryEngine::new();
        engine.put(Cf::State, b"key", b"state").unwrap();
        engine.put(Cf::Mempool, b"key", b"mempool").unwrap();

        assert_eq!(engine.get(Cf::State, b"key").unwrap(), Some(b"state".to_vec()));
        assert_eq!(
            engine.get(Cf::Mempool, b"key").unwrap(),
            Some(b"mempool".to_vec())
        );
        assert_eq!(engine.get(Cf::Blocks, b"key").unwrap(), None);
    }

    #[test]
    fn write_batch_applies_all_ops() {
        let engine = MemoryEngine::new();
        engine.put(Cf::Transactions, b"old", b"stale").unwrap();

        engine
            .write_batch(vec![
                WriteBatchOp::Put {
                    cf: Cf::Transactions,
                    key: b"new-1".to_vec(),
                    value: b"v1".to_vec(),
                },
                WriteBatchOp::Put {
                    cf: Cf::Transactions,
                    key: b"new-2".to_vec(),
                    value: b"v2".to_vec(),
                },
                WriteBatchOp::Delete {
                    cf: Cf::Transactions,
                    key: b"old".to_vec(),
                },
            ])
            .unwrap();

        assert_eq!(
            engine.get(Cf::Transactions, b"old").unwrap(),
            None,
            "batch delete must apply"
        );
        assert_eq!(
            engine.get(Cf::Transactions, b"new-1").unwrap(),
            Some(b"v1".to_vec())
        );
        assert_eq!(
            engine.get(Cf::Transactions, b"new-2").unwrap(),
            Some(b"v2".to_vec())
        );
    }

    #[test]
    fn iterate_prefix_is_sorted_by_key_not_insertion_order() {
        let engine = MemoryEngine::new();
        // Deliberately out-of-order insertion (incl. keys that only sort
        // later as byte strings): iteration must come back lexicographic.
        let keys_in: [&[u8]; 6] = [b"b2", b"a10", b"c3", b"a2", b"b1", b"a1"];
        for key in keys_in {
            engine.put(Cf::Blocks, key, b"v").unwrap();
        }

        let keys: Vec<Vec<u8>> = engine
            .iterate_prefix(Cf::Blocks, b"")
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();

        let mut expected = keys.clone();
        expected.sort();
        assert_eq!(keys, expected);
        assert_eq!(
            keys,
            vec![
                b"a1".to_vec(),
                b"a10".to_vec(),
                b"a2".to_vec(),
                b"b1".to_vec(),
                b"b2".to_vec(),
                b"c3".to_vec(),
            ]
        );
    }

    #[test]
    fn iterate_prefix_excludes_non_matching_and_cross_cf_keys() {
        let engine = MemoryEngine::new();
        engine.put(Cf::StateHistory, b"0000000000000001addr", b"h1").unwrap();
        engine.put(Cf::StateHistory, b"0000000000000002addr", b"h2").unwrap();
        engine.put(Cf::StateHistory, b"ffffffffffffffffaddr", b"h-max").unwrap();
        engine.put(Cf::State, b"0000000000000001addr", b"current").unwrap();

        let entries = engine.iterate_prefix(Cf::StateHistory, b"000000000000000").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, b"h1");
        assert_eq!(entries[1].1, b"h2");

        // Empty prefix sweeps the whole CF, and only that CF.
        let all = engine.iterate_prefix(Cf::StateHistory, b"").unwrap();
        assert_eq!(all.len(), 3);
        let state_only = engine.iterate_prefix(Cf::State, b"").unwrap();
        assert_eq!(state_only.len(), 1);
        assert_eq!(state_only[0].1, b"current");
    }

    #[test]
    fn iterate_prefix_on_empty_cf_or_absent_prefix_is_empty() {
        let engine = MemoryEngine::new();
        assert!(engine.iterate_prefix(Cf::Meta, b"").unwrap().is_empty());

        engine.put(Cf::Meta, b"tip_height", b"\x01").unwrap();
        assert!(engine.iterate_prefix(Cf::Meta, b"nope").unwrap().is_empty());
    }

    #[test]
    fn cf_ordinals_are_unique_and_stable() {
        let mut seen = std::collections::HashSet::new();
        for (i, cf) in Cf::ALL.iter().enumerate() {
            assert_eq!(cf.ordinal(), i);
            assert!(seen.insert(cf.ordinal()), "duplicate ordinal for {cf:?}");
        }
        assert_eq!(seen.len(), 7, "ADR-0024 fixes exactly 7 column families");
    }
}
