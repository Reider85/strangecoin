//! S1-P19: EventBus fan-out on a live node — three subscribers each receive
//! the events the SyncEngine announces (DoD Этап 1 №4: «Events bus работает
//! (3 subscribers)»; the engine is the single announcer per ADR-0010).

mod common;

use common::*;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use strangecoin::events::NodeEvent;
use strangecoin::network::sync_engine::Incoming;
use strangecoin::{Blockchain, ChainSnapshot, Transaction};

#[test]
fn three_subscribers_each_receive_live_node_events() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let port = random_port();
    let (sync_tx, _sync_rx) = mpsc::channel::<ChainSnapshot>();

    let dir = TestDir::new("events_live");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let (mut node, _peers) = create_node_for_test(&bc, port);
    node.start_server(port, sync_tx);

    // Three independent subscribers on the node's live bus.
    let bus = node.event_bus.clone();
    let rx1 = bus.subscribe();
    let rx2 = bus.subscribe();
    let rx3 = bus.subscribe();

    // start_server spawned the SyncEngine inbox — the node's real producer
    // handle; every event below travels through the production pipeline.
    let inbox = node
        .inbox
        .clone()
        .expect("start_server must spawn the SyncEngine inbox");

    let keypairs = generate_keypairs(2);
    let alice = keypairs[0].0.clone();
    let bob = keypairs[1].0.clone();
    assert!(bc.grant_initial_balance_to_first_wallet(&alice).unwrap());

    // Event 1: a transaction accepted by the engine.
    let mut transfer = Transaction {
        sender: alice.clone(),
        receiver: bob.clone(),
        amount: 500,
        nonce: 1,
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut transfer, &keypairs[0].1);
    let txid = hex::encode(strangecoin_core::serialize::txid(&transfer));
    assert!(
        inbox.push(Incoming::NewTx {
            tx: transfer,
            from: None
        }),
        "fresh inbox must accept the transaction"
    );

    // Event 2: a candidate chain adopted by the engine.
    let prefix = bc.chain_snapshot();
    let child = craft_child(&prefix, vec![coinbase_tx("miner", 0)]);
    let child_hash = child.hash.clone();
    let mut candidate = prefix;
    candidate.push(child);
    assert!(
        inbox.push(Incoming::CandidateChain {
            chain: candidate,
            balances: None,
            mempool_txs: Vec::new(),
            pending_transactions: Vec::new(),
            difficulty: bc.difficulty(),
            from: None,
        }),
        "fresh inbox must accept the candidate"
    );

    // Each subscriber must see both events, in publish order.
    for (name, rx) in [("subscriber1", &rx1), ("subscriber2", &rx2), ("subscriber3", &rx3)] {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen_tx = false;
        let mut seen_block = false;
        while !seen_block {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "{name}: timed out (seen_tx={seen_tx}, seen_block={seen_block})"
            );
            let event = rx
                .recv_timeout(left)
                .unwrap_or_else(|e| panic!("{name}: no event delivered: {e}"));
            match event {
                NodeEvent::TxAccepted { txid: got } => {
                    assert_eq!(got, txid, "{name}: wrong txid");
                    assert!(!seen_tx, "{name}: duplicate TxAccepted");
                    seen_tx = true;
                }
                NodeEvent::StatePersisted { height } => {
                    assert_eq!(height, 2, "{name}: wrong persisted height");
                }
                NodeEvent::BlockApplied { height, hash } => {
                    assert_eq!(height, 2, "{name}: wrong applied height");
                    assert_eq!(hash, child_hash, "{name}: wrong tip hash");
                    assert!(seen_tx, "{name}: TxAccepted must precede BlockApplied");
                    seen_block = true;
                }
                other => panic!("{name}: unexpected event {other:?}"),
            }
        }
    }

    drop(node);
}
