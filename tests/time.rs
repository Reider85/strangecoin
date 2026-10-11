mod common;

use common::*;
use std::time::{SystemTime, UNIX_EPOCH};
use strangecoin::{Block, BlockchainFacade, Transaction};

// Ненулевой coinbase обязателен: state::apply_block отклоняет блоки без него,
// а tx_root должен быть посчитан по фактическому списку транзакций.
fn zero_reward_coinbase() -> Transaction {
    Transaction {
        sender: "coinbase".to_string(),
        receiver: "miner".to_string(),
        amount: 0,
        nonce: 0,
        chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: true,
    }
}

fn create_block_with_timestamp(bc: &BlockchainFacade, timestamp: u64) -> bool {
    let previous_block = bc.tip().unwrap();
    let mut block = Block {
        index: previous_block.index + 1,
        timestamp,
        transactions: vec![zero_reward_coinbase()],
        previous_hash: previous_block.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: previous_block.target.clone(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
    block.hash = bc.calculate_hash(&block);
    bc.push_block_unchecked(block);
    bc.validate_chain()
}

#[test]
fn reject_block_timestamp_too_far_future() {
    let _dir = TestDir::new("time_future");
    let bc = create_test_blockchain(_dir.path());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let future_time = now + 7200 + 1;

    let result = create_block_with_timestamp(&bc, future_time);
    assert!(!result, "Block with timestamp > now+2h should be rejected");
}

#[test]
fn accept_valid_timestamp() {
    let _dir = TestDir::new("time_valid");
    let bc = create_test_blockchain(_dir.path());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let result = create_block_with_timestamp(&bc, now);
    assert!(result, "Block with current timestamp should be accepted");
}

#[test]
fn reject_block_before_mtp() {
    let _dir = TestDir::new("time_mtp");
    let bc = create_test_blockchain(_dir.path());

    let timestamps = [
        100u64, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1100, 1200,
    ];
    for &ts in &timestamps {
        let previous_block = bc.tip().unwrap();
        let mut block = Block {
            index: previous_block.index + 1,
            timestamp: ts,
            transactions: vec![zero_reward_coinbase()],
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target: previous_block.target.clone(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
        block.hash = bc.calculate_hash(&block);
        bc.push_block_unchecked(block);
    }

    assert!(
        bc.validate_chain(),
        "Chain with valid timestamps should pass"
    );

    let chain = bc.chain_snapshot();
    let mtp = strangecoin::consensus::median_time_past(&chain, chain.len() as u64);
    let too_old = mtp;

    let result = create_block_with_timestamp(&bc, too_old);
    assert!(
        !result,
        "Block with timestamp <= MTP ({} should be rejected",
        too_old
    );
}
