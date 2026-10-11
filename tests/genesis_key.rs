//! BUG-S0-015 / S1.5-P01 — genesis key must not live in code.
//!
//! The node validates the mainnet/testnet genesis from the network-specific
//! genesis file using only the initial holder's **public** key. There is no
//! API in the crate that derives the genesis private key. A regression guard
//! fails this test if the old public seed string is reintroduced into
//! consensus source. Per-network expected hashes: BUG-S1-004.

use strangecoin::consensus::{load_genesis, regtest_genesis_block, validate_genesis};
use strangecoin::consensus::EXPECTED_GENESIS_HASH;
use strangecoin::consensus::EXPECTED_TESTNET_GENESIS_HASH;
use strangecoin::consensus::{CHAIN_ID_MAINNET, CHAIN_ID_REGTEST, CHAIN_ID_TESTNET};
use strangecoin::error::StrangecoinError;
use strangecoin::consensus::CURRENT_CONSENSUS_VERSION;

fn repo_path(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_genesis_json() -> serde_json::Value {
    let path = repo_path("genesis.json");
    let text = std::fs::read_to_string(&path).expect("genesis.json must exist at repo root");
    serde_json::from_str(&text).expect("genesis.json must be valid JSON")
}

#[test]
fn genesis_validates_without_private_key_in_code() {
    let genesis_path = repo_path("genesis.json");
    let block = load_genesis(genesis_path.to_str().unwrap())
        .expect("load_genesis must work from pubkey-only genesis.json");
    validate_genesis(&block, strangecoin::consensus::CHAIN_ID_MAINNET)
        .expect("validate_genesis must accept the EXPECTED_GENESIS_HASH genesis");
    assert_eq!(block.index, 0);
    assert!(block.transactions[0].is_coinbase);
}

#[test]
fn initial_holder_address_derives_from_pubkey_only() {
    let genesis_path = repo_path("genesis.json");
    let block = load_genesis(genesis_path.to_str().unwrap()).expect("load genesis");

    let json = read_genesis_json();
    let pk_hex = json["initial_holder_pubkey"]
        .as_str()
        .expect("genesis.json must expose initial_holder_pubkey");
    let pk_hex = pk_hex.strip_prefix("0x").unwrap_or(pk_hex);
    let pk_bytes = hex::decode(pk_hex).expect("pubkey hex");
    let public_key =
        secp256k1::PublicKey::from_slice(&pk_bytes).expect("valid secp256k1 pubkey");
    let network_id = json["network_id"].as_u64().expect("network_id") as u32;
    let addr = strangecoin::address::encode_address(&public_key, network_id)
        .expect("encode_address");

    assert_eq!(
        block.transactions[0].receiver, addr,
        "genesis allocation receiver must be derived only from initial_holder_pubkey"
    );
}

#[test]
fn genesis_json_contains_only_public_key_material() {
    let json = read_genesis_json();
    let allowed = [
        "format_version",
        "network_id",
        "chain_id",
        "timestamp",
        "initial_holder_pubkey",
        "initial_amount",
        "block_reward",
        "tail_emission_rate",
        "max_supply_pre_tail",
        "target_block_time",
        "retarget_interval",
        "genesis_hash",
    ];
    let obj = json.as_object().expect("genesis.json must be an object");
    for key in obj.keys() {
        assert!(
            allowed.contains(&key.as_str()),
            "unexpected genesis.json field `{key}` — only public genesis params allowed"
        );
    }

    let pk_hex = json["initial_holder_pubkey"]
        .as_str()
        .expect("initial_holder_pubkey required");
    let pk_hex = pk_hex.strip_prefix("0x").unwrap_or(pk_hex);
    let pk_bytes = hex::decode(pk_hex).expect("pubkey hex");
    assert_eq!(pk_bytes.len(), 33, "compressed secp256k1 pubkey is 33 bytes");
    assert!(
        pk_bytes[0] == 0x02 || pk_bytes[0] == 0x03,
        "pubkey must be compressed (0x02/0x03 prefix)"
    );
}

#[test]
fn consensus_source_has_no_genesis_seed() {
    let src = std::fs::read_to_string(repo_path("src/consensus/mod.rs"))
        .expect("src/consensus/mod.rs must be readable");
    assert!(
        !src.contains("strangecoin-genesis-seed"),
        "BUG-S0-015: genesis seed string must not appear in consensus source (SCIP-0001)"
    );
    assert!(
        !src.contains("genesis_keypair"),
        "BUG-S0-015: genesis_keypair must not be reintroduced (SCIP-0001)"
    );
}

#[test]
fn expected_genesis_hash_matches_committed_genesis() {
    let genesis_path = repo_path("genesis.json");
    let block = load_genesis(genesis_path.to_str().unwrap()).expect("load genesis");
    let hash = strangecoin_core::serialize::block_hash(&block);
    assert_eq!(
        hash, EXPECTED_GENESIS_HASH,
        "committed genesis.json must hash to EXPECTED_GENESIS_HASH"
    );
}

// ---------------------------------------------------------------------------
// BUG-S1-004: per-network genesis validation
// ---------------------------------------------------------------------------

#[test]
fn testnet_genesis_validates_against_its_own_hash() {
    let genesis_path = repo_path("genesis-testnet.json");
    let block = load_genesis(genesis_path.to_str().unwrap())
        .expect("load_genesis must work from genesis-testnet.json");
    validate_genesis(&block, CHAIN_ID_TESTNET)
        .expect("testnet genesis must match EXPECTED_TESTNET_GENESIS_HASH");
    let hash = strangecoin_core::serialize::block_hash(&block);
    assert_eq!(
        hash, EXPECTED_TESTNET_GENESIS_HASH,
        "committed genesis-testnet.json must hash to EXPECTED_TESTNET_GENESIS_HASH"
    );
    assert_eq!(block.transactions[0].chain_id, CHAIN_ID_TESTNET);
}

#[test]
fn regtest_genesis_validates_without_a_file() {
    let block = regtest_genesis_block(CURRENT_CONSENSUS_VERSION);
    validate_genesis(&block, CHAIN_ID_REGTEST).expect("regtest genesis is always accepted");
    assert_eq!(block.index, 0);
    assert_eq!(block.transactions[0].chain_id, CHAIN_ID_REGTEST);
    assert!(block.transactions[0].is_coinbase);
}

#[test]
fn mainnet_genesis_is_rejected_on_testnet_and_vice_versa() {
    let mainnet = load_genesis(repo_path("genesis.json").to_str().unwrap()).expect("mainnet");
    let testnet =
        load_genesis(repo_path("genesis-testnet.json").to_str().unwrap()).expect("testnet");

    let err = validate_genesis(&mainnet, CHAIN_ID_TESTNET)
        .expect_err("mainnet genesis must not validate on testnet");
    assert!(
        matches!(err, StrangecoinError::GenesisMismatch { .. }),
        "{err:?}"
    );

    let err = validate_genesis(&testnet, CHAIN_ID_MAINNET)
        .expect_err("testnet genesis must not validate on mainnet");
    assert!(
        matches!(err, StrangecoinError::GenesisMismatch { .. }),
        "{err:?}"
    );
}

#[test]
fn genesis_file_with_mismatched_network_fields_is_rejected() {
    // load_genesis enforces network_id == chain_id inside the file
    // (invariant #10): a file that declares network 2 but chain 1 is broken.
    let mut json = read_genesis_json();
    json["network_id"] = serde_json::json!(2);
    json["chain_id"] = serde_json::json!(1);
    let dir = std::env::temp_dir().join(format!("sc_genesis_mismatch_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("genesis.json");
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).expect("write temp genesis");

    let err = load_genesis(path.to_str().unwrap())
        .expect_err("network_id != chain_id inside genesis.json must be rejected");
    assert!(
        matches!(err, StrangecoinError::GenesisNetworkMismatch { .. }),
        "{err:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
