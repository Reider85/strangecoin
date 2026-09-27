mod common;

use common::*;
use std::sync::{Arc, RwLock};

#[test]
fn double_spend_rejected() {
    let dir = temp_db_dir("double_spend");
    let bc = Arc::new(RwLock::new(create_test_blockchain(&dir)));
    let keypairs = generate_keypairs(2);
    let sender = keypairs[0].0.clone();
    let receiver1 = keypairs[1].0.clone();
    let receiver2 = "some_other_address".to_string();

    {
        let mut bc = bc.write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&sender));
    }

    let balance_before = {
        let bc = bc.read().unwrap();
        bc.balances.get(&sender).map(|a| a.balance).unwrap_or(0)
    };
    assert_eq!(balance_before, 10000);

    let tx_amount = 5000u64;
    let mut tx1 = Transaction {
        sender: sender.clone(),
        receiver: receiver1.clone(),
        amount: tx_amount,
        nonce: 1,
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut tx1, &keypairs[0].1);

    let mut tx2 = Transaction {
        sender: sender.clone(),
        receiver: receiver2,
        amount: tx_amount,
        nonce: 1,
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut tx2, &keypairs[0].1);

    {
        let mut bc = bc.write().unwrap();
        let result1 = bc.add_transaction(tx1);
        assert!(result1.is_ok(), "First transaction should be accepted");

        let result2 = bc.add_transaction(tx2);
        assert!(result2.is_err(), "Double spend should be rejected");
    }

    {
        let mut bc = bc.write().unwrap();
        mine_current(&mut bc);
    }

    let balance_after = {
        let bc = bc.read().unwrap();
        bc.balances.get(&sender).map(|a| a.balance).unwrap_or(0)
    };
    assert_eq!(
        balance_after,
        balance_before - tx_amount,
        "Sender balance should be deducted exactly once"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
