//! BF-S3-P01 — explicit one-shot LevelDB → RocksDB converter.
//!
//! The node no longer auto-migrates on open: operators run this tool once
//! for a pre-v3.0.0 database and point `storage.path` at the result.
//!
//! ```text
//! cargo run --example migrate-leveldb --features legacy-migrate -- \
//!     <legacy_dir> <rocksdb_dir>
//! ```
//!
//! The legacy directory is renamed to `<name>.leveldb.bak` on success
//! (ADR-0008 rollback — never deleted). Failures leave both sides untouched.

#[path = "../tests/common/legacy_reader.rs"]
mod legacy_migration;

use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "usage: migrate-leveldb <legacy_dir> <rocksdb_dir>\n\
         build with: cargo run --example migrate-leveldb --features legacy-migrate -- ..."
    );
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let legacy = PathBuf::from(args.next().unwrap_or_else(|| usage()));
    let rocks = PathBuf::from(args.next().unwrap_or_else(|| usage()));
    if args.next().is_some() {
        usage();
    }

    match legacy_migration::auto_migrate_legacy(&legacy, &rocks) {
        Ok(Some(report)) => println!(
            "migrated: blocks={} accounts={} mempool_txs={}; legacy kept at {}.leveldb.bak",
            report.blocks,
            report.accounts,
            report.mempool_txs,
            legacy.display()
        ),
        Ok(None) => println!(
            "no legacy database at {} — nothing to migrate",
            legacy.display()
        ),
        Err(err) => {
            eprintln!("LevelDB → RocksDB migration failed: {err}");
            std::process::exit(1);
        }
    }
}
