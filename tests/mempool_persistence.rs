//! Mempool CF persistence (S3-P07, ADR-0024): prefix-iterate load on
//! open (the Stage 1 full-DB scan is dead) and delete-on-mine (the
//! "stale txs accumulate forever" leak is dead).
//!
//! Also carries the config-wiring acceptance of the prompt:
//! `storage.path` decides where the database opens (S3-P08 removed the
//! `storage.engine` key — RocksDB is the only backend, and a legacy
//! port directory is auto-migrated, see `tests/leveldb_migration.rs`).

mod common;

use common::*;
use std::path::Path;

use strangecoin::config::StorageConfig;
use strangecoin::serialize::txid;
use strangecoin::storage::{RocksDbEngine, StorageEngine};
use strangecoin::{BlockchainFacade, Transaction};

fn open_at(dir: &Path) -> BlockchainFacade {
    create_test_blockchain(dir)
}

/// Raw engine handle over a closed database (RocksDB's LOCK forbids a
/// second open while a facade is alive — drop first).
fn inspect_mempool_keys(dir: &Path) -> usize {
    let engine = RocksDbEngine::open(dir).expect("raw engine must open the closed DB");
    engine
        .iterate_prefix(strangecoin::storage::Cf::Mempool, b"")
        .expect("mempool iterate")
        .len()
}

/// Prefix-load restores the pending tx (against the *real* sender
/// account), the mined tx's key is deleted from the CF, and a later
/// reopen sees exactly that state.
#[test]
fn mempool_prefix_loads_and_deletes_on_mine() {
    let dir = TestDir::new("mempool_persistence");
    let (wallet, wallet_sk) = generate_keypair();
    let (alice, _) = generate_keypair();

    // --- open 1: grant + queue tx1 + explicit save ---
    let bc = open_at(dir.path());
    assert!(bc
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let mut tx1 = Transaction::transfer(
        &wallet,
        &alice,
        100,
        bc.get_nonce(&wallet) + 1,
        bc.chain_id(),
    );
    sign_transaction(&mut tx1, &wallet_sk);
    let tx1_id = txid(&tx1);
    bc.apply_tx(tx1.clone()).expect("tx1 accepted");
    assert_eq!(bc.mempool_len(), 1);
    bc.save_state();
    drop(bc);

    // The CF itself carries the entry (not just the in-memory pool).
    assert_eq!(
        inspect_mempool_keys(dir.path()),
        1,
        "saved mempool must live in the mempool CF"
    );

    // --- open 2: prefix-load restores tx1; mining it deletes the key ---
    let bc = open_at(dir.path());
    assert_eq!(
        bc.mempool_len(),
        1,
        "pending tx must survive the reopen (prefix-iterate load)"
    );
    assert!(
        bc.mempool_contains(&tx1_id),
        "the exact tx must be restored, with its sender account for nonce/balance checks"
    );
    assert_eq!(bc.chain_len(), 2, "chain survives the reopen too");
    mine_current(&bc);
    assert!(bc.mempool_is_empty(), "mined tx left the pool");
    drop(bc);

    assert_eq!(
        inspect_mempool_keys(dir.path()),
        0,
        "mined tx must be DELETED from the mempool CF (no accumulation)"
    );

    // --- open 3: nothing pending, chain intact ---
    let bc = open_at(dir.path());
    assert!(bc.mempool_is_empty(), "reopen after mine stays empty");
    assert_eq!(bc.chain_len(), 3, "grant + tx1 blocks persisted");
    // `>=` not `==`: the coinbase reward lands on an arbitrary account
    // (miner = first key of the state map), possibly alice herself.
    assert!(
        bc.get_balance(&alice) >= 100,
        "the mined transfer applied to alice"
    );
    drop(bc);
}

/// `config.storage.path` wired for real (S3-P07/S3-P08): rocksdb writes
/// at the configured path and never touches the legacy port directory
/// (which only exists as an auto-migration *source*, not a backend —
/// `tests/leveldb_migration.rs` covers that path).
#[test]
fn storage_config_wires_path() {
    let port = random_port();
    let port_dir = strangecoin::test_support::blockchain_db_path_for_port(port);
    let _ = std::fs::remove_dir_all(port_dir.clone()); // leftovers from prior runs
    let bak_dir = {
        let mut bak = port_dir.as_os_str().to_owned();
        bak.push(".leveldb.bak");
        std::path::PathBuf::from(bak)
    };
    let _ = std::fs::remove_dir_all(&bak_dir);

    let dir = TestDir::new("storage_config_wiring");
    let rocks_path = dir.path().join("configured_rocksdb");

    let bc = BlockchainFacade::new(
        port,
        strangecoin::consensus::CHAIN_ID_REGTEST,
        StorageConfig {
            path: rocks_path.clone(),
        },
    );
    assert_eq!(
        bc.chain_len(),
        1,
        "fresh production-path open installs genesis"
    );
    drop(bc);
    assert!(
        rocks_path.join("CURRENT").exists(),
        "the configured storage.path must hold the rocksdb database"
    );
    assert!(
        !port_dir.exists() && !bak_dir.exists(),
        "an open without legacy data must not touch the port directory"
    );

    // Reopen through the same config: the per-block schema loads what the
    // first open saved (production constructor, engine load path).
    let reopened = BlockchainFacade::new(
        port,
        strangecoin::consensus::CHAIN_ID_REGTEST,
        StorageConfig {
            path: rocks_path.clone(),
        },
    );
    assert_eq!(reopened.chain_len(), 1, "reopen loads instead of resetting");
    drop(reopened);
}
