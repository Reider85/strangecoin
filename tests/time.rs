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
    };
    let hash = bc.calculate_hash(&block);
    let mut block = block;
    block.hash = hash;
    bc.chain.push(block);
    bc.validate_chain()
}

#[test]
fn reject_block_timestamp_too_far_future() {
    let dir = temp_db_dir("time_future");
    let mut bc = create_test_blockchain(&dir);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let future_time = now + 7200 + 1;

    let result = create_block_with_timestamp(&mut bc, future_time);
    assert!(!result, "Block with timestamp > now+2h should be rejected");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accept_valid_timestamp() {
    let dir = temp_db_dir("time_valid");
    let mut bc = create_test_blockchain(&dir);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let result = create_block_with_timestamp(&mut bc, now);
    assert!(result, "Block with current timestamp should be accepted");

    let _ = std::fs::remove_dir_all(&dir);
}
