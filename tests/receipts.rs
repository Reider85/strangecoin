//! Node-level receipts collection (S1.5-P14, invariant #18): `commit_block`
//! lands the tip block's receipts on the facade; adoption/rebuild recompute
//! them deterministically from the chain.

mod common;

use common::*;
use strangecoin::Transaction;
use strangecoin_core::vm::TxStatus;

#[test]
fn genesis_commit_produces_receipts() {
    let _dir = TestDir::new("receipts_genesis");
    let bc = create_test_blockchain(_dir.path());

    let receipts = bc.tip_receipts();
    assert_eq!(receipts.len(), 1, "genesis coinbase produces one receipt");
    assert_eq!(receipts[0].status, TxStatus::Success);
    assert_eq!(receipts[0].gas_used, 0);
    assert_eq!(receipts[0].block_height, 0);
}

#[test]
fn mined_transfer_block_collects_receipts() {
    let _dir = TestDir::new("receipts_mine");
    let bc = create_test_blockchain(_dir.path());

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    let mut tx = Transaction::transfer(
        &addr,
        "recipient",
        50,
        1,
        strangecoin::consensus::CHAIN_ID_REGTEST,
    );
    sign_transaction(&mut tx, &keypairs[0].1);
    let expected_txid = strangecoin::serialize::txid(&tx);
    bc.apply_tx(tx).expect("tx must enter the mempool");
    mine_current(&bc);

    let receipts = bc.tip_receipts();
    let transfer = receipts
        .iter()
        .find(|r| r.txid == expected_txid)
        .expect("transfer receipt must be collected");
    assert_eq!(transfer.status, TxStatus::Success);
    assert_eq!(transfer.gas_used, 0);
    assert_eq!(transfer.block_height, 2);
}

#[test]
fn rebuild_state_cache_recomputes_receipts() {
    let _dir = TestDir::new("receipts_rebuild");
    let bc = create_test_blockchain(_dir.path());

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    let mut tx = Transaction::transfer(
        &addr,
        "recipient",
        25,
        1,
        strangecoin::consensus::CHAIN_ID_REGTEST,
    );
    sign_transaction(&mut tx, &keypairs[0].1);
    bc.apply_tx(tx).expect("tx must enter the mempool");
    mine_current(&bc);

    let before = bc.tip_receipts();
    assert!(!before.is_empty());

    bc.rebuild_state_cache().expect("rebuild must succeed");
    let after = bc.tip_receipts();
    assert_eq!(before, after, "receipts must be recomputed identically");
}

#[test]
fn receipts_are_replaced_not_accumulated() {
    let _dir = TestDir::new("receipts_replace");
    let bc = create_test_blockchain(_dir.path());

    let genesis_receipts = bc.tip_receipts();
    assert_eq!(genesis_receipts.len(), 1);

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    let grant_receipts = bc.tip_receipts();
    assert!(
        grant_receipts.len() <= 2,
        "tip receipts belong to the tip block only, got {}",
        grant_receipts.len()
    );
    assert!(grant_receipts.iter().all(|r| r.block_height == 1));
    assert_ne!(
        genesis_receipts.len(),
        grant_receipts.len(),
        "new block replaces the tip receipts"
    );
}

/// S1.5-FBP07 / ADR-0016: the mined header commits to the receipts SMT root,
/// and a light client verifies a receipt proof against that header root —
/// no full state, no re-execution.
#[test]
fn light_client_verifies_receipt_proof_against_header_root() {
    let _dir = TestDir::new("receipts_light_client");
    let bc = create_test_blockchain(_dir.path());

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());

    let mut tx = Transaction::transfer(
        &addr,
        "recipient",
        50,
        1,
        strangecoin::consensus::CHAIN_ID_REGTEST,
    );
    sign_transaction(&mut tx, &keypairs[0].1);
    let expected_txid = strangecoin::serialize::txid(&tx);
    bc.apply_tx(tx).expect("tx must enter the mempool");
    mine_current(&bc);

    // The header commitment equals the recomputed receipts subroot and is
    // non-zero for a non-genesis block.
    let tip = bc.tip().expect("chain must have a tip");
    let receipts = bc.tip_receipts();
    let expected_root = strangecoin_core::state::receipts_root(&receipts);
    assert_ne!(tip.receipts_root, [0u8; 32], "mined header must commit");
    assert_eq!(
        tip.receipts_root, expected_root,
        "header receipts_root must match the applied receipts"
    );

    // Light-client flow: header root + receipt + 256-sibling proof. The
    // verifier touches no State — only the three inputs a peer would send.
    let receipt = receipts
        .iter()
        .find(|r| r.txid == expected_txid)
        .expect("transfer receipt must be collected");
    let proof = strangecoin_core::state::prove_receipt(&receipts, &expected_txid)
        .expect("proof for a tip receipt must exist");
    assert!(
        strangecoin_core::state::verify_receipt_proof(&tip.receipts_root, receipt, &proof),
        "proof must verify against the header-committed receipts root"
    );

    // Negative: a tampered receipt (same txid, different gas) fails.
    let mut tampered = receipt.clone();
    tampered.gas_used += 1;
    assert!(
        !strangecoin_core::state::verify_receipt_proof(&tip.receipts_root, &tampered, &proof),
        "a tampered receipt must not verify"
    );
}

/// S1.5-FBP07: the genesis header may keep the zero receipts root (build-time
/// constant, ADR-0016) — mined blocks past genesis may not.
#[test]
fn genesis_zero_receipts_root_is_tolerated_but_mined_blocks_commit() {
    let _dir = TestDir::new("receipts_genesis_zero");
    let bc = create_test_blockchain(_dir.path());

    let genesis = bc.tip().expect("genesis must exist");
    assert_eq!(genesis.index, 0);
    assert_eq!(
        genesis.receipts_root, [0u8; 32],
        "genesis header keeps the zero receipts root"
    );

    let keypairs = generate_keypairs(1);
    let addr = keypairs[0].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&addr).unwrap());
    let grant = bc.tip().expect("grant block must exist");
    assert_eq!(grant.index, 1);
    assert_ne!(
        grant.receipts_root, [0u8; 32],
        "grant block must commit to its receipts root"
    );
}
