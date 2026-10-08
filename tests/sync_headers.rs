mod common;

use common::*;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use strangecoin::network::sync;
use strangecoin::ChainSnapshot;

fn to_addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{}", port)
        .parse()
        .expect("valid socket addr")
}

fn shutdown_flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

// ADR-0011: the server accepts on the test runtime; multi_thread keeps it
// live while the test thread drives the (now async) headers-first client.
// The NETWORK_TEST_LOCK std Mutex is held for the whole test by design
// (cross-binary serialization); nothing on this runtime contends for it.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn new_node_syncs_20_blocks_via_headers_first() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let port = random_port();
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();

    let server_dir = TestDir::new("hf_server");
    let client_dir = TestDir::new("hf_client");
    let server_bc = Arc::new(create_test_blockchain(server_dir.path()));
    let client_bc = Arc::new(create_test_blockchain(client_dir.path()));

    let keypairs = generate_keypairs(2);
    let funded = keypairs[0].0.clone();
    let other = keypairs[1].0.clone();

    assert!(server_bc
        .grant_initial_balance_to_first_wallet(&funded)
        .unwrap());
    for _ in 0..19 {
        create_and_mine_tx(&server_bc, &funded, &other, 1, &keypairs[0].1);
    }
    assert_eq!(server_bc.chain_len(), 21, "server must hold 21 blocks");

    let (mut node, _peers) = create_node_for_test(&server_bc, port);
    node.start_server(port, sync_tx.clone()).await;

    let shutdown = shutdown_flag();
    let local = client_bc.chain_snapshot();
    let outcome = sync::sync_headers_first(
        to_addr(port),
        strangecoin::consensus::CHAIN_ID_REGTEST,
        &local,
        &shutdown,
    )
    .await
    .expect("headers-first sync must succeed against a P16 peer");

    let candidate = outcome
        .candidate
        .expect("fresh node must plan an adoption");
    assert_eq!(outcome.headers_ingested, 21);
    assert_eq!(outcome.blocks_downloaded, 20);
    // The loop only plans and downloads; adoption is the SyncEngine's job
    // (ADR-0010), so this test adopts through the facade API directly.
    assert!(client_bc
        .adopt_candidate(candidate, None, Vec::new(), client_bc.difficulty())
        .expect("downloaded candidate must pass validation"));
    assert_eq!(client_bc.chain_len(), 21, "client did not reach server tip");
    assert!(client_bc.validate_chain(), "synced chain failed validation");
    assert_eq!(
        client_bc.get_balance(&funded),
        10000 - 19,
        "funded balance wrong after sync"
    );
    assert_eq!(
        client_bc.get_balance(&other),
        19,
        "recipient balance wrong after sync"
    );
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn equal_chain_reports_nothing_better() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let port = random_port();
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();

    let server_dir = TestDir::new("hf_eq_server");
    let client_dir = TestDir::new("hf_eq_client");
    let server_bc = Arc::new(create_test_blockchain(server_dir.path()));
    let client_bc = Arc::new(create_test_blockchain(client_dir.path()));

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
    assert!(server_bc
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    create_and_mine_tx(&server_bc, &addrs[0], &addrs[1], 500, &keypairs[0].1);
    create_and_mine_tx(&server_bc, &addrs[0], &addrs[2], 300, &keypairs[0].1);
    assert_eq!(server_bc.chain_len(), 4);

    // Client holds a byte-identical copy of the server chain.
    assert!(adopt_from(&client_bc, &server_bc));

    let (mut node, _peers) = create_node_for_test(&server_bc, port);
    node.start_server(port, sync_tx.clone()).await;

    let shutdown = shutdown_flag();
    let local = client_bc.chain_snapshot();
    let outcome = sync::sync_headers_first(
        to_addr(port),
        strangecoin::consensus::CHAIN_ID_REGTEST,
        &local,
        &shutdown,
    )
    .await
    .expect("headers-first exchange must succeed");

    assert!(
        outcome.candidate.is_none(),
        "equal chain must not be planned as a candidate"
    );
    assert_eq!(outcome.headers_ingested, 4);
    assert_eq!(outcome.blocks_downloaded, 0, "nothing was missing");
    assert_eq!(client_bc.chain_len(), 4);
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn longer_fork_resolved_via_headers_first() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let port = random_port();
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();

    let server_dir = TestDir::new("hf_fork_server");
    let client_dir = TestDir::new("hf_fork_client");
    let server_bc = Arc::new(create_test_blockchain(server_dir.path()));
    let client_bc = Arc::new(create_test_blockchain(client_dir.path()));

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    // Divergent branches after genesis: server ends longer (5 blocks) than
    // the client's local branch (4 blocks), so headers-first must plan a
    // reorg, download the fork bodies and adopt.
    assert!(server_bc
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    create_and_mine_tx(&server_bc, &addrs[0], &addrs[1], 500, &keypairs[0].1);
    create_and_mine_tx(&server_bc, &addrs[0], &addrs[2], 300, &keypairs[0].1);
    create_and_mine_tx(&server_bc, &addrs[1], &addrs[2], 100, &keypairs[1].1);
    assert_eq!(server_bc.chain_len(), 5);

    assert!(client_bc
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    create_and_mine_tx(&client_bc, &addrs[0], &addrs[1], 2000, &keypairs[0].1);
    create_and_mine_tx(&client_bc, &addrs[0], &addrs[2], 1000, &keypairs[0].1);
    assert_eq!(client_bc.chain_len(), 4);

    let (mut node, _peers) = create_node_for_test(&server_bc, port);
    node.start_server(port, sync_tx.clone()).await;

    let shutdown = shutdown_flag();
    let local = client_bc.chain_snapshot();
    let outcome = sync::sync_headers_first(
        to_addr(port),
        strangecoin::consensus::CHAIN_ID_REGTEST,
        &local,
        &shutdown,
    )
    .await
    .expect("headers-first sync must succeed");

    let candidate = outcome
        .candidate
        .expect("longer fork must win fork choice");
    assert!(client_bc
        .adopt_candidate(candidate, None, Vec::new(), client_bc.difficulty())
        .expect("fork candidate must pass validation"));
    assert_eq!(client_bc.chain_len(), 5, "client did not reorg onto tip");
    assert!(client_bc.validate_chain(), "reorged chain failed validation");

    let server_state = server_bc.state_snapshot();
    for (addr, expected) in &server_state {
        assert_eq!(
            client_bc.get_balance(addr),
            expected.balance,
            "balance mismatch after reorg for {}",
            addr
        );
    }
}
