//! # Storage (S3-P07) — monolith edge over `strangecoin-storage`
//!
//! Two layers:
//!
//! * **Re-exports** of the storage crate surface (`StorageEngine`, `Cf`,
//!   `RocksDbEngine`, the pure `schema` key codec) so monolith call sites
//!   reach persistence through one module (S3-P07 task 1).
//! * [`Storage`] — the engine selector owned by `Blockchain`: RocksDB
//!   ([`strangecoin_storage::RocksDbEngine`], default, per ADR-0024) or the
//!   **transitional** LevelDB blob backend ([`legacy`], `storage.engine =
//!   "leveldb"`, removed in S3-P08).
//!
//! `rusty-leveldb` is deliberately **not** imported here — it lives in
//! [`legacy`] (S3-P07 deviation note: the task says "delete rusty-leveldb
//! from this file"; the transitional engine itself survives until S3-P08).
//!
//! Lock ordering (ADR-0024 §Decision "Concurrency"): the RocksDB engine is
//! `Send + Sync` with no interior mutex, so the Stage 1 `Arc<Mutex<DB>>`
//! level is gone from the hierarchy for the default path — see the
//! re-documents order in `src/error.rs`. The legacy variant still carries
//! its `Mutex` inside the `blockchain` lock scope, exactly as before.

pub mod codec;
pub mod legacy;

pub use strangecoin_storage::schema;
pub use strangecoin_storage::{
    // schema::* per S3-P07 task 1 — key constructors + version constants
    block_by_height_key,
    block_hash_index_key,
    contract_storage_key,
    decode_block_height_key,
    decode_mempool_key,
    decode_schema_version,
    decode_state_history_key,
    encode_schema_version,
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

use crate::config::{StorageConfig, StorageEngineKind};
use crate::error::StrangecoinError;

/// Engine selector shared by every `Blockchain`.
///
/// Open rules (S3-P07, ADR-0024 "Config surface"):
/// * `engine = "rocksdb"` (default) → `RocksDbEngine` at `config.path`
///   (`storage.path` is wired for real now; default `./data/rocksdb`);
/// * `engine = "leveldb"` (transitional) → the historical
///   `<exe_dir>/blockchain_db_{port}` blob database (`legacy_path`; the
///   never-wired `storage.path` key is ignored for the legacy engine —
///   no legacy data ever lived there).
#[derive(Clone)]
pub enum Storage {
    /// Transitional blob schema (`engine = "leveldb"`, removed in S3-P08).
    Legacy(legacy::LegacyStorage),
    /// Per-block/per-account schema over the `StorageEngine` trait
    /// (ADR-0024) — `RocksDbEngine` in production, test mocks in tests.
    Engine(Arc<dyn StorageEngine>),
}

impl Storage {
    /// Open per `config`. `legacy_path` is the port-derived historical
    /// directory used only by `engine = "leveldb"`.
    pub fn open(config: &StorageConfig, legacy_path: &Path) -> Result<Self, StrangecoinError> {
        match config.engine {
            StorageEngineKind::LevelDb => {
                Ok(Self::Legacy(legacy::LegacyStorage::new(legacy_path)?))
            }
            StorageEngineKind::Rocksdb => Self::open_rocks(&config.path),
        }
    }

    /// Open (creating the directory if needed) the default RocksDB backend
    /// at `path`. Used by `open_local`, the wire-deserialize path and
    /// `Storage::open` for `engine = "rocksdb"`.
    pub fn open_rocks(path: &Path) -> Result<Self, StrangecoinError> {
        std::fs::create_dir_all(path)?;
        let engine = RocksDbEngine::open(path)?;
        Ok(Self::Engine(Arc::new(engine)))
    }

    /// Wrap an already-constructed engine (counting/mock engines in tests).
    pub fn from_engine(engine: Arc<dyn StorageEngine>) -> Self {
        Self::Engine(engine)
    }

    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy(_))
    }

    /// The per-block engine, or `None` while on the transitional LevelDB
    /// backend (call sites branch on [`Self::is_legacy`]).
    pub fn engine(&self) -> Option<&Arc<dyn StorageEngine>> {
        match self {
            Self::Engine(engine) => Some(engine),
            Self::Legacy(_) => None,
        }
    }

    /// The transitional LevelDB handle, or `None` on the default engine.
    pub fn legacy(&self) -> Option<&legacy::LegacyStorage> {
        match self {
            Self::Legacy(inner) => Some(inner),
            Self::Engine(_) => None,
        }
    }
}
