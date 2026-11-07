//! `RocksDbEngine` — the production [`StorageEngine`] backend (S3-P06),
//! built on the pinned `rocksdb` crate (Stage 3 Pin Policy, ADR-0008 +
//! ADR-0024).
//!
//! # Open path (ADR-0024)
//!
//! [`RocksDbEngine::open`] creates the database with all seven ADR-0024
//! column families (`create_missing_column_families`), applies
//! `level_compaction_dynamic_level_bytes` to the DB options **and** to
//! every CF descriptor (the option is per-column-family; the DB-level
//! value only covers the implicit `default` CF), then runs the
//! fail-closed schema-version guard:
//!
//! - `meta.schema_version` absent (fresh database) → initialized to
//!   [`SCHEMA_VERSION`];
//! - present and equal → open succeeds;
//! - present but different → [`StorageError::SchemaVersion`];
//! - present but not 4 bytes → [`StorageError::Corrupt`].
//!
//! There is **no auto-upgrade on open**: layout upgrades happen only in
//! `migrations.rs` (S3-P08), never implicitly (ADR-0024 §Decision
//! "Schema versioning").
//!
//! # Concurrency / lock ordering
//!
//! `rocksdb::DB` is `Send + Sync`, so the engine holds a bare `DB` with
//! no interior mutex — this **removes** the Stage 1 `Arc<Mutex<DB>>`
//! innermost lock level from the monolith hierarchy (ADR-0024 §Decision
//! "Concurrency"; the monolith re-documents its order in S3-P07). CF
//! handles are fetched through the single-threaded `cf_handle` map, which
//! is read-only after `open` (no `create_cf`/`drop_cf` at runtime).
//!
//! # Write / durability contract
//!
//! `write_batch` commits through RocksDB's default `WriteOptions`
//! (`sync = false`): the batch is **atomic** and reaches the WAL before
//! `write` returns, but the WAL is not fsynced — process-crash recovery
//! rides the WAL (see the crash smoke in `tests/rocks_engine.rs`),
//! power-loss durability is the operator-level `flush()` barrier plus
//! the S3-P10 crash suite.
//!
//! `flush` drains **all eight** CFs (default + the seven named ones) to
//! SST files. *Deviation (FBP, confirmed by the reviewer): the prompt's
//! literal `flush(&default_cf)` would flush only the implicit default CF
//! — a CF this engine never writes to — making `flush` a no-op for every
//! byte the trait stores; a trait documented as a "durability barrier"
//! must drain the CFs that hold the data.*
//!
//! # Determinism (Stage 3 checklist, V-21 extension)
//!
//! No `SystemTime` / `Instant` / RNG anywhere in this module: reads,
//! writes, iteration order (bytewise comparator) and the schema guard are
//! pure functions of `(db state, arguments)`. The only I/O is the storage
//! itself.

use crate::engine::{Cf, KvPair, StorageEngine, StorageError, WriteBatchOp};
use crate::schema::{
    decode_schema_version, encode_schema_version, meta_key, META_SCHEMA_VERSION, SCHEMA_VERSION,
};

use rocksdb::{
    ColumnFamilyDescriptor, Direction, FlushOptions, IteratorMode, Options, WriteBatch, DB,
};

/// Number of column families touched by [`flush`](StorageEngine::flush):
/// the implicit `default` CF plus [`Cf::ALL`].
const FLUSHED_CFS: usize = Cf::ALL.len() + 1;

/// Production [`StorageEngine`] backend over RocksDB (ADR-0008 engine
/// decision, ADR-0024 schema). The raw `rocksdb::DB` never leaves this
/// crate (ADR-0024 §Decision "Trait surface").
#[derive(Debug)]
pub struct RocksDbEngine {
    db: DB,
}

impl RocksDbEngine {
    /// Open (creating if needed) the database at `path` with all seven
    /// ADR-0024 column families and run the schema-version guard.
    ///
    /// Errors: [`StorageError::Rocksdb`] for backend/lock/corruption
    /// failures, [`StorageError::SchemaVersion`] on a version mismatch,
    /// [`StorageError::Corrupt`] on an unreadable version record.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, StorageError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.set_level_compaction_dynamic_level_bytes(true);

        let descriptors: Vec<ColumnFamilyDescriptor> = Cf::ALL
            .iter()
            .map(|cf| ColumnFamilyDescriptor::new(cf.name(), cf_options()))
            .collect();

        let db = DB::open_cf_descriptors(&opts, path, descriptors)?;
        let engine = Self { db };
        engine.init_schema_version()?;
        Ok(engine)
    }

    /// Resolve a CF handle. Every CF is created at `open`, so a missing
    /// handle means the engine invariant is broken (fail-closed rather
    /// than panic: the trait's contract is `Result`, not abort).
    fn handle(&self, cf: Cf) -> Result<&rocksdb::ColumnFamily, StorageError> {
        self.db.cf_handle(cf.name()).ok_or(StorageError::Corrupt(
            "column family handle missing after open",
        ))
    }

    /// Fresh database → write `SCHEMA_VERSION`; existing → verify.
    /// No auto-upgrade (ADR-0024): `migrations.rs` (S3-P08) owns that.
    fn init_schema_version(&self) -> Result<(), StorageError> {
        let handle = self.handle(Cf::Meta)?;
        match self.db.get_cf(handle, meta_key(META_SCHEMA_VERSION))? {
            None => {
                self.db.put_cf(
                    handle,
                    meta_key(META_SCHEMA_VERSION),
                    encode_schema_version(SCHEMA_VERSION),
                )?;
                Ok(())
            }
            Some(bytes) => match decode_schema_version(&bytes) {
                None => Err(StorageError::Corrupt(
                    "meta.schema_version: not a 4-byte big-endian u32",
                )),
                Some(found) if found != SCHEMA_VERSION => Err(StorageError::SchemaVersion {
                    found,
                    expected: SCHEMA_VERSION,
                }),
                Some(_) => Ok(()),
            },
        }
    }
}

/// Column-family options: the DB-level `Options` passed to
/// `open_cf_descriptors` only covers the implicit `default` CF, so the
/// ADR-0008 dynamic-level-bytes setting must be applied per descriptor
/// too (otherwise the seven real CFs would keep static level sizing).
fn cf_options() -> Options {
    let mut opts = Options::default();
    opts.set_level_compaction_dynamic_level_bytes(true);
    opts
}

impl StorageEngine for RocksDbEngine {
    fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.db.get_cf(self.handle(cf)?, key)?)
    }

    fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError> {
        // Resolve every handle while building: a handle error aborts
        // before `write`, so the DB sees either the whole batch or none
        // of it (RocksDB WriteBatch is atomic per se — this only keeps
        // the trait's all-or-nothing contract explicit).
        let mut batch = WriteBatch::default();
        for op in ops {
            match op {
                WriteBatchOp::Put { cf, key, value } => {
                    batch.put_cf(self.handle(cf)?, key, value);
                }
                WriteBatchOp::Delete { cf, key } => {
                    batch.delete_cf(self.handle(cf)?, key);
                }
            }
        }
        self.db.write(batch)?;
        Ok(())
    }

    fn iterate_prefix(&self, cf: Cf, prefix: &[u8]) -> Result<Vec<KvPair>, StorageError> {
        // Plain forward seek + manual prefix cut, NOT
        // `prefix_iterator_cf`: without a prefix extractor configured on
        // the CFs, `prefix_same_as_start` does not stop the iterator at
        // the prefix boundary, so the cut has to be explicit. With the
        // default bytewise comparator the first key that fails
        // `starts_with` sorts after every matching key — `break` loses
        // nothing. Empty prefix = whole CF, sorted (trait contract).
        let handle = self.handle(cf)?;
        let iter = self
            .db
            .iterator_cf(handle, IteratorMode::From(prefix, Direction::Forward));

        let mut out = Vec::new();
        for item in iter {
            let (key, value) = item?;
            if !key.starts_with(prefix) {
                break;
            }
            out.push((key.into_vec(), value.into_vec()));
        }
        Ok(out)
    }

    fn flush(&self) -> Result<(), StorageError> {
        let mut handles: Vec<&rocksdb::ColumnFamily> = Vec::with_capacity(FLUSHED_CFS);
        if let Some(default) = self.db.cf_handle("default") {
            handles.push(default);
        }
        for cf in Cf::ALL {
            handles.push(self.handle(cf)?);
        }
        self.db.flush_cfs_opt(&handles, &FlushOptions::default())?;
        Ok(())
    }
}

impl Drop for RocksDbEngine {
    fn drop(&mut self) {
        // Stage 1 `Storage::drop` precedent: best-effort drain on
        // shutdown, never panic inside a destructor.
        if let Err(e) = self.flush() {
            tracing::warn!(error = %e, "RocksDB flush failed during shutdown");
        }
    }
}
