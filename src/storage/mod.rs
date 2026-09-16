use rusty_leveldb::{DB, Options};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

#[derive(Clone)]
pub struct Storage {
    db: Arc<Mutex<DB>>,
}

impl Storage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, rusty_leveldb::Status> {
        let db = DB::open(path, Options::default())?;
        Ok(Storage {
            db: Arc::new(Mutex::new(db)),
        })
    }

    pub fn from_db(db: Arc<Mutex<DB>>) -> Self {
        Storage { db }
    }

    pub fn db(&self) -> Arc<Mutex<DB>> {
        Arc::clone(&self.db)
    }
}

impl Drop for Storage {
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