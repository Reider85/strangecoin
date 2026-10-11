mod common;

use common::*;
use std::sync::Arc;
use strangecoin::Transaction;

#[test]
fn double_spend_rejected() {
    let _dir = TestDir::new("double_spend");
    let bc = Arc::new(create_test_blockchain(_dir.path()));
    let keypairs = generate_keypairs(2);
    let sender = keypairs[0].0.clone();
    let receiver1 = keypairs[1].0.clone();
    let receiver2 = "some_other_address".to_string();

    assert!(bc.grant_initial_balance_to_first_wallet(&sender).unwrap());

    let balance_before = bc.get_balance(&sender);
    assert_eq!(balance_before, 10000);

    let tx_amount = 5000u64;
    let mut tx1 = Transaction {
        sender: sender.clone(),
        receiver: receiver1.clone(),
        amount: tx_amount,
        nonce: 1,
        chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut tx1, &keypairs[0].1);

    let mut tx2 = Transaction {
        sender: sender.clone(),
        receiver: receiver2,
        amount: tx_amount,
        nonce: 1,
        chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut tx2, &keypairs[0].1);

    let result1 = bc.apply_tx(tx1);
    assert!(result1.is_ok(), "First transaction should be accepted");

    let result2 = bc.apply_tx(tx2);
    assert!(result2.is_err(), "Double spend should be rejected");

    mine_current(&bc);

    let balance_after = bc.get_balance(&sender);
    assert_eq!(
        balance_after,
        balance_before - tx_amount,
        "Sender balance should be deducted exactly once"
    );
}
