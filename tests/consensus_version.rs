mod common;

use common::*;
use std::time::{SystemTime, UNIX_EPOCH};
use strangecoin::Block;
use strangecoin::Transaction;

#[test]
fn stale_consensus_version_rejected() {
    let dir = TestDir::new("stale_version");
    let mut bc = create_test_blockchain(dir.path());

    let previous_block = bc.chain.last().unwrap().clone();

    let coinbase_tx = Transaction {
        sender: "coinbase".to_string(),
        receiver: "miner".to_string(),
        amount: strangecoin::economics::emission::block_reward_at_height(
            previous_block.index + 1,
            bc.balances.values().map(|a| a.balance).sum(),
        ),
        nonce: 0,
        chain_id: strangecoin::consensus::current_chain_id(),
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
    };
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    bc.chain.push(block);

    assert!(
        !bc.validate_chain(),
        "Chain with stale consensus_version (0) should be rejected"
    );
}

#[test]
fn future_consensus_version_rejected() {
    let dir = TestDir::new("future_version");
    let mut bc = create_test_blockchain(dir.path());

    let previous_block = bc.chain.last().unwrap().clone();

    let coinbase_tx = Transaction {
        sender: "coinbase".to_string(),
        receiver: "miner".to_string(),
        amount: strangecoin::economics::emission::block_reward_at_height(
            previous_block.index + 1,
            bc.balances.values().map(|a| a.balance).sum(),
        ),
        nonce: 0,
        chain_id: strangecoin::consensus::current_chain_id(),
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
    };
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    bc.chain.push(block);

    assert!(
        !bc.validate_chain(),
        "Chain with future consensus_version (99) should be rejected"
    );
}

#[test]
fn correct_consensus_version_accepted() {
    let dir = TestDir::new("correct_version");
    let mut bc = create_test_blockchain(dir.path());

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    mine_current(&mut bc);

    assert!(
        bc.validate_chain(),
        "Chain with correct consensus_version should be accepted"
    );
}
