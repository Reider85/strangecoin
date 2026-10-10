use strangecoin_core::error::CoreError;
use strangecoin_core::state::{apply_block, compute_state_root, root_after, State};
use strangecoin_core::types::{Block, Transaction};

use proptest::prelude::*;

fn coinbase(receiver: &str, amount: u64) -> Transaction {
    Transaction {
        sender: "coinbase".to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce: 0,
        chain_id: 3,
        signature: Vec::new(),
        is_coinbase: true,
    }
}

fn transfer(sender: &str, receiver: &str, amount: u64, nonce: u64) -> Transaction {
    Transaction {
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce,
        chain_id: 3,
        signature: Vec::new(),
        is_coinbase: false,
    }
}

fn genesis_block(txs: Vec<Transaction>) -> Block {
    Block {
        index: 0,
        timestamp: 0,
        transactions: txs,
        previous_hash: String::new(),
        hash: "genesis_hash".to_string(),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn transfer_block(index: u64, txs: Vec<Transaction>) -> Block {
    let mut all_txs = vec![coinbase("miner", 0)];
    all_txs.extend(txs);
    Block {
        index,
        timestamp: index * 600,
        transactions: all_txs,
        previous_hash: format!("prev_{}", index - 1),
        hash: format!("hash_{}", index),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

#[test]
fn empty_state_root_deterministic() {
    let state = State::new();
    let root = compute_state_root(&state.balances);
    let root2 = compute_state_root(&state.balances);
    assert_eq!(root, root2);
    assert_ne!(root, [0u8; 32]);
}

#[test]
fn genesis_changes_root() {
    let state = State::new();
    let root_before = compute_state_root(&state.balances);

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let new_state = apply_block(&state, &genesis).unwrap();
    let root_after_genesis = compute_state_root(&new_state.balances);

    assert_ne!(root_before, root_after_genesis);
}

#[test]
fn root_after_matches_computed() {
    let state = State::new();
    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)]);
    let root = root_after(&after_genesis, &block, true).unwrap();

    let new_state = apply_block(&after_genesis, &block).unwrap();
    let expected_root = compute_state_root(&new_state.balances);

    assert_eq!(root, expected_root);
}

#[test]
fn tamper_state_root_rejected() {
    let state = State::new();
    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();

    let mut block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)]);
    let correct_root = root_after(&after_genesis, &block, true).unwrap();

    block.state_root = correct_root;
    assert!(root_after(&after_genesis, &block, false).is_ok());

    let mut tampered = block.clone();
    tampered.state_root[0] ^= 0xff;
    let result = root_after(&after_genesis, &tampered, false);
    assert!(result.is_err());
}

#[test]
fn zero_state_root_rejected_without_opt_in() {
    // BUG-S1-002 / SCIP-0002: a non-genesis block without a state commitment
    // is rejected unless the caller explicitly opts in (legacy regtest).
    let state = State::new();
    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)]);
    assert_eq!(block.state_root, [0u8; 32], "fixture must be a zero-root block");

    let err = root_after(&after_genesis, &block, false)
        .expect_err("a zero state_root must be rejected when a commitment is required");
    assert!(
        matches!(err, CoreError::StateRootMismatch { .. }),
        "{err:?}"
    );

    // The same block is tolerated with the explicit opt-in.
    assert!(root_after(&after_genesis, &block, true).is_ok());
}

#[test]
fn genesis_zero_state_root_is_always_tolerated() {
    // Invariant #19 exempts genesis from the mandatory state commitment.
    let state = State::new();
    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    assert_eq!(genesis.state_root, [0u8; 32]);
    assert!(root_after(&state, &genesis, false).is_ok());
}

#[test]
fn chain_roots_consistent() {
    let state = State::new();
    let genesis = genesis_block(vec![coinbase("alice", 5000), coinbase("bob", 3000)]);
    let s0 = apply_block(&state, &genesis).unwrap();

    let b1 = transfer_block(1, vec![transfer("alice", "bob", 500, 1)]);
    let s1 = apply_block(&s0, &b1).unwrap();
    let r1 = root_after(&s0, &b1, true).unwrap();
    assert_eq!(r1, compute_state_root(&s1.balances));

    let b2 = transfer_block(2, vec![transfer("bob", "alice", 200, 1)]);
    let s2 = apply_block(&s1, &b2).unwrap();
    let r2 = root_after(&s1, &b2, true).unwrap();
    assert_eq!(r2, compute_state_root(&s2.balances));

    assert_ne!(r1, r2);
}

proptest! {
    #[test]
    fn proptest_root_consistent(
        sender_bal in 1000u64..1_000_000u64,
        amount in 1u64..500u64,
    ) {
        let state = State::new();
        let genesis = genesis_block(vec![coinbase("alice", sender_bal)]);
        let s0 = apply_block(&state, &genesis).unwrap();

        let txs = vec![transfer("alice", "bob", amount, 1)];
        let block = transfer_block(1, txs);

        let root = root_after(&s0, &block, true).unwrap();
        let new_state = apply_block(&s0, &block).unwrap();
        let expected = compute_state_root(&new_state.balances);

        prop_assert_eq!(root, expected);
    }

    #[test]
    fn proptest_deterministic_root(
        bal1 in 1000u64..100_000u64,
        bal2 in 1000u64..100_000u64,
    ) {
        let mut accounts1 = std::collections::HashMap::new();
        accounts1.insert("alice".to_string(), strangecoin_core::AccountState { balance: bal1, nonce: 0 });
        accounts1.insert("bob".to_string(), strangecoin_core::AccountState { balance: bal2, nonce: 0 });

        let mut accounts2 = std::collections::HashMap::new();
        accounts2.insert("bob".to_string(), strangecoin_core::AccountState { balance: bal2, nonce: 0 });
        accounts2.insert("alice".to_string(), strangecoin_core::AccountState { balance: bal1, nonce: 0 });

        prop_assert_eq!(
            compute_state_root(&accounts1),
            compute_state_root(&accounts2),
            "root must be independent of insertion order"
        );
    }
}
