//! Print canonical golden vectors for block serialization (BUG-S1-007).
//!
//! Regenerate the literal hex pinned in `tests/serialize_golden.rs` after a
//! deliberate `FORMAT_VERSION` bump + ADR. Any unplanned diff here means
//! `serialize_block` / `serialize_block_header` / `block_hash` changed
//! silently — revert or follow the bump procedure.

use strangecoin_core::serialize::*;
use strangecoin_core::types::{Block, Transaction};

fn block1() -> Block {
    Block {
        index: 0,
        timestamp: 0,
        transactions: vec![],
        previous_hash: "0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        hash: "".to_string(),
        nonce: 0,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn block2() -> Block {
    let tx1 = Transaction {
        sender: "sender1".to_string(),
        receiver: "receiver1".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
            47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
        ],
        is_coinbase: false,
    };
    let tx2 = Transaction {
        sender: "".to_string(),
        receiver: "miner1".to_string(),
        amount: 5000000000,
        nonce: 0,
        chain_id: 1,
        signature: vec![],
        is_coinbase: true,
    };
    Block {
        index: 1,
        timestamp: 1234567890,
        transactions: vec![tx1, tx2],
        previous_hash: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        hash: "".to_string(),
        nonce: 42,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn main() {
    let b1 = block1();
    let b2 = block2();

    println!("FORMAT_VERSION = {}", FORMAT_VERSION);
    println!();
    println!("// serialize_block(block1)");
    println!("{}", hex::encode(serialize_block(&b1)));
    println!();
    println!("// serialize_block_header(block2)");
    println!("{}", hex::encode(serialize_block_header(&b2)));
    println!();
    println!("// serialize_block(block2)");
    println!("{}", hex::encode(serialize_block(&b2)));
    println!();
    println!("// block_hash(block1)");
    println!("{}", hex::encode(block_hash(&b1)));
    println!();
    println!("// block_hash(block2)");
    println!("{}", hex::encode(block_hash(&b2)));
}
