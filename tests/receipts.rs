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
    assert!(grant_receipts
        .iter()
        .all(|r| r.block_height == 1));
    assert_ne!(
        genesis_receipts.len(),
        grant_receipts.len(),
        "new block replaces the tip receipts"
    );
}
