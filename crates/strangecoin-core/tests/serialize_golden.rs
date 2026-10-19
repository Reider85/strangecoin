//! Golden vectors for canonical serialization (FORMAT_VERSION = 7).
//!
//! BUG-S1-007: any change to `serialize_block`, `serialize_block_header`, or
//! `block_hash` must fail these tests. To land such a change: bump
//! `FORMAT_VERSION`, record an ADR, regenerate the literals via
//! `cargo run -p strangecoin-core --example print_serialized_block`, and
//! update the vectors here in the same commit. Do not "fix" a failing test
//! by copying the new output into the literals without that procedure.
//!
//! S1.5-P08: FORMAT_VERSION 4 -> 5 — Transaction became a per-variant enum
//! (Transfer/Create/Call); `fee` is part of the signed payload of every
//! variant. The full bump (including EXPECTED_*_GENESIS_HASH regeneration)
//! is recorded in the P08 commit and Changelog.
//!
//! S1.5-FBP04: FORMAT_VERSION 5 -> 6 — `Create` carries `constructor_params`
//! (canonical type-name string; empty = no params) and `gas_limit`
//! (constructor budget). The shared version byte also moves headers and
//! receipts, so genesis hashes were regenerated in the same commit.
//!
//! S1.5-FBP07: FORMAT_VERSION 6 -> 7 — the canonical header encoding gains
//! `receipts_root` (32-byte SMT commitment after `state_root`; ADR-0016,
//! `HEADER_WIRE_LEN` 189 -> 221). Genesis hashes were regenerated in the
//! same commit (BUG-S1-007).

use strangecoin_core::abi::{AbiParam, AbiType};
use strangecoin_core::serialize::*;
use strangecoin_core::types::{Block, CallTx, CreateTx, Transaction, TransferTx};

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

#[test]
fn test_golden_tx1_unsigned() {
    let expected = hex::decode("07000000000773656e64657231000000097265636569766572310000000000000064000000000000000000000000000000010000000100").unwrap();
    assert_eq!(serialize_transaction(&tx1()), expected);
}

#[test]
fn test_golden_tx1_signed() {
    let expected = hex::decode("07000000000773656e64657231000000097265636569766572310000000000000064000000000000000000000000000000010000000100000000410102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f4041").unwrap();
    assert_eq!(serialize_transaction_signed(&tx1()), expected);
}

#[test]
fn test_golden_tx1_txid() {
    let id1 = txid(&tx1());
    let id2 = txid(&tx1());
    assert_eq!(id1, id2, "txid must be deterministic");
    assert_ne!(id1, [0u8; 32], "txid must not be zero");
}

#[test]
fn test_golden_tx2_unsigned() {
    let expected =
        hex::decode("070000000000000000066d696e657231000000012a05f200000000000000000000000000000000000000000101")
            .unwrap();
    assert_eq!(serialize_transaction(&tx2()), expected);
}

#[test]
fn test_golden_tx2_signed() {
    let expected = hex::decode(
        "070000000000000000066d696e657231000000012a05f20000000000000000000000000000000000000000010100000000",
    )
    .unwrap();
    assert_eq!(serialize_transaction_signed(&tx2()), expected);
}

#[test]
fn test_golden_tx2_txid() {
    let id1 = txid(&tx2());
    let id2 = txid(&tx2());
    assert_eq!(id1, id2, "txid must be deterministic");
    assert_ne!(id1, [0u8; 32], "txid must not be zero");
}

#[test]
fn test_golden_tx3_unsigned() {
    let expected =
        hex::decode("070000000005616c69636500000003626f6200000000000f423f0000000000000000000000000000002a0000000300")
            .unwrap();
    assert_eq!(serialize_transaction(&tx3()), expected);
}

#[test]
fn test_golden_tx3_signed() {
    let expected = hex::decode("070000000005616c69636500000003626f6200000000000f423f0000000000000000000000000000002a000000030000000041aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    assert_eq!(serialize_transaction_signed(&tx3()), expected);
}

#[test]
fn test_golden_tx3_txid() {
    let id1 = txid(&tx3());
    let id2 = txid(&tx3());
    assert_eq!(id1, id2, "txid must be deterministic");
    assert_ne!(id1, [0u8; 32], "txid must not be zero");
}

#[test]
fn test_golden_create_unsigned() {
    let tx = Transaction::Create(CreateTx {
        from: "sender1".to_string(),
        init_code: vec![1, 2, 3],
        init_args: vec![4, 5],
        constructor_params: Vec::new(),
        endowment: 50,
        gas_limit: 0,
        fee: 0,
        nonce: 1,
        chain_id: 1,
        signature: Vec::new(),
    });
    let expected =
        hex::decode("07010000000773656e646572310000000301020300000002040500000000000000000000003200000000000000000000000000000000000000000000000100000001")
            .unwrap();
    assert_eq!(serialize_transaction(&tx), expected);
}

#[test]
fn test_golden_create_signed() {
    let tx = Transaction::Create(CreateTx {
        from: "sender1".to_string(),
        init_code: vec![1, 2, 3],
        init_args: vec![4, 5],
        constructor_params: Vec::new(),
        endowment: 50,
        gas_limit: 0,
        fee: 0,
        nonce: 1,
        chain_id: 1,
        signature: Vec::new(),
    });
    let expected =
        hex::decode("07010000000773656e64657231000000030102030000000204050000000000000000000000320000000000000000000000000000000000000000000000010000000100000000")
            .unwrap();
    assert_eq!(serialize_transaction_signed(&tx), expected);
}

/// S1.5-FBP04: the new Create fields land on the wire —
/// `constructor_params` as the comma-joined canonical type names and
/// `gas_limit` as a u64 after `endowment`.
#[test]
fn test_golden_create_with_params_unsigned() {
    let tx = Transaction::create_with(
        "sender1",
        vec![1, 2, 3],
        vec![4, 5],
        vec![
            AbiParam::new("", AbiType::Uint(256)),
            AbiParam::new("", AbiType::Address),
        ],
        50,
        100_000,
        1,
        1,
    );
    let expected =
        hex::decode("07010000000773656e64657231000000030102030000000204050000000f75696e743235362c61646472657373000000000000003200000000000186a00000000000000000000000000000000100000001")
            .unwrap();
    assert_eq!(serialize_transaction(&tx), expected);

    // Round-trip: the decoder recovers params and budget.
    let decoded = deserialize_transaction(&expected).expect("v7 create decodes");
    match decoded {
        Transaction::Create(create) => {
            assert_eq!(create.gas_limit, 100_000);
            assert_eq!(
                create.constructor_params,
                vec![
                    AbiParam::new("", AbiType::Uint(256)),
                    AbiParam::new("", AbiType::Address),
                ]
            );
        }
        other => panic!("expected Create, got {other:?}"),
    }
}

#[test]
fn test_golden_call_unsigned() {
    let tx = Transaction::Call(CallTx {
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
    let expected =
        hex::decode("07020000000773656e64657231000000097265636569766572310000000109000000000000000700000000000186a00000000000000000000000000000000100000001")
            .unwrap();
    assert_eq!(serialize_transaction(&tx), expected);
}

#[test]
fn test_golden_call_signed() {
    let tx = Transaction::Call(CallTx {
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
    let expected =
        hex::decode("07020000000773656e64657231000000097265636569766572310000000109000000000000000700000000000186a0000000000000000000000000000000010000000100000000")
            .unwrap();
    assert_eq!(serialize_transaction_signed(&tx), expected);
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
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    let h1 = block_hash(&block1);
    let h2 = block_hash(&block1);
    assert_eq!(h1, h2, "block_hash must be deterministic");
    assert_ne!(h1, [0u8; 32], "genesis block hash must not be zero");
    assert_eq!(
        hex::encode(h1),
        "582fbafde592c35fd4407dc94162fdd34ca89852c85cc86176c30c1e881995ee",
        "golden block_hash(block1) changed"
    );
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
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    let serialized = serialize_block(&block1);
    let actual_hex = hex::encode(&serialized);
    let expected_hex = "070000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0000000000000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
    assert_eq!(actual_hex, expected_hex);
}

#[test]
fn test_golden_block2_header() {
    let block2 = Block {
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
    };
    let serialized = serialize_block_header(&block2);
    let actual_hex = hex::encode(&serialized);
    let expected_hex = "07000000000000000100000000499602d211111111111111111111111111111111111111111111111111111111111111110000000000000000000000000000000000000000000000000000000000000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff000000000000002a0000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
    assert_eq!(actual_hex, expected_hex);
}

#[test]
fn test_golden_block2_hash() {
    let block2 = Block {
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
    };
    let h1 = block_hash(&block2);
    let h2 = block_hash(&block2);
    assert_eq!(h1, h2, "block_hash must be deterministic");
    assert_ne!(h1, [0u8; 32], "block hash must not be zero");
    assert_eq!(
        hex::encode(h1),
        "2ebbcba914867677f24cec3a59cfb877b93c91ca16f896d1af771014131b6fe4",
        "golden block_hash(block2) changed"
    );
}

#[test]
fn test_golden_block2() {
    let block2 = Block {
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
    };
    let serialized = serialize_block(&block2);
    let actual_hex = hex::encode(&serialized);
    let expected_hex = "07000000000000000100000000499602d211111111111111111111111111111111111111111111111111111111111111110000000000000000000000000000000000000000000000000000000000000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff000000000000002a0000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000007c07000000000773656e64657231000000097265636569766572310000000000000064000000000000000000000000000000010000000100000000410102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404100000031070000000000000000066d696e657231000000012a05f20000000000000000000000000000000000000000010100000000";
    assert_eq!(actual_hex, expected_hex);
}
