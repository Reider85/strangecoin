mod common;

use common::*;
use std::sync::Arc;
use strangecoin::{Blockchain, BlockchainFacade};

#[test]
fn three_instances_receive_transfer() {
    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    let dirs: Vec<TestDir> = (0..3)
        .map(|i| TestDir::new(&format!("inst_{}", i)))
        .collect();
    let mut instances: Vec<Arc<BlockchainFacade>> = Vec::new();
    for dir in &dirs {
        instances.push(Arc::new(create_test_blockchain(dir.path())));
    }

    assert!(instances[0]
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());

    for i in 1..3 {
        assert!(
            adopt_from(&instances[i], &instances[0]),
            "Instance {} did not adopt chain",
            i + 1
        );
    }
    assert_balances(&instances, &addrs, &[10000, 0, 0]);

    for i in 1..3 {
        if !instances[i].has_account(&addrs[i])
            && !instances[i]
                .grant_initial_balance_to_first_wallet(&addrs[i])
                .unwrap_or(false)
        {
            instances[i].ensure_account(&addrs[i]);
        }
    }
    assert_balances(&instances, &addrs, &[10000, 0, 0]);

    let amount = 1000u64;
    create_and_mine_tx(&instances[0], &addrs[0], &addrs[1], amount, &keypairs[0].1);

    for i in 1..3 {
        assert!(
            adopt_from(&instances[i], &instances[0]),
            "Instance {} did not adopt chain with tx",
            i + 1
        );
    }

    let expected = [10000 - amount, amount, 0];
    assert_balances(&instances, &addrs, &expected);
}

#[test]
fn real_network_three_nodes() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let ports = [random_port(), random_port(), random_port()];
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let net_path = exe_dir.join("network.json");
    let saved_net = std::fs::read_to_string(&net_path).ok();
    let net_content = format!(
        r#"{{"peers":["127.0.0.1:{}","127.0.0.1:{}","127.0.0.1:{}"]}}"#,
        ports[0], ports[1], ports[2]
    );
    std::fs::write(&net_path, net_content).unwrap();
    let (sync_tx, _sync_rx) = std::sync::mpsc::channel::<Blockchain>();
    let dirs: Vec<TestDir> = (0..3)
        .map(|i| TestDir::new(&format!("net_{}", i)))
        .collect();
    let mut nodes: Vec<(strangecoin::Node, Arc<BlockchainFacade>)> = Vec::new();
    for (i, p) in ports.iter().enumerate() {
        let bc = Arc::new(create_test_blockchain(dirs[i].path()));
        let (mut node, _peers) = create_node_for_test(&bc, *p);
        node.start_server(*p, sync_tx.clone());
        nodes.push((node, bc));
    }

    for i in 0..3 {
        nodes[i].0.discover_peers();
    }

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
    assert!(
        nodes[0]
            .1
            .grant_initial_balance_to_first_wallet(&addrs[0])
            .unwrap(),
        "Grant not created"
    );

    for _ in 0..4 {
        for i in 0..3 {
            let mut sync_node = create_sync_node(
                &nodes[i].1,
                &nodes[i].0.peers,
                &nodes[i].0.address,
                &nodes[i].0.rate_limiter,
            );
            sync_node.sync_blockchain(sync_tx.clone());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    for i in 1..3 {
        if !nodes[i].1.has_account(&addrs[i]) {
            if !nodes[i]
                .1
                .grant_initial_balance_to_first_wallet(&addrs[i])
                .unwrap_or(false)
            {
                nodes[i].1.ensure_account(&addrs[i]);
            }
            nodes[i].1.save_state();
        }
    }

    create_and_mine_tx(&nodes[0].1, &addrs[0], &addrs[1], 1000, &keypairs[0].1);

    for _ in 0..6 {
        for i in 0..3 {
            let mut sync_node = create_sync_node(
                &nodes[i].1,
                &nodes[i].0.peers,
                &nodes[i].0.address,
                &nodes[i].0.rate_limiter,
            );
            sync_node.sync_blockchain(sync_tx.clone());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    let expected = [10000u64 - 1000, 1000, 0];
    for i in 0..3 {
        let bal = nodes[i].1.get_balance(&addrs[i]);
        assert_eq!(bal, expected[i], "Node {}: balance mismatch", i + 1);
        assert!(nodes[i].1.validate_chain(), "Node {}: chain invalid", i + 1);
    }

    if let Some(saved) = saved_net {
        std::fs::write(&net_path, saved).unwrap();
    } else {
        let _ = std::fs::remove_file(&net_path);
    }
}

#[test]
fn real_network_fast_registration_race() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let ports = [random_port(), random_port(), random_port()];
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let net_path = exe_dir.join("network.json");
    let saved_net = std::fs::read_to_string(&net_path).ok();
    let net_content = format!(
        r#"{{"peers":["127.0.0.1:{}","127.0.0.1:{}","127.0.0.1:{}"]}}"#,
        ports[0], ports[1], ports[2]
    );
    std::fs::write(&net_path, net_content).unwrap();
    let (sync_tx, _sync_rx) = std::sync::mpsc::channel::<Blockchain>();
    let dirs: Vec<TestDir> = (0..3)
        .map(|i| TestDir::new(&format!("fast_{}", i)))
        .collect();
    let mut nodes: Vec<(
        strangecoin::Node,
        Arc<BlockchainFacade>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    )> = Vec::new();
    for (i, p) in ports.iter().enumerate() {
        let bc = Arc::new(create_test_blockchain(dirs[i].path()));
        let (mut node, peers) = create_node_for_test(&bc, *p);
        node.start_server(*p, sync_tx.clone());
        nodes.push((node, bc, peers));
    }

    for i in 0..3 {
        nodes[i].0.discover_peers();
    }

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
    for i in 0..3 {
        if !nodes[i].1.has_account(&addrs[i]) {
            if !nodes[i]
                .1
                .grant_initial_balance_to_first_wallet(&addrs[i])
                .unwrap_or(false)
            {
                nodes[i].1.ensure_account(&addrs[i]);
            }
            nodes[i].1.save_state();
        }
    }

    for _ in 0..6 {
        for i in 0..3 {
            let mut sync_node = create_sync_node(
                &nodes[i].1,
                &nodes[i].2,
                &nodes[i].0.address,
                &nodes[i].0.rate_limiter,
            );
            sync_node.sync_blockchain(sync_tx.clone());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    // ChainSelector (work → timestamp → hash) converged all nodes onto one
    // chain during sync; the grant funds live on whichever address won the
    // fork choice, not necessarily addrs[0].
    let funded_idx = (0..3)
        .find(|&i| nodes[0].1.get_balance(&addrs[i]) >= 1000)
        .expect("grant funds should exist on the adopted chain");
    let dest_idx = (0..3)
        .find(|&i| i != funded_idx && nodes[0].1.get_balance(&addrs[i]) == 0)
        .expect("at least one address should have zero balance");

    create_and_mine_tx(
        &nodes[0].1,
        &addrs[funded_idx],
        &addrs[dest_idx],
        1000,
        &keypairs[funded_idx].1,
    );

    for _ in 0..6 {
        for i in 0..3 {
            let mut sync_node = create_sync_node(
                &nodes[i].1,
                &nodes[i].2,
                &nodes[i].0.address,
                &nodes[i].0.rate_limiter,
            );
            sync_node.sync_blockchain(sync_tx.clone());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    for i in 0..3 {
        assert!(nodes[i].1.validate_chain(), "Node {}: chain invalid", i + 1);
        let expected = if i == funded_idx {
            10000 - 1000
        } else if i == dest_idx {
            1000
        } else {
            0
        };
        let bal = nodes[i].1.get_balance(&addrs[i]);
        assert_eq!(bal, expected, "Node {}: balance mismatch", i + 1);
    }

    if let Some(saved) = saved_net {
        std::fs::write(&net_path, saved).unwrap();
    } else {
        let _ = std::fs::remove_file(&net_path);
    }
}
