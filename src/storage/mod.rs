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
//!   (S3-P08): its write-side conversion lives in [`migrations`], and
//!   nothing here opens LevelDB for storage anymore.
//!
//! BF-S3-P01: `rusty-leveldb` is a **dev-dependency only** — the read side
//! lives in `tests/common/legacy_reader.rs` and the `migrate-leveldb`
//! example (`--features legacy-migrate`). The node open path fails closed
//! on a legacy directory with the conversion command instead of migrating.
//!
//! Lock ordering (ADR-0024 §Decision "Concurrency"): the RocksDB engine is
//! `Send + Sync` with no interior mutex, so the Stage 1 `Arc<Mutex<DB>>`
//! level is gone from the hierarchy — see the re-documented order in
//! `src/error.rs`.
//!
//! [`migrations`]: migrations

pub mod acp_nonce;
pub mod codec;
#[cfg(feature = "legacy-migrate")]
pub mod migrations;

pub use acp_nonce::MetaNonceStore;
pub use strangecoin_storage::schema;
pub use strangecoin_storage::{
    // schema::* per S3-P07 task 1 — key constructors + version constants
    block_by_height_key,
    block_hash_index_key,
    contract_storage_key,
    contract_storage_root_key,
    decode_block_height_key,
    decode_expiry_marker_key,
    decode_expiry_marker_value,
    decode_mempool_key,
    decode_schema_version,
    decode_state_history_key,
    decode_tx_index_value,
    // snapshot (S3-P13, ADR-0026): file format + commitment verification
    default_filename,
    encode_expiry_marker_value,
    encode_schema_version,
    encode_tx_index_value,
    expiry_marker_key,
    export as export_snapshot,
    import as import_snapshot,
    mempool_key,
    mempool_sender_prefix,
    meta_key,
    // pruning (S3-P12, ADR-0025): module re-export + the items the
    // monolith edge reaches directly (config mapping, save-path trigger)
    pruning,
    should_sweep,
    state_history_key,
    state_key,
    sweep,
    tx_index_key,
    tx_raw_key,
    verify_file as verify_snapshot_file,
    CachedEngine,
    Cf,
    KeyPrefix,
    KvPair,
    PruneReport,
    PruningConfig,
    RocksDbEngine,
    SnapshotError,
    SnapshotHeader,
    StorageEngine,
    StorageError,
    WriteBatchOp,
    META_CHAIN_BASE,
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
/// convert it once with the `migrate-leveldb` tooling example (BF-S3-P01);
/// the converted legacy copy is kept as `.leveldb.bak`.
///
/// S3-P10: when `block_cache_capacity > 0` the engine is wrapped in a
/// [`CachedEngine`] (LRU over `Cf::Blocks`/`Cf::Transactions` point reads;
/// state CF never cached — `StateCache` owns it). Capacity 0 = no wrapper.
#[derive(Clone)]
pub struct Storage(Arc<dyn StorageEngine>);

impl Storage {
    /// Open (creating the directory if needed) the RocksDB backend at
    /// `path`. Used by `open_local`, the wire-deserialize path and the
    /// production open path after the auto-migration hook.
    ///
    /// `block_cache_capacity` — LRU entry budget for block/tx reads
    /// (S3-P10; 0 disables the cache). The pre-cache signature is kept
    /// as [`Self::open_rocks`] with the default capacity for call sites
    /// that have no config on hand (tests, shadow replay).
    pub fn open_rocks(path: &Path) -> Result<Self, StrangecoinError> {
        Self::open_rocks_with_cache(path, DEFAULT_BLOCK_CACHE_CAPACITY)
    }

    /// [`Self::open_rocks`] with an explicit block-cache capacity.
    pub fn open_rocks_with_cache(
        path: &Path,
        block_cache_capacity: usize,
    ) -> Result<Self, StrangecoinError> {
        std::fs::create_dir_all(path)?;
        let engine = RocksDbEngine::open(path)?;
        let boxed: Arc<dyn StorageEngine> = if block_cache_capacity == 0 {
            Arc::new(engine)
        } else {
            Arc::new(CachedEngine::new(engine, block_cache_capacity))
        };
        Ok(Self(boxed))
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

/// Default LRU capacity for [`Storage::open_rocks`] call sites without a
/// config (matches `StorageConfig::block_cache_capacity`'s default —
/// 4096 entries; S3-P10 / prompt task 3).
pub const DEFAULT_BLOCK_CACHE_CAPACITY: usize = 4096;
