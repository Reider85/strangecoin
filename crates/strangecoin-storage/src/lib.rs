//! Strangecoin storage crate (Stage 3, strangler pattern — S3-P04).
//!
//! This crate owns everything below the monolith's persistence edge:
//!
//! - [`engine`] — the `StorageEngine` trait: an engine-agnostic contract over
//!   seven column families (get/put/delete/atomic `write_batch`/sorted
//!   `iterate_prefix`/flush), plus `StorageError`. Defined per
//!   `docs/ADR/0024-storage-schema-and-engine.md` (which amends the engine
//!   decision of `docs/ADR/0008-rocksdb-plan.md`); the raw DB handle never
//!   leaves this crate.
//! - [`schema`] — pure key-encoding functions and schema versioning
//!   (S3-P05): [`schema::SCHEMA_VERSION`], [`schema::KeyPrefix`] tags for
//!   index sub-keyspaces, per-CF key constructors (big-endian heights /
//!   nonces) and their decoders. Pure byte functions — no I/O, no clock,
//!   no RNG; key *structure* only, values stay opaque bytes.
//! - [`rocks`] (S3-P06, feature `rocksdb`) — [`RocksDbEngine`], the
//!   production backend: open with all seven ADR-0024 column families,
//!   fail-closed schema-version guard, atomic `WriteBatch`, sorted prefix
//!   iteration, all-CF flush.
//!
//! Division of labor (ADR-0024 §Decision "Values"): the engine stores
//! **opaque bytes**. Encoding/decoding of consensus types (`Block`,
//! `AccountState`, `Transaction`) stays at the monolith edge (S3-P07), so
//! this crate never needs to know the canonical wire format. The crate may
//! depend on `strangecoin-core` (pure, 0 I/O) for the snapshot (S3-P13) and
//! witness (S3-P11) modules; the reverse dependency is forbidden.
//!
//! No LevelDB code lives here: the S3-P08 one-shot LevelDB → RocksDB
//! migration reads the legacy blob schema from the monolith edge
//! (`src/storage/migrations.rs`, default-on feature `legacy-migrate`) —
//! this crate only receives the already-decoded snapshot as opaque
//! `WriteBatchOp`s.

pub mod engine;
#[cfg(feature = "rocksdb")]
pub mod rocks;
pub mod schema;

pub use engine::{Cf, KvPair, StorageEngine, StorageError, WriteBatchOp};
#[cfg(feature = "rocksdb")]
pub use rocks::RocksDbEngine;
pub use schema::{
    block_by_height_key, block_hash_index_key, contract_storage_key, decode_block_height_key,
    decode_mempool_key, decode_schema_version, decode_state_history_key, encode_schema_version,
    mempool_key, mempool_sender_prefix, meta_key, state_history_key, state_key, tx_index_key,
    tx_raw_key, KeyPrefix, META_CHAIN_ID, META_DIFFICULTY, META_SCHEMA_VERSION, META_TIP_HASH,
    META_TIP_HEIGHT, SCHEMA_VERSION,
};
