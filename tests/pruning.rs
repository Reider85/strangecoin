//! S3-P12: state-history pruning — height-window sweep, archival mode,
//! expiry markers (`docs/ADR/0025-state-pruning-strategy.md`).
//!
//! КГ mapping:
//!
//! 1. height-window deletes + archival no-op + deterministic order —
//!    the first three tests (mirror the storage-crate unit tests here
//!    at the integration level, on both mock and real backends);
//! 2. current state / blocks never pruned — negative asserts in every
//!    sweep test;
//! 3. expiry markers written, current state untouched (SCIP boundary);
//! 4. config wiring — `src/config.rs` unit tests
//!    (`pruning_*` / `pruning_config_resolves_archival_from_node_mode`);
//! 5. disk-usage smoke — key-count comparison on two real RocksDB
//!    directories.
//!
//! **Deviation (prompt-mandated)**: the disk smoke asserts *key counts*,
//! not on-disk bytes — RocksDB compaction reclaims space asynchronously,
//! so a byte-level assert would be flaky; `PruneReport::
//! freed_bytes_estimate` records the byte sum as an estimate.

mod common;

use common::*;
use std::collections::BTreeMap;
use std::sync::Mutex;

use strangecoin::storage::{
    block_by_height_key, decode_expiry_marker_value, decode_state_history_key, expiry_marker_key,
    meta_key, should_sweep, state_history_key, state_key, sweep, Cf, KvPair, PruningConfig,
    RocksDbEngine, StorageEngine, StorageError, WriteBatchOp,
};

/// Minimal in-memory engine for sweep-level tests: one `BTreeMap` per
/// CF (sorted — mirrors the trait contract; the storage crate's own
/// mock is `#[cfg(test)]`-private, `per_block_writes.rs` precedent).
struct SweepEngine {
    cfs: Mutex<[BTreeMap<Vec<u8>, Vec<u8>>; 7]>,
}

impl SweepEngine {
    fn new() -> Self {
        Self {
            cfs: Mutex::new(std::array::from_fn(|_| BTreeMap::new())),
        }
    }

    fn seed(&self, cf: Cf, key: Vec<u8>, value: Vec<u8>) {
        self.cfs.lock().expect("lock")[cf.ordinal()].insert(key, value);
    }

    fn keys(&self, cf: Cf) -> Vec<Vec<u8>> {
        self.cfs.lock().expect("lock")[cf.ordinal()]
            .keys()
            .cloned()
            .collect()
    }
}

impl StorageEngine for SweepEngine {
    fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.cfs.lock().expect("lock")[cf.ordinal()]
            .get(key)
            .cloned())
    }

    fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError> {
        let mut cfs = self.cfs.lock().expect("lock");
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

    fn iterate_prefix(&self, cf: Cf, prefix: &[u8]) -> Result<Vec<KvPair>, StorageError> {
        let cfs = self.cfs.lock().expect("lock");
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

fn cfg(retention: u64, sweep_every: u64) -> PruningConfig {
    PruningConfig {
        retention_blocks: retention,
        sweep_every_blocks: sweep_every,
        ..PruningConfig::default()
    }
}

const ADDR_A: &str = "sc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ADDR_B: &str = "sc1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn seed_history(engine: &SweepEngine, address: &str, heights: impl IntoIterator<Item = u64>) {
    for h in heights {
        engine.seed(
            Cf::StateHistory,
            state_history_key(h, address),
            format!("snapshot-{h}").into_bytes(),
        );
    }
}

/// Snapshot heights remaining in `state_history` (bare keys only —
/// the `0xFF` expiry-marker sub-space decodes to a garbage height via
/// the snapshot split and is skipped by tag).
fn history_heights(keys: &[Vec<u8>]) -> Vec<u64> {
    let mut heights: Vec<u64> = keys
        .iter()
        .filter(|k| k.first() != Some(&0xFFu8))
        .filter_map(|k| decode_state_history_key(k).map(|(h, _)| h))
        .collect();
    heights.sort_unstable();
    heights
}

// ---------------------------------------------------------------------------
// 1-2. Window deletes + archival/disabled no-ops (in-memory backend)
// ---------------------------------------------------------------------------

#[test]
fn sweep_deletes_history_below_window_and_spares_state_blocks_meta() {
    let engine = SweepEngine::new();
    seed_history(&engine, ADDR_A, 0..=300);
    seed_history(&engine, ADDR_B, 0..=60);
    engine.seed(Cf::State, state_key(ADDR_A), b"current-a".to_vec());
    engine.seed(Cf::State, state_key(ADDR_B), b"current-b".to_vec());
    engine.seed(Cf::Blocks, block_by_height_key(42), b"block-42".to_vec());
    engine.seed(Cf::Meta, meta_key("tip_height"), vec![0, 0, 1]);

    // tip 300, retention 100 → cutoff 200.
    let report = sweep(&engine, 300, cfg(100, 100)).expect("sweep");
    assert!(!report.skipped);
    assert_eq!(report.pruned_keys, 201 + 61, "A: 0..=200, B: 0..=60");
    assert!(report.freed_bytes_estimate > 0, "estimate must count bytes");

    let heights = history_heights(&engine.keys(Cf::StateHistory));
    assert!(
        heights.iter().all(|&h| h > 200),
        "only in-window snapshots survive, got {heights:?}"
    );
    assert!(heights.contains(&300), "tip-height snapshot survives");

    // Negative (КГ): current state, blocks, meta untouched.
    assert_eq!(
        engine.get(Cf::State, &state_key(ADDR_A)).unwrap(),
        Some(b"current-a".to_vec())
    );
    assert_eq!(
        engine.get(Cf::State, &state_key(ADDR_B)).unwrap(),
        Some(b"current-b".to_vec())
    );
    assert_eq!(
        engine.get(Cf::Blocks, &block_by_height_key(42)).unwrap(),
        Some(b"block-42".to_vec())
    );
    assert_eq!(
        engine.get(Cf::Meta, &meta_key("tip_height")).unwrap(),
        Some(vec![0, 0, 1])
    );
}

#[test]
fn archival_disabled_zero_window_and_unreached_window_are_noops() {
    let cases: [(&str, PruningConfig); 4] = [
        (
            "archival",
            PruningConfig {
                archival: true,
                ..cfg(100, 100)
            },
        ),
        (
            "disabled",
            PruningConfig {
                enabled: false,
                ..cfg(100, 100)
            },
        ),
        ("zero-retention", cfg(0, 100)),
        ("window-unreached", cfg(100, 100)),
    ];
    for (label, c) in cases {
        let engine = SweepEngine::new();
        seed_history(&engine, ADDR_A, 0..50);
        let before = engine.keys(Cf::StateHistory);
        // "window-unreached": tip 40 < retention 100.
        let tip = if label == "window-unreached" {
            40
        } else {
            1000
        };
        let report = sweep(&engine, tip, c).expect("sweep");
        assert!(report.skipped, "{label} must skip");
        assert_eq!(report.pruned_keys, 0, "{label}");
        assert_eq!(engine.keys(Cf::StateHistory), before, "{label}: untouched");

        // The automatic trigger agrees with the bypass rules.
        if label == "archival" {
            assert!(!should_sweep(tip, c), "archival never auto-sweeps");
        }
    }

    // Determinism (КГ): identical inputs → identical reports + state.
    let build = || {
        let engine = SweepEngine::new();
        seed_history(&engine, ADDR_A, 0..=300);
        seed_history(&engine, ADDR_B, 0..=80);
        engine.seed(Cf::State, state_key(ADDR_A), b"current".to_vec());
        engine
    };
    let a = build();
    let b = build();
    let ra = sweep(&a, 300, cfg(100, 100)).expect("sweep a");
    let rb = sweep(&b, 300, cfg(100, 100)).expect("sweep b");
    assert_eq!(ra, rb, "same inputs → same report");
    assert_eq!(a.keys(Cf::StateHistory), b.keys(Cf::StateHistory));
    assert_eq!(a.keys(Cf::State), b.keys(Cf::State));
    let again = sweep(&a, 300, cfg(100, 100)).expect("re-sweep");
    assert_eq!(again.pruned_keys, 0, "idempotent at the same tip");
    assert_eq!(
        again.markers_written, 0,
        "marker value unchanged → no rewrite"
    );
}

// ---------------------------------------------------------------------------
// 3. Expiry markers on the production backend (real RocksDB)
// ---------------------------------------------------------------------------

#[test]
fn expiry_marker_written_for_dormant_address_without_deleting_state() {
    let dir = TestDir::new("pruning_marker");
    let engine = RocksDbEngine::open(dir.path()).expect("open rocks");
    let fresh = "sc1cccccccccccccccccccccccccccccccccc";

    // Dormant address: snapshots only inside the aged-out zone. Fresh
    // address: snapshots on both sides of the cutoff.
    for h in 10..=50u64 {
        engine
            .put(Cf::StateHistory, &state_history_key(h, ADDR_B), b"old")
            .expect("seed");
    }
    for h in (10..=60u64).chain(250..=300u64) {
        engine
            .put(Cf::StateHistory, &state_history_key(h, fresh), b"mixed")
            .expect("seed");
    }
    // SCIP boundary: current-state records for both.
    engine
        .put(Cf::State, &state_key(ADDR_B), b"dormant-current")
        .expect("seed state");
    engine
        .put(Cf::State, &state_key(fresh), b"fresh-current")
        .expect("seed state");

    let report = sweep(&engine, 300, cfg(100, 100)).expect("sweep");
    assert_eq!(
        report.markers_written, 1,
        "only the dormant address is marked"
    );
    assert_eq!(report.markers_removed, 0);

    // Marker present: `0xFF ‖ address` → last-touch height (50).
    let marker = engine
        .get(Cf::StateHistory, &expiry_marker_key(ADDR_B))
        .expect("get")
        .expect("dormant marker must exist");
    assert_eq!(decode_expiry_marker_value(&marker), Some(50));
    assert!(
        engine
            .get(Cf::StateHistory, &expiry_marker_key(fresh))
            .expect("get")
            .is_none(),
        "touched address carries no marker"
    );

    // Current state never deleted (КГ).
    assert_eq!(
        engine.get(Cf::State, &state_key(ADDR_B)).unwrap(),
        Some(b"dormant-current".to_vec())
    );
    assert_eq!(
        engine.get(Cf::State, &state_key(fresh)).unwrap(),
        Some(b"fresh-current".to_vec())
    );
}

// ---------------------------------------------------------------------------
// 5. Disk-usage smoke (keys-vs-bytes deviation, see module docs)
// ---------------------------------------------------------------------------

#[test]
fn disk_usage_smoke_pruned_key_count_below_control() {
    const TIP: u64 = 2000;
    const RETENTION: u64 = 500;

    let seed = |dir: &std::path::Path| -> RocksDbEngine {
        let engine = RocksDbEngine::open(dir).expect("open rocks");
        for h in 0..=TIP {
            for addr in [ADDR_A, ADDR_B] {
                engine
                    .put(Cf::StateHistory, &state_history_key(h, addr), &[0xABu8; 64])
                    .expect("seed history");
            }
        }
        engine
            .put(Cf::State, &state_key(ADDR_A), b"current")
            .expect("seed state");
        engine
            .put(Cf::Blocks, &block_by_height_key(TIP), b"tip-block")
            .expect("seed block");
        engine.flush().expect("flush seed");
        engine
    };

    let dir_pruned = TestDir::new("pruning_smoke_pruned");
    let dir_control = TestDir::new("pruning_smoke_control");
    let pruned = seed(dir_pruned.path());
    let control = seed(dir_control.path());

    let count = |engine: &RocksDbEngine| -> usize {
        engine
            .iterate_prefix(Cf::StateHistory, b"")
            .expect("iterate")
            .len()
    };
    let before = count(&pruned);
    assert_eq!(before, count(&control), "identical seeds");

    let report = sweep(&pruned, TIP, cfg(RETENTION, 100)).expect("sweep");
    assert!(!report.skipped);
    assert!(report.freed_bytes_estimate > 0);

    let after = count(&pruned);
    assert!(
        after < before,
        "pruning must shrink the history key set: {after} < {before} \
         (deviation: key counts, not bytes — compaction is async)"
    );
    // Exactly the aged-out zone went away: (0..=1500) × 2 addresses.
    assert_eq!(before - after, (TIP - RETENTION + 1) as usize * 2);

    // Control untouched; consensus CFs identical on both sides.
    assert_eq!(count(&control), before, "control never swept");
    assert_eq!(
        pruned.iterate_prefix(Cf::State, b"").unwrap().len(),
        control.iterate_prefix(Cf::State, b"").unwrap().len(),
        "state CF untouched"
    );
    assert_eq!(
        pruned.iterate_prefix(Cf::Blocks, b"").unwrap().len(),
        control.iterate_prefix(Cf::Blocks, b"").unwrap().len(),
        "blocks CF untouched"
    );
}

// ---------------------------------------------------------------------------
// 4. Adopt-path trigger e2e (facade, real RocksDB, config wiring)
// ---------------------------------------------------------------------------

#[test]
fn adopt_path_sweeps_state_history_on_height_boundaries() {
    let dir = TestDir::new("pruning_trigger");
    let bc = create_test_blockchain(dir.path());

    // retention 100 / sweep every 50 blocks — sweeps land at tips
    // 50/100/…/250; the last one (tip 250) uses cutoff 150.
    let effective = cfg(100, 50);
    bc.set_pruning(effective);
    assert_eq!(bc.pruning(), effective, "set_pruning/getter round-trip");
    assert!(should_sweep(250, bc.pruning()), "boundary tip must trigger");
    assert!(
        !should_sweep(251, bc.pruning()),
        "off-boundary tip must not"
    );

    let (wallet, wallet_sk) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let (alice, _) = generate_keypair();
    while bc.chain_len() < 251 {
        create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    }
    let chain_len = bc.chain_len();
    assert_eq!(chain_len, 251, "test premise: tip at 250");
    let balance = bc.get_balance(&wallet);
    assert!(balance > 0, "funded wallet survives");
    drop(bc);

    // Raw-engine cross-check (indices.rs precedent): the on-disk keys
    // are exactly what the schema prescribes.
    let engine = RocksDbEngine::open(dir.path()).expect("reopen raw");
    let keys = engine
        .iterate_prefix(Cf::StateHistory, b"")
        .expect("iterate");
    let heights = history_heights(&keys.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>());
    assert!(
        heights.iter().all(|&h| h > 150),
        "history at/below the last cutoff (150) must be gone, \
         min={:?} max={:?}",
        heights.first(),
        heights.last()
    );
    assert!(
        heights.contains(&250),
        "the tip's own snapshots stay inside the window"
    );

    // Negative (КГ): the chain and current state are fully intact.
    let blocks = engine.iterate_prefix(Cf::Blocks, b"").expect("blocks");
    let block_heights: Vec<u64> = blocks
        .iter()
        .filter_map(|(k, _)| strangecoin::storage::decode_block_height_key(k))
        .collect();
    assert_eq!(
        block_heights,
        (0..chain_len as u64).collect::<Vec<u64>>(),
        "every block survives the sweep"
    );
    let state_keys = engine.iterate_prefix(Cf::State, b"").expect("state");
    assert!(
        state_keys
            .iter()
            .any(|(k, _)| k.as_slice() == wallet.as_bytes()),
        "current state entry for the wallet survives"
    );
}

#[test]
fn archival_node_mode_never_prunes_history() {
    let dir = TestDir::new("pruning_archival");
    let bc = create_test_blockchain(dir.path());

    // Same aggressive cadence as the trigger test, but archival mode
    // must bypass every boundary (should_sweep gate + sweep no-op).
    let archival = PruningConfig {
        archival: true,
        ..cfg(100, 10)
    };
    bc.set_pruning(archival);
    assert!(!should_sweep(100, bc.pruning()), "archival: no auto-sweep");

    let (wallet, wallet_sk) = generate_keypair();
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let (alice, _) = generate_keypair();
    while bc.chain_len() < 151 {
        create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    }
    drop(bc);

    let engine = RocksDbEngine::open(dir.path()).expect("reopen raw");
    let keys = engine
        .iterate_prefix(Cf::StateHistory, b"")
        .expect("iterate");
    let heights = history_heights(&keys.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>());
    assert!(
        heights.contains(&1),
        "archival keeps history from the first blocks (min={:?})",
        heights.first()
    );
    assert!(
        heights.iter().all(|&h| h <= 150),
        "no out-of-chain heights appear"
    );
    assert!(heights.len() >= 150, "essentially all heights kept");
}
