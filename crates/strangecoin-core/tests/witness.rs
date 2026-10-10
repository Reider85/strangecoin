use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::state::witness::{build_witness, verify_block_stateless};
use strangecoin_core::state::{apply_block, State};
use strangecoin_core::types::{AccountState, Block, Transaction};
use strangecoin_core::CoreError;

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

fn transfer_block(index: u64, txs: Vec<Transaction>, state: &State) -> Block {
    let mut all_txs = vec![coinbase("miner", 0)];
    all_txs.extend(txs);
    let mut block = Block {
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
    };
    let post = apply_block(state, &block).unwrap();
    block.state_root = SparseMerkleTrie::compute_root(&post.balances);
    block
}

#[test]
fn build_witness_then_verify() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 1000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let mut genesis_block = genesis;
    genesis_block.state_root = genesis_root;

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let witness = build_witness(&after_genesis, &block).unwrap();

    let result = verify_block_stateless(&genesis_root, &block, &witness);
    assert!(result.is_ok());
}

#[test]
fn tampered_balance_rejected() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 1000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let mut genesis_block = genesis;
    genesis_block.state_root = genesis_root;

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let mut witness = build_witness(&after_genesis, &block).unwrap();

    if let Some(alice_proof) = witness.proofs.get_mut("alice") {
        alice_proof.balance = 999999;
    }

    let result = verify_block_stateless(&genesis_root, &block, &witness);
    assert!(result.is_err());
}

#[test]
fn witness_contains_only_touched_addresses() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 5000,
            nonce: 0,
        },
    );
    state.balances.insert(
        "charlie".to_string(),
        AccountState {
            balance: 3000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 5000), coinbase("charlie", 3000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let mut genesis_block = genesis;
    genesis_block.state_root = genesis_root;

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let witness = build_witness(&after_genesis, &block).unwrap();

    assert!(witness.proofs.contains_key("alice"));
    assert!(witness.proofs.contains_key("bob"));
    assert!(witness.proofs.contains_key("miner"));
    assert!(!witness.proofs.contains_key("charlie"));
    assert_eq!(witness.proofs.len(), 3);
}

#[test]
fn coinbase_only_block_witness() {
    let state = State::new();

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let mut genesis_block = genesis;
    genesis_block.state_root = genesis_root;

    let block = transfer_block(1, vec![], &after_genesis);
    let witness = build_witness(&after_genesis, &block).unwrap();

    assert!(witness.proofs.contains_key("miner"));
    assert_eq!(witness.proofs.len(), 1);

    let result = verify_block_stateless(&genesis_root, &block, &witness);
    assert!(result.is_ok());
}

#[test]
fn wrong_parent_root_rejected() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 1000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let mut genesis_block = genesis;
    genesis_block.state_root = genesis_root;

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let witness = build_witness(&after_genesis, &block).unwrap();

    let wrong_root = [0xff; 32];
    let result = verify_block_stateless(&wrong_root, &block, &witness);
    assert!(result.is_err());
}

#[test]
fn chain_of_blocks_witness() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 10000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 10000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let mut genesis_block = genesis;
    genesis_block.state_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let block1 = transfer_block(1, vec![transfer("alice", "bob", 500, 1)], &after_genesis);
    let witness1 = build_witness(&after_genesis, &block1).unwrap();
    verify_block_stateless(&genesis_block.state_root, &block1, &witness1).unwrap();

    let after_block1 = apply_block(&after_genesis, &block1).unwrap();
    let block2 = transfer_block(2, vec![transfer("bob", "charlie", 200, 1)], &after_block1);
    let witness2 = build_witness(&after_block1, &block2).unwrap();
    verify_block_stateless(&block1.state_root, &block2, &witness2).unwrap();
}

proptest! {
    #[test]
    fn proptest_witness_roundtrip(
        sender_bal in 1000u64..1_000_000u64,
        amount in 1u64..500u64,
    ) {
        let mut state = State::new();
        state.balances.insert(
            "alice".to_string(),
            AccountState { balance: sender_bal, nonce: 0 },
        );

        let genesis = genesis_block(vec![coinbase("alice", sender_bal)]);
        let after_genesis = apply_block(&state, &genesis).unwrap();
        let mut genesis_block = genesis;
        genesis_block.state_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

        let block = transfer_block(1, vec![transfer("alice", "bob", amount, 1)], &after_genesis);
        let witness = build_witness(&after_genesis, &block).unwrap();

        let result = verify_block_stateless(&genesis_block.state_root, &block, &witness);
        prop_assert!(result.is_ok(), "witness verification failed: {:?}", result.err());
    }

    #[test]
    fn proptest_tamper_detected(
        sender_bal in 1000u64..1_000_000u64,
        amount in 1u64..500u64,
    ) {
        let mut state = State::new();
        state.balances.insert(
            "alice".to_string(),
            AccountState { balance: sender_bal, nonce: 0 },
        );

        let genesis = genesis_block(vec![coinbase("alice", sender_bal)]);
        let after_genesis = apply_block(&state, &genesis).unwrap();
        let mut genesis_block = genesis;
        genesis_block.state_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

        let block = transfer_block(1, vec![transfer("alice", "bob", amount, 1)], &after_genesis);
        let mut witness = build_witness(&after_genesis, &block).unwrap();

        if let Some(proof) = witness.proofs.get_mut("alice") {
            proof.balance ^= 1;
        }

        let result = verify_block_stateless(&genesis_block.state_root, &block, &witness);
        prop_assert!(result.is_err(), "tampered witness should be rejected");
    }
}

/// BUG-S1-003: the post-root recompute must reject an arbitrary commitment.
#[test]
fn forged_state_root_rejected() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 1000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let witness = build_witness(&after_genesis, &block).unwrap();

    let mut forged = block;
    forged.state_root = [0xaa; 32];
    let result = verify_block_stateless(&genesis_root, &forged, &witness);
    assert!(matches!(
        result,
        Err(CoreError::PostStateRootMismatch { .. })
    ));
}

/// BUG-S1-003: a post-commitment that also tampers an untouched account is
/// rejected — the recompute is anchored to the parent root, so an untouched
/// leaf cannot diverge (the residual Option A would leave open).
#[test]
fn untouched_account_tamper_in_post_commitment_rejected() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 5000,
            nonce: 0,
        },
    );
    state.balances.insert(
        "charlie".to_string(),
        AccountState {
            balance: 3000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 5000), coinbase("charlie", 3000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let witness = build_witness(&after_genesis, &block).unwrap();

    let mut fake_post = apply_block(&after_genesis, &block).unwrap();
    fake_post.balances.insert(
        "charlie".to_string(),
        AccountState {
            balance: 999_999,
            nonce: 0,
        },
    );
    let mut forged = block;
    forged.state_root = SparseMerkleTrie::compute_root(&fake_post.balances);

    let result = verify_block_stateless(&genesis_root, &forged, &witness);
    assert!(matches!(
        result,
        Err(CoreError::PostStateRootMismatch { .. })
    ));
}

/// BUG-S1-003 / SCIP-0002: a non-genesis zero state_root can never match the
/// recomputed root and is rejected by the stateless verifier.
#[test]
fn zero_state_root_on_non_genesis_rejected() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 1000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let witness = build_witness(&after_genesis, &block).unwrap();

    let mut zeroed = block;
    zeroed.state_root = [0u8; 32];
    let result = verify_block_stateless(&genesis_root, &zeroed, &witness);
    assert!(matches!(
        result,
        Err(CoreError::PostStateRootMismatch { .. })
    ));
}

/// BUG-S1-003: a witness missing a touched address is rejected before the
/// partial apply could default the account to (0, 0).
#[test]
fn missing_touched_address_rejected() {
    let mut state = State::new();
    state.balances.insert(
        "alice".to_string(),
        AccountState {
            balance: 1000,
            nonce: 0,
        },
    );

    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let after_genesis = apply_block(&state, &genesis).unwrap();
    let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

    let block = transfer_block(1, vec![transfer("alice", "bob", 100, 1)], &after_genesis);
    let mut witness = build_witness(&after_genesis, &block).unwrap();
    witness.proofs.remove("bob");

    let result = verify_block_stateless(&genesis_root, &block, &witness);
    assert!(matches!(result, Err(CoreError::WitnessVerificationFailed)));
}

/// BUG-S1-003: genesis keeps the invariant #19 exemption — a zero state_root
/// on index 0 is tolerated (same rule as `root_after` / SCIP-0002).
#[test]
fn genesis_zero_state_root_is_tolerated() {
    let state = State::new();
    let genesis = genesis_block(vec![coinbase("alice", 1000)]);
    let empty_root = SparseMerkleTrie::compute_root(&state.balances);
    let witness = build_witness(&state, &genesis).unwrap();

    let result = verify_block_stateless(&empty_root, &genesis, &witness);
    assert!(result.is_ok());

    let mut forged = genesis;
    forged.state_root = [0xbb; 32];
    let result = verify_block_stateless(&empty_root, &forged, &witness);
    assert!(matches!(
        result,
        Err(CoreError::PostStateRootMismatch { .. })
    ));
}

proptest! {
    /// BUG-S1-003: any bit-flip of the committed post-root is rejected.
    #[test]
    fn proptest_forged_state_root_rejected(
        sender_bal in 1000u64..1_000_000u64,
        amount in 1u64..500u64,
        flip in 0usize..32,
    ) {
        let mut state = State::new();
        state.balances.insert(
            "alice".to_string(),
            AccountState { balance: sender_bal, nonce: 0 },
        );

        let genesis = genesis_block(vec![coinbase("alice", sender_bal)]);
        let after_genesis = apply_block(&state, &genesis).unwrap();
        let genesis_root = SparseMerkleTrie::compute_root(&after_genesis.balances);

        let block = transfer_block(1, vec![transfer("alice", "bob", amount, 1)], &after_genesis);
        let witness = build_witness(&after_genesis, &block).unwrap();

        let mut forged = block;
        forged.state_root[flip] ^= 0xff;
        let result = verify_block_stateless(&genesis_root, &forged, &witness);
        prop_assert!(
            matches!(result, Err(CoreError::PostStateRootMismatch { .. })),
            "forged post-root must be rejected, got {:?}",
            result
        );
    }
}
