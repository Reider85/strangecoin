//! S3-P11: stateless light-client e2e — node B verifies a block **without
//! holding the full state**: it receives only `(parent_state_root, block,
//! witness bytes)` and runs `verify_block_stateless`.
//!
//! DoD row 2 (`prompt-stage3.md`): «Stateless validation работает (light
//! client верифицирует block без полного state)».
//!
//! S3-P11 deviations (FBP, fixed in the commit): witness-over-SMT — the
//! Verkle replacement is consensus-breaking and SCIP-gated (ADR-0006);
//! no `strangecoin-net` wire integration — carrying witnesses over gossip
//! is Stage 4+ residual, the wire form here is the in-process codec.

mod common;

use common::*;
use strangecoin::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::witness::{
    deserialize_state_witness, serialize_state_witness, verify_block_stateless,
};
use strangecoin_core::CoreError;

/// Node A: grant block + two transfer blocks — a chain whose tip state
/// contains accounts that exist only from height ≥ 1.
fn mined_chain(tag: &str) -> (strangecoin::BlockchainFacade, TestDir, String, String) {
    let dir = TestDir::new(tag);
    let bc = create_test_blockchain(dir.path());
    let (wallet, wallet_sk) = generate_keypair();
    bc.grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests");
    let (alice, _) = generate_keypair();
    create_and_mine_tx(&bc, &wallet, &alice, 7, &wallet_sk);
    create_and_mine_tx(&bc, &wallet, &alice, 3, &wallet_sk);
    (bc, dir, wallet, alice)
}

/// The last block carrying a real (non-coinbase) transfer, its parent's
/// `state_root` (the pre-state root a light client anchors at) and the
/// block itself.
fn pick_transfer_block(chain: &[strangecoin::Block]) -> (usize, strangecoin::Block, [u8; 32]) {
    let height = chain
        .iter()
        .rposition(|b| b.transactions.iter().any(|tx| !tx.is_coinbase()))
        .expect("chain must contain a transfer block");
    assert!(height > 0, "transfer block must have a parent");
    let block = chain[height].clone();
    let parent_root = chain[height - 1].state_root;
    (height, block, parent_root)
}

/// Positive path: A builds the witness, B (fresh temp dir, genesis-only
/// state — it never held the touched accounts) decodes the wire bytes and
/// verifies the block against the parent root alone.
#[test]
fn light_client_verifies_block_without_full_state() {
    let (a, _dir_a, _wallet, alice) = mined_chain("stateless_light_a");
    let chain = a.chain_snapshot();
    let (height, block, parent_root) = pick_transfer_block(&chain);

    // Control: A holds the touched account, B must not.
    assert!(a.has_account(&alice), "node A holds the touched account");
    let dir_b = TestDir::new("stateless_light_b");
    let b = create_test_blockchain(dir_b.path());
    assert!(
        !b.has_account(&alice),
        "node B must not hold the touched account"
    );

    // A → wire → B: canonical codec round-trip in between.
    let witness = a
        .build_witness_for_block(height as u64)
        .expect("witness must build");
    let wire = serialize_state_witness(&witness);
    let decoded = deserialize_state_witness(&wire).expect("wire decode must succeed");
    assert_eq!(decoded, witness, "wire round-trip must be lossless");

    // Light verification: parent root + block + witness, no state needed.
    verify_block_stateless(&parent_root, &block, CHAIN_ID_REGTEST, &decoded)
        .expect("light client must verify the block");

    // Node-side API on A (the parent lives in A's chain).
    a.verify_incoming_block_with_witness(&block, &decoded)
        .expect("facade stateless verify must succeed");

    // B cannot use the facade path — it lacks the parent block — which is
    // exactly why the light path anchors at a verified parent root.
    assert!(b
        .verify_incoming_block_with_witness(&block, &decoded)
        .is_err());
}

/// Negative: one flipped byte inside a proof hash — the structure still
/// decodes, but the Merkle proof no longer anchors at the parent root.
#[test]
fn corrupted_witness_proof_byte_is_rejected() {
    let (a, _dir_a, _wallet, _alice) = mined_chain("stateless_corrupt");
    let chain = a.chain_snapshot();
    let (height, block, parent_root) = pick_transfer_block(&chain);

    let witness = a
        .build_witness_for_block(height as u64)
        .expect("witness must build");
    let mut wire = serialize_state_witness(&witness);
    // The encoding ends with the last proof hash of the last (sorted)
    // address: flipping it corrupts proof content, not framing.
    let last = wire.len() - 1;
    wire[last] ^= 0xff;
    let decoded = deserialize_state_witness(&wire).expect("structure remains valid");

    let result = verify_block_stateless(&parent_root, &block, CHAIN_ID_REGTEST, &decoded);
    assert!(
        matches!(result, Err(CoreError::WitnessVerificationFailed)),
        "corrupt proof must fail with WitnessVerificationFailed, got {:?}",
        result
    );
    assert!(a
        .verify_incoming_block_with_witness(&block, &decoded)
        .is_err());
}

/// Negative: a witness stripped of a touched address must be rejected
/// before the partial apply could default the account to (0, 0).
#[test]
fn witness_missing_touched_address_is_rejected() {
    let (a, _dir_a, _wallet, alice) = mined_chain("stateless_stripped");
    let chain = a.chain_snapshot();
    let (height, block, parent_root) = pick_transfer_block(&chain);

    let mut witness = a
        .build_witness_for_block(height as u64)
        .expect("witness must build");
    assert!(
        witness.proofs.contains_key(&alice),
        "fixture: alice must be a touched address"
    );
    witness.proofs.remove(&alice);

    let decoded = deserialize_state_witness(&serialize_state_witness(&witness))
        .expect("stripped witness still decodes");
    let result = verify_block_stateless(&parent_root, &block, CHAIN_ID_REGTEST, &decoded);
    assert!(
        matches!(result, Err(CoreError::WitnessVerificationFailed)),
        "missing touched address must fail with WitnessVerificationFailed, got {:?}",
        result
    );
}
