pub use strangecoin_core::consensus::*;

#[derive(serde::Deserialize)]
pub struct GenesisConfig {
    pub format_version: u8,
    pub network_id: u32,
    pub chain_id: u32,
    pub timestamp: u64,
    pub initial_holder_pubkey: String,
    pub initial_amount: u64,
    pub block_reward: u64,
    pub tail_emission_rate: f64,
    pub max_supply_pre_tail: u64,
    pub target_block_time: u64,
    pub retarget_interval: u64,
    pub genesis_hash: String,
}

// BUG-S0-015 / SCIP-0001: no genesis private key in code.
// The testnet seed-derived key is burned; mainnet must use an offline-generated
// key (pubkey only in genesis.json). See docs/SCIP/scip-0001-genesis-key-replacement.md.

pub fn load_genesis(path: &str) -> Result<crate::Block, crate::error::StrangecoinError> {
    let json = std::fs::read_to_string(path)?;
    let genesis: GenesisConfig = serde_json::from_str(&json)?;

    // Enforce network_id == chain_id (invariant #10)
    if genesis.network_id != genesis.chain_id {
        return Err(crate::error::StrangecoinError::GenesisNetworkMismatch {
            network_id: genesis.network_id,
            chain_id: genesis.chain_id,
        });
    }

    let pk_hex = genesis
        .initial_holder_pubkey
        .strip_prefix("0x")
        .unwrap_or(&genesis.initial_holder_pubkey);
    let pk_bytes = hex::decode(pk_hex)?;
    let public_key = secp256k1::PublicKey::from_slice(&pk_bytes)?;
    let initial_holder_addr = crate::address::encode_address(&public_key, genesis.network_id)?;

    let genesis_tx = crate::Transaction {
        sender: "genesis".to_string(),
        receiver: initial_holder_addr,
        amount: genesis.initial_amount,
        nonce: 0,
        chain_id: genesis.chain_id,
        signature: Vec::new(),
        is_coinbase: true,
    };

    let target_bytes =
        hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")?;
    let mut target_arr = [0u8; 32];
    target_arr.copy_from_slice(&target_bytes);

    let mut block = crate::Block {
        index: 0,
        timestamp: genesis.timestamp,
        transactions: vec![genesis_tx],
        previous_hash: "0".repeat(64),
        hash: String::new(),
        nonce: 0,
        target: hex::encode(target_arr),
        consensus_version: strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);

    let hash = strangecoin_core::serialize::block_hash(&block);
    block.hash = hex::encode(hash);
    Ok(block)
}

pub fn validate_genesis(
    block: &crate::Block,
    chain_id: u32,
) -> Result<(), crate::error::StrangecoinError> {
    if strangecoin_core::consensus::is_regtest(chain_id) {
        return Ok(());
    }
    let expected = match chain_id {
        strangecoin_core::consensus::CHAIN_ID_MAINNET => {
            strangecoin_core::consensus::EXPECTED_GENESIS_HASH
        }
        strangecoin_core::consensus::CHAIN_ID_TESTNET => {
            strangecoin_core::consensus::EXPECTED_TESTNET_GENESIS_HASH
        }
        other => {
            return Err(crate::error::StrangecoinError::ConfigError(format!(
                "unknown network ID: {}",
                other
            )));
        }
    };
    let hash = strangecoin_core::serialize::block_hash(block);
    if hash != expected {
        return Err(crate::error::StrangecoinError::GenesisMismatch {
            expected,
            got: hash,
        });
    }
    Ok(())
}

/// Genesis file name for a public network (regtest builds its own genesis).
pub fn genesis_file_for_chain(
    chain_id: u32,
) -> Option<std::path::PathBuf> {
    let file_name = match chain_id {
        strangecoin_core::consensus::CHAIN_ID_MAINNET => "genesis.json",
        strangecoin_core::consensus::CHAIN_ID_TESTNET => "genesis-testnet.json",
        _ => return None,
    };
    let exe_path =
        std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
    let exe_dir = exe_path
        .parent()
        .expect("Не удалось получить директорию исполняемого файла");
    Some(exe_dir.join(file_name))
}

/// The deterministic regtest genesis (BUG-S1-004): built in code, no file,
/// no expected-hash pin — regtest chains are disposable.
pub fn regtest_genesis_block(consensus_version: u32) -> crate::Block {
    use strangecoin_core::consensus::CHAIN_ID_REGTEST;

    let genesis_tx = crate::Transaction {
        sender: "genesis".to_string(),
        receiver: "regtest_initial_holder".to_string(),
        amount: 1_000_000_000,
        nonce: 0,
        chain_id: CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: true,
    };
    let target_bytes =
        hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
            .expect("valid target hex");
    let mut target_arr = [0u8; 32];
    target_arr.copy_from_slice(&target_bytes);
    let mut block = crate::Block {
        index: 0,
        timestamp: 0,
        transactions: vec![genesis_tx],
        previous_hash: "0".repeat(64),
        hash: String::new(),
        nonce: 0,
        target: hex::encode(target_arr),
        consensus_version,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
    block.hash = hex::encode(strangecoin_core::serialize::block_hash(&block));
    block
}
