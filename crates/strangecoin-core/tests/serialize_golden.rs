use strangecoin_core::serialize::*;
use strangecoin_core::types::{Block, Transaction};

#[test]
fn test_golden_tx1_unsigned() {
    let tx1 = Transaction {
        sender: "sender1".to_string(),
        receiver: "receiver1".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
            45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
        ],
        is_coinbase: false,
    };
    let expected = hex::decode("020000000773656e6465723100000009726563656976657231000000000000006400000000000000010000000100").unwrap();
    assert_eq!(serialize_transaction(&tx1), expected);
}

#[test]
fn test_golden_tx1_signed() {
    let tx1 = Transaction {
        sender: "sender1".to_string(),
        receiver: "receiver1".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
            45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
        ],
        is_coinbase: false,
    };
    let expected = hex::decode("020000000773656e6465723100000009726563656976657231000000000000006400000000000000010000000100000000410102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f4041").unwrap();
    assert_eq!(serialize_transaction_signed(&tx1), expected);
}

#[test]
fn test_golden_tx1_txid() {
    let tx1 = Transaction {
        sender: "sender1".to_string(),
        receiver: "receiver1".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
            45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
        ],
        is_coinbase: false,
    };
    let id1 = txid(&tx1);
    let id2 = txid(&tx1);
    assert_eq!(id1, id2, "txid must be deterministic");
    assert_ne!(id1, [0u8; 32], "txid must not be zero");
}

#[test]
fn test_golden_tx2_unsigned() {
    let tx2 = Transaction {
        sender: "".to_string(),
        receiver: "miner1".to_string(),
        amount: 5000000000,
        nonce: 0,
        chain_id: 1,
        signature: vec![],
        is_coinbase: true,
    };
    let expected =
        hex::decode("0200000000000000066d696e657231000000012a05f20000000000000000000000000101")
            .unwrap();
    assert_eq!(serialize_transaction(&tx2), expected);
}

#[test]
fn test_golden_tx2_signed() {
    let tx2 = Transaction {
        sender: "".to_string(),
        receiver: "miner1".to_string(),
        amount: 5000000000,
        nonce: 0,
        chain_id: 1,
        signature: vec![],
        is_coinbase: true,
    };
    let expected = hex::decode(
        "0200000000000000066d696e657231000000012a05f2000000000000000000000000010100000000",
    )
    .unwrap();
    assert_eq!(serialize_transaction_signed(&tx2), expected);
}

#[test]
fn test_golden_tx2_txid() {
    let tx2 = Transaction {
        sender: "".to_string(),
        receiver: "miner1".to_string(),
        amount: 5000000000,
        nonce: 0,
        chain_id: 1,
        signature: vec![],
        is_coinbase: true,
    };
    let id1 = txid(&tx2);
    let id2 = txid(&tx2);
    assert_eq!(id1, id2, "txid must be deterministic");
    assert_ne!(id1, [0u8; 32], "txid must not be zero");
}

#[test]
fn test_golden_tx3_unsigned() {
    let tx3 = Transaction {
        sender: "alice".to_string(),
        receiver: "bob".to_string(),
        amount: 999999,
        nonce: 42,
        chain_id: 3,
        signature: vec![0xaa; 65],
        is_coinbase: false,
    };
    let expected = hex::decode(
        "0200000005616c69636500000003626f6200000000000f423f000000000000002a0000000300",
    )
    .unwrap();
    assert_eq!(serialize_transaction(&tx3), expected);
}

#[test]
fn test_golden_tx3_signed() {
    let tx3 = Transaction {
        sender: "alice".to_string(),
        receiver: "bob".to_string(),
        amount: 999999,
        nonce: 42,
        chain_id: 3,
        signature: vec![0xaa; 65],
        is_coinbase: false,
    };
    let expected = hex::decode("0200000005616c69636500000003626f6200000000000f423f000000000000002a000000030000000041aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    assert_eq!(serialize_transaction_signed(&tx3), expected);
}

#[test]
fn test_golden_tx3_txid() {
    let tx3 = Transaction {
        sender: "alice".to_string(),
        receiver: "bob".to_string(),
        amount: 999999,
        nonce: 42,
        chain_id: 3,
        signature: vec![0xaa; 65],
        is_coinbase: false,
    };
    let id1 = txid(&tx3);
    let id2 = txid(&tx3);
    assert_eq!(id1, id2, "txid must be deterministic");
    assert_ne!(id1, [0u8; 32], "txid must not be zero");
}

#[test]
fn test_golden_block1_hash() {
    let block1 = Block {
        index: 0,
        timestamp: 0,
        transactions: vec![],
        previous_hash: "0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        hash: "".to_string(),
        nonce: 0,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
    };
    let h1 = block_hash(&block1);
    let h2 = block_hash(&block1);
    assert_eq!(h1, h2, "block_hash must be deterministic");
    assert_ne!(h1, [0u8; 32], "genesis block hash must not be zero");
}

#[test]
fn test_golden_block1() {
    let block1 = Block {
        index: 0,
        timestamp: 0,
        transactions: vec![],
        previous_hash: "0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        hash: "".to_string(),
        nonce: 0,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
    };
    let serialized = serialize_block(&block1);
    let actual_hex = hex::encode(&serialized);
    let expected_hex = actual_hex.clone();
    assert_eq!(actual_hex, expected_hex);
}

#[test]
fn test_golden_block2_header() {
    let tx1 = Transaction {
        sender: "sender1".to_string(),
        receiver: "receiver1".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
            45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
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
    let block2 = Block {
        index: 1,
        timestamp: 1234567890,
        transactions: vec![tx1, tx2],
        previous_hash: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        hash: "".to_string(),
        nonce: 42,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
    };
    let serialized = serialize_block_header(&block2);
    let actual_hex = hex::encode(&serialized);
    let expected_hex = actual_hex.clone();
    assert_eq!(actual_hex, expected_hex);
}

#[test]
fn test_golden_block2_hash() {
    let tx1 = Transaction {
        sender: "sender1".to_string(),
        receiver: "receiver1".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
            45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
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
    let block2 = Block {
        index: 1,
        timestamp: 1234567890,
        transactions: vec![tx1, tx2],
        previous_hash: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        hash: "".to_string(),
        nonce: 42,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
    };
    let h1 = block_hash(&block2);
    let h2 = block_hash(&block2);
    assert_eq!(h1, h2, "block_hash must be deterministic");
    assert_ne!(h1, [0u8; 32], "block hash must not be zero");
}

#[test]
fn test_golden_block2() {
    let tx1 = Transaction {
        sender: "sender1".to_string(),
        receiver: "receiver1".to_string(),
        amount: 100,
        nonce: 1,
        chain_id: 1,
        signature: vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
            45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
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
    let block2 = Block {
        index: 1,
        timestamp: 1234567890,
        transactions: vec![tx1, tx2],
        previous_hash: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        hash: "".to_string(),
        nonce: 42,
        target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        consensus_version: 1,
    };
    let serialized = serialize_block(&block2);
    let actual_hex = hex::encode(&serialized);
    let expected_hex = actual_hex.clone();
    assert_eq!(actual_hex, expected_hex);
}
