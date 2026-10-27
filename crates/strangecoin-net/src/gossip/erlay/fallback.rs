//! S2-P15 — mandatory full-relay fallback (ADR-0021, ROADMAP3 risk
//! «Erlay reconciliation может приводить к потере txs»).
//!
//! When sketch reconciliation fails (malformed envelope, difference above
//! capacity after adaptive growth, missing peer sketch), the delta is
//! unknown — so this round degrades to **full relay**: announce our txids to
//! the reconciliation partner as chunked INV message(s) and let the existing
//! S2-P13 `handle_inv` → `GET_DATA` → `TX` path fetch whatever the peer
//! lacks. The peer's decode failure is symmetric (both sides XOR the same
//! two envelopes), so the peer's own fallback pushes its inventory the other
//! way and both mempools converge — correctness wins over the temporary
//! bandwidth cost (ADR-0021 «Fallback policy»).
//!
//! **Deviation from the prompt's literal signature** (recorded per FBP rule
//! «все отклонения — явно»):
//!
//! * `mempool: &Mempool` → `blockchain: &dyn BlockchainView` — `Mempool`
//!   lives in the monolith and `strangecoin-net` must not depend on it
//!   (S2-P06 cycle break; same deviation as `gossip/broadcast.rs`);
//! * extra `peer: &NodeId` — a failed round is a *per-peer* failure, so the
//!   fallback targets the reconciliation partner instead of fanning out to
//!   every connection (broadcast semantics stay available via
//!   `broadcast::announce_tx`);
//! * extra `state: &mut GossipState` — per-peer INV dedup, same as every
//!   other announce path;
//! * `txids: &[[u8; 32]]` slice instead of an owned `Vec` — callers pass a
//!   borrow of their known set; an **empty** slice means "announce the whole
//!   mempool inventory" (the decode-failure case, where nothing is known
//!   about what is missing).
//!
//! Items are sorted by txid before chunking (determinism checklist #4: the
//! same set always produces the same wire order) and chunked at
//! [`MAX_INV_BATCH`] so a 1000-tx inventory never trips the INV batch bound.

use crate::blockchain_view::BlockchainView;
use crate::gossip::broadcast::GossipState;
use crate::gossip::inv::{Inv, InventoryItem, MAX_INV_BATCH};
use crate::noise::NodeId;
use crate::peer::PeerManager;
use strangecoin_core::serialize::txid;

/// Announce `txids` to `peer` as chunked INV(s) — the full-relay fallback
/// for a failed reconciliation round.
///
/// Returns how many inventory items were actually sent (items the peer
/// already demonstrably knows are skipped, matching `announce_tx` dedup).
/// Send failures are logged and skipped, never propagated: the fallback is
/// an optimization-recovery path, and the INV announce loop (S2-P13) keeps
/// retrying independently.
pub fn fallback_full_relay(
    txids: &[[u8; 32]],
    peer: &NodeId,
    peer_manager: &PeerManager,
    state: &mut GossipState,
    blockchain: &dyn BlockchainView,
) -> usize {
    let mut items: Vec<InventoryItem> = if txids.is_empty() {
        // Decode failure with no identified delta → complete inventory
        // exchange: everything pending in the mempool goes on the wire.
        blockchain
            .mempool_txs()
            .iter()
            .map(|tx| InventoryItem::tx(txid(tx)))
            .collect()
    } else {
        txids.iter().map(|id| InventoryItem::tx(*id)).collect()
    };
    items.sort_by_key(|item| item.hash);
    items.dedup();

    let mut sent = 0usize;
    for chunk in items.chunks(MAX_INV_BATCH) {
        let fresh: Vec<InventoryItem> = chunk
            .iter()
            .copied()
            .filter(|item| !state.knows(peer, item))
            .collect();
        if fresh.is_empty() {
            continue;
        }
        let payload = match (Inv {
            items: fresh.clone(),
        })
        .encode()
        {
            Ok(payload) => payload,
            Err(err) => {
                tracing::warn!(target = "net::erlay", error = %err, "fallback INV encode failed");
                continue;
            }
        };
        match peer_manager.send_to_peer(peer, &payload) {
            Ok(()) => sent += fresh.len(),
            Err(err) => tracing::warn!(
                target = "net::erlay",
                peer = %peer,
                error = %err,
                "fallback INV send failed, continuing"
            ),
        }
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_is_sorted_and_deduplicated_before_chunking() {
        // Pure ordering/dedup discipline: 3 txids, two identical → the
        // encoded item list is ascending with one duplicate removed.
        let mut a = [1u8; 32];
        a[31] = 3;
        let mut b = [1u8; 32];
        b[31] = 1;
        let mut c = [1u8; 32];
        c[31] = 2;
        let mut items = vec![
            InventoryItem::tx(a),
            InventoryItem::tx(b),
            InventoryItem::tx(c),
            InventoryItem::tx(b),
        ];
        items.sort_by_key(|item| item.hash);
        items.dedup();
        assert_eq!(items.len(), 3);
        assert!(items.windows(2).all(|w| w[0].hash < w[1].hash));
    }
}
