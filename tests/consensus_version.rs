mod common;

use common::*;
use std::time::{SystemTime, UNIX_EPOCH};
use strangecoin::Block;
use strangecoin::Transaction;

#[test]
fn stale_consensus_version_rejected() {
    let dir = TestDir::new("stale_version");
    let bc = create_test_blockchain(dir.path());

    let previous_block = bc.tip().unwrap();

    let coinbase_tx = Transaction {
        sender: "coinbase".to_string(),
        receiver: "miner".to_string(),
        amount: strangecoin::economics::emission::block_reward_at_height_for_chain(
            previous_block.index + 1,
            bc.total_supply(),
            strangecoin::consensus::CHAIN_ID_REGTEST,
        ),
        nonce: 0,
        chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: true,
    };

    let mut block = Block {
        index: previous_block.index + 1,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        transactions: vec![coinbase_tx],
        previous_hash: previous_block.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: previous_block.target.clone(),
        consensus_version: 0,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    bc.push_block_unchecked(block);

    assert!(
        !bc.validate_chain(),
        "Chain with stale consensus_version (0) should be rejected"
    );
}

#[test]
fn future_consensus_version_rejected() {
    let dir = TestDir::new("future_version");
    let bc = create_test_blockchain(dir.path());

    let previous_block = bc.tip().unwrap();

    let coinbase_tx = Transaction {
        sender: "coinbase".to_string(),
        receiver: "miner".to_string(),
        amount: strangecoin::economics::emission::block_reward_at_height_for_chain(
            previous_block.index + 1,
            bc.total_supply(),
            strangecoin::consensus::CHAIN_ID_REGTEST,
        ),
        nonce: 0,
        chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: true,
    };

    let mut block = Block {
        index: previous_block.index + 1,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        transactions: vec![coinbase_tx],
        previous_hash: previous_block.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: previous_block.target.clone(),
        consensus_version: 99,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    bc.push_block_unchecked(block);

    assert!(
        !bc.validate_chain(),
        "Chain with future consensus_version (99) should be rejected"
    );
}

#[test]
fn correct_consensus_version_accepted() {
    let dir = TestDir::new("correct_version");
    let bc = create_test_blockchain(dir.path());

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    // mine_block() needs a pending transaction to build a block from.
    let mut tx = Transaction {
        sender: addr.clone(),
        receiver: "recipient".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut tx, &keypairs[0].1);
    bc.apply_tx(tx).expect("transaction rejected");

    mine_current(&bc);

    assert!(
        bc.validate_chain(),
        "Chain with correct consensus_version should be accepted"
    );
}
