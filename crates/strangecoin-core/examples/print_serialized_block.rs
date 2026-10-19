//! Print canonical golden vectors for block serialization (BUG-S1-007).
//!
//! Regenerate the literal hex pinned in `tests/serialize_golden.rs` after a
//! deliberate `FORMAT_VERSION` bump + ADR. Any unplanned diff here means
//! `serialize_block` / `serialize_block_header` / `block_hash` changed
//! silently — revert or follow the bump procedure.
//!
//! Last regeneration: S1.5-FBP07 — FORMAT_VERSION 6 → 7 (header gains
//! `receipts_root`, ADR-0016); genesis hashes regenerated via
//! `examples/print_genesis_hashes.rs` in the same commit.

use strangecoin_core::serialize::*;
use strangecoin_core::types::{Block, CallTx, Transaction, TransferTx};

fn tx1() -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "sender1".to_string(),
        to: "receiver1".to_string(),
        amount: 100,
        fee: 0,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
            47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
        ],
        is_coinbase: false,
    })
}

fn tx2() -> Transaction {
    Transaction::coinbase("", "miner1", 5_000_000_000, 1)
}

fn tx3() -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "alice".to_string(),
        to: "bob".to_string(),
        amount: 999_999,
        fee: 0,
        nonce: 42,
        chain_id: 3,
        signature: vec![0xaa; 65],
        is_coinbase: false,
    })
}

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
        receipts_root: [0u8; 32],
    }
}

fn block2() -> Block {
    Block {
        index: 1,
        timestamp: 1234567890,
        transactions: vec![tx1(), tx2()],
        previous_hash: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        hash: "".to_string(),
        nonce: 42,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    }
}

fn main() {
    let b1 = block1();
    let b2 = block2();

    let create = Transaction::create("sender1", vec![1, 2, 3], vec![4, 5], 50, 1, 1);
    let call = Transaction::Call(CallTx {
        from: "sender1".to_string(),
        to: "receiver1".to_string(),
        data: vec![9],
        value: 7,
        gas_limit: 100_000,
        fee: 0,
        nonce: 1,
        chain_id: 1,
        signature: Vec::new(),
    });

    println!("FORMAT_VERSION = {}", FORMAT_VERSION);
    println!();
    println!("// tx1 unsigned");
    println!("{}", hex::encode(serialize_transaction(&tx1())));
    println!();
    println!("// tx1 signed");
    println!("{}", hex::encode(serialize_transaction_signed(&tx1())));
    println!();
    println!("// tx2 unsigned");
    println!("{}", hex::encode(serialize_transaction(&tx2())));
    println!();
    println!("// tx2 signed");
    println!("{}", hex::encode(serialize_transaction_signed(&tx2())));
    println!();
    println!("// tx3 unsigned");
    println!("{}", hex::encode(serialize_transaction(&tx3())));
    println!();
    println!("// tx3 signed");
    println!("{}", hex::encode(serialize_transaction_signed(&tx3())));
    println!();
    println!("// create unsigned");
    println!("{}", hex::encode(serialize_transaction(&create)));
    println!();
    println!("// create signed");
    println!("{}", hex::encode(serialize_transaction_signed(&create)));
    println!();
    println!("// call unsigned");
    println!("{}", hex::encode(serialize_transaction(&call)));
    println!();
    println!("// call signed");
    println!("{}", hex::encode(serialize_transaction_signed(&call)));
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
