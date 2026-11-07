//! Real-backend tests for `RocksDbEngine` (S3-P06).
//!
//! Every test owns a fresh `tempfile` directory — no shared paths, no
//! `NETWORK_TEST_LOCK`-style coordination needed (RocksDB locks per
//! directory). The suite covers the КГ list: open/round-trip per CF,
//! batch atomicity across reopen, sorted prefix iteration + CF isolation,
//! the fail-closed schema-version guard, and a process-crash smoke.
//!
//! The crate builds without the `rocksdb` feature too
//! (`--no-default-features`): this file compiles to nothing then.
#![cfg(feature = "rocksdb")]

use std::process::Command;

use strangecoin_storage::{
    block_by_height_key, decode_block_height_key, decode_mempool_key, encode_schema_version,
    mempool_key, mempool_sender_prefix, meta_key, Cf, RocksDbEngine, StorageEngine, StorageError,
    WriteBatchOp, META_SCHEMA_VERSION, SCHEMA_VERSION,
};

/// Fixed-length bech32-like senders (the schema's fixed-length address
/// invariant, cf. `schema.rs` tests).
fn sender(fill: &str) -> String {
    format!("sc1{}", fill.repeat(32))
}

#[test]
fn open_initializes_schema_version_on_a_fresh_db() {
    let dir = tempfile::tempdir().unwrap();
    let engine = RocksDbEngine::open(dir.path()).unwrap();

    let stored = engine
        .get(Cf::Meta, &meta_key(META_SCHEMA_VERSION))
        .unwrap();
    assert_eq!(
        stored,
        Some(encode_schema_version(SCHEMA_VERSION).to_vec()),
        "fresh DB must be initialized to SCHEMA_VERSION (ADR-0024 guard)"
    );
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn put_get_delete_roundtrip_per_cf_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = RocksDbEngine::open(dir.path()).unwrap();

        // The same key bytes land in all seven CFs independently —
        // column families are disjoint key spaces.
        for cf in Cf::ALL {
            assert_eq!(engine.get(cf, b"rk").unwrap(), None);
            engine.put(cf, b"rk", b"v1").unwrap();
            assert_eq!(engine.get(cf, b"rk").unwrap(), Some(b"v1".to_vec()));
        }
        for cf in Cf::ALL {
            engine.put(cf, b"rk", b"v2").unwrap();
            assert_eq!(engine.get(cf, b"rk").unwrap(), Some(b"v2".to_vec()));
        }

        // Delete is idempotent (trait contract) and CF-scoped.
        engine.delete(Cf::State, b"rk").unwrap();
        engine.delete(Cf::State, b"rk").unwrap();
        assert_eq!(engine.get(Cf::State, b"rk").unwrap(), None);
        assert_eq!(engine.get(Cf::Blocks, b"rk").unwrap(), Some(b"v2".to_vec()));
        // Drop below flushes (all-CF flush contract).
    }

    // Reopen: the schema guard passes (version still 1) and every write
    // except the deleted one persisted through Drop's flush.
    let engine = RocksDbEngine::open(dir.path()).unwrap();
    for cf in Cf::ALL {
        if cf == Cf::State {
            assert_eq!(
                engine.get(cf, b"rk").unwrap(),
                None,
                "deleted key stays deleted"
            );
        } else {
            assert_eq!(engine.get(cf, b"rk").unwrap(), Some(b"v2".to_vec()));
        }
    }
}

#[test]
fn write_batch_applies_all_100_ops_atomically_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let s = sender("b");
    {
        let engine = RocksDbEngine::open(dir.path()).unwrap();
        engine
            .put(Cf::Transactions, b"to-delete", b"stale")
            .unwrap();

        // 100 mempool puts (BE-nonce keys from the S3-P05 codec) + one
        // delete, committed as a single WriteBatch. RocksDB batches are
        // atomic by construction — the observable contract this pins is
        // "all 100 or none survive the reopen", never a partial batch.
        let mut ops: Vec<WriteBatchOp> = (0..100u64)
            .map(|nonce| WriteBatchOp::Put {
                cf: Cf::Mempool,
                key: mempool_key(&s, nonce),
                value: nonce.to_be_bytes().to_vec(),
            })
            .collect();
        ops.push(WriteBatchOp::Delete {
            cf: Cf::Transactions,
            key: b"to-delete".to_vec(),
        });
        engine.write_batch(ops).unwrap();
        // No explicit flush: reopen must see the batch via WAL/memtable
        // persistence regardless (Drop flushes, but nothing depends on
        // that here beyond durability).
    }

    let engine = RocksDbEngine::open(dir.path()).unwrap();
    for nonce in 0..100u64 {
        assert_eq!(
            engine.get(Cf::Mempool, &mempool_key(&s, nonce)).unwrap(),
            Some(nonce.to_be_bytes().to_vec()),
            "batch op {nonce} must be present — no partial batches"
        );
    }
    assert_eq!(engine.get(Cf::Transactions, b"to-delete").unwrap(), None);
}

#[test]
fn iterate_prefix_is_sorted_and_cf_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let engine = RocksDbEngine::open(dir.path()).unwrap();

    let s1 = sender("x");
    let s2 = sender("y");

    // Deliberately shuffled insertion order incl. byte-carry nonces.
    for nonce in [5u64, 0, 1, 256, u64::MAX] {
        engine
            .put(Cf::Mempool, &mempool_key(&s1, nonce), &nonce.to_be_bytes())
            .unwrap();
    }
    for nonce in [7u64, 1] {
        engine
            .put(Cf::Mempool, &mempool_key(&s2, nonce), &nonce.to_be_bytes())
            .unwrap();
    }
    // Same bytes in a different CF — must never leak into the Mempool scan.
    engine
        .put(Cf::State, &mempool_key(&s1, 3), b"decoy")
        .unwrap();

    let entries = engine
        .iterate_prefix(Cf::Mempool, &mempool_sender_prefix(&s1))
        .unwrap();
    let nonces: Vec<u64> = entries
        .iter()
        .map(|(k, v)| {
            assert_eq!(v, &k[k.len() - 8..], "value mirrors the BE nonce tail");
            decode_mempool_key(k).expect("schema key decodes").1
        })
        .collect();
    assert_eq!(
        nonces,
        vec![0, 1, 5, 256, u64::MAX],
        "numeric (BE) order, s1 only"
    );

    let all_mempool = engine.iterate_prefix(Cf::Mempool, b"").unwrap();
    assert_eq!(all_mempool.len(), 7, "empty prefix sweeps the whole CF");
    let mut sorted = all_mempool.clone();
    sorted.sort();
    assert_eq!(all_mempool, sorted, "iteration is lexicographically sorted");

    let state_only = engine.iterate_prefix(Cf::State, b"").unwrap();
    assert_eq!(state_only.len(), 1, "CF isolation: decoy lives in State");
    assert_eq!(state_only[0].1, b"decoy");

    assert!(engine
        .iterate_prefix(Cf::Mempool, b"zzz")
        .unwrap()
        .is_empty());

    // Height BE ordering on the real backend (blocks CF): insert out of
    // order, iterate, decode — ascending numeric heights.
    for height in [9u64, 10, 255, 256, 1] {
        engine
            .put(
                Cf::Blocks,
                &block_by_height_key(height),
                &height.to_be_bytes(),
            )
            .unwrap();
    }
    let heights: Vec<u64> = engine
        .iterate_prefix(Cf::Blocks, b"")
        .unwrap()
        .iter()
        .filter_map(|(k, _)| decode_block_height_key(k))
        .collect();
    assert_eq!(heights, vec![1, 9, 10, 255, 256]);
}

#[test]
fn schema_version_guard_fails_closed_on_mismatch_and_corruption() {
    // Version mismatch: hand-write 99 over the guard key, reopen.
    let mismatch_dir = tempfile::tempdir().unwrap();
    {
        let engine = RocksDbEngine::open(mismatch_dir.path()).unwrap();
        engine
            .put(
                Cf::Meta,
                &meta_key(META_SCHEMA_VERSION),
                &encode_schema_version(99),
            )
            .unwrap();
    }
    match RocksDbEngine::open(mismatch_dir.path()) {
        Err(StorageError::SchemaVersion { found, expected }) => {
            assert_eq!((found, expected), (99, SCHEMA_VERSION));
        }
        other => panic!("expected SchemaVersion error, got {other:?}"),
    }

    // Corruption: the record exists but is not a 4-byte u32.
    let corrupt_dir = tempfile::tempdir().unwrap();
    {
        let engine = RocksDbEngine::open(corrupt_dir.path()).unwrap();
        engine
            .put(Cf::Meta, &meta_key(META_SCHEMA_VERSION), &[0, 0, 1])
            .unwrap();
    }
    match RocksDbEngine::open(corrupt_dir.path()) {
        Err(StorageError::Corrupt(msg)) => assert!(msg.contains("schema_version")),
        other => panic!("expected Corrupt error, got {other:?}"),
    }
}

/// Process-crash smoke (S3-P06 КГ): an unflushed put survives an
/// abrupt process death and is restored from the WAL on reopen.
///
/// The child leg of this very test re-runs itself in a subprocess
/// (same exe, `--exact` filter + env marker), writes one key with no
/// `flush`/`Drop` at all, then `abort()`s. The parent then reopens the
/// same directory and asserts the value is there.
///
/// *Deviation (FBP, confirmed by the reviewer): the prompt prescribed
/// `std::mem::forget` + reopen in-process. A forgotten `RocksDbEngine`
/// leaks its `rocksdb::DB`, which keeps the OS LOCK file held — on
/// Windows `LockFileEx` conflicts even inside one process, so the reopen
/// would fail with a lock error instead of proving WAL recovery. The
/// subprocess abort keeps the prompt's intent (no flush, no destructor,
/// WAL restore) and works on every platform.*
///
/// Limitations (documented per КГ): `abort()` ≠ power loss — the WAL
/// bytes already sit in the OS page cache when `put` returns (`sync =
/// false` only skips fsync); a real kill/corruption suite belongs to
/// S3-P10.
#[test]
fn crash_smoke_wal_restores_after_child_process_abort() {
    const CHILD_ENV: &str = "STRANGECOIN_S3_P06_CRASH_CHILD";

    if let Ok(child_dir) = std::env::var(CHILD_ENV) {
        // Child leg: open (fresh init), put, die without running Drop.
        let engine = RocksDbEngine::open(&child_dir).expect("child open");
        engine
            .put(Cf::Meta, b"crash_key", b"crash_val")
            .expect("child put");
        std::process::abort();
    }

    // Parent leg: spawn the child, wait for its non-clean death.
    let dir = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().expect("test binary path");
    let output = Command::new(&exe)
        .arg("crash_smoke_wal_restores_after_child_process_abort")
        .arg("--exact")
        .arg("--nocapture")
        .env(CHILD_ENV, dir.path())
        .output()
        .expect("spawn child test process");
    assert!(
        !output.status.success(),
        "child must die in abort(), not exit cleanly — output: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Reopen replays the WAL: the put exists even though no flush and no
    // destructor ever ran for that DB instance.
    let engine = RocksDbEngine::open(dir.path()).expect("reopen after crash");
    assert_eq!(
        engine.get(Cf::Meta, b"crash_key").unwrap(),
        Some(b"crash_val".to_vec()),
        "WAL must restore the unflushed put after an abrupt process death"
    );
}
