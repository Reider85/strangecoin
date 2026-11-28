//! # One-shot LevelDB → RocksDB migration — conversion half (S3-P08, BF-S3-P01)
//!
//! ADR-0008 §Decision («one-shot `migrate_from_leveldb()` utility with
//! rollback capability») + ADR-0024 §Decision (legacy blob layout = schema
//! version **0**, this schema = **1**; upgrades happen only through
//! `migrations.rs`, never implicitly on open).
//!
//! BF-S3-P01 split: this module is the **pure write side** — a parsed
//! [`LegacySnapshot`] lands in the per-block schema through one atomic
//! batch. The LevelDB **reader** is no longer a runtime dependency: it lives
//! in `tests/common/legacy_migration.rs` (dev-only `rusty-leveldb`) and in
//! the `migrate-leveldb` tooling example:
//! `cargo run --example migrate-leveldb --features legacy-migrate`.
//! The node open path never opens LevelDB — a legacy directory is refused
//! with the conversion command instead of being auto-migrated.
//!
//! Entry points:
//!
//! * [`migrate_snapshot_to_rocksdb`] — parse-first, temp + atomic-rename
//!   conversion of an already-parsed legacy snapshot. Read-only with
//!   respect to the legacy directory; the caller owns the rollback rename.
//! * [`probe_rocks`] / [`RocksProbe`] — read-only target classification
//!   used by the tooling's crash-window resume (same-tip check).
//!
//! ## Fail-closed contract (КГ)
//!
//! 1. **Parse-all before write**: the tooling decodes every legacy blob in
//!    memory first; a present-but-unparsable blob is
//!    [`StorageError::Corrupt`] and nothing is written anywhere. This check
//!    is enforced by the reader before [`migrate_snapshot_to_rocksdb`] is
//!    called — never a partial apply (stricter than the legacy open path).
//! 2. **Temp + atomic rename**: the new database is written to
//!    `{target}.migrating` (a stale one from an interrupted attempt is
//!    cleaned first), flushed, closed, and only then renamed onto the
//!    target. A crash mid-conversion leaves the target absent and the
//!    legacy directory untouched.
//! 3. **Mempool entries** that do not parse as transactions are skipped
//!    by the reader with a warning (legacy semantics: the old loader
//!    ignored them too — they were never usable).
//! 4. **Divergent datasets** (legacy present *and* the target already
//!    holds a different chain) → [`StorageError::Corrupt`]: the tooling
//!    compares recomputed canonical tip hashes via [`probe_rocks`] and
//!    neither side is modified.
//!
//! ## Deviations (FBP, S3-P08 + BF-S3-P01)
//!
//! * Lives in the **monolith**, not in `strangecoin-storage` — blob parsing
//!   and value encoding are monolith-edge knowledge by design (ADR-0024
//!   §Decision "Values", `src/storage/codec.rs`: "the storage crate never
//!   sees a Block / AccountState / Transaction").
//! * BF-S3-P01: the read side moved to dev-only tooling; ADR-0008's
//!   "run automatically on first startup" became an explicit operator
//!   step (recorded in Changelog 3.0.1 and STAGE3_SUMMARY §6).
//! * `state_history` is **not** backfilled for migrated heights — the
//!   legacy schema stored current-state only (ADR-0024 explicitly notes
//!   this); history starts at the first adopt after the migration.
//! * No `meta.chain_id` write: nothing reads or writes that key in the
//!   S3-P07 schema either (grep-verified during S3-P08).
//! * **Tip hash is recomputed**, not copied from the legacy JSON: canonical
//!   block serialization excludes `hash` and the load re-derives it from
//!   the header bytes (BUG-S1-007 — hashes are recomputed across
//!   `FORMAT_VERSION` bumps), so a stored value drifted by a version
//!   bump would brick the fail-closed load of an otherwise sound
//!   chain. (Documented deviation: the legacy blob kept whatever the
//!   writer believed.)
//!
//! Determinism (Stage 3 checklist): pure byte transforms + path renames —
//! no `SystemTime`/`Instant`, no RNG, no network; `.migrating` names are
//! static strings. Batch op order follows the snapshot's insertion order
//! (unique keys, one atomic `write_batch`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use strangecoin_core::types::{Block, Transaction};
use strangecoin_storage::{
    Cf, RocksDbEngine, StorageEngine, StorageError, WriteBatchOp, META_DIFFICULTY, META_TIP_HASH,
    META_TIP_HEIGHT,
};

use crate::storage::{codec, schema::*};
use crate::AccountState;

/// What the conversion moved out of the legacy database
/// (prompt-specified shape — logged via `tracing::info` in the tooling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReport {
    pub blocks: usize,
    pub accounts: usize,
    pub mempool_txs: usize,
}

/// Fully parsed legacy database — the fail-closed intermediate: the reader
/// fills this in memory before the write side touches any directory.
/// Public fields let the dev-only reader build it without exposing the
/// codec helpers (`pub(crate)` in `src/storage/codec.rs`).
#[derive(Debug, Default)]
pub struct LegacySnapshot {
    /// `None` = no (or empty) chain blob — a pre-storage/Stage-0 database;
    /// the open path installs fresh genesis on top of the migrated state.
    pub chain: Option<Vec<Block>>,
    pub accounts: Option<HashMap<String, AccountState>>,
    pub contract_storage: Option<strangecoin_core::state::ContractStorageMap>,
    pub difficulty: Option<u32>,
    pub mempool_txs: Vec<Transaction>,
}

impl LegacySnapshot {
    pub fn report(&self) -> MigrationReport {
        MigrationReport {
            blocks: self.chain.as_ref().map(Vec::len).unwrap_or(0),
            accounts: self.accounts.as_ref().map(HashMap::len).unwrap_or(0),
            mempool_txs: self.mempool_txs.len(),
        }
    }
}

fn corrupt(msg: &'static str) -> StorageError {
    StorageError::Corrupt(msg)
}

/// Lexical normalization for the same-path guard: canonicalize when the
/// path exists, otherwise canonicalize the parent (the migration target
/// may not exist yet) and re-append the file name.
fn normalized(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map(|canonical| canonical.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

/// Scratch sibling of the migration target (same parent ⇒ same volume ⇒
/// `rename` is atomic).
fn tmp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rocksdb".to_string());
    target.with_file_name(format!("{name}.migrating"))
}

// ---------------------------------------------------------------------------
// Write side (per-block schema)
// ---------------------------------------------------------------------------

/// One atomic batch over the target snapshot (ADR-0024 §Decision "Write
/// path" shape: everything the legacy database held, exactly once).
fn build_ops(snapshot: &LegacySnapshot) -> Result<Vec<WriteBatchOp>, StorageError> {
    let mut ops: Vec<WriteBatchOp> = Vec::new();

    if let Some(chain) = &snapshot.chain {
        for (height, block) in chain.iter().enumerate() {
            ops.push(WriteBatchOp::Put {
                cf: Cf::Blocks,
                key: block_by_height_key(height as u64),
                value: codec::encode_block(block),
            });
        }
        let tip = chain.len() as u64 - 1;
        ops.push(WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_TIP_HEIGHT),
            value: codec::encode_u64(tip),
        });
        // Tip meta carries the **recomputed** canonical hash, not the
        // legacy JSON's stored `hash` field: canonical serialization
        // excludes `hash` and `deserialize_block` re-derives it from the
        // header bytes (BUG-S1-007 — hashes are recomputed across
        // `FORMAT_VERSION` bumps), so a stored value drifted by a version
        // bump would brick the fail-closed load of an otherwise sound
        // chain. (Documented deviation: the legacy blob kept whatever the
        // writer believed.)
        let tip_hash = strangecoin_core::serialize::block_hash(&chain[tip as usize]);
        ops.push(WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_TIP_HASH),
            value: tip_hash.to_vec(),
        });
    }

    if let Some(difficulty) = snapshot.difficulty {
        ops.push(WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_DIFFICULTY),
            value: codec::encode_u32(difficulty),
        });
    }

    if let Some(balances) = &snapshot.accounts {
        for (address, account) in balances {
            ops.push(WriteBatchOp::Put {
                cf: Cf::State,
                key: state_key(address),
                value: codec::encode_account(account),
            });
        }
    }

    if let Some(contract_storage) = &snapshot.contract_storage {
        for (address, slots) in contract_storage.iter() {
            for (slot, value) in slots.iter() {
                ops.push(WriteBatchOp::Put {
                    cf: Cf::ContractStorage,
                    key: contract_storage_key(address, slot),
                    value: value.clone(),
                });
            }
        }
    }

    for tx in &snapshot.mempool_txs {
        ops.push(WriteBatchOp::Put {
            cf: Cf::Mempool,
            key: mempool_key(tx.sender(), tx.nonce()),
            value: codec::encode_tx(tx),
        });
    }

    Ok(ops)
}

/// Open a fresh RocksDB at `dir` (schema version 1 is initialized by the
/// engine guard) and land the snapshot in one atomic batch + flush.
fn write_snapshot(dir: &Path, snapshot: &LegacySnapshot) -> Result<MigrationReport, StorageError> {
    let engine = RocksDbEngine::open(dir)?;
    let ops = build_ops(snapshot)?;
    engine.write_batch(ops)?;
    engine.flush()?;
    drop(engine);
    Ok(snapshot.report())
}

/// One-shot conversion: parsed legacy blob schema → per-block schema.
///
/// Called by the dev-only tooling reader after parse-all succeeded. The
/// legacy directory is **only checked for existence** (the tooling renames
/// it to `.leveldb.bak` after a successful conversion). The target must be
/// absent or an empty directory; the data lands through
/// `{target}.migrating` + atomic `rename`, so a crash at any point leaves
/// either nothing or a complete database (КГ «fail-closed, без partial
/// apply»).
pub fn migrate_snapshot_to_rocksdb(
    snapshot: &LegacySnapshot,
    legacy_path: &Path,
    new_path: &Path,
) -> Result<MigrationReport, StorageError> {
    if normalized(legacy_path) == normalized(new_path) {
        return Err(corrupt(
            "storage.path must not equal the legacy LevelDB directory",
        ));
    }
    if !legacy_path.exists() {
        return Err(StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "legacy LevelDB directory does not exist",
        )));
    }
    // 1. The target must not hold data (fail-closed before any work;
    //    a non-RocksDB directory errors out here too).
    if let Some(probe) = probe_rocks(new_path)? {
        if probe.has_data {
            return Err(corrupt("migration target already exists with data"));
        }
        // Schema-initialized RocksDB without data — replaced below.
    }

    // 2. Write to the scratch directory; drop the engine (RocksDB handle)
    //    before the rename below.
    let tmp = tmp_path(new_path);
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(&tmp)?;
    let written = write_snapshot(&tmp, snapshot);
    let report = match written {
        Ok(report) => report,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(err);
        }
    };

    // 3. Atomic placement: the target was verified data-free in step 1
    //    (empty directory or a schema-initialized RocksDB without data).
    if new_path.exists() {
        if let Err(err) = std::fs::remove_dir_all(new_path) {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(StorageError::Io(err));
        }
    }
    if let Some(parent) = new_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if let Err(err) = std::fs::rename(&tmp, new_path) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(StorageError::Io(err));
    }

    Ok(report)
}

/// What the migration target currently is (read-only probe).
///
/// * `Ok(None)` — absent or an empty directory (a fresh target);
/// * `Ok(Some(..))` — a valid RocksDB database; `has_data` distinguishes
///   "schema-initialized but empty" (still a fresh target) from a
///   database that actually holds chain/state/mempool records;
/// * `Err(_)` — a non-empty directory that is *not* a RocksDB database →
///   fail closed, nothing is touched (never deleted, never written).
pub fn probe_rocks(rocks_path: &Path) -> Result<Option<RocksProbe>, StorageError> {
    if !rocks_path.exists() {
        return Ok(None);
    }
    if rocks_path.read_dir()?.next().is_none() {
        return Ok(None);
    }
    // `CURRENT` is written by every RocksDB open; its absence means this
    // directory is something else entirely — probe must not "adopt" it.
    if !rocks_path.join("CURRENT").exists() {
        return Err(corrupt(
            "migration target exists but is not a rocksdb database",
        ));
    }
    let engine = RocksDbEngine::open(rocks_path)?;
    let tip_height = engine.get(Cf::Meta, &meta_key(META_TIP_HEIGHT))?;
    let tip_hash = engine.get(Cf::Meta, &meta_key(META_TIP_HASH))?;
    let has_data = tip_height.is_some()
        || !engine.iterate_prefix(Cf::Blocks, b"")?.is_empty()
        || !engine.iterate_prefix(Cf::State, b"")?.is_empty()
        || !engine.iterate_prefix(Cf::Mempool, b"")?.is_empty();
    drop(engine);
    Ok(Some(RocksProbe {
        has_data,
        tip_hash: tip_hash
            .map(|bytes| codec::decode_hash(&bytes))
            .transpose()?,
    }))
}

/// Probe result of [`probe_rocks`].
#[derive(Debug)]
pub struct RocksProbe {
    pub has_data: bool,
    /// Hex `Block::hash` of the stored tip, or `None` when the database
    /// has records but no chain (chain-less legacy migration).
    pub tip_hash: Option<String>,
}
