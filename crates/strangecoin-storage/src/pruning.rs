//! State pruning — height-window sweep over `state_history`
//! (S3-P12, `docs/ADR/0025-state-pruning-strategy.md`).
//!
//! A full node deletes pre-block account snapshots older than
//! [`PruningConfig::retention_blocks`]; an archival node
//! ([`PruningConfig::archival`]) bypasses the sweep entirely. The only
//! data the sweep may ever touch is the `state_history` column family —
//! current `state`, `blocks`, `transactions`, `contract_storage`,
//! `mempool` and `meta` are consensus-necessary or re-derivable and are
//! **never** pruned (ADR-0025 §"Prune / never-prune exhaustive list").
//!
//! # Mechanics (ADR-0025 §Decision "Mechanics")
//!
//! `cutoff = tip_height − retention_blocks` (saturating via
//! `checked_sub`: while `tip < retention` the window has not been
//! reached and the sweep is a no-op). Every `state_history` key with
//! `height ≤ cutoff` is deleted through one atomic
//! [`StorageEngine::write_batch`]. `enabled = false`,
//! `retention_blocks = 0` and archival mode are no-ops
//! ([`PruneReport::skipped`]).
//!
//! # Trigger
//!
//! The adopt/commit path calls [`should_sweep`] (stateless
//! `tip % sweep_every_blocks == 0`, evaluated in
//! `state_cache::save_state_engine`) and then [`sweep`]. The rule is
//! pure height arithmetic with no persisted "last swept" cursor: a
//! re-run at the same tip is idempotent (deleting an absent key is a
//! no-op), so restarts, reorgs and crash-recovery windows all re-derive
//! the same decision (Stage 3 determinism checklist #1: no
//! `SystemTime::now()`, no `Instant::now()`, no RNG anywhere in this
//! module — V-21 extension).
//!
//! # Expiry markers (touch tracking, SCIP preparation)
//!
//! While classifying keys the sweep learns which addresses still have a
//! snapshot inside the window (`touched`) and which only have aged-out
//! ones (`dormant`). For every dormant address it upserts a
//! **marker record** — [`schema::expiry_marker_key`]
//! (`0xFF ‖ address`, value = last-touch height) in the same
//! `state_history` CF — and removes a stale marker when an address
//! becomes touched again. Markers are *data, not behavior*: current
//! `state` entries are never deleted, `state.apply_block` semantics do
//! not change, and enforcement of expiry stays SCIP-gated (ADR-0025
//! §Decision "State expiry").
//!
//! **Interpretation note (deviation, FBP)** — ADR-0025's prune table
//! lists markers as pruned by the "same window". A literal height
//! age-out is self-contradictory for the last-touch value we store
//! (`value ≤ cutoff` holds by construction at creation, so the marker
//! would die in the same batch that writes it). The table is therefore
//! implemented as *lifecycle governed by the same sweep pass*: markers
//! are created/refreshed for dormant addresses and removed on revival,
//! which keeps them bounded to one record per dormant address (≤ the
//! account count) instead of growing without limit. Recorded in the
//! S3-P12 commit message.
//!
//! # Determinism
//!
//! Same `(engine state, tip_height, cfg)` → same ops in the same
//! order: `iterate_prefix` is sorted by the trait contract, the
//! aggregate maps are `BTreeMap`/`BTreeSet`, and the op sequence is
//! fixed (aged-out deletes → marker puts → marker removals).

use std::collections::{BTreeMap, BTreeSet};

use crate::engine::{Cf, StorageEngine, StorageError, WriteBatchOp};
use crate::schema::{
    decode_expiry_marker_key, decode_expiry_marker_value, decode_state_history_key,
    encode_expiry_marker_value, expiry_marker_key, KeyPrefix,
};

fn default_enabled() -> bool {
    true
}

/// ADR-0025 default retention window: 5000 blocks.
fn default_retention_blocks() -> u64 {
    5000
}

/// ADR-0025 default sweep cadence: every 100 blocks of chain progress.
fn default_sweep_every_blocks() -> u64 {
    100
}

/// Pruning surface of `config.toml` (`[pruning]`) plus the node-mode
/// resolution (ADR-0025 §Decision "Config surface (sketch)").
///
/// `enabled` / `retention_blocks` / `sweep_every_blocks` deserialize
/// straight from the `[pruning]` table (per-field defaults keep a
/// partial table valid). `archival` is `#[serde(skip)]`: it is resolved
/// from the separate `[node] mode` table by
/// `Config::pruning_config()` on the monolith edge and never appears in
/// `config.toml` under `[pruning]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PruningConfig {
    /// Master switch; `false` = sweep never runs (report `skipped`).
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Snapshots older than `tip − retention_blocks` are deleted.
    /// `0` = no-op (ADR-0025 §Decision "Mechanics": nothing is ever
    /// deleted under a zero window — protects a misconfigured node
    /// from wiping its whole history in one sweep).
    #[serde(default = "default_retention_blocks")]
    pub retention_blocks: u64,

    /// Height-based trigger cadence for [`should_sweep`]; `0` disables
    /// the automatic trigger (direct [`sweep`] callers still work).
    #[serde(default = "default_sweep_every_blocks")]
    pub sweep_every_blocks: u64,

    /// `true` when `[node] mode = "archival"`: history is kept in
    /// full, [`sweep`] returns `skipped`. Never (de)serialized — the
    /// operator writes the mode into the `[node]` table, not here.
    #[serde(skip)]
    pub archival: bool,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            retention_blocks: default_retention_blocks(),
            sweep_every_blocks: default_sweep_every_blocks(),
            archival: false,
        }
    }
}

/// Outcome of one [`sweep`] call (logged via `tracing::info!` when
/// anything changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PruneReport {
    /// `state_history` snapshot keys deleted this sweep.
    pub pruned_keys: u64,
    /// Sum of key + value bytes of every deleted record — an
    /// *estimate*: RocksDB reclaims space asynchronously through
    /// compaction, so tests assert on key counts, not on-disk bytes
    /// (S3-P12 deviation, prompt-mandated).
    pub freed_bytes_estimate: u64,
    /// `true` when the sweep did not run at all (archival mode,
    /// `enabled = false`, `retention_blocks = 0`, or the window has
    /// not been reached yet).
    pub skipped: bool,
    /// Marker records written or refreshed for dormant addresses.
    pub markers_written: u64,
    /// Stale marker records removed because the address gained a
    /// snapshot inside the window again (revival).
    pub markers_removed: u64,
}

/// Height-based trigger for the adopt/commit path (ADR-0025: "every
/// `pruning.sweep_every_blocks` blocks of chain progress; never wall
/// clock").
///
/// Stateless and idempotent: `tip % sweep_every_blocks == 0` re-fires
/// on every save that lands on a boundary (restarts included), which is
/// harmless because [`sweep`] deletes only keys that still exist.
/// `sweep_every_blocks = 0` disables the automatic trigger (guard
/// against a division by zero).
pub fn should_sweep(tip_height: u64, cfg: PruningConfig) -> bool {
    cfg.enabled
        && !cfg.archival
        // `> 0` guard first: `is_multiple_of(0)` must never be reached
        // (division by zero), and zero cadence = trigger off.
        && cfg.sweep_every_blocks > 0
        && tip_height.is_multiple_of(cfg.sweep_every_blocks)
}

/// Delete `state_history` snapshots older than the retention window
/// (ADR-0025), maintain expiry markers, report what changed.
///
/// * `tip_height` — the current chain tip (injected, never read from a
///   clock: determinism checklist #1).
/// * Order of operations is deterministic (module docs §Determinism);
///   all mutations land in one atomic batch.
/// * Never touches any CF but [`Cf::StateHistory`].
pub fn sweep(
    engine: &dyn StorageEngine,
    tip_height: u64,
    cfg: PruningConfig,
) -> Result<PruneReport, StorageError> {
    // Archival / disabled / zero-window: explicit no-op (ADR-0025).
    if !cfg.enabled || cfg.archival || cfg.retention_blocks == 0 {
        return Ok(PruneReport {
            skipped: true,
            ..PruneReport::default()
        });
    }
    // Window not reached yet (`tip < retention`): nothing is older
    // than `tip − retention` because that height does not exist.
    let Some(cutoff) = tip_height.checked_sub(cfg.retention_blocks) else {
        return Ok(PruneReport {
            skipped: true,
            ..PruneReport::default()
        });
    };

    // Whole-CF walk: sorted by the trait contract, so the delete list
    // and the aggregate maps derive a deterministic op order. The one
    // walk classifies bare snapshots vs the `0xFF` marker sub-space
    // (schema::KeyPrefix::ExpiryMarker).
    let entries = engine.iterate_prefix(Cf::StateHistory, b"")?;

    let mut aged_keys: Vec<Vec<u8>> = Vec::new();
    let mut freed_bytes_estimate: u64 = 0;
    // Address → newest aged-out height (BTreeMap: sorted op order).
    let mut dormant: BTreeMap<String, u64> = BTreeMap::new();
    // Address → existing marker (key, decoded value) for refresh/removal.
    let mut markers: BTreeMap<String, (Vec<u8>, Option<u64>)> = BTreeMap::new();
    // Addresses with at least one snapshot inside the window.
    let mut touched: BTreeSet<String> = BTreeSet::new();

    for (key, value) in entries {
        if key.first() == Some(&KeyPrefix::ExpiryMarker.byte()) {
            if let Some(address) = decode_expiry_marker_key(&key) {
                markers.insert(
                    address.to_string(),
                    (key, decode_expiry_marker_value(&value)),
                );
            }
            continue;
        }
        let Some((height, address)) =
            decode_state_history_key(&key).map(|(h, a)| (h, a.to_string()))
        else {
            // Structurally broken key: leave it alone (maintenance
            // never repairs or destroys data it cannot classify).
            continue;
        };
        if height <= cutoff {
            freed_bytes_estimate += (key.len() + value.len()) as u64;
            aged_keys.push(key);
            let entry = dormant.entry(address).or_insert(height);
            if height > *entry {
                *entry = height;
            }
        } else {
            touched.insert(address);
        }
    }

    let mut ops: Vec<WriteBatchOp> = Vec::new();

    // 1. Aged-out snapshots (iteration order = sorted).
    for key in &aged_keys {
        ops.push(WriteBatchOp::Delete {
            cf: Cf::StateHistory,
            key: key.clone(),
        });
    }

    // 2. Markers for addresses with no snapshot inside the window.
    //    Skip when the existing marker already carries the same
    //    last-touch height (no write amplification on re-sweeps).
    let mut markers_written = 0u64;
    for (address, last_touch) in &dormant {
        if touched.contains(address) {
            // Fresh snapshots exist — the address is not dormant.
            continue;
        }
        let value = encode_expiry_marker_value(*last_touch).to_vec();
        match markers.get(address) {
            Some((_, Some(existing))) if *existing == *last_touch => {}
            _ => {
                ops.push(WriteBatchOp::Put {
                    cf: Cf::StateHistory,
                    key: expiry_marker_key(address),
                    value,
                });
                markers_written += 1;
            }
        }
    }

    // 3. Revival: an address with in-window snapshots drops its stale
    //    marker (dormant and touched are disjoint for the Put above;
    //    this Delete only ever fires for touched addresses).
    let mut markers_removed = 0u64;
    for address in &touched {
        if let Some((key, _)) = markers.get(address) {
            ops.push(WriteBatchOp::Delete {
                cf: Cf::StateHistory,
                key: key.clone(),
            });
            markers_removed += 1;
        }
    }

    if !ops.is_empty() {
        engine.write_batch(ops)?;
    }

    let report = PruneReport {
        pruned_keys: aged_keys.len() as u64,
        freed_bytes_estimate,
        skipped: false,
        markers_written,
        markers_removed,
    };
    if report.pruned_keys > 0 || report.markers_written > 0 || report.markers_removed > 0 {
        tracing::info!(
            target: "storage::pruning",
            tip_height,
            cutoff,
            pruned_keys = report.pruned_keys,
            freed_bytes_estimate = report.freed_bytes_estimate,
            markers_written = report.markers_written,
            markers_removed = report.markers_removed,
            "state_history sweep complete"
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::KvPair;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Minimal in-memory engine for sweep unit tests: one `BTreeMap`
    /// per CF (sorted — mirrors the trait contract; same shape as the
    /// `engine.rs` test mock, duplicated here because that one lives in
    /// a private `#[cfg(test)]` module — `per_block_writes.rs`
    /// precedent).
    struct SweepTestEngine {
        cfs: Mutex<[BTreeMap<Vec<u8>, Vec<u8>>; Cf::ALL.len()]>,
    }

    impl SweepTestEngine {
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

    impl StorageEngine for SweepTestEngine {
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

    fn cfg(retention: u64) -> PruningConfig {
        PruningConfig {
            retention_blocks: retention,
            ..PruningConfig::default()
        }
    }

    /// Seed `heights` of `address` snapshots plus untouched control
    /// records in `state` / `blocks` / `meta`.
    fn seed_history(
        engine: &SweepTestEngine,
        address: &str,
        heights: impl IntoIterator<Item = u64>,
    ) {
        for h in heights {
            engine.seed(
                Cf::StateHistory,
                crate::schema::state_history_key(h, address),
                format!("snapshot-{h}").into_bytes(),
            );
        }
    }

    #[test]
    fn sweep_deletes_only_snapshots_older_than_the_window() {
        let engine = SweepTestEngine::new();
        let addr = "sc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        seed_history(&engine, addr, 0..=300);
        engine.seed(
            Cf::State,
            crate::schema::state_key(addr),
            b"current".to_vec(),
        );
        engine.seed(
            Cf::Blocks,
            crate::schema::block_by_height_key(7),
            b"block".to_vec(),
        );
        engine.seed(
            Cf::Meta,
            crate::schema::meta_key("tip_height"),
            vec![0, 0, 1],
        );

        // tip 300, retention 100 → cutoff 200: heights 0..=200 go.
        let report = sweep(&engine, 300, cfg(100)).expect("sweep");
        assert!(!report.skipped);
        assert_eq!(report.pruned_keys, 201, "heights 0..=200");
        assert!(report.freed_bytes_estimate > 0);

        let remaining: Vec<u64> = engine
            .keys(Cf::StateHistory)
            .iter()
            .filter_map(|k| decode_state_history_key(k).map(|(h, _)| h))
            .collect();
        assert_eq!(remaining, (201..=300).collect::<Vec<u64>>());

        // Negative (КГ): current state, blocks and meta survive.
        assert_eq!(
            engine
                .get(Cf::State, &crate::schema::state_key(addr))
                .unwrap(),
            Some(b"current".to_vec())
        );
        assert_eq!(
            engine
                .get(Cf::Blocks, &crate::schema::block_by_height_key(7))
                .unwrap(),
            Some(b"block".to_vec())
        );
        assert_eq!(
            engine
                .get(Cf::Meta, &crate::schema::meta_key("tip_height"))
                .unwrap(),
            Some(vec![0, 0, 1])
        );
    }

    #[test]
    fn archival_disabled_and_zero_window_are_noops() {
        for (label, c) in [
            (
                "archival",
                PruningConfig {
                    archival: true,
                    ..cfg(100)
                },
            ),
            (
                "disabled",
                PruningConfig {
                    enabled: false,
                    ..cfg(100)
                },
            ),
            ("zero-retention", cfg(0)),
        ] {
            let engine = SweepTestEngine::new();
            seed_history(&engine, "sc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0..50);
            let before = engine.keys(Cf::StateHistory);
            let report = sweep(&engine, 1000, c).expect("sweep");
            assert!(report.skipped, "{label} must skip");
            assert_eq!(report.pruned_keys, 0, "{label}");
            assert_eq!(engine.keys(Cf::StateHistory), before, "{label}: untouched");
        }
    }

    #[test]
    fn window_not_reached_is_a_noop() {
        let engine = SweepTestEngine::new();
        seed_history(&engine, "sc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0..50);
        let before = engine.keys(Cf::StateHistory);
        // tip 50 < retention 100 → checked_sub fails → skipped.
        let report = sweep(&engine, 50, cfg(100)).expect("sweep");
        assert!(report.skipped);
        assert_eq!(engine.keys(Cf::StateHistory), before);
    }

    #[test]
    fn expiry_markers_track_dormancy_without_deleting_current_state() {
        let dormant = "sc1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let fresh = "sc1cccccccccccccccccccccccccccccccccc";
        let engine = SweepTestEngine::new();
        // Dormant: snapshots only at 10..=50 (all outside a 100-window
        // at tip 300). Fresh: snapshots both aged-out and in-window.
        seed_history(&engine, dormant, 10..=50);
        seed_history(&engine, fresh, 10..=150);
        seed_history(&engine, fresh, 250..=300);
        engine.seed(
            Cf::State,
            crate::schema::state_key(dormant),
            b"dormant-current".to_vec(),
        );
        engine.seed(
            Cf::State,
            crate::schema::state_key(fresh),
            b"fresh-current".to_vec(),
        );

        let report = sweep(&engine, 300, cfg(100)).expect("sweep");
        assert_eq!(
            report.markers_written, 1,
            "only the dormant address gets a marker"
        );
        assert_eq!(report.markers_removed, 0);

        // Marker present with the last-touch height, value round-trips.
        let marker_key = crate::schema::expiry_marker_key(dormant);
        let raw = engine
            .get(Cf::StateHistory, &marker_key)
            .unwrap()
            .expect("dormant marker must exist");
        assert_eq!(
            decode_expiry_marker_value(&raw),
            Some(50),
            "last aged-out height"
        );
        assert!(engine
            .get(Cf::StateHistory, &crate::schema::expiry_marker_key(fresh))
            .unwrap()
            .is_none());

        // SCIP boundary (КГ): current state never deleted.
        assert_eq!(
            engine
                .get(Cf::State, &crate::schema::state_key(dormant))
                .unwrap(),
            Some(b"dormant-current".to_vec())
        );
        assert_eq!(
            engine
                .get(Cf::State, &crate::schema::state_key(fresh))
                .unwrap(),
            Some(b"fresh-current".to_vec())
        );
    }

    #[test]
    fn revival_removes_the_marker_on_the_next_sweep() {
        let addr = "sc1dddddddddddddddddddddddddddddddddd";
        let engine = SweepTestEngine::new();
        seed_history(&engine, addr, 10..=50);
        let first = sweep(&engine, 300, cfg(100)).expect("sweep 1");
        assert_eq!(first.markers_written, 1);

        // Address revives: a fresh snapshot lands inside the window.
        seed_history(&engine, addr, 250..=301);
        let second = sweep(&engine, 301, cfg(100)).expect("sweep 2");
        assert_eq!(second.markers_removed, 1, "stale marker dropped");
        assert!(engine
            .get(Cf::StateHistory, &crate::schema::expiry_marker_key(addr))
            .unwrap()
            .is_none());
        // The in-window snapshots survive.
        assert!(engine
            .get(
                Cf::StateHistory,
                &crate::schema::state_history_key(300, addr)
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn identical_inputs_produce_identical_reports_and_state() {
        let build = || {
            let engine = SweepTestEngine::new();
            seed_history(&engine, "sc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0..=300);
            seed_history(&engine, "sc1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 0..=80);
            engine.seed(
                Cf::State,
                crate::schema::state_key("sc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                b"current".to_vec(),
            );
            engine
        };
        let a = build();
        let b = build();
        let report_a = sweep(&a, 300, cfg(100)).expect("sweep a");
        let report_b = sweep(&b, 300, cfg(100)).expect("sweep b");
        assert_eq!(report_a, report_b, "same inputs → same report");
        assert_eq!(a.keys(Cf::StateHistory), b.keys(Cf::StateHistory));
        assert_eq!(
            a.keys(Cf::State),
            b.keys(Cf::State),
            "state untouched by sweep"
        );
        assert_eq!(a.keys(Cf::State).len(), 1, "control state entry survives");

        // Re-running at the same tip is idempotent.
        let again = sweep(&a, 300, cfg(100)).expect("sweep again");
        assert_eq!(again.pruned_keys, 0, "nothing left to delete");
        assert_eq!(
            again.markers_written, 0,
            "marker value unchanged → no rewrite"
        );
    }

    #[test]
    fn should_sweep_is_height_based_and_gated() {
        let c = PruningConfig::default(); // enabled, not archival, every 100
        assert!(should_sweep(0, c));
        assert!(should_sweep(100, c));
        assert!(!should_sweep(99, c));
        assert!(!should_sweep(150, c));
        assert!(!should_sweep(
            100,
            PruningConfig {
                archival: true,
                ..c
            }
        ));
        assert!(!should_sweep(
            100,
            PruningConfig {
                enabled: false,
                ..c
            }
        ));
        assert!(!should_sweep(
            100,
            PruningConfig {
                sweep_every_blocks: 0,
                ..c
            }
        ));
        // Zero cadence must not divide by zero.
        assert!(!should_sweep(
            u64::MAX,
            PruningConfig {
                sweep_every_blocks: 0,
                ..c
            }
        ));
    }
}
