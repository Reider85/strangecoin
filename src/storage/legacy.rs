//! Transitional LevelDB backend — `storage.engine = "leveldb"` (S3-P07).
//!
//! `rusty-leveldb` lives in this file, not in [`super`]: the S3-P07 task
//! "delete rusty-leveldb from `src/storage/mod.rs`" removes the import
//! from the re-export surface while the blob schema stays reachable as
//! the transitional engine until the S3-P08 one-shot migration (ADR-0024
//! §Decision "Config surface": `"leveldb"` transitional, then removed).
//!
//! The legacy blob keys (`chain`/`balances`/`difficulty`/`contract_storage`
//! and the mempool `sender:nonce` entries) and their load/save paths are
//! unchanged — S3-P08's `migrations.rs` is what converts them to the
//! per-block schema.

use rusty_leveldb::{Options, DB};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Stage 1/2 LevelDB handle (`Arc<Mutex<DB>>`), kept verbatim so existing
/// databases open byte-compatible. The DB lock level disappears from the
/// lock ordering for the RocksDB path (ADR-0024 §Decision "Concurrency");
/// it still exists here, inside the same `blockchain` lock scope as before.
#[derive(Clone)]
pub struct LegacyStorage {
    db: Arc<Mutex<DB>>,
}

impl LegacyStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, rusty_leveldb::Status> {
        let db = DB::open(path, Options::default())?;
        Ok(LegacyStorage {
            db: Arc::new(Mutex::new(db)),
        })
    }

    pub fn from_db(db: Arc<Mutex<DB>>) -> Self {
        LegacyStorage { db }
    }

    pub fn db(&self) -> Arc<Mutex<DB>> {
        Arc::clone(&self.db)
    }
}

impl Drop for LegacyStorage {
    fn drop(&mut self) {
        info!("Flushing and closing LevelDB storage");
        if let Ok(mut db) = self.db.lock() {
            if let Err(e) = db.flush() {
                warn!(error = %e, "Error flushing LevelDB during shutdown");
            }
        }
        info!("LevelDB storage closed");
    }
}
