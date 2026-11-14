//! S3-P10 integration smoke: the LRU block cache sits on the production
//! `block_by_hash` read path without changing results.
//!
//! КГ: «LRU block cache capacity-bounded, документировано» — unit-level
//! behaviour (hit/miss/eviction/invalidation) lives in
//! `strangecoin-storage/src/cache.rs`; this file proves the monolith
//! wiring (`StorageConfig::block_cache_capacity` →
//! `Storage::open_rocks_with_cache` → `CachedEngine`) serves stable
//! bytes through warm reads, and that a reorg-style delete invalidates
//! instead of serving a stale hit.
//!
//! No feature gate: the monolith always builds `strangecoin-storage`
//! with its default `rocksdb` feature (the root crate has no `rocksdb`
//! feature of its own — cf. `tests/indices.rs`, which also uses
//! `RocksDbEngine` unconditionally).

mod common;

use common::*;
use strangecoin::storage::{Cf, Storage, WriteBatchOp};
use strangecoin_core::serialize::serialize_block;

/// `Block` has no `PartialEq` — compare through the canonical encoding
/// (byte-identical iff structurally identical).
fn block_bytes(b: &strangecoin::Block) -> Vec<u8> {
    serialize_block(b)
}

/// Warm reads through the production constructor (`open_local` →
/// `Storage::open_rocks` → default capacity 4096 `CachedEngine`) must be
/// stable: first call populates the LRU, second is a pure hit, both
/// decode to the same canonical block.
#[test]
fn block_by_hash_warm_reads_are_stable_through_cached_open() {
    let dir = TestDir::new("lru_block_by_hash");
    let bc = create_test_blockchain(dir.path());
    let (wallet, wallet_sk) = generate_keypair();
    bc.grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests");
    let (alice, _) = generate_keypair();
    while bc.chain_len() < 5 {
        create_and_mine_tx(&bc, &wallet, &alice, 1, &wallet_sk);
    }

    let snapshot = bc.chain_snapshot();
    let tip = snapshot.last().expect("chain non-empty");
    let tip_hash = tip.hash.clone();

    // First call: hash→height index get + body get (both cacheable CFs).
    let first = bc
        .block_by_hash(&tip_hash)
        .expect("tip must resolve via hash→height index + body get");
    // Second call: pure LRU hits on the same two keys.
    let second = bc
        .block_by_hash(&tip_hash)
        .expect("second lookup must also resolve");
    assert_eq!(first.hash, second.hash, "warm reads are stable");
    assert_eq!(
        block_bytes(&first),
        block_bytes(&second),
        "cached re-reads decode to the identical block"
    );
    // Cross-check against the in-memory chain (the cache must not
    // diverge from the authoritative snapshot).
    assert_eq!(
        block_bytes(&first),
        block_bytes(tip),
        "cached block_by_hash equals the chain snapshot tip"
    );

    // Reopen through the same path (fresh CachedEngine over the same
    // directory): results still match — cache mode is not load-bearing
    // for correctness across restarts.
    drop(bc);
    let reopened = create_test_blockchain(dir.path());
    let after_restart = reopened
        .block_by_hash(&tip_hash)
        .expect("reopen resolves the same hash");
    assert_eq!(
        block_bytes(&after_restart),
        block_bytes(&first),
        "restart with a cold cache serves the same block"
    );
}

/// Reorg-style delete through the engine must invalidate the cache: a
/// stale hit would keep serving the deleted block body.
#[test]
fn deleting_block_keys_invalidates_cached_reads() {
    let dir = TestDir::new("lru_invalidate");
    let path = dir.path().join("db");
    let storage = Storage::open_rocks_with_cache(&path, 16).expect("cached open at capacity 16");
    let engine = storage.engine().clone();

    // Seed one block-body-shaped record through the engine (opaque bytes
    // — the cache never decodes).
    let height_key = 7u64.to_be_bytes().to_vec();
    engine
        .put(Cf::Blocks, &height_key, b"block-body-v1")
        .expect("seed");
    assert_eq!(
        engine.get(Cf::Blocks, &height_key).unwrap(),
        Some(b"block-body-v1".to_vec()),
        "first read populates the LRU"
    );
    assert_eq!(
        engine.get(Cf::Blocks, &height_key).unwrap(),
        Some(b"block-body-v1".to_vec()),
        "second read is a cache hit with the same bytes"
    );

    // Reorg Phase A equivalent: delete the key via write_batch.
    engine
        .write_batch(vec![WriteBatchOp::Delete {
            cf: Cf::Blocks,
            key: height_key.clone(),
        }])
        .expect("delete");
    assert_eq!(
        engine.get(Cf::Blocks, &height_key).unwrap(),
        None,
        "delete must invalidate — a stale hit would still serve v1"
    );

    // Re-insert (re-adoption): new value must be visible, not the old.
    engine
        .put(Cf::Blocks, &height_key, b"block-body-v2")
        .expect("re-insert");
    assert_eq!(
        engine.get(Cf::Blocks, &height_key).unwrap(),
        Some(b"block-body-v2".to_vec()),
        "re-adopted value wins over any pre-delete cache entry"
    );
}

/// Capacity 0 (config off-switch) is a pure pass-through: reads work,
/// nothing is retained across a decorator boundary.
#[test]
fn zero_capacity_open_serves_reads_without_caching() {
    let dir = TestDir::new("lru_capacity_zero");
    let path = dir.path().join("db");
    let storage = Storage::open_rocks_with_cache(&path, 0).expect("zero-cap open");
    let engine = storage.engine().clone();

    engine.put(Cf::Transactions, b"txid", b"raw").expect("put");
    assert_eq!(
        engine.get(Cf::Transactions, b"txid").unwrap(),
        Some(b"raw".to_vec()),
        "capacity 0 still serves reads"
    );

    // Persist across reopen (durability is the engine's, not the cache's).
    // Drop the Arc'd engine too — a live `RocksDbEngine` holds the OS
    // LOCK, so `storage` alone going out of scope is not enough.
    drop(engine);
    drop(storage);
    let bare = Storage::open_rocks_with_cache(&path, 0).expect("reopen");
    assert_eq!(
        bare.engine().get(Cf::Transactions, b"txid").unwrap(),
        Some(b"raw".to_vec()),
        "value persisted regardless of cache mode"
    );
}
