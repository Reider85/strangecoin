mod common;

use common::*;
use std::sync::{Arc, RwLock};

#[test]
fn three_instances_receive_transfer() {
    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    let mut dirs: Vec<String> = Vec::new();
    let mut instances: Vec<Arc<RwLock<Blockchain>>> = Vec::new();
    for i in 0..3 {
        let dir = temp_db_dir(&format!("inst_{}", i));
        let dir_str = dir.to_string_lossy().to_string();
        dirs.push(dir_str);
        instances.push(Arc::new(RwLock::new(create_test_blockchain(&dir))));
    }

    {
        let mut bc = instances[0].write().unwrap();
        assert!(bc.grant_initial_balance_to_first_wallet(&addrs[0]));
    }

    for i in 1..3 {
        assert!(
            adopt_from(&instances[i], &instances[0]),
            "Instance {} did not adopt chain",
            i + 1
        );
    }
    assert_balances(&instances, &addrs, &[10000, 0, 0]);

    for i in 1..3 {
        let mut bc = instances[i].write().unwrap();
        if !bc.balances.contains_key(&addrs[i]) {
            if !bc.grant_initial_balance_to_first_wallet(&addrs[i]) {
                bc.balances.entry(addrs[i].clone()).or_default();
            }
        }
    }
    assert_balances(&instances, &addrs, &[10000, 0, 0]);

    let amount = 1000u64;
    {
        let mut bc = instances[0].write().unwrap();
        create_and_mine_tx(&mut bc, &addrs[0], &addrs[1], amount, &keypairs[0].1);
    }

    for i in 1..3 {
        assert!(
            adopt_from(&instances[i], &instances[0]),
            "Instance {} did not adopt chain with tx",
            i + 1
        );
    }

    let expected = [10000 - amount, amount, 0];
    assert_balances(&instances, &addrs, &expected);

    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn real_network_three_nodes() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap();
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let net_path = exe_dir.join("network.json");
    let saved_net = std::fs::read_to_string(&net_path).ok();
    let net_content =
        r#"{"peers":["127.0.0.1:18281","127.0.0.1:18282","127.0.0.1:18283"]}"#;
    std::fs::write(&net_path, net_content).unwrap();

    let ports = [18281u16, 18282, 18283];
    let (sync_tx, _sync_rx) = std::sync::mpsc::channel::<Blockchain>();
    let mut nodes: Vec<(strangecoin::Node, String, Arc<RwLock<Blockchain>>)> = Vec::new();
    let mut dirs = Vec::new();
    for (i, p) in ports.iter().enumerate() {
        let dir = temp_db_dir(&format!("net_{}", i));
        let dir_str = dir.to_string_lossy().to_string();
        dirs.push(dir_str.clone());
        let bc = Arc::new(RwLock::new(create_test_blockchain(&dir)));
        let (node, _peers) = create_node_for_test(&bc, *p);
        nodes.push((node, dir_str, bc));
    }

    for i in 0..3 {
        nodes[i].0.discover_peers();
    }

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
    {
        let mut bc = nodes[0].2.write().unwrap();
        assert!(
            bc.grant_initial_balance_to_first_wallet(&addrs[0]),
            "Grant not created"
        );
    }

    for round in 0..4 {
        for i in 0..3 {
            let mut sync_node = create_sync_node(
                &nodes[i].2,
                &nodes[i].0.peers,
                &nodes[i].0.address,
                &nodes[i].0.rate_limiter,
            );
            sync_node.sync_blockchain(sync_tx.clone());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = round;
    }

    for i in 1..3 {
        let mut bc = nodes[i].2.write().unwrap();
        if !bc.balances.contains_key(&addrs[i]) {
            if !bc.grant_initial_balance_to_first_wallet(&addrs[i]) {
                bc.balances.entry(addrs[i].clone()).or_default();
            }
            bc.save_state();
        }
    }

    {
        let mut bc = nodes[0].2.write().unwrap();
        create_and_mine_tx(&mut bc, &addrs[0], &addrs[1], 1000, &keypairs[0].1);
    }

    for round in 0..6 {
        for i in 0..3 {
            let mut sync_node = create_sync_node(
                &nodes[i].2,
                &nodes[i].0.peers,
                &nodes[i].0.address,
                &nodes[i].0.rate_limiter,
            );
            sync_node.sync_blockchain(sync_tx.clone());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = round;
    }

    let expected = [10000u64 - 1000, 1000, 0];
    for i in 0..3 {
        let bc = nodes[i].2.read().unwrap();
        let bal = bc.balances.get(&addrs[i]).map(|a| a.balance).unwrap_or(0);
        assert_eq!(bal, expected[i], "Node {}: balance mismatch", i + 1);
        assert!(bc.validate_chain(), "Node {}: chain invalid", i + 1);
    }

    if let Some(saved) = saved_net {
        std::fs::write(&net_path, saved).unwrap();
    } else {
        let _ = std::fs::remove_file(&net_path);
    }
    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn real_network_fast_registration_race() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap();
    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let net_path = exe_dir.join("network.json");
    let saved_net = std::fs::read_to_string(&net_path).ok();
    let net_content =
        r#"{"peers":["127.0.0.1:18291","127.0.0.1:18292","127.0.0.1:18293"]}"#;
    std::fs::write(&net_path, net_content).unwrap();

    let ports = [18291u16, 18292, 18293];
    let (sync_tx, _sync_rx) = std::sync::mpsc::channel::<Blockchain>();
    let mut nodes: Vec<(
        strangecoin::Node,
        Arc<RwLock<Blockchain>>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    )> = Vec::new();
    let mut dirs = Vec::new();
    for (i, p) in ports.iter().enumerate() {
        let dir = temp_db_dir(&format!("fast_{}", i));
        dirs.push(dir.to_string_lossy().to_string());
        let bc = Arc::new(RwLock::new(create_test_blockchain(&dir)));
        let (node, peers) = create_node_for_test(&bc, *p);
        nodes.push((node, bc, peers));
    }

    for i in 0..3 {
        nodes[i].0.discover_peers();
    }

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
    for i in 0..3 {
        let mut bc = nodes[i].1.write().unwrap();
        if !bc.balances.contains_key(&addrs[i]) {
            if !bc.grant_initial_balance_to_first_wallet(&addrs[i]) {
                bc.balances.entry(addrs[i].clone()).or_default();
            }
            bc.save_state();
        }
    }

    for round in 0..6 {
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
        let _ = round;
    }

    {
        let mut bc = nodes[0].1.write().unwrap();
        create_and_mine_tx(&mut bc, &addrs[0], &addrs[1], 1000, &keypairs[0].1);
    }

    for round in 0..6 {
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
        let _ = round;
    }

    let expected = [10000u64 - 1000, 1000, 0];
    for i in 0..3 {
        let bc = nodes[i].1.read().unwrap();
        let bal = bc.balances.get(&addrs[i]).map(|a| a.balance).unwrap_or(0);
        assert!(bc.validate_chain(), "Node {}: chain invalid", i + 1);
        assert_eq!(bal, expected[i], "Node {}: balance mismatch", i + 1);
    }

    if let Some(saved) = saved_net {
        std::fs::write(&net_path, saved).unwrap();
    } else {
        let _ = std::fs::remove_file(&net_path);
    }
    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}
