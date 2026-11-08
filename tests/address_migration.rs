//! BUG-S0-017 — legacy base64(pubkey) address migration on DB open.
//!
//! `migrate_addresses_to_bech32` (S1-P15) runs only on the DB-open path
//! (`BlockchainFacade::new(port, chain_id, storage)` → `open_blockchain`).
//! These tests seed a LevelDB fixture the way a Stage 0 node left it —
//! balances keyed by base64(pubkey) — and verify the open path rewrites
//! them to bech32, resets a chain that still contains legacy tx
//! addresses, and is idempotent across reopens.
//!
//! S3-P08: the `engine = "leveldb"` backend is gone — the fixture now
//! rides the one-shot LevelDB → RocksDB auto-migration
//! (`storage::migrations::auto_migrate_legacy`): the port directory is
//! converted into a fresh RocksDB and kept as `.leveldb.bak`, then the
//! *same* open-path runtime migrations run on the converted state. This
//! is also the regression net for "Stage 0 shapes survive the
//! conversion" (the fixture still carries the Stage 0
//! `HashMap<String, u64>` balances blob — the migration's dual-format
//! decode path).
#![cfg(feature = "legacy-migrate")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use strangecoin::config::StorageConfig;
use strangecoin::test_support::{blockchain_db_path_for_port, random_port, seed_legacy_leveldb};
use strangecoin::{address, consensus, Block, BlockchainFacade, Transaction};

/// Magic strings the migration deliberately leaves alone.
const MAGIC_FIELDS: &[&str] = &[
    "genesis",
    "coinbase",
    "initial_wallet_address",
    "regtest_initial_holder",
    "recipient",
];

/// Stage 0 stored addresses as base64(pubkey) without checksum.
fn legacy_address(sk: &SecretKey) -> String {
    let secp = Secp256k1::new();
    let pk = PublicKey::from_secret_key(&secp, sk);
    BASE64.encode(pk.serialize())
}

/// The bech32 address the migration must recompute from the same key.
fn expected_bech32(sk: &SecretKey) -> String {
    let secp = Secp256k1::new();
    let pk = PublicKey::from_secret_key(&secp, sk);
    address::encode_address(&pk, consensus::CHAIN_ID_REGTEST).expect("bech32 encode")
}

/// (legacy base64 address, expected bech32 address)
fn keypair() -> (String, String) {
    let sk = SecretKey::new(&mut rand::rngs::OsRng);
    (legacy_address(&sk), expected_bech32(&sk))
}

fn seed_balances(db_path: &Path, balances: &HashMap<String, u64>) {
    seed_legacy_leveldb(db_path, balances, None);
}

fn seed_balances_and_chain(db_path: &Path, balances: &HashMap<String, u64>, chain: &[Block]) {
    seed_legacy_leveldb(db_path, balances, Some(chain));
}

/// Open through the production constructor with a fresh RocksDB target:
/// the open path auto-migrates the seeded port directory first (S3-P08),
/// then loads and runs the runtime migrations exactly like a real node.
fn open_migrated(port: u16, rocks_path: &Path) -> BlockchainFacade {
    BlockchainFacade::new(
        port,
        strangecoin::consensus::CHAIN_ID_REGTEST,
        StorageConfig {
            path: rocks_path.to_path_buf(),
        },
    )
}

/// Minimal block that deserializes; on regtest `validate_genesis` is a no-op
/// and the open path never re-validates non-genesis bodies. The `hash` is
/// the real canonical one: serialization excludes it and the per-block
/// schema's load re-derives it (BUG-S1-007), so a fixture must be
/// self-consistent for the tip cross-check to pass.
fn stub_block(index: u64, transactions: Vec<Transaction>) -> Block {
    let mut block = Block {
        index,
        timestamp: index,
        transactions,
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

/// Guards the port-derived DB dir *and* its S3-P08 rollback copy:
/// removed on drop like `TestDir`.
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

/// Every account key must be bech32 or a known magic string — never base64.
fn assert_no_legacy_keys(facade: &BlockchainFacade) {
    for key in facade.account_keys() {
        let is_magic = MAGIC_FIELDS.contains(&key.as_str());
        assert!(
            is_magic || address::decode_address(&key).is_ok(),
            "account key {key} must be a bech32 address or magic string"
        );
    }
}

#[test]
fn legacy_base64_balances_migrate_to_bech32_on_open() {
    let port = random_port();
    let db = PortDbGuard::new(port);
    let rocks = std::env::temp_dir().join(format!("strangecoin_s3p08_addr_a_{port}"));
    let _ = std::fs::remove_dir_all(&rocks);

    let (legacy_a, bech32_a) = keypair();
    let (legacy_b, bech32_b) = keypair();

    let mut balances = HashMap::new();
    balances.insert(legacy_a.clone(), 10_000u64);
    balances.insert(legacy_b.clone(), 5_000u64);
    // Non-address magic string: must pass through the migration untouched.
    balances.insert("initial_wallet_address".to_string(), 123u64);
    seed_balances(db.path(), &balances);

    // Open #1: auto-migration converts the fixture, then the DB-open path
    // runs `migrate_addresses_to_bech32` on the converted state.
    let facade = open_migrated(port, &rocks);

    assert_no_legacy_keys(&facade);
    let keys = facade.account_keys();
    assert!(
        !keys.iter().any(|k| k == &legacy_a),
        "legacy key a must be gone"
    );
    assert!(
        !keys.iter().any(|k| k == &legacy_b),
        "legacy key b must be gone"
    );
    assert!(
        keys.iter().any(|k| k == &bech32_a),
        "bech32 key a must exist"
    );
    assert!(
        keys.iter().any(|k| k == &bech32_b),
        "bech32 key b must exist"
    );

    assert_eq!(facade.get_balance(&bech32_a), 10_000, "balance a migrated");
    assert_eq!(facade.get_balance(&bech32_b), 5_000, "balance b migrated");
    assert_eq!(
        facade.get_balance("initial_wallet_address"),
        123,
        "magic-string account untouched"
    );
    assert_eq!(
        facade.total_supply(),
        10_000 + 5_000 + 123,
        "supply preserved"
    );

    // S3-P08 auto-run side effects: the legacy directory became the
    // rollback copy, the conversion landed at storage.path.
    assert!(
        !db.path().exists(),
        "legacy port directory must be renamed away by the auto-migration"
    );
    assert!(
        db.bak_path().exists(),
        "rollback copy (.leveldb.bak) must preserve the legacy data"
    );
    assert!(
        rocks.join("CURRENT").exists(),
        "RocksDB must be created at storage.path"
    );

    // The migration is persisted by `save_state` at the end of the open path.
    drop(facade);

    // Open #2: idempotent — nothing left to migrate, balances stay put.
    let reopened = open_migrated(port, &rocks);
    assert_no_legacy_keys(&reopened);
    assert_eq!(
        reopened.account_keys().len(),
        keys.len(),
        "reopen must not add or drop accounts"
    );
    assert_eq!(reopened.get_balance(&bech32_a), 10_000);
    assert_eq!(reopened.get_balance(&bech32_b), 5_000);
    assert_eq!(reopened.get_balance("initial_wallet_address"), 123);
    assert_eq!(reopened.total_supply(), 10_000 + 5_000 + 123);

    let _ = std::fs::remove_dir_all(&rocks);
}

#[test]
fn legacy_base64_addresses_in_chain_reset_and_migrate() {
    let port = random_port();
    let db = PortDbGuard::new(port);
    let rocks = std::env::temp_dir().join(format!("strangecoin_s3p08_addr_b_{port}"));
    let _ = std::fs::remove_dir_all(&rocks);

    let (legacy_a, bech32_a) = keypair();
    let (legacy_b, bech32_b) = keypair();

    let mut balances = HashMap::new();
    balances.insert(legacy_a.clone(), 10_000u64);
    balances.insert(legacy_b.clone(), 5_000u64);

    // Stage 0 chain: a genesis stub plus a block whose tx fields still carry
    // base64 addresses — the migration must detect it and reset the chain.
    let genesis = stub_block(
        0,
        vec![Transaction::coinbase(
            "genesis",
            "regtest_initial_holder",
            0,
            consensus::CHAIN_ID_REGTEST,
        )],
    );
    let legacy_block = stub_block(
        1,
        vec![Transaction::transfer(
            &legacy_a,
            &legacy_b,
            100,
            1,
            consensus::CHAIN_ID_REGTEST,
        )],
    );
    seed_balances_and_chain(db.path(), &balances, &[genesis, legacy_block]);

    // Open #1: the two-stage path — format conversion, then the legacy
    // address detection on the converted chain forces a chain reset.
    let facade = open_migrated(port, &rocks);

    assert_no_legacy_keys(&facade);
    assert_eq!(facade.get_balance(&bech32_a), 10_000, "balance a migrated");
    assert_eq!(facade.get_balance(&bech32_b), 5_000, "balance b migrated");
    assert_eq!(facade.total_supply(), 15_000, "supply preserved");
    assert!(!db.path().exists(), "legacy directory renamed to .bak");
    assert!(db.bak_path().exists(), "rollback copy preserved");

    let chain = facade.chain_snapshot();
    assert_eq!(
        chain.len(),
        1,
        "legacy chain must be reset to fresh genesis"
    );
    for block in &chain {
        for tx in &block.transactions {
            let fields: [&str; 2] = [tx.sender(), tx.receiver().unwrap_or("")];
            for field in fields {
                let is_magic = MAGIC_FIELDS.contains(&field);
                assert!(
                    is_magic || address::decode_address(field).is_ok(),
                    "no base64 pubkey may remain in chain tx fields, got {field}"
                );
            }
        }
    }
    drop(facade);

    // Open #2: idempotent — no second reset, balances survive.
    let reopened = open_migrated(port, &rocks);
    assert_eq!(
        reopened.chain_len(),
        1,
        "reopen must not reset the chain again"
    );
    assert_eq!(reopened.get_balance(&bech32_a), 10_000);
    assert_eq!(reopened.get_balance(&bech32_b), 5_000);
    assert_eq!(reopened.total_supply(), 15_000);
    assert_no_legacy_keys(&reopened);

    let _ = std::fs::remove_dir_all(&rocks);
}
