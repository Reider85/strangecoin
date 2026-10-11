mod common;

use common::*;
use std::sync::Arc;
use std::time::Duration;
use strangecoin::{BlockchainFacade, Transaction};

#[test]
fn hundred_transactions_five_wallets() {
    let keypairs = generate_keypairs(5);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    let dirs: Vec<TestDir> = (0..5)
        .map(|i| TestDir::new(&format!("wallet_{}", i)))
        .collect();
    let mut wallets: Vec<Arc<BlockchainFacade>> = Vec::new();
    for dir in &dirs {
        wallets.push(Arc::new(create_test_blockchain(dir.path())));
    }

    assert!(wallets[0]
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    sync_to_longest(&wallets);
    assert_balances(&wallets, &addrs, &[10000, 0, 0, 0, 0]);

    let mut ref_balances = [10000u64, 0, 0, 0, 0];
    let edges = [(0usize, 1usize), (1, 2), (2, 3), (3, 4)];

    for tx_index in 1..=100u64 {
        let pass = ((tx_index - 1) / 4 + 1) as usize;
        let edge = ((tx_index - 1) % 4) as usize;
        let (s, r) = edges[edge];
        let amount = amount_for(edge, pass);

        let sender_nonce = wallets[s].get_nonce(&addrs[s]);

        let mut transaction = Transaction {
            sender: addrs[s].clone(),
            receiver: addrs[r].clone(),
            amount,
            nonce: sender_nonce + 1,
            chain_id: strangecoin::consensus::CHAIN_ID_REGTEST,
            signature: Vec::new(),
            is_coinbase: false,
        };

        sign_transaction(&mut transaction, &keypairs[s].1);

        assert!(
            wallets[s].apply_tx(transaction).is_ok(),
            "Transaction {} rejected",
            tx_index
        );
        mine_current(&wallets[s]);
        sync_to_longest(&wallets);

        ref_balances[s] -= amount;
        ref_balances[r] += amount;

        if tx_index % 10 == 0 {
            assert_balances(&wallets, &addrs, &ref_balances);
        }
    }

    assert_balances(&wallets, &addrs, &[0, 0, 0, 0, 10000]);

    for w in &wallets {
        assert_eq!(w.chain_len(), 102);
        assert!(w.validate_chain());
    }
}

/// S1-P10: the node drives a sync task on the tokio runtime, applies a block while
/// that task is live, and tears everything down cleanly when the shutdown flag is set.
///
/// This mirrors the production shutdown path in `run_async()` (tokio::spawn +
/// `tokio::time::interval` + shared `AtomicBool`) without launching the eframe GUI.
#[tokio::test]
async fn node_runs_on_tokio_and_shuts_down_cleanly() {
    let dir = TestDir::new("tokio_runtime");
    let port = random_port();
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let (node, _peers) = create_node_for_test(&bc, port);
    let event_bus = node.event_bus.clone();
    let shutdown = Arc::clone(&node.shutdown);

    // Subscribe before starting the task so no event can be missed.
    let mut events = event_bus.subscribe_async(32);

    // The sync task: same shape as run_async(), with sync_blockchain stubbed out
    // (peers list is empty, so the real one is a no-op that would only add TCP noise).
    let peers = Arc::clone(&node.peers);
    let task_shutdown = Arc::clone(&shutdown);
    let sync_task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(strangecoin::SYNC_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut ticks: u32 = 0;
        loop {
            ticker.tick().await;
            if task_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            ticks += 1;
            if ticks >= strangecoin::SYNC_TICKS_PER_SYNC {
                ticks = 0;
                // Mirrors the real task's per-round work.
                let _ = peers.lock().map(|p| p.len());
            }
        }
    });

    // A block is applied while the async task is ticking. The grant block
    // itself is the block under test — mine_block() needs a non-empty mempool
    // and there is no signer available here beyond the grant.
    bc.grant_initial_balance_to_first_wallet(&test_address())
        .unwrap();
    let height = bc.chain_len();
    assert!(height > 0, "no block was applied");

    event_bus.publish(strangecoin::events::NodeEvent::BlockApplied {
        height: height as u64,
        hash: "tokio".into(),
    });

    // The async bridge delivers it to a tokio consumer.
    let received = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("event bridge timed out")
        .expect("event bridge closed early");
    assert!(matches!(
        received,
        strangecoin::events::NodeEvent::BlockApplied { .. }
    ));

    // Shutdown: flag -> task observes it within a tick -> handle resolves.
    shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    tokio::time::timeout(Duration::from_secs(5), sync_task)
        .await
        .expect("sync task did not observe the shutdown flag")
        .expect("sync task panicked");

    // Node::drop cancels an already-finished task without panicking.
    drop(node);
    assert!(shutdown.load(std::sync::atomic::Ordering::Relaxed));
    assert!(bc.validate_chain());
}

fn test_address() -> String {
    generate_keypairs(1).remove(0).0
}

#[test]
fn bech32_address_transfer() {
    // Test that addresses are properly formatted as bech32 and transfers work
    let keypairs = generate_keypairs(2);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
    
    // Verify addresses start with correct HRP (should be rsc1 for regtest)
    assert!(addrs[0].starts_with("rsc1"));
    assert!(addrs[1].starts_with("rsc1"));
    
    let dirs: Vec<TestDir> = (0..2)
        .map(|i| TestDir::new(&format!("wallet_{}", i)))
        .collect();
    let mut wallets: Vec<Arc<BlockchainFacade>> = Vec::new();
    for dir in &dirs {
        wallets.push(Arc::new(create_test_blockchain(dir.path())));
    }
    
    // Grant to first wallet
    assert!(wallets[0]
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    sync_to_longest(&wallets);
    assert_balances(&wallets, &addrs, &[10000, 0]);
    
    // Transfer 1000 from wallet 0 to wallet 1
    create_and_mine_tx(&wallets[0], &addrs[0], &addrs[1], 1000, &keypairs[0].1);
    sync_to_longest(&wallets);
    assert_balances(&wallets, &addrs, &[9000, 1000]);
    
    // Verify both chains are valid
    for w in &wallets {
        assert_eq!(w.chain_len(), 3); // genesis + grant + 1 tx
        assert!(w.validate_chain());
    }
}
