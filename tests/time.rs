mod common;

use common::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn create_block_with_timestamp(bc: &mut Blockchain, timestamp: u64) -> bool {
    let previous_block = bc.chain.last().unwrap().clone();
    let block = Block {
        index: previous_block.index + 1,
        timestamp,
        transactions: vec![],
        previous_hash: previous_block.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: previous_block.target.clone(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    let hash = bc.calculate_hash(&block);
    let mut block = block;
    block.hash = hash;
    bc.chain.push(block);
    bc.validate_chain()
}

#[test]
fn reject_block_timestamp_too_far_future() {
    let _dir = TestDir::new("time_future");
    let mut bc = create_test_blockchain(_dir.path());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let future_time = now + 7200 + 1;

    let result = create_block_with_timestamp(&mut bc, future_time);
    assert!(!result, "Block with timestamp > now+2h should be rejected");
}

#[test]
fn accept_valid_timestamp() {
    let _dir = TestDir::new("time_valid");
    let mut bc = create_test_blockchain(_dir.path());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let result = create_block_with_timestamp(&mut bc, now);
    assert!(result, "Block with current timestamp should be accepted");
}

#[test]
fn reject_block_before_mtp() {
    let _dir = TestDir::new("time_mtp");
    let mut bc = create_test_blockchain(_dir.path());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let timestamps = [100u64, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1100, 1200];
    for &ts in &timestamps {
        let previous_block = bc.chain.last().unwrap().clone();
        let block = Block {
            index: previous_block.index + 1,
            timestamp: ts,
            transactions: vec![],
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target: previous_block.target.clone(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        let hash = bc.calculate_hash(&block);
        let mut block = block;
        block.hash = hash;
        bc.chain.push(block);
    }

    assert!(bc.validate_chain(), "Chain with valid timestamps should pass");

    let mtp = strangecoin::consensus::median_time_past(&bc.chain, bc.chain.len() as u64);
    let too_old = mtp;

    let result = create_block_with_timestamp(&mut bc, too_old);
    assert!(!result, "Block with timestamp <= MTP ({} should be rejected", too_old);
}
