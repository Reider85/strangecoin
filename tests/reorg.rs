mod common;

use common::*;
use std::sync::{Arc, RwLock};

#[test]
fn no_rollback_on_shorter_chain() {
    let dir1 = temp_db_dir("norb_1");
    let dir2 = temp_db_dir("norb_2");
    let bc1 = Arc::new(RwLock::new(create_test_blockchain(&dir1)));
    let bc2 = Arc::new(RwLock::new(create_test_blockchain(&dir2)));
    let keypairs = generate_keypairs(2);
    let a1 = keypairs[0].0.clone();
    let a2 = keypairs[1].0.clone();

    {
        let mut bc = bc1.write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&a1));
        create_and_mine_tx(&mut bc, &a1, &a2, 1000, &keypairs[0].1);
    }
    {
        let mut bc = bc2.write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&a2));
    }

    assert!(
        !adopt_from(&bc1, &bc2),
        "Node rolled back to shorter chain"
    );
    let bc1_guard = bc1.read().unwrap();
    assert_eq!(bc1_guard.chain.len(), 3, "Node lost mined block");
    let received = bc1_guard.balances.get(&a2).map(|a| a.balance).unwrap_or(0);
    assert_eq!(received, 1000, "Recipient balance changed on rollback rejection");
    drop(bc1_guard);

    let _ = std::fs::remove_dir_all(&dir1);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn full_reorg_longer_chain() {
    let dir_a = temp_db_dir("reorg_a");
    let dir_b = temp_db_dir("reorg_b");

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    let bc_a = Arc::new(RwLock::new(create_test_blockchain(&dir_a)));
    let bc_b = Arc::new(RwLock::new(create_test_blockchain(&dir_b)));

    {
        let mut bc = bc_a.write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&addrs[0]));
        create_and_mine_tx(&mut bc, &addrs[0], &addrs[1], 2000, &keypairs[0].1);
        create_and_mine_tx(&mut bc, &addrs[0], &addrs[2], 1000, &keypairs[0].1);
    }
    assert_eq!(bc_a.read().unwrap().chain.len(), 4);

    {
        let mut bc = bc_b.write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&addrs[0]));
        create_and_mine_tx(&mut bc, &addrs[0], &addrs[1], 500, &keypairs[0].1);
        create_and_mine_tx(&mut bc, &addrs[0], &addrs[2], 300, &keypairs[0].1);
        create_and_mine_tx(&mut bc, &addrs[1], &addrs[2], 100, &keypairs[1].1);
    }
    assert_eq!(bc_b.read().unwrap().chain.len(), 5);

    assert!(
        adopt_from(&bc_a, &bc_b),
        "Chain A did not adopt longer chain B"
    );
    assert_eq!(bc_a.read().unwrap().chain.len(), 5);

    let balances_a = bc_a.read().unwrap().balances.clone();
    let balances_b = bc_b.read().unwrap().balances.clone();
    for (addr, expected) in &balances_b {
        let a_bal = balances_a.get(addr).map(|a| a.balance).unwrap_or(0);
        assert_eq!(
            a_bal, expected.balance,
            "Balance mismatch after reorg for {}",
            addr
        );
    }

    assert!(bc_a.read().unwrap().validate_chain(), "Chain A invalid after reorg");

    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}
