//! BUG-S1-004 / BUG-S1-010 — chain_id comes from the node configuration,
//! not from a hardcoded constant. A mainnet node (network_id = 1) must
//! reject foreign-chain transactions in the mempool and in block
//! validation, and its coinbase schedule must be the mainnet one.

mod common;

use common::*;
use strangecoin::error::StrangecoinError;
use strangecoin::Transaction;
use strangecoin_core::consensus::{CHAIN_ID_MAINNET, CHAIN_ID_REGTEST, CHAIN_ID_TESTNET};

fn signed_tx(
    sender: &str,
    receiver: &str,
    amount: u64,
    nonce: u64,
    chain_id: u32,
    key: &secp256k1::SecretKey,
) -> Transaction {
    let mut tx = Transaction {
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce,
        chain_id,
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut tx, key);
    tx
}

#[test]
fn cross_chain_tx_rejected_in_mempool() {
    let dir = TestDir::new("chain_id_mempool");
    let bc = create_test_blockchain_for_network(dir.path(), CHAIN_ID_MAINNET);
    assert_eq!(bc.chain_id(), CHAIN_ID_MAINNET);

    let (addr, key) = generate_keypair();
    bc.with_state_cache_mut(|sc| sc.credit(&addr, 1_000_000));

    let testnet_tx = signed_tx(&addr, "recipient", 10, 1, CHAIN_ID_TESTNET, &key);
    let err = bc
        .apply_tx(testnet_tx)
        .expect_err("testnet tx must be rejected by a mainnet mempool");
    assert!(
        matches!(
            err,
            StrangecoinError::InvalidChainId {
                expected: CHAIN_ID_MAINNET,
                got: CHAIN_ID_TESTNET
            }
        ),
        "{err:?}"
    );
    assert!(bc.mempool_is_empty());
}

#[test]
fn correct_chain_tx_accepted() {
    let dir = TestDir::new("chain_id_accept");
    let bc = create_test_blockchain_for_network(dir.path(), CHAIN_ID_MAINNET);

    let (addr, key) = generate_keypair();
    bc.with_state_cache_mut(|sc| sc.credit(&addr, 1_000_000));

    let mainnet_tx = signed_tx(&addr, "recipient", 10, 1, CHAIN_ID_MAINNET, &key);
    bc.apply_tx(mainnet_tx)
        .expect("same-chain tx must be accepted");
    assert_eq!(bc.mempool_len(), 1);
}

#[test]
fn block_with_foreign_chain_id_tx_rejected() {
    let dir = TestDir::new("chain_id_block");
    let bc = create_test_blockchain(dir.path()); // regtest node
    let genesis = bc.tip().unwrap();

    let (addr, key) = generate_keypair();
    bc.with_state_cache_mut(|sc| sc.credit(&addr, 1_000_000));
    let foreign = signed_tx(&addr, "recipient", 10, 1, CHAIN_ID_TESTNET, &key);

    // A structurally valid block (position/hash/tx_root/target OK) whose
    // transfer carries a foreign chain_id: validate_and_apply must fail on
    // the chain_id gate before any state transition.
    let coinbase = Transaction {
        sender: "coinbase".to_string(),
        receiver: addr.clone(),
        amount: 0,
        nonce: 0,
        chain_id: CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: true,
    };
    let mut block = strangecoin::Block {
        index: 1,
        timestamp: genesis.timestamp + 1,
        transactions: vec![coinbase, foreign],
        previous_hash: genesis.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: genesis.target.clone(),
        consensus_version: strangecoin::consensus::CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));

    let err = bc
        .add_block(block)
        .expect_err("a block carrying a foreign-chain tx must be rejected");
    assert!(
        matches!(
            err,
            StrangecoinError::InvalidChainId {
                expected: CHAIN_ID_REGTEST,
                got: CHAIN_ID_TESTNET
            }
        ),
        "{err:?}"
    );
}

#[test]
fn mainnet_facade_uses_mainnet_reward_schedule() {
    let dir = TestDir::new("chain_id_reward");
    let bc = create_test_blockchain_for_network(dir.path(), CHAIN_ID_MAINNET);

    let (addr, key) = generate_keypair();
    bc.with_state_cache_mut(|sc| sc.credit(&addr, 1_000_000));

    // Funded wallet tx so the mining loop has something to pack.
    let tx = signed_tx(&addr, "recipient", 0, 1, CHAIN_ID_MAINNET, &key);
    bc.apply_tx(tx).expect("tx accepted");

    let supply_before = bc.total_supply();
    mine_current(&bc);
    let supply_after = bc.total_supply();

    let mainnet_reward = strangecoin::economics::emission::block_reward_at_height_for_chain(
        1,
        supply_before,
        CHAIN_ID_MAINNET,
    );
    assert_eq!(
        supply_after - supply_before,
        mainnet_reward,
        "mainnet node must issue the mainnet coinbase schedule"
    );
    assert!(
        mainnet_reward > 0,
        "regression: hardcoded regtest reward (0) leaked into mainnet mode"
    );
}

#[test]
fn coinbase_above_mainnet_schedule_rejected() {
    let dir = TestDir::new("chain_id_overpay");
    let bc = create_test_blockchain_for_network(dir.path(), CHAIN_ID_MAINNET);
    let genesis = bc.tip().unwrap();

    let (addr, _key) = generate_keypair();
    let overpay = strangecoin::economics::emission::block_reward_at_height_for_chain(
        1,
        10_000,
        CHAIN_ID_MAINNET,
    ) + 1;

    let coinbase = Transaction {
        sender: "coinbase".to_string(),
        receiver: addr,
        amount: overpay,
        nonce: 0,
        chain_id: CHAIN_ID_MAINNET,
        signature: Vec::new(),
        is_coinbase: true,
    };
    let mut block = strangecoin::Block {
        index: 1,
        timestamp: genesis.timestamp + 1,
        transactions: vec![coinbase],
        previous_hash: genesis.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: genesis.target.clone(),
        consensus_version: strangecoin::consensus::CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));

    let err = bc
        .add_block(block)
        .expect_err("coinbase above the mainnet schedule must be rejected");
    assert!(
        matches!(err, StrangecoinError::InvalidCoinbaseAmount { .. }),
        "{err:?}"
    );
}
