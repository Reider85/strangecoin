//! BUG-S0-017 — legacy base64(pubkey) address migration on DB open.
//!
//! `migrate_addresses_to_bech32` (S1-P15) runs only on the DB-open path
//! (`BlockchainFacade::new(port, strangecoin::consensus::CHAIN_ID_REGTEST)` → `open_blockchain`). These tests seed a
//! LevelDB fixture the way a Stage 0 node left it — balances keyed by
//! base64(pubkey) — and verify the open path rewrites them to bech32,
//! resets a chain that still contains legacy tx addresses, and is
//! idempotent across reopens.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use strangecoin::test_support::{blockchain_db_path_for_port, random_port};
use strangecoin::{address, consensus, storage::Storage, Block, BlockchainFacade, Transaction};

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
    let storage = Storage::new(db_path).expect("seed DB must open");
    {
        let db_arc = storage.db();
        let mut db = db_arc.lock().expect("seed DB lock");
        db.put(b"balances", &serde_json::to_vec(balances).expect("balances json"))
            .expect("seed balances");
    }
    // Drop closes the LevelDB handle so the open path can take the lock.
    drop(storage);
}

fn seed_balances_and_chain(
    db_path: &Path,
    balances: &HashMap<String, u64>,
    chain: &[Block],
) {
    let storage = Storage::new(db_path).expect("seed DB must open");
    {
        let db_arc = storage.db();
        let mut db = db_arc.lock().expect("seed DB lock");
        db.put(b"balances", &serde_json::to_vec(balances).expect("balances json"))
            .expect("seed balances");
        db.put(b"chain", &serde_json::to_vec(chain).expect("chain json"))
            .expect("seed chain");
    }
    drop(storage);
}

/// Minimal block that deserializes; on regtest `validate_genesis` is a no-op
/// and the open path never re-validates non-genesis bodies.
fn stub_block(index: u64, transactions: Vec<Transaction>) -> Block {
    Block {
        index,
        timestamp: index,
        transactions,
        previous_hash: "0".repeat(64),
        hash: format!("{:064x}", index),
        nonce: 0,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

/// Guards the port-derived DB dir: removed on drop like `TestDir`.
struct PortDbGuard(PathBuf);

impl PortDbGuard {
    fn new(port: u16) -> Self {
        Self(blockchain_db_path_for_port(port))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PortDbGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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

    let (legacy_a, bech32_a) = keypair();
    let (legacy_b, bech32_b) = keypair();

    let mut balances = HashMap::new();
    balances.insert(legacy_a.clone(), 10_000u64);
    balances.insert(legacy_b.clone(), 5_000u64);
    // Non-address magic string: must pass through the migration untouched.
    balances.insert("initial_wallet_address".to_string(), 123u64);
    seed_balances(db.path(), &balances);

    // Open #1: DB-open path runs `migrate_addresses_to_bech32`.
    let facade = BlockchainFacade::new(port, strangecoin::consensus::CHAIN_ID_REGTEST);

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
    assert!(keys.iter().any(|k| k == &bech32_a), "bech32 key a must exist");
    assert!(keys.iter().any(|k| k == &bech32_b), "bech32 key b must exist");

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

    // The migration is persisted by `save_state` at the end of the open path.
    drop(facade);

    // Open #2: idempotent — nothing left to migrate, balances stay put.
    let reopened = BlockchainFacade::new(port, strangecoin::consensus::CHAIN_ID_REGTEST);
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
}

#[test]
fn legacy_base64_addresses_in_chain_reset_and_migrate() {
    let port = random_port();
    let db = PortDbGuard::new(port);

    let (legacy_a, bech32_a) = keypair();
    let (legacy_b, bech32_b) = keypair();

    let mut balances = HashMap::new();
    balances.insert(legacy_a.clone(), 10_000u64);
    balances.insert(legacy_b.clone(), 5_000u64);

    // Stage 0 chain: a genesis stub plus a block whose tx fields still carry
    // base64 addresses — the migration must detect it and reset the chain.
    let genesis = stub_block(
        0,
        vec![Transaction {
            sender: "genesis".to_string(),
            receiver: "regtest_initial_holder".to_string(),
            amount: 0,
            nonce: 0,
            chain_id: consensus::CHAIN_ID_REGTEST,
            signature: Vec::new(),
            is_coinbase: true,
        }],
    );
    let legacy_block = stub_block(
        1,
        vec![Transaction {
            sender: legacy_a.clone(),
            receiver: legacy_b.clone(),
            amount: 100,
            nonce: 1,
            chain_id: consensus::CHAIN_ID_REGTEST,
            signature: Vec::new(),
            is_coinbase: false,
        }],
    );
    seed_balances_and_chain(db.path(), &balances, &[genesis, legacy_block]);

    // Open #1: legacy tx addresses force a chain reset to fresh genesis.
    let facade = BlockchainFacade::new(port, strangecoin::consensus::CHAIN_ID_REGTEST);

    assert_no_legacy_keys(&facade);
    assert_eq!(facade.get_balance(&bech32_a), 10_000, "balance a migrated");
    assert_eq!(facade.get_balance(&bech32_b), 5_000, "balance b migrated");
    assert_eq!(facade.total_supply(), 15_000, "supply preserved");

    let chain = facade.chain_snapshot();
    assert_eq!(
        chain.len(),
        1,
        "legacy chain must be reset to fresh genesis"
    );
    for block in &chain {
        for tx in &block.transactions {
            for field in [&tx.sender, &tx.receiver] {
                let is_magic = MAGIC_FIELDS.contains(&field.as_str());
                assert!(
                    is_magic || address::decode_address(field).is_ok(),
                    "no base64 pubkey may remain in chain tx fields, got {field}"
                );
            }
        }
    }
    drop(facade);

    // Open #2: idempotent — no second reset, balances survive.
    let reopened = BlockchainFacade::new(port, strangecoin::consensus::CHAIN_ID_REGTEST);
    assert_eq!(
        reopened.chain_len(),
        1,
        "reopen must not reset the chain again"
    );
    assert_eq!(reopened.get_balance(&bech32_a), 10_000);
    assert_eq!(reopened.get_balance(&bech32_b), 5_000);
    assert_eq!(reopened.total_supply(), 15_000);
    assert_no_legacy_keys(&reopened);
}
