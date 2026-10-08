mod common;

use common::*;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use strangecoin::events::NodeEvent;
use strangecoin::network::sync_engine::{self, Incoming};
use strangecoin::{Block, BlockchainFacade, ChainSnapshot};

/// S1-P18 / ADR-0010: two peers race the same candidate (and competing
/// forks) into the inbox simultaneously. The SyncEngine is the single
/// consumer, so processing order is queue order — the final state and the
/// event counts must be deterministic regardless of arrival order.
///
/// Phases (deterministic, no sleeps for synchronization — pushes are joined
/// through a barrier and events are collected from the bus):
///
/// 1. identical branch twice → exactly one `BlockApplied`, no reorg;
/// 2. worse fork twice → zero events, chain unchanged;
/// 3. better fork twice → exactly one `BlockApplied` + one `BlockReorged`.
#[test]
fn concurrent_candidates_race_through_one_engine() {
    let main_dir = TestDir::new("engine_main");
    let a_dir = TestDir::new("engine_a");
    let b_dir = TestDir::new("engine_b");
    let c_dir = TestDir::new("engine_c");

    let main: Arc<BlockchainFacade> = Arc::new(create_test_blockchain(main_dir.path()));

    let keypairs = generate_keypairs(3);
    let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

    // Three competing branches off the shared genesis: lengths 4, 3 and 5.
    // Blocks share the regtest target, so chain length acts as cumulative
    // work for fork choice.
    let fork_a: Arc<BlockchainFacade> = Arc::new(create_test_blockchain(a_dir.path()));
    assert!(fork_a
        .grant_initial_balance_to_first_wallet(&addrs[0])
        .unwrap());
    for _ in 0..2 {
        create_and_mine_tx(&fork_a, &addrs[0], &addrs[1], 1, &keypairs[0].1);
    }
    assert_eq!(fork_a.chain_len(), 4);

    let fork_b: Arc<BlockchainFacade> = Arc::new(create_test_blockchain(b_dir.path()));
    assert!(fork_b
        .grant_initial_balance_to_first_wallet(&addrs[1])
        .unwrap());
    create_and_mine_tx(&fork_b, &addrs[1], &addrs[0], 1, &keypairs[1].1);
    assert_eq!(fork_b.chain_len(), 3);

    let fork_c: Arc<BlockchainFacade> = Arc::new(create_test_blockchain(c_dir.path()));
    assert!(fork_c
        .grant_initial_balance_to_first_wallet(&addrs[2])
        .unwrap());
    for _ in 0..3 {
        create_and_mine_tx(&fork_c, &addrs[2], &addrs[0], 1, &keypairs[2].1);
    }
    assert_eq!(fork_c.chain_len(), 5);

    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();
    let shutdown = Arc::new(AtomicBool::new(false));
    // Subscribe before spawning the engine so no event can be missed.
    let events = main.event_bus().subscribe();
    let inbox = sync_engine::spawn(
        Arc::clone(&main),
        main.event_bus(),
        sync_tx,
        Arc::clone(&shutdown),
        Arc::new(strangecoin::network::RateLimiter::new(10, 100)),
    );

    // Collect events until `quiet` passes with nothing new (bounded by `max`).
    let drain = |quiet: Duration, max: Duration| -> Vec<NodeEvent> {
        let mut out = Vec::new();
        let deadline = Instant::now() + max;
        while Instant::now() < deadline {
            match events.recv_timeout(quiet) {
                Ok(e) => out.push(e),
                Err(_) => break,
            }
        }
        out
    };

    // Two peers push the same branch at the same moment.
    let push_pair = |chain: Vec<Block>, difficulty: u32| {
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = [28601u16, 28602]
            .into_iter()
            .map(|port| {
                let inbox = inbox.clone();
                let barrier = Arc::clone(&barrier);
                let chain = chain.clone();
                let from: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
                thread::spawn(move || {
                    barrier.wait();
                    inbox.push(Incoming::CandidateChain {
                        chain,
                        balances: None,
                        mempool_txs: Vec::new(),
                        pending_transactions: Vec::new(),
                        difficulty,
                        from: Some(from),
                    })
                })
            })
            .collect();
        for h in handles {
            h.join().expect("pusher thread must not panic");
        }
    };

    // Phase 1 — identical candidates race: applied exactly once, no reorg.
    push_pair(fork_a.chain_snapshot(), fork_a.difficulty());
    let ev = drain(Duration::from_millis(500), Duration::from_secs(5));
    let count = |ev: &[NodeEvent], f: &dyn Fn(&NodeEvent) -> bool| {
        ev.iter().filter(|e| f(e)).count()
    };
    let is_applied = |e: &NodeEvent| matches!(e, NodeEvent::BlockApplied { .. });
    let is_reorg = |e: &NodeEvent| matches!(e, NodeEvent::BlockReorged { .. });
    let is_persisted = |e: &NodeEvent| matches!(e, NodeEvent::StatePersisted { .. });
    assert_eq!(
        count(&ev, &is_applied),
        1,
        "identical racing candidates must apply once: {:?}",
        ev
    );
    assert_eq!(count(&ev, &is_reorg), 0, "first adoption is not a reorg: {:?}", ev);
    assert_eq!(
        count(&ev, &is_persisted),
        1,
        "one persistence announcement: {:?}",
        ev
    );
    assert_eq!(main.chain_len(), 4, "chain must reach the raced branch");
    assert_eq!(main.tip_hash(), fork_a.tip_hash());

    // Phase 2 — a worse fork raced twice: fork choice rejects it silently.
    push_pair(fork_b.chain_snapshot(), fork_b.difficulty());
    let ev = drain(Duration::from_millis(500), Duration::from_secs(5));
    assert!(
        ev.is_empty(),
        "worse fork must produce no events: {:?}",
        ev
    );
    assert_eq!(main.chain_len(), 4, "worse fork must not replace the chain");
    assert_eq!(main.tip_hash(), fork_a.tip_hash());

    // Phase 3 — a better fork raced twice: one adoption, one reorg.
    push_pair(fork_c.chain_snapshot(), fork_c.difficulty());
    let ev = drain(Duration::from_millis(500), Duration::from_secs(5));
    assert_eq!(
        count(&ev, &is_applied),
        1,
        "better fork must apply once: {:?}",
        ev
    );
    assert_eq!(
        count(&ev, &is_reorg),
        1,
        "better fork must reorg exactly once: {:?}",
        ev
    );
    assert_eq!(main.chain_len(), 5, "chain must reach the winning fork");
    assert_eq!(main.tip_hash(), fork_c.tip_hash());
    assert!(main.validate_chain(), "final chain must validate");

    shutdown.store(true, Ordering::Relaxed);
    // The engine observes the flag on its next poll tick and drops its
    // facade handle, letting the test dirs clean up.
    thread::sleep(Duration::from_millis(300));
}
