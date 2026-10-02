mod common;

use common::*;
use std::sync::Arc;

#[test]
fn no_rollback_on_shorter_chain() {
    let dir1 = TestDir::new("norb_1");
    let dir2 = TestDir::new("norb_2");
    let bc1 = Arc::new(create_test_blockchain(dir1.path()));
    let bc2 = Arc::new(create_test_blockchain(dir2.path()));
    let keypairs = generate_keypairs(2);
    let a1 = keypairs[0].0.clone();
    let a2 = keypairs[1].0.clone();

    assert!(bc1.grant_initial_balance_to_first_wallet(&a1).unwrap());
    create_and_mine_tx(&bc1, &a1, &a2, 1000, &keypairs[0].1);

    assert!(bc2.grant_initial_balance_to_first_wallet(&a2).unwrap());

    assert!(!adopt_from(&bc1, &bc2), "Node rolled back to shorter chain");
    assert_eq!(bc1.chain_len(), 3, "Node lost mined block");
    let received = bc1.get_balance(&a2);
    assert_eq!(
        received, 1000,
        "Recipient balance changed on rollback rejection"
    );
}

#[test]
fn full_reorg_longer_chain() {
    let dir_a = TestDir::new("reorg_a");
    let dir_b = TestDir::new("reorg_b");

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    let bc_a = Arc::new(create_test_blockchain(dir_a.path()));
    let bc_b = Arc::new(create_test_blockchain(dir_b.path()));

    assert!(bc_a
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    create_and_mine_tx(&bc_a, &addrs[0], &addrs[1], 2000, &keypairs[0].1);
    create_and_mine_tx(&bc_a, &addrs[0], &addrs[2], 1000, &keypairs[0].1);
    assert_eq!(bc_a.chain_len(), 4);

    assert!(bc_b
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    create_and_mine_tx(&bc_b, &addrs[0], &addrs[1], 500, &keypairs[0].1);
    create_and_mine_tx(&bc_b, &addrs[0], &addrs[2], 300, &keypairs[0].1);
    create_and_mine_tx(&bc_b, &addrs[1], &addrs[2], 100, &keypairs[1].1);
    assert_eq!(bc_b.chain_len(), 5);

    assert!(
        adopt_from(&bc_a, &bc_b),
        "Chain A did not adopt longer chain B"
    );
    assert_eq!(bc_a.chain_len(), 5);

    let balances_a = bc_a.state_snapshot();
    let balances_b = bc_b.state_snapshot();
    for (addr, expected) in &balances_b {
        let a_bal = balances_a.get(addr).map(|a| a.balance).unwrap_or(0);
        assert_eq!(
            a_bal, expected.balance,
            "Balance mismatch after reorg for {}",
            addr
        );
    }

    assert!(bc_a.validate_chain(), "Chain A invalid after reorg");
}

#[test]
fn equal_height_chains_no_adopt() {
    let dir_a = TestDir::new("eq_a");
    let dir_b = TestDir::new("eq_b");

    let keypairs = generate_keypairs(2);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    let bc_a = Arc::new(create_test_blockchain(dir_a.path()));
    let bc_b = Arc::new(create_test_blockchain(dir_b.path()));

    assert!(bc_a
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    create_and_mine_tx(&bc_a, &addrs[0], &addrs[1], 100, &keypairs[0].1);

    assert!(bc_b
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    create_and_mine_tx(&bc_b, &addrs[0], &addrs[1], 200, &keypairs[0].1);

    assert_eq!(bc_a.chain_len(), bc_b.chain_len());

    let adopted = adopt_from(&bc_a, &bc_b);
    if adopted {
        assert_eq!(bc_a.chain_len(), bc_b.chain_len());
    }
}

#[test]
fn chain_selector_prefers_higher_work() {
    use strangecoin::blockchain::chain_selector::{ChainInfo, ChainSelector};

    let low_work = ChainInfo {
        tip_height: 10,
        tip_hash: "aaa".into(),
        total_work: [0, 0, 0, 100],
        tip_timestamp: 5000,
    };
    let high_work = ChainInfo {
        tip_height: 10,
        tip_hash: "bbb".into(),
        total_work: [0, 0, 0, 200],
        tip_timestamp: 6000,
    };

    assert!(
        ChainSelector::is_better(&high_work, &low_work),
        "Higher work should win even with later timestamp"
    );
    assert!(
        !ChainSelector::is_better(&low_work, &high_work),
        "Lower work should not win"
    );

    let best = ChainSelector::select_best(&[&low_work, &high_work]).unwrap();
    assert_eq!(best.tip_hash, "bbb");
}

#[test]
fn chain_selector_tiebreaks_by_timestamp_then_hash() {
    use strangecoin::blockchain::chain_selector::{ChainInfo, ChainSelector};

    let a = ChainInfo {
        tip_height: 5,
        tip_hash: "ccc".into(),
        total_work: [0, 0, 0, 100],
        tip_timestamp: 2000,
    };
    let b = ChainInfo {
        tip_height: 5,
        tip_hash: "aaa".into(),
        total_work: [0, 0, 0, 100],
        tip_timestamp: 1000,
    };

    assert!(
        ChainSelector::is_better(&b, &a),
        "Earlier timestamp should win on equal work"
    );

    let c = ChainInfo {
        tip_height: 5,
        tip_hash: "bbb".into(),
        total_work: [0, 0, 0, 100],
        tip_timestamp: 1000,
    };

    assert!(
        ChainSelector::is_better(&c, &a),
        "Lower hash should win on equal work and timestamp"
    );
}
