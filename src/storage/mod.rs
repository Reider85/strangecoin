//! # Storage (S3-P07, S3-P08) — monolith edge over `strangecoin-storage`
//!
//! Two layers:
//!
//! * **Re-exports** of the storage crate surface (`StorageEngine`, `Cf`,
//!   `RocksDbEngine`, the pure `schema` key codec) so monolith call sites
//!   reach persistence through one module (S3-P07 task 1).
//! * [`Storage`] — the single-backend handle owned by `Blockchain`: the
//!   per-block/per-account schema over [`strangecoin_storage::RocksDbEngine`]
//!   (ADR-0024). The transitional LevelDB variant of S3-P07 is gone
//!   (S3-P08): its one-shot conversion lives in [`migrations`] behind the
//!   default-on `legacy-migrate` feature, and nothing here opens LevelDB
//!   for storage anymore.
//!
//! `rusty-leveldb` is reachable only through [`migrations`] (read-only
//! legacy reader) and the test fixture seeder — see the S3-P08 deviation
//! note in the commit / `Cargo.toml` (the КГ `cargo tree -i rusty-leveldb`
//! expectation of "dev-deps only" is met only for `--no-default-features`
//! builds: production auto-migration needs a runtime reader).
//!
//! Lock ordering (ADR-0024 §Decision "Concurrency"): the RocksDB engine is
//! `Send + Sync` with no interior mutex, so the Stage 1 `Arc<Mutex<DB>>`
//! level is gone from the hierarchy — see the re-documented order in
//! `src/error.rs`.
//!
//! [`migrations`]: migrations

pub mod codec;
#[cfg(feature = "legacy-migrate")]
pub mod migrations;

pub use strangecoin_storage::schema;
pub use strangecoin_storage::{
    // schema::* per S3-P07 task 1 — key constructors + version constants
    block_by_height_key,
    block_hash_index_key,
    contract_storage_key,
    contract_storage_root_key,
    decode_block_height_key,
    decode_mempool_key,
    decode_schema_version,
    decode_state_history_key,
    decode_tx_index_value,
    encode_schema_version,
    encode_tx_index_value,
    mempool_key,
    mempool_sender_prefix,
    meta_key,
    state_history_key,
    state_key,
    tx_index_key,
    tx_raw_key,
    Cf,
    KeyPrefix,
    KvPair,
    RocksDbEngine,
    StorageEngine,
    StorageError,
    WriteBatchOp,
    META_CHAIN_ID,
    META_DIFFICULTY,
    META_SCHEMA_VERSION,
    META_TIP_HASH,
    META_TIP_HEIGHT,
    SCHEMA_VERSION,
};

use std::path::Path;
use std::sync::Arc;

use crate::error::StrangecoinError;

/// Engine handle shared by every `Blockchain` (S3-P08: one backend).
///
/// Open rule (ADR-0024 "Config surface"): `storage.path` (default
/// `./data/rocksdb`) is opened as a `RocksDbEngine`; the historical
/// `<exe_dir>/blockchain_db_{port}` directory is never opened directly —
/// when it exists it is converted first (`migrations::auto_migrate_legacy`,
/// invoked from the DB-open path) and then kept as `.leveldb.bak`.
#[derive(Clone)]
pub struct Storage(Arc<dyn StorageEngine>);

impl Storage {
    /// Open (creating the directory if needed) the RocksDB backend at
    /// `path`. Used by `open_local`, the wire-deserialize path and the
    /// production open path after the auto-migration hook.
    pub fn open_rocks(path: &Path) -> Result<Self, StrangecoinError> {
        std::fs::create_dir_all(path)?;
        let engine = RocksDbEngine::open(path)?;
        Ok(Self(Arc::new(engine)))
    }

    /// Wrap an already-constructed engine (counting/mock engines in tests).
    pub fn from_engine(engine: Arc<dyn StorageEngine>) -> Self {
        Self(engine)
    }

    /// The per-block engine backing this handle.
    pub fn engine(&self) -> &Arc<dyn StorageEngine> {
        &self.0
    }
}
