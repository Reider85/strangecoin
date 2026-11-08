//! S3-P08 — one-shot LevelDB → RocksDB migration (ADR-0008 + ADR-0024).
//!
//! Covers the prompt's acceptance checklist:
//! * fixture conversion: blocks/accounts/mempool/tip/difficulty/contract
//!   slots land in the per-block schema (direct API, raw-engine asserts);
//! * auto-run on first open + rename to `.leveldb.bak` (rollback kept)
//!   and idempotency across reopens;
//! * fail-closed on a corrupt legacy blob — no partial apply, no target
//!   directory, legacy untouched;
//! * crash-window resume (conversion done, `.bak` rename not) and the
//!   divergent-chains refusal.
#![cfg(feature = "legacy-migrate")]

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use strangecoin::config::StorageConfig;
use strangecoin::storage::migrations::{
    auto_migrate_legacy, migrate_leveldb_to_rocksdb, MigrationReport,
};
use strangecoin::storage::{
    contract_storage_key, decode_block_height_key, decode_schema_version, meta_key, state_key, Cf,
    RocksDbEngine, StorageEngine, META_DIFFICULTY, META_SCHEMA_VERSION, META_TIP_HASH,
    META_TIP_HEIGHT,
};
use strangecoin::test_support::{
    blockchain_db_path_for_port, generate_keypair, random_port, seed_legacy_leveldb_full,
    sign_transaction, LegacySeed, TestDir,
};
use strangecoin::{AccountState, Block, BlockchainFacade, Transaction};

const CHAIN_ID: u32 = strangecoin::consensus::CHAIN_ID_REGTEST;

/// Minimal block that deserializes and survives the open path (regtest
/// `validate_genesis` is a no-op; non-genesis bodies are not re-checked).
/// The `hash` is the real canonical one (serialization excludes it, the
/// load re-derives it — BUG-S1-007), so the fixture is self-consistent.
fn stub_block(index: u64, sender: &str, receiver: &str) -> Block {
    let mut block = Block {
        index,
        timestamp: index,
        transactions: vec![Transaction::transfer(
            sender,
            receiver,
            100,
            index + 1,
            CHAIN_ID,
        )],
        previous_hash: "0".repeat(64),
        hash: String::new(),
        nonce: 0,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    block.hash = hex::encode(strangecoin_core::serialize::block_hash(&block));
    block
}

/// Two-block chain over `sender`/`receiver` accounts (bech32 fixtures, so
/// the open path's address migration leaves it alone).
fn two_block_chain(sender: &str, receiver: &str) -> Vec<Block> {
    vec![
        stub_block(0, sender, receiver),
        stub_block(1, sender, receiver),
    ]
}

fn default_balances(sender: &str, receiver: &str) -> HashMap<String, AccountState> {
    let mut balances = HashMap::new();
    balances.insert(sender.to_string(), AccountState::eoa(1_000_000, 0));
    balances.insert(receiver.to_string(), AccountState::eoa(500_000, 0));
    balances
}

fn contract_fixture() -> strangecoin::blockchain::state_cache::ContractStorageJson {
    // `hex::encode([7u8; 32])` — the fixed-width slot key shape the
    // legacy blob stored (sorted (hex_key, value) pairs per address).
    vec![(
        "contract1".to_string(),
        vec![(hex::encode([7u8; 32]), b"v1".to_vec())],
    )]
}

/// Guards the port-derived fixture directory *and* its rollback copy.
struct PortDbGuard {
    port_dir: PathBuf,
}

impl PortDbGuard {
    fn new(port: u16) -> Self {
        Self {
            port_dir: blockchain_db_path_for_port(port),
        }
    }
    fn path(&self) -> &Path {
        &self.port_dir
    }
    fn bak_path(&self) -> PathBuf {
        let mut bak = self.port_dir.as_os_str().to_owned();
        bak.push(".leveldb.bak");
        PathBuf::from(bak)
    }
}

impl Drop for PortDbGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.port_dir);
        let _ = std::fs::remove_dir_all(self.bak_path());
    }
}

// ---------------------------------------------------------------------------
// КГ 1: fixture → per-block schema, counts + raw-engine equivalence
// ---------------------------------------------------------------------------

#[test]
fn migrates_chain_state_contract_and_mempool_to_rocksdb() {
    let dir = TestDir::new("s3p08_migrate");
    let legacy = dir.path().join("legacy_db");
    let target = dir.path().join("target");

    let (alice, alice_sk) = generate_keypair();
    let (bob, _) = generate_keypair();
    let mut tx = Transaction::transfer(&alice, &bob, 10, 1, CHAIN_ID);
    sign_transaction(&mut tx, &alice_sk);

    let seed = LegacySeed {
        balances: default_balances(&alice, &bob),
        chain: Some(two_block_chain(&alice, &bob)),
        contract_storage: Some(contract_fixture()),
        difficulty: Some(42),
        mempool_txs: vec![tx.clone()],
    };
    seed_legacy_leveldb_full(&legacy, &seed);

    let report =
        migrate_leveldb_to_rocksdb(&legacy, &target).expect("fixture migration must succeed");
    assert_eq!(
        report,
        MigrationReport {
            blocks: 2,
            accounts: 2,
            mempool_txs: 1,
        },
        "MigrationReport counts must match the fixture"
    );

    // Conversion is read-only w.r.t. the legacy directory (the rollback
    // rename belongs to the auto-run), and the scratch dir is gone.
    assert!(legacy.exists(), "direct migration must not touch legacy");
    assert!(
        !dir.path().join("legacy_db.leveldb.bak").exists(),
        "direct migration must not rename — that is auto-run's job"
    );
    assert!(
        !dir.path().join("target.migrating").exists(),
        "the scratch directory must be renamed away"
    );

    // Reopen raw and compare against the fixture (КГ: blocks/scores/mempool/
    // tip verified).
    let engine = RocksDbEngine::open(&target).expect("migrated database must open");
    let tip_height = engine
        .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))
        .expect("meta read")
        .expect("tip_height must exist");
    assert_eq!(tip_height, 1u64.to_be_bytes().to_vec());
    let tip_hash = engine
        .get(Cf::Meta, &meta_key(META_TIP_HASH))
        .expect("meta read")
        .expect("tip_hash must exist");
    let seeded_chain = seed.chain.as_ref().expect("fixture chain");
    assert_eq!(tip_hash, hex::decode(&seeded_chain[1].hash).unwrap());
    let difficulty = engine
        .get(Cf::Meta, &meta_key(META_DIFFICULTY))
        .expect("meta read")
        .expect("difficulty must exist");
    assert_eq!(difficulty, 42u32.to_be_bytes().to_vec());

    let version = engine
        .get(Cf::Meta, &meta_key(META_SCHEMA_VERSION))
        .expect("meta read")
        .expect("schema version must be initialized by the open guard");
    assert_eq!(decode_schema_version(&version), Some(1));

    let block_entries = engine
        .iterate_prefix(Cf::Blocks, b"")
        .expect("blocks iterate");
    assert_eq!(block_entries.len(), 2);
    let heights: Vec<u64> = block_entries
        .iter()
        .filter_map(|(key, _)| decode_block_height_key(key))
        .collect();
    assert_eq!(heights, vec![0, 1], "BE heights must ascend from 0");
    let decoded: Vec<Block> = block_entries
        .iter()
        .map(|(_, value)| strangecoin_core::serialize::deserialize_block(value).expect("block"))
        .collect();
    assert_eq!(
        decoded.iter().map(|b| &b.hash).collect::<Vec<_>>(),
        seeded_chain.iter().map(|b| &b.hash).collect::<Vec<_>>(),
        "block bodies must round-trip byte-identically"
    );

    let state_entries = engine
        .iterate_prefix(Cf::State, b"")
        .expect("state iterate");
    assert_eq!(state_entries.len(), 2, "both accounts must land in state");
    for (address, account) in &seed.balances {
        let raw = engine
            .get(Cf::State, &state_key(address))
            .expect("state read")
            .unwrap_or_else(|| panic!("account {address} must be migrated"));
        let decoded: AccountState = serde_json::from_slice(&raw).expect("account json");
        assert_eq!(&decoded, account, "account {address} must round-trip");
    }

    let slot = engine
        .get(
            Cf::ContractStorage,
            &contract_storage_key("contract1", &[7u8; 32]),
        )
        .expect("contract read")
        .expect("the seeded slot must be migrated");
    assert_eq!(slot, b"v1".to_vec());

    let mempool_entries = engine
        .iterate_prefix(Cf::Mempool, b"")
        .expect("mempool iterate");
    assert_eq!(mempool_entries.len(), 1, "the pending tx must be migrated");
    drop(engine);

    let _ = std::fs::remove_dir_all(&target);
}

// ---------------------------------------------------------------------------
// КГ 2: auto-run + .bak rename + idempotency across reopens
// ---------------------------------------------------------------------------

#[test]
fn auto_run_converts_renames_to_bak_and_is_idempotent() {
    let port = random_port();
    let db = PortDbGuard::new(port);
    let rocks = std::env::temp_dir().join(format!("strangecoin_s3p08_auto_{port}"));
    let _ = std::fs::remove_dir_all(&rocks);

    let (alice, alice_sk) = generate_keypair();
    let (bob, _) = generate_keypair();
    let mut pending = Transaction::transfer(&alice, &bob, 10, 1, CHAIN_ID);
    sign_transaction(&mut pending, &alice_sk);

    let mut balances = default_balances(&alice, &bob);
    // The mempool restore validates against the *loaded* account: the
    // pending tx spends alice's nonce-1 slot with plenty of balance.
    balances.insert(alice.clone(), AccountState::eoa(1_000_000, 0));
    let seed = LegacySeed {
        balances,
        chain: Some(two_block_chain(&alice, &bob)),
        contract_storage: Some(contract_fixture()),
        difficulty: Some(42),
        mempool_txs: vec![pending.clone()],
    };
    seed_legacy_leveldb_full(db.path(), &seed);

    let config = || StorageConfig {
        path: rocks.clone(),
    };

    // Open #1: auto-run converts the fixture, then the chain loads.
    let facade = BlockchainFacade::new(port, CHAIN_ID, config());
    assert_eq!(
        facade.chain_len(),
        2,
        "the migrated chain must load at open"
    );
    assert_eq!(
        facade.get_balance(&alice),
        1_000_000,
        "migrated balances must load"
    );
    assert_eq!(
        facade.mempool_len(),
        1,
        "the migrated pending tx must restore into the mempool"
    );
    assert!(
        facade.mempool_contains(&strangecoin::serialize::txid(&pending)),
        "the exact pending tx must restore"
    );
    assert!(
        !db.path().exists(),
        "auto-run must rename the legacy directory away"
    );
    assert!(
        db.bak_path().exists(),
        "rollback copy must preserve the legacy data (ADR-0008)"
    );
    assert!(rocks.join("CURRENT").exists(), "rocksdb at storage.path");
    let mut scratch = rocks.as_os_str().to_owned();
    scratch.push(".migrating");
    assert!(
        !PathBuf::from(scratch).exists(),
        "no scratch directory may remain"
    );
    drop(facade);

    // Open #2: idempotent — no legacy directory left, nothing re-migrates,
    // the rollback copy and the data stay put.
    let reopened = BlockchainFacade::new(port, CHAIN_ID, config());
    assert_eq!(reopened.chain_len(), 2, "reopen must not reset anything");
    assert_eq!(reopened.get_balance(&alice), 1_000_000);
    assert_eq!(reopened.mempool_len(), 1, "mempool survives the reopen");
    assert!(!db.path().exists(), "legacy stays renamed");
    assert!(db.bak_path().exists(), "rollback copy stays untouched");
    drop(reopened);

    let _ = std::fs::remove_dir_all(&rocks);
}

// ---------------------------------------------------------------------------
// КГ 3: corrupt legacy → fail-closed, no partial apply
// ---------------------------------------------------------------------------

#[test]
fn corrupt_legacy_fails_closed() {
    let dir = TestDir::new("s3p08_corrupt");
    let legacy = dir.path().join("legacy_db");
    let target = dir.path().join("target");

    // A chain blob that is not JSON — the fail-closed trigger.
    {
        let mut db =
            rusty_leveldb::DB::open(&legacy, rusty_leveldb::Options::default()).expect("seed");
        db.put(b"chain", b"{ definitely not json")
            .expect("seed corrupt chain");
        db.put(
            b"balances",
            &serde_json::to_vec(&HashMap::<String, u64>::new()).unwrap(),
        )
        .expect("seed balances");
        db.flush().expect("flush");
    }

    // Direct API: error before anything is written.
    let err =
        migrate_leveldb_to_rocksdb(&legacy, &target).expect_err("corrupt legacy must not convert");
    assert!(
        err.to_string().contains("corrupt"),
        "fail-closed error expected, got: {err}"
    );
    assert!(!target.exists(), "no target database may appear");
    assert!(
        !dir.path().join("target.migrating").exists(),
        "no scratch directory may remain"
    );
    assert!(legacy.exists(), "legacy must stay untouched");

    // Auto-run level: the node refuses to start, nothing is created.
    let port = random_port();
    let db = PortDbGuard::new(port);
    let rocks = std::env::temp_dir().join(format!("strangecoin_s3p08_corrupt_{port}"));
    let _ = std::fs::remove_dir_all(&rocks);
    {
        let mut handle =
            rusty_leveldb::DB::open(db.path(), rusty_leveldb::Options::default()).expect("seed");
        handle.put(b"chain", b"[[[").expect("seed corrupt chain");
        handle.flush().expect("flush");
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let _ = BlockchainFacade::new(
            port,
            CHAIN_ID,
            StorageConfig {
                path: rocks.clone(),
            },
        );
    }));
    assert!(
        outcome.is_err(),
        "a corrupt legacy database must abort the open"
    );
    assert!(
        !rocks.exists(),
        "fail-closed: the rocksdb directory must never be created"
    );
    assert!(db.path().exists(), "legacy must stay for manual recovery");
    assert!(!db.bak_path().exists(), "no rollback rename on failure");
    let _ = std::fs::remove_dir_all(&rocks);
}

// ---------------------------------------------------------------------------
// КГ (deviation note: crash-window resume) + divergent chains refusal
// ---------------------------------------------------------------------------

#[test]
fn interrupted_migration_resumes_and_divergent_chains_refuse() {
    let dir = TestDir::new("s3p08_resume");
    let (alice, _) = generate_keypair();
    let (bob, _) = generate_keypair();

    // --- resume: conversion landed, the `.bak` rename did not (crash
    //     window). The legacy directory is still in place. ---
    let legacy = dir.path().join("legacy_a");
    let target = dir.path().join("target_a");
    seed_legacy_leveldb_full(
        &legacy,
        &LegacySeed {
            balances: default_balances(&alice, &bob),
            chain: Some(two_block_chain(&alice, &bob)),
            ..Default::default()
        },
    );
    migrate_leveldb_to_rocksdb(&legacy, &target).expect("first conversion");
    assert!(legacy.exists(), "direct migration leaves legacy alone");

    let report = auto_migrate_legacy(&legacy, &target)
        .expect("resume must finish the rollback rename")
        .expect("a migration report is produced");
    assert_eq!(report.blocks, 2, "the report reflects the same chain");
    assert!(!legacy.exists(), "resume completes the rename");
    assert!(
        dir.path().join("legacy_a.leveldb.bak").exists(),
        "rollback copy appears on resume too"
    );
    let engine = RocksDbEngine::open(&target).expect("target reopens");
    let tip = engine
        .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))
        .expect("read")
        .expect("tip survives the resume");
    assert_eq!(tip, 1u64.to_be_bytes().to_vec(), "no rewrite happened");
    drop(engine);

    // --- divergent: the target holds a 2-block chain, a *different*
    //     legacy database shows up next to it → fail closed. ---
    let legacy_b = dir.path().join("legacy_b");
    let target_b = dir.path().join("target_b");
    seed_legacy_leveldb_full(
        &legacy_b,
        &LegacySeed {
            balances: default_balances(&alice, &bob),
            chain: Some(two_block_chain(&alice, &bob)),
            ..Default::default()
        },
    );
    migrate_leveldb_to_rocksdb(&legacy_b, &target_b).expect("first conversion");
    // Same directory, different chain: 3 blocks instead of 2.
    let longer_chain: Vec<Block> = two_block_chain(&alice, &bob)
        .into_iter()
        .chain(std::iter::once(stub_block(2, &alice, &bob)))
        .collect();
    seed_legacy_leveldb_full(
        &legacy_b,
        &LegacySeed {
            chain: Some(longer_chain),
            ..Default::default()
        },
    );
    let err = auto_migrate_legacy(&legacy_b, &target_b)
        .expect_err("divergent chains must refuse to resolve themselves");
    assert!(
        err.to_string().contains("different chains"),
        "expected the divergent-chains refusal, got: {err}"
    );
    assert!(legacy_b.exists(), "legacy stays for manual recovery");
    assert!(
        !dir.path().join("legacy_b.leveldb.bak").exists(),
        "no rollback rename on refusal"
    );
    let engine = RocksDbEngine::open(&target_b).expect("target reopens");
    let tip = engine
        .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))
        .expect("read")
        .expect("tip intact");
    assert_eq!(
        tip,
        1u64.to_be_bytes().to_vec(),
        "the existing chain must not be overwritten"
    );
    drop(engine);
}
