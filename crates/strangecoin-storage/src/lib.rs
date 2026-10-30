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
//!   (`KeyPrefix` constructors, `SCHEMA_VERSION`), implemented in S3-P05.
//! - `RocksDbEngine` (S3-P06, feature `rocksdb`) — the production backend.
//!
//! Division of labor (ADR-0024 §Decision "Values"): the engine stores
//! **opaque bytes**. Encoding/decoding of consensus types (`Block`,
//! `AccountState`, `Transaction`) stays at the monolith edge (S3-P07), so
//! this crate never needs to know the canonical wire format. The crate may
//! depend on `strangecoin-core` (pure, 0 I/O) for the snapshot (S3-P13) and
//! witness (S3-P11) modules; the reverse dependency is forbidden.
//!
//! No LevelDB code lives here: `rusty-leveldb` stays in the monolith until
//! the S3-P08 migration removes it.

pub mod engine;
pub mod schema;

pub use engine::{StorageEngine, StorageError, WriteBatchOp};
