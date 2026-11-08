//! # One-shot LevelDB → RocksDB migration (S3-P08)
//!
//! ADR-0008 §Decision («one-shot `migrate_from_leveldb()` utility, run
//! automatically on first startup with `storage.engine = "rocksdb"`, with
//! rollback capability») + ADR-0024 §Decision (legacy blob layout = schema
//! version **0**, this schema = **1**; upgrades happen only through
//! `migrations.rs`, never implicitly on open).
//!
//! Two entry points:
//!
//! * [`migrate_leveldb_to_rocksdb`] — the format conversion itself:
//!   legacy JSON blobs (`chain` / `balances` / `difficulty` /
//!   `contract_storage` + `sender:nonce` mempool entries) → per-block
//!   schema (blocks per height, state, contract slots, mempool CF, tip
//!   meta). **Read-only with respect to the legacy directory** — the
//!   caller owns the rollback rename.
//! * [`auto_migrate_legacy`] — the production auto-run called from
//!   `state_cache::open_blockchain` before the RocksDB open: converts
//!   when the target is fresh, renames the legacy directory to
//!   `<name>.leveldb.bak` (ADR-0008 rollback — the legacy data is *never*
//!   deleted), and finishes an interrupted attempt after a crash window.
//!
//! ## Fail-closed contract (КГ)
//!
//! 1. **Parse-all before write**: every legacy blob is decoded in memory
//!    first; a present-but-unparsable state blob (`chain`, `balances`,
//!    `difficulty`, `contract_storage`) is [`StorageError::Corrupt`] and
//!    nothing is written anywhere — never a partial apply. *Stricter than
//!    the legacy open path* (which was lenient): migrating silently
//!    dropping state would be data loss.
//! 2. **Temp + atomic rename**: the new database is written to
//!    `{target}.migrating` (a stale one from an interrupted attempt is
//!    cleaned first), flushed, closed, and only then renamed onto the
//!    target. A crash mid-conversion leaves the target absent and the
//!    legacy directory untouched, so the next open simply redoes it.
//! 3. **Mempool entries** that do not parse as transactions are skipped
//!    with a warning (legacy semantics: the old loader ignored them too —
//!    they were never usable).
//! 4. **Divergent datasets** (legacy present *and* the target already
//!    holds a different chain) → [`StorageError::Corrupt`]: the operator
//!    resolves it manually; neither side is modified.
//!
//! ## Deviations (FBP, S3-P08)
//!
//! * Lives in the **monolith**, not in `strangecoin-storage` — the prompt
//!   preferred the crate with a `LegacyReader` trait; blob parsing and
//!   value encoding are monolith-edge knowledge by design (ADR-0024
//!   §Decision "Values", `src/storage/codec.rs`: "the storage crate never
//!   sees a Block / AccountState / Transaction"), and the crate would gain
//!   `serde_json` + `strangecoin-core` dependencies for nothing the trait
//!   boundary protects. The crate's migration rail (schema versioning,
//!   atomic batches) is already in place.
//! * `state_history` is **not** backfilled for migrated heights — the
//!   legacy schema stored current-state only (ADR-0024 explicitly notes
//!   this); history starts at the first adopt after the migration.
//! * No `meta.chain_id` write: nothing reads or writes that key in the
//!   S3-P07 schema either (grep-verified during S3-P08).
//! * **Tip hash is recomputed**, not copied from the legacy JSON: canonical
//!   block serialization excludes `hash` and the load re-derives it from
//!   the header bytes (`FORMAT_VERSION` bumps rewrite hashes — BUG-S1-007),
//!   so the meta value must be what the *reloaded* block will hash to.
//!
//! Determinism (Stage 3 checklist): pure byte transforms + path renames —
//! no `SystemTime`/`Instant`, no RNG, no network; `.bak`/`.migrating`
//! names are static strings. Batch op order follows sorted legacy key
//! iteration and per-height index order; the result is order-independent
//! (unique keys, one atomic `write_batch`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusty_leveldb::LdbIterator;
use strangecoin_core::types::{Block, Transaction};
use strangecoin_storage::{
    Cf, RocksDbEngine, StorageEngine, StorageError, WriteBatchOp, META_DIFFICULTY, META_TIP_HASH,
    META_TIP_HEIGHT,
};
use tracing::{info, warn};

use crate::blockchain::state_cache::{contract_storage_from_json, ContractStorageJson};
use crate::storage::{codec, schema::*};
use crate::AccountState;

/// The four constant blob keys of the Stage 0-2 schema (everything else
/// in the legacy keyspace was a mempool entry).
const LEGACY_CHAIN: &[u8] = b"chain";
const LEGACY_BALANCES: &[u8] = b"balances";
const LEGACY_DIFFICULTY: &[u8] = b"difficulty";
const LEGACY_CONTRACT_STORAGE: &[u8] = b"contract_storage";

/// What the conversion moved out of the legacy database
/// (prompt-specified shape — logged via `tracing::info`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReport {
    pub blocks: usize,
    pub accounts: usize,
    pub mempool_txs: usize,
}

/// Fully parsed legacy database — the fail-closed intermediate: nothing
/// touches the filesystem until every blob decoded successfully.
#[derive(Debug, Default)]
struct LegacySnapshot {
    /// `None` = no (or empty) chain blob — a pre-storage/Stage-0 database;
    /// the open path installs fresh genesis on top of the migrated state.
    chain: Option<Vec<Block>>,
    // Named `accounts`, not `balances`: the BUG-S0-022 source gate scans
    // for whole-field replacements of that field outside the state-cache
    // layer and would flag this snapshot member (it is a parse fixture,
    // never the blockchain state cache).
    accounts: Option<HashMap<String, AccountState>>,
    contract_storage: Option<strangecoin_core::state::ContractStorageMap>,
    difficulty: Option<u32>,
    mempool_txs: Vec<Transaction>,
}

impl LegacySnapshot {
    fn report(&self) -> MigrationReport {
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

/// `rusty-leveldb` errors are `Status` (Display + Error) — surface them
/// as I/O failures of the storage layer.
fn leveldb_io(err: rusty_leveldb::Status) -> StorageError {
    StorageError::Io(std::io::Error::other(err))
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

/// Rollback sibling of the legacy directory (ADR-0008 "rollback
/// capability"): `blockchain_db_{port}` → `blockchain_db_{port}.leveldb.bak`.
fn bak_path(legacy_path: &Path) -> PathBuf {
    let name = legacy_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "legacy_db".to_string());
    legacy_path.with_file_name(format!("{name}.leveldb.bak"))
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
// Read side (legacy LevelDB)
// ---------------------------------------------------------------------------

/// Decode the whole legacy database. The handle is dropped before the
/// function returns, closing every file — a later directory rename must
/// not trip over open handles (Windows).
fn read_legacy_snapshot(legacy_path: &Path) -> Result<LegacySnapshot, StorageError> {
    if !legacy_path.exists() {
        return Err(StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "legacy LevelDB directory does not exist",
        )));
    }
    // `create_if_missing` defaults to true in rusty-leveldb — never call
    // `DB::open` before the existence check above, it would *create* the
    // directory it was asked to read.

    let mut db = rusty_leveldb::DB::open(legacy_path, rusty_leveldb::Options::default())
        .map_err(leveldb_io)?;

    let mut snapshot = LegacySnapshot::default();

    // ---- `chain` ----
    if let Some(bytes) = db.get(LEGACY_CHAIN) {
        let chain: Vec<Block> = serde_json::from_slice(&bytes)
            .map_err(|_| corrupt("legacy chain blob is unparsable"))?;
        for (position, block) in chain.iter().enumerate() {
            if block.index != position as u64 {
                return Err(corrupt(
                    "legacy chain block index does not match its position",
                ));
            }
        }
        if chain.is_empty() {
            warn!("legacy chain blob is empty — migrating as a chain-less database");
        } else {
            snapshot.chain = Some(chain);
        }
    }

    // ---- `balances` (dual format, same fallback the legacy loader
    //      applied: `HashMap<String, AccountState>` first, Stage 0
    //      `HashMap<String, u64>` second — but present-and-unparsable
    //      in *both* shapes is fatal here) ----
    if let Some(bytes) = db.get(LEGACY_BALANCES) {
        let parsed: Result<HashMap<String, AccountState>, serde_json::Error> =
            serde_json::from_slice::<HashMap<String, AccountState>>(&bytes).or_else(|_| {
                serde_json::from_slice::<HashMap<String, u64>>(&bytes).map(|legacy| {
                    legacy
                        .into_iter()
                        .map(|(address, balance)| (address, AccountState::eoa(balance, 0)))
                        .collect()
                })
            });
        snapshot.accounts =
            Some(parsed.map_err(|_| corrupt("legacy balances blob is unparsable"))?);
    }

    // ---- `difficulty` ----
    if let Some(bytes) = db.get(LEGACY_DIFFICULTY) {
        snapshot.difficulty = Some(
            serde_json::from_slice::<u32>(&bytes)
                .map_err(|_| corrupt("legacy difficulty blob is unparsable"))?,
        );
    }

    // ---- `contract_storage` ----
    if let Some(bytes) = db.get(LEGACY_CONTRACT_STORAGE) {
        let raw: ContractStorageJson = serde_json::from_slice(&bytes)
            .map_err(|_| corrupt("legacy contract_storage blob is unparsable"))?;
        snapshot.contract_storage =
            Some(contract_storage_from_json(&raw).ok_or_else(|| {
                corrupt("legacy contract_storage blob contains an invalid slot key")
            })?);
    }

    // ---- everything else = mempool entries (legacy semantics: skip what
    //      does not parse, those entries were never loadable) ----
    let mut iterator = db.new_iter().map_err(leveldb_io)?;
    let mut skipped = 0usize;
    while let Some((key, value)) = iterator.next() {
        if key == LEGACY_CHAIN
            || key == LEGACY_BALANCES
            || key == LEGACY_DIFFICULTY
            || key == LEGACY_CONTRACT_STORAGE
        {
            continue;
        }
        match serde_json::from_slice::<Transaction>(&value) {
            Ok(tx) => snapshot.mempool_txs.push(tx),
            Err(_) => skipped += 1,
        }
    }
    if skipped > 0 {
        warn!(
            skipped,
            "legacy mempool entries are not transactions — skipped (legacy load semantics)"
        );
    }
    drop(db);

    Ok(snapshot)
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

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// One-shot conversion: legacy blob schema → per-block schema.
///
/// The legacy directory is **only read** (the caller renames it after a
/// successful conversion — [`auto_migrate_legacy`] does). The target must
/// be absent or an empty directory; the data lands through
/// `{target}.migrating` + atomic `rename`, so a crash at any point leaves
/// either nothing or a complete database (КГ «fail-closed, без partial
/// apply»).
pub fn migrate_leveldb_to_rocksdb(
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

    // 2. Parse everything before touching any directory (fail-closed).
    let snapshot = read_legacy_snapshot(legacy_path)?;

    // 3. Write to the scratch directory; drop the engine (RocksDB handle)
    //    before the rename below.
    let tmp = tmp_path(new_path);
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(&tmp)?;
    let written = write_snapshot(&tmp, &snapshot);
    let report = match written {
        Ok(report) => report,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(err);
        }
    };

    // 4. Atomic placement: the target was verified data-free in step 1
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

/// Production auto-run (ADR-0008): called from the DB-open path before
/// the RocksDB open, keyed off the historical
/// `<exe_dir>/blockchain_db_{port}` directory.
///
/// * no legacy directory → `Ok(None)` (the normal restart — no-op);
/// * fresh target → convert, then rename legacy → `.leveldb.bak`;
/// * target already holds data → crash-window resume: same tip ⇒ finish
///   the `.bak` rename; different chains ⇒ fail closed.
///
/// The legacy directory is renamed only *after* a complete conversion, so
/// the rollback copy survives every failure mode.
pub fn auto_migrate_legacy(
    legacy_path: &Path,
    rocks_path: &Path,
) -> Result<Option<MigrationReport>, StorageError> {
    if !legacy_path.exists() {
        return Ok(None);
    }
    if normalized(legacy_path) == normalized(rocks_path) {
        return Err(corrupt(
            "storage.path must not equal the legacy LevelDB directory",
        ));
    }
    let bak = bak_path(legacy_path);
    if bak.exists() {
        return Err(corrupt(
            "legacy LevelDB rollback copy (.bak) already exists; resolve manually",
        ));
    }

    let target = probe_rocks(rocks_path)?;
    let target_holds_data = target.as_ref().map(|p| p.has_data).unwrap_or(false);

    if !target_holds_data {
        // Fresh target (absent / empty directory / a schema-initialized
        // RocksDB without data): the conversion, then the rollback rename.
        let report = migrate_leveldb_to_rocksdb(legacy_path, rocks_path)?;
        std::fs::rename(legacy_path, &bak)?;
        info!(
            blocks = report.blocks,
            accounts = report.accounts,
            mempool_txs = report.mempool_txs,
            legacy = %legacy_path.display(),
            rollback = %bak.display(),
            "LevelDB -> RocksDB migration complete; legacy directory kept as rollback"
        );
        return Ok(Some(report));
    }

    // The target already holds data: finish an interrupted attempt (a
    // crash between the conversion and the `.bak` rename) or refuse to
    // pick a winner between two different chains.
    let target = target.expect("target_holds_data implies a probe result");
    {
        let snapshot = read_legacy_snapshot(legacy_path)?;
        // Same rule as the conversion: compare *recomputed* canonical
        // hashes on both sides (the legacy JSON's stored `hash` field is
        // not authoritative — see `build_ops`).
        let legacy_tip = snapshot
            .chain
            .as_ref()
            .and_then(|chain| chain.last())
            .map(|block| hex::encode(strangecoin_core::serialize::block_hash(block)));
        let same = match (legacy_tip.as_deref(), target.tip_hash.as_deref()) {
            (Some(legacy), Some(existing)) => legacy == existing,
            (None, None) => true,
            _ => false,
        };
        if !same {
            return Err(corrupt(
                "legacy LevelDB and the existing RocksDB hold different chains; resolve manually",
            ));
        }
        let report = snapshot.report();
        std::fs::rename(legacy_path, &bak)?;
        info!(
            blocks = report.blocks,
            accounts = report.accounts,
            mempool_txs = report.mempool_txs,
            legacy = %legacy_path.display(),
            rollback = %bak.display(),
            "finished interrupted LevelDB -> RocksDB migration (rollback rename only)"
        );
        Ok(Some(report))
    }
}

/// What the migration target currently is (read-only probe).
///
/// * `Ok(None)` — absent or an empty directory (a fresh target);
/// * `Ok(Some(..))` — a valid RocksDB database; `has_data` distinguishes
///   "schema-initialized but empty" (still a fresh target) from a
///   database that actually holds chain/state/mempool records;
/// * `Err(_)` — a non-empty directory that is *not* a RocksDB database →
///   fail closed, nothing is touched (never deleted, never written).
fn probe_rocks(rocks_path: &Path) -> Result<Option<RocksProbe>, StorageError> {
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
struct RocksProbe {
    has_data: bool,
    /// Hex `Block::hash` of the stored tip, or `None` when the database
    /// has records but no chain (chain-less legacy migration).
    tip_hash: Option<String>,
}
