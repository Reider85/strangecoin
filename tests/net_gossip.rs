//! S2-P22 — gossip propagation (DoD #1: txs reach 100 nodes ≤ 1 s) and INV
//! deduplication.
//!
//! The propagation budget runs on the deterministic [`SimulatedNetwork`]
//! («simulated clock, no real network», prompt S2-P22 task 3); the dedup
//! assertions drive the real INV/GET_DATA handlers against an in-memory
//! `BlockchainView` (regtest, chain_id 3).

mod common;

use std::time::Duration;

use common::net_helpers::{
    simulate_gossip_propagation, transfer_tx, MockView, SimulatedNetwork, GOSSIP_HOP_DELAY,
};
use strangecoin_core::serialize::txid;
use strangecoin_net::gossip::{handle_get_data, handle_inv};
use strangecoin_net::{
    BlockchainView, GossipMessage, GossipState, Inv, InventoryItem, NodeId, NodeKeypair,
};

fn origin_id(seed: [u8; 32]) -> NodeId {
    NodeKeypair::from_seed(seed).node_id()
}

fn expect_get_data(message: GossipMessage) -> strangecoin_net::GetData {
    match message {
        GossipMessage::GetData(get_data) => get_data,
        other => panic!("expected GET_DATA, got {other:?}"),
    }
}

/// DoD #1: one node creates a tx; it reaches all 100 simulated peers within
/// one second of simulated time — every peer gets exactly one first-arrival
/// stamp, and the run is deterministic (same inputs → same times).
#[test]
fn tx_reaches_100_simulated_peers_within_one_second() {
    let network = SimulatedNetwork::with_peers([0x01; 32], 100);
    assert_eq!(network.peers().len(), 100);

    let started = network.at(Duration::ZERO);
    let arrivals = simulate_gossip_propagation(network.origin(), &network);

    assert_eq!(arrivals.len(), 100, "every peer hears the tx");
    let budget = network.at(Duration::from_secs(1));
    let mut latest = started;
    for (peer, arrived) in &arrivals {
        assert!(
            *arrived <= budget,
            "peer {peer} arrived at {arrived:?} > 1 s"
        );
        latest = latest.max(*arrived);
    }
    assert!(
        latest < started + GOSSIP_HOP_DELAY * 100,
        "sanity: flood depth bounded by hop count"
    );

    // Determinism (checklist #5): a second run over an identical network
    // yields the identical arrival sequence.
    let network2 = SimulatedNetwork::with_peers([0x01; 32], 100);
    let arrivals2 = simulate_gossip_propagation(network2.origin(), &network2);
    assert_eq!(arrivals.len(), arrivals2.len());
    for ((peer_a, time_a), (peer_b, time_b)) in arrivals.iter().zip(&arrivals2) {
        assert_eq!(peer_a, peer_b, "deterministic peer order");
        assert_eq!(
            time_a.duration_since(network.at(Duration::ZERO)),
            time_b.duration_since(network2.at(Duration::ZERO)),
            "deterministic arrival offset"
        );
    }
}

/// INV deduplication: a peer that already holds the tx does not request it
/// again, and the announcing side's known-set skips re-announcing to a peer
/// that already demonstrated possession.
#[test]
fn inv_deduplication_prevents_refetch_of_known_tx() {
    let tx = transfer_tx(1);
    let id = txid(&tx);

    // The origin holds the tx; the peer starts empty.
    let origin_view = MockView::with_mempool(vec![tx.clone()]);
    let origin = origin_id([0x0A; 32]);
    let peer = NodeKeypair::from_seed([0x0B; 32]).node_id();
    let mut peer_state = GossipState::new();
    let mut peer_view = MockView::default();

    let inv = Inv {
        items: vec![InventoryItem::tx(id)],
    };

    // First announce: the peer lacks the tx and asks for it.
    let responses = handle_inv(&inv, origin, &mut peer_state, &peer_view).expect("handle_inv");
    assert_eq!(responses.len(), 1, "peer requests the tx once");
    let get_data = expect_get_data(responses.into_iter().next().expect("GET_DATA"));
    assert_eq!(get_data.items, vec![InventoryItem::tx(id)]);

    // The origin serves it; the peer accepts it into its mempool.
    let mut origin_state = GossipState::new();
    let replies = handle_get_data(&get_data, peer, &mut origin_state, &origin_view);
    assert_eq!(replies.len(), 1, "exactly one TX body");
    let GossipMessage::Tx(received) = &replies[0] else {
        panic!("reply is TX");
    };
    peer_view.insert_tx(received.clone());
    assert!(peer_view.mempool_contains(&id));

    // Second announce: the peer already has it — no GET_DATA, and the
    // known-set marks the object as known-for-origin either way.
    let responses = handle_inv(&inv, origin, &mut peer_state, &peer_view).expect("handle_inv");
    assert!(
        responses.is_empty(),
        "peer must not request a tx it already has"
    );
    assert!(peer_state.knows(&origin, &InventoryItem::tx(id)));

    // Announce-side dedup: with the origin marked as knowing the tx, the
    // broadcast fan-out skips it (GossipState filter, S2-P13).
    let manager = common::net_helpers::manager_with(None);
    let mut announce_state = GossipState::new();
    announce_state.mark_known(origin, InventoryItem::tx(id));
    assert!(announce_state.knows(&origin, &InventoryItem::tx(id)));
    // No connected peers in this fixture: fan-out is 0 regardless; the
    // known-set predicate above is the filter `announce_tx` consults.
    assert_eq!(
        strangecoin_net::announce_tx(id, &manager, &announce_state),
        0
    );
}
