mod common;

use common::*;
use std::sync::Arc;
use std::sync::RwLock;

#[test]
fn hundred_transactions_five_wallets() {
    let keypairs = generate_keypairs(5);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    let dirs: Vec<TestDir> = (0..5).map(|i| TestDir::new(&format!("wallet_{}", i))).collect();
    let mut wallets: Vec<Arc<RwLock<Blockchain>>> = Vec::new();
    for dir in &dirs {
        wallets.push(Arc::new(RwLock::new(create_test_blockchain(dir.path()))));
    }

    {
        let mut bc = wallets[0].write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&addrs[0]).unwrap());
    }
    sync_to_longest(&wallets);
    assert_balances(&wallets, &addrs, &[10000, 0, 0, 0, 0]);

    let mut ref_balances = [10000u64, 0, 0, 0, 0];
    let edges = [(0usize, 1usize), (1, 2), (2, 3), (3, 4)];

    for tx_index in 1..=100u64 {
        let pass = ((tx_index - 1) / 4 + 1) as usize;
        let edge = ((tx_index - 1) % 4) as usize;
        let (s, r) = edges[edge];
        let amount = amount_for(edge, pass);

        let sender_nonce = {
            let bc = wallets[s].read().unwrap();
            bc.balances.get(&addrs[s]).map(|a| a.nonce).unwrap_or(0)
        };

        let mut transaction = Transaction {
            sender: addrs[s].clone(),
            receiver: addrs[r].clone(),
            amount,
            nonce: sender_nonce + 1,
            chain_id: strangecoin::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: false,
        };

        sign_transaction(&mut transaction, &keypairs[s].1);

        {
            let mut bc = wallets[s].write().unwrap();
            assert!(
                bc.add_transaction(transaction).is_ok(),
                "Transaction {} rejected",
                tx_index
            );
            mine_current(&mut bc);
        }
        sync_to_longest(&wallets);

        ref_balances[s] -= amount;
        ref_balances[r] += amount;

        if tx_index % 10 == 0 {
            assert_balances(&wallets, &addrs, &ref_balances);
        }
    }

    assert_balances(&wallets, &addrs, &[0, 0, 0, 0, 10000]);

    for w in &wallets {
        let bc = w.read().unwrap();
        assert_eq!(bc.chain.len(), 102);
        assert!(bc.validate_chain());
    }
}
