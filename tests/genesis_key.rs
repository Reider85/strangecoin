//! BUG-S0-015 / S1.5-P01 — genesis key must not live in code.
//!
//! The node validates the mainnet/testnet genesis from `genesis.json` using
//! only the initial holder's **public** key. There is no API in the crate that
//! derives the genesis private key. A regression guard fails this test if the
//! old public seed string is reintroduced into consensus source.

use strangecoin::consensus::{load_genesis, validate_genesis};
use strangecoin::consensus::EXPECTED_GENESIS_HASH;

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
    validate_genesis(&block, false)
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
