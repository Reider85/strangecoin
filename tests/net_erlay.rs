//! S2-P22 — Erlay reconciliation (DoD #3: 2 peers × 1000 txs, 800 shared →
//! only the 200-tx delta transmitted, ≥ 80 % reduction) plus the mandatory
//! full-relay fallback on decode failure (ADR-0021, no tx loss).
//!
//! Regtest (chain_id 3), in-memory fixtures, injected clock — no sleeps.

mod common;

use std::time::Instant;

use common::net_helpers::{transfer_tx, MockView};
use strangecoin_core::serialize::txid;
use strangecoin_net::gossip::{
    create_sketch, handle_get_data, handle_inv, ReconciliationConfig, ReconciliationState,
};
use strangecoin_net::{
    BlockchainView, GossipMessage, GossipState, Inv, InventoryItem, NodeId, NodeKeypair,
};

fn node_id(seed: [u8; 32]) -> NodeId {
    NodeKeypair::from_seed(seed).node_id()
}

fn txids_for_nonces(nonces: std::ops::Range<u64>) -> Vec<[u8; 32]> {
    nonces.map(|n| txid(&transfer_tx(n))).collect()
}

fn expect_get_data(message: GossipMessage) -> strangecoin_net::GetData {
    match message {
        GossipMessage::GetData(get_data) => get_data,
        other => panic!("expected GET_DATA, got {other:?}"),
    }
}

/// A = 800 shared + 200 exclusive; B = 800 shared + 200 exclusive. Each
/// side resolves only its own half of the difference, the delta crosses as
/// one INV + GET_DATA exchange, and the full-relay baseline (A's whole
/// 1000-tx set) shrinks by ≥ 80 %.
#[test]
fn two_peers_800_shared_transmit_only_200_delta() {
    // Shared: nonces 0..800. A-exclusive: 800..1000. B-exclusive: 1000..1200.
    let shared: Vec<[u8; 32]> = txids_for_nonces(0..800);
    let a_only: Vec<[u8; 32]> = txids_for_nonces(800..1000);
    let b_only: Vec<[u8; 32]> = txids_for_nonces(1000..1200);

    let set_a: std::collections::HashSet<[u8; 32]> =
        shared.iter().chain(a_only.iter()).copied().collect();
    let set_b: std::collections::HashSet<[u8; 32]> =
        shared.iter().chain(b_only.iter()).copied().collect();
    assert_eq!(set_a.len(), 1000);
    assert_eq!(set_b.len(), 1000);

    // Capacity covers the symmetric difference (400 short IDs).
    let config = ReconciliationConfig {
        initial_capacity: 512,
        ..Default::default()
    };
    let mut state_a = ReconciliationState::new(config.clone());
    let mut state_b = ReconciliationState::new(config);
    state_a.note_txs(set_a.iter().copied());
    state_b.note_txs(set_b.iter().copied());

    let sketch_a = create_sketch(&set_a, 512);
    let sketch_b = create_sketch(&set_b, 512);

    let peer_a = node_id([0xA1; 32]);
    let peer_b = node_id([0xB2; 32]);

    // Each side decodes only the TxIds IT holds (lexicographic order).
    let exclusive_a = state_a
        .reconcile_at(&peer_b, &sketch_b, Instant::now())
        .expect("A decodes");
    let exclusive_b = state_b
        .reconcile_at(&peer_a, &sketch_a, Instant::now())
        .expect("B decodes");
    assert_eq!(exclusive_a.len(), 200, "A's txs B lacks");
    assert_eq!(exclusive_b.len(), 200, "B's txs A lacks");
    assert!(
        exclusive_a.windows(2).all(|w| w[0] < w[1]),
        "lexicographic wire order"
    );

    // A → B: one INV of the delta; B requests exactly the 200 missing bodies.
    let mut view_a = MockView::default();
    let mut view_b = MockView::default();
    // B starts with its own full set (800 shared + 200 B-exclusive).
    for nonce in 0..800u64 {
        view_b.insert_tx(transfer_tx(nonce));
    }
    for nonce in 1000..1200u64 {
        view_b.insert_tx(transfer_tx(nonce));
    }
    assert_eq!(view_b.mempool.len(), 1000);
    let inv_a = Inv {
        items: exclusive_a
            .iter()
            .map(|id| InventoryItem::tx(*id))
            .collect(),
    };
    let mut gossip_b = GossipState::new();
    let requests = handle_inv(&inv_a, peer_a, &mut gossip_b, &view_b).expect("handle_inv");
    assert_eq!(requests.len(), 1, "single GET_DATA");
    let get_data = expect_get_data(requests.into_iter().next().expect("GET_DATA"));
    assert_eq!(get_data.items.len(), 200, "only the delta is requested");

    // Serve from A's side: fill view_a with the full 1000-tx set first.
    for nonce in 0..1000u64 {
        view_a.insert_tx(transfer_tx(nonce));
    }
    let mut gossip_a = GossipState::new();
    let replies = handle_get_data(&get_data, peer_b, &mut gossip_a, &view_a);
    let mut transmitted = 0;
    for reply in replies {
        if let GossipMessage::Tx(tx) = reply {
            view_b.insert_tx(tx);
            transmitted += 1;
        }
    }
    assert_eq!(transmitted, 200, "only 200 bodies crossed the wire");

    // DoD #3: against a naive full relay of A's 1000-tx mempool the
    // reconciliation delta is ≥ 80 % smaller (200/1000 = exactly 80 %).
    let naive_full_relay = 1000;
    let reduction = 1.0 - (transmitted as f64 / naive_full_relay as f64);
    assert!(
        reduction >= 0.80,
        "bandwidth reduction {:.1}% < 80% ({transmitted}/{naive_full_relay})",
        reduction * 100.0
    );

    // No tx loss: B converged to the union (800 shared + 200 A-only +
    // 200 B-exclusive = 1200).
    assert_eq!(view_b.mempool.len(), 1200, "B converged, no tx loss");
}

/// A corrupt sketch makes the decode fail (the mandatory full-relay
/// fallback signal, ADR-0021) — the fallback then pushes the entire local
/// inventory as one INV, and the peer ends up with every tx: no loss.
#[test]
fn reconciliation_failure_falls_back_to_full_relay_without_tx_loss() {
    // Fully disjoint sets: A = nonces 0..500, B = nonces 500..1000.
    let set_a: std::collections::HashSet<[u8; 32]> = txids_for_nonces(0..500).into_iter().collect();
    let set_b: std::collections::HashSet<[u8; 32]> =
        txids_for_nonces(500..1000).into_iter().collect();

    let mut state_a = ReconciliationState::new(ReconciliationConfig {
        initial_capacity: 600,
        ..Default::default()
    });
    state_a.note_txs(set_a.iter().copied());

    let peer_b = node_id([0xB2; 32]);
    let mut sketch_b = create_sketch(&set_b, 600);
    // Corrupt the envelope magic — a malformed sketch must fail cleanly
    // (NetError::Protocol), never panic, never decode a wrong difference.
    sketch_b[0] ^= 0xFF;

    let result = state_a.reconcile_at(&peer_b, &sketch_b, Instant::now());
    assert!(
        result.is_err(),
        "corrupt sketch must surface the fallback signal"
    );

    // Fallback: full relay of A's entire inventory (the `fallback_full_relay`
    // content path), driven through the real INV/GET_DATA handlers.
    let mut view_a = MockView::default();
    for nonce in 0..500u64 {
        view_a.insert_tx(transfer_tx(nonce));
    }
    let mut view_b = MockView::default();
    for nonce in 500..1000u64 {
        view_b.insert_tx(transfer_tx(nonce));
    }

    let full_inventory: Vec<[u8; 32]> = set_a.iter().copied().collect();
    let fallback_inv = Inv {
        items: full_inventory
            .iter()
            .map(|id| InventoryItem::tx(*id))
            .collect(),
    };
    let peer_a = node_id([0xA1; 32]);
    let mut gossip_b = GossipState::new();
    let requests = handle_inv(&fallback_inv, peer_a, &mut gossip_b, &view_b).expect("handle_inv");
    assert_eq!(requests.len(), 1, "single GET_DATA for the full inventory");
    let get_data = expect_get_data(requests.into_iter().next().expect("GET_DATA"));
    assert_eq!(
        get_data.items.len(),
        500,
        "B lacks all 500 of A's txs (disjoint sets)"
    );

    let mut gossip_a = GossipState::new();
    let replies = handle_get_data(&get_data, peer_b, &mut gossip_a, &view_a);
    for reply in replies {
        if let GossipMessage::Tx(tx) = reply {
            view_b.insert_tx(tx);
        }
    }
    assert_eq!(
        view_b.mempool.len(),
        1000,
        "fallback relayed everything — no tx loss"
    );
    for nonce in 0..500u64 {
        assert!(
            view_b.mempool_contains(&txid(&transfer_tx(nonce))),
            "A's tx {nonce} arrived via fallback"
        );
    }
}
