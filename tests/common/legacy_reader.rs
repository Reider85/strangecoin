//! BF-S3-P01 — dev-only LevelDB reader and migration orchestration.
//!
//! The node runtime has no `rusty-leveldb` dependency: this module lives in
//! `tests/common/` and is compiled only into the feature-gated migration
//! tests and the `migrate-leveldb` tooling example (both can use dev-deps).
//! The pure write side (`LegacySnapshot` → per-block RocksDB schema) stays
//! in `strangecoin::storage::migrations` so monolith-edge codec helpers keep
//! their `pub(crate)` visibility.
//!
//! Included via `#[path = "common/legacy_reader.rs"] mod legacy_migration;`
//! (or `../tests/common/...` from `examples/`).
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusty_leveldb::LdbIterator;
use strangecoin::blockchain::state_cache::ContractStorageJson;
pub use strangecoin::storage::migrations::MigrationReport;
use strangecoin::storage::migrations::{migrate_snapshot_to_rocksdb, probe_rocks, LegacySnapshot};
use strangecoin::storage::StorageError;
use strangecoin::AccountState;
use strangecoin_core::types::{Block, Transaction};
use tracing::{info, warn};

const LEGACY_CHAIN: &[u8] = b"chain";
const LEGACY_BALANCES: &[u8] = b"balances";
const LEGACY_DIFFICULTY: &[u8] = b"difficulty";
const LEGACY_CONTRACT_STORAGE: &[u8] = b"contract_storage";

fn corrupt(msg: &'static str) -> StorageError {
    StorageError::Corrupt(msg)
}

fn leveldb_io(err: rusty_leveldb::Status) -> StorageError {
    StorageError::Io(std::io::Error::other(err))
}

/// Same-path guard, mirrored from the write side (BF-S3-P01 split).
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

/// Duplicate of the monolith's `pub(crate)` JSON slot decoder: the reader
/// is outside the crate now, so the shape stays private on both sides.
fn contract_storage_from_json(
    raw: &ContractStorageJson,
) -> Option<strangecoin_core::state::ContractStorageMap> {
    let mut map = strangecoin_core::state::ContractStorageMap::new();
    for (address, slots) in raw {
        let mut storage = strangecoin_core::state::ContractStorage::new();
        for (hex_key, value) in slots {
            let bytes = hex::decode(hex_key).ok()?;
            let key: [u8; 32] = bytes.try_into().ok()?;
            storage.insert(key, value.clone());
        }
        map.insert(address.clone(), storage);
    }
    Some(map)
}

/// What a Stage 0-2 node could leave in `<exe_dir>/blockchain_db_{port}`
/// (S3-P08 fixture shape — exactly the keys the migration reads).
#[derive(Default)]
pub struct LegacySeed {
    pub balances: HashMap<String, AccountState>,
    pub chain: Option<Vec<Block>>,
    pub contract_storage: Option<ContractStorageJson>,
    pub difficulty: Option<u32>,
    pub mempool_txs: Vec<Transaction>,
}

fn open_seed_db(path: &Path) -> rusty_leveldb::DB {
    rusty_leveldb::DB::open(path, rusty_leveldb::Options::default()).expect("seed DB must open")
}

fn finish_seed_db(mut db: rusty_leveldb::DB) {
    db.flush().expect("seed DB flush");
}

/// Seed a legacy LevelDB blob database the way a Stage 0 node left it
/// (`HashMap<String, u64>` balances + optional `chain` JSON blobs).
pub fn seed_legacy_leveldb(path: &Path, balances: &HashMap<String, u64>, chain: Option<&[Block]>) {
    let mut db = open_seed_db(path);
    db.put(
        b"balances",
        &serde_json::to_vec(balances).expect("balances json"),
    )
    .expect("seed balances");
    if let Some(chain) = chain {
        db.put(b"chain", &serde_json::to_vec(chain).expect("chain json"))
            .expect("seed chain");
    }
    finish_seed_db(db);
}

/// Seed every blob/mempool key of the full Stage 1-2 schema.
pub fn seed_legacy_leveldb_full(path: &Path, seed: &LegacySeed) {
    let mut db = open_seed_db(path);
    if !seed.balances.is_empty() {
        db.put(
            b"balances",
            &serde_json::to_vec(&seed.balances).expect("balances json"),
        )
        .expect("seed balances");
    }
    if let Some(chain) = &seed.chain {
        db.put(b"chain", &serde_json::to_vec(chain).expect("chain json"))
            .expect("seed chain");
    }
    if let Some(difficulty) = seed.difficulty {
        db.put(
            b"difficulty",
            &serde_json::to_vec(&difficulty).expect("difficulty json"),
        )
        .expect("seed difficulty");
    }
    if let Some(contract_storage) = &seed.contract_storage {
        db.put(
            b"contract_storage",
            &serde_json::to_vec(contract_storage).expect("contract_storage json"),
        )
        .expect("seed contract storage");
    }
    for tx in &seed.mempool_txs {
        let key = format!("{}:{}", tx.sender(), tx.nonce()).into_bytes();
        db.put(&key, &serde_json::to_vec(tx).expect("tx json"))
            .expect("seed mempool tx");
    }
    finish_seed_db(db);
}

/// Decode the whole legacy database. The handle is dropped before the
/// function returns, closing every file — a later directory rename must
/// not trip over open handles (Windows).
pub fn read_legacy_snapshot(legacy_path: &Path) -> Result<LegacySnapshot, StorageError> {
    if !legacy_path.exists() {
        return Err(StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "legacy LevelDB directory does not exist",
        )));
    }
    // `create_if_missing` defaults to true in rusty-leveldb — never call
    // `DB::open` before the existence check above.
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

    // ---- `balances` (dual format: modern AccountState first, Stage 0
    //      `HashMap<String, u64>` second; both unparsable = fatal) ----
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

/// Explicit one-shot conversion (the CLI/test entry point).
pub fn migrate_leveldb_to_rocksdb(
    legacy_path: &Path,
    new_path: &Path,
) -> Result<MigrationReport, StorageError> {
    if normalized(legacy_path) == normalized(new_path) {
        return Err(corrupt(
            "storage.path must not equal the legacy LevelDB directory",
        ));
    }
    let snapshot = read_legacy_snapshot(legacy_path)?;
    migrate_snapshot_to_rocksdb(&snapshot, legacy_path, new_path)
}

/// Explicit equivalent of the former runtime auto-run (ADR-0008): converts
/// a legacy directory when the target is fresh, renames the legacy
/// directory to `<name>.leveldb.bak`, and finishes an interrupted attempt
/// after a crash window. Called by the tooling/tests, never by the node.
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

    // Crash-window resume: same canonical tip ⇒ finish the `.bak` rename;
    // different chains ⇒ fail closed, neither side modified.
    let target = target.expect("target_holds_data implies a probe result");
    let snapshot = read_legacy_snapshot(legacy_path)?;
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
