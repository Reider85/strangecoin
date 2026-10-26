//! S2-P13 — gossip broadcast + INV/GET_DATA handlers.
//!
//! The announce side pushes one-item `INV` messages to every connected,
//! non-banned peer that does not already know the object
//! ([`GossipState`], per-peer known-set). The receive side answers a peer's
//! `INV` with a `GET_DATA` for the objects the local node lacks and serves
//! incoming `GET_DATA` from the mempool / canonical chain via
//! [`BlockchainView`].
//!
//! **Deviation from the prompt's literal signatures** (recorded per FBP
//! rule «все отклонения — явно»):
//!
//! * the prompt types the handlers with `mempool: &Mempool`; `Mempool` lives
//!   in the monolith and `strangecoin-net` must not depend on it (S2-P06
//!   cycle break) — both handlers take `&dyn BlockchainView` only, which
//!   covers mempool *and* chain lookups;
//! * per-peer INV dedup (prompt task 5) needs state the literal
//!   `announce_tx(txid, peer_manager)` signature cannot carry — hence the
//!   extra [`GossipState`] parameter and the `from: NodeId` handler
//!   parameters that key the known-sets.
//!
//! **Determinism** (Stage 2 checklist): no clock, no RNG, no I/O — announce
//! iterates `PeerManager::connected_peers()` (sorted by `node_id`,
//! determinism #4); `GossipState` is only point-queried, never iterated for
//! output.

use super::get_data::{GetData, NotFound};
use super::inv::{Inv, InventoryItem};
use super::GossipMessage;
use crate::blockchain_view::BlockchainView;
use crate::error::NetError;
use crate::noise::NodeId;
use crate::peer::PeerManager;
use std::collections::{HashMap, HashSet};

/// Bound of the per-peer known-set; on overflow the peer's set is cleared
/// (same eviction discipline as the sync engine's `SEEN_CAP` — gossip
/// dedup is an optimization, not a safety property).
pub const KNOWN_SET_CAP: usize = 4096;

/// Per-peer inventory knowledge: which objects each peer is known to have.
///
/// Fed from two events: a peer announcing an object in `INV` (they have it)
/// and us serving an object via `GET_DATA` (they now have it). `announce_*`
/// skips peers whose set contains the object — no redundant INV traffic.
#[derive(Debug, Default)]
pub struct GossipState {
    known: HashMap<NodeId, HashSet<InventoryItem>>,
}

impl GossipState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `peer` has `item`.
    pub fn mark_known(&mut self, peer: NodeId, item: InventoryItem) {
        let set = self.known.entry(peer).or_default();
        if set.len() >= KNOWN_SET_CAP {
            set.clear();
        }
        set.insert(item);
    }

    /// Does `peer` already have `item`?
    pub fn knows(&self, peer: &NodeId, item: &InventoryItem) -> bool {
        self.known.get(peer).is_some_and(|set| set.contains(item))
    }

    /// Drop the known-set of a disconnected peer (bounded memory).
    pub fn forget_peer(&mut self, peer: &NodeId) {
        self.known.remove(peer);
    }
}

/// `announce_tx` — broadcast `INV(tx)` to every connected, non-banned peer
/// that does not already know `txid`. Returns how many peers got the INV.
pub fn announce_tx(txid: [u8; 32], peer_manager: &PeerManager, state: &GossipState) -> usize {
    announce(peer_manager, state, InventoryItem::tx(txid))
}

/// `announce_block` — broadcast `INV(block)` to every connected, non-banned
/// peer that does not already know `block_hash`. Returns the peer count.
pub fn announce_block(
    block_hash: [u8; 32],
    peer_manager: &PeerManager,
    state: &GossipState,
) -> usize {
    announce(peer_manager, state, InventoryItem::block(block_hash))
}

fn announce(peer_manager: &PeerManager, state: &GossipState, item: InventoryItem) -> usize {
    let payload = match (Inv { items: vec![item] }).encode() {
        Ok(payload) => payload,
        Err(err) => {
            tracing::warn!(target = "net::gossip", error = %err, "INV encode failed");
            return 0;
        }
    };
    let mut sent = 0usize;
    for peer in peer_manager.connected_peers() {
        if peer_manager.is_banned(&peer) {
            continue;
        }
        if state.knows(&peer, &item) {
            continue;
        }
        match peer_manager.send_to_peer(&peer, &payload) {
            Ok(()) => sent += 1,
            Err(err) => tracing::warn!(
                target = "net::gossip",
                peer = %peer,
                error = %err,
                "INV broadcast to peer failed, continuing"
            ),
        }
    }
    sent
}

/// `handle_inv` — a peer announced `inv`; mark everything as known-for-`from`
/// and request (as one `GET_DATA`) the items the local node does not have.
///
/// Returns the response messages for the caller to send back to `from`
/// (empty when the peer announced nothing new to us).
pub fn handle_inv(
    inv: &Inv,
    from: NodeId,
    state: &mut GossipState,
    blockchain: &dyn BlockchainView,
) -> Result<Vec<GossipMessage>, NetError> {
    let mut missing: Vec<InventoryItem> = Vec::new();
    for item in &inv.items {
        // The announcing peer demonstrably has the object.
        state.mark_known(from, *item);
        let have = match item.kind {
            super::inv::InvKind::Tx => blockchain.mempool_contains(&item.hash),
            super::inv::InvKind::Block => blockchain.chain_contains_block(&hex::encode(item.hash)),
        };
        if !have {
            missing.push(*item);
        }
    }
    if missing.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![GossipMessage::GetData(GetData { items: missing })])
    }
}

/// `handle_get_data` — a peer requested `get_data`; answer each known item
/// with the full `TX`/`BLOCK` and mark it known-for-`from`; lump the rest
/// into one `NOTFOUND`.
///
/// Note: txid lookup covers the mempool only — a tx already mined in a
/// canonical block is not retrievable by txid through `BlockchainView`
/// (`chain_contains_tx` needs the full transaction); the peer falls back to
/// NOTFOUND + its own sync path. Documented S2-P13 scope.
pub fn handle_get_data(
    get_data: &GetData,
    from: NodeId,
    state: &mut GossipState,
    blockchain: &dyn BlockchainView,
) -> Vec<GossipMessage> {
    let mut out: Vec<GossipMessage> = Vec::new();
    let mut missing: Vec<InventoryItem> = Vec::new();
    for item in &get_data.items {
        match item.kind {
            super::inv::InvKind::Tx => match blockchain.mempool_tx(&item.hash) {
                Some(tx) => {
                    state.mark_known(from, *item);
                    out.push(GossipMessage::Tx(tx));
                }
                None => missing.push(*item),
            },
            super::inv::InvKind::Block => match blockchain.block_by_hash(&hex::encode(item.hash)) {
                Some(block) => {
                    state.mark_known(from, *item);
                    out.push(GossipMessage::Block(block));
                }
                None => missing.push(*item),
            },
        }
    }
    if !missing.is_empty() {
        out.push(GossipMessage::NotFound(NotFound { items: missing }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gossip_state_marks_and_forgets() {
        let mut state = GossipState::new();
        let peer = NodeId::from_public_bytes([1u8; 32]);
        let item = InventoryItem::tx([9u8; 32]);
        assert!(!state.knows(&peer, &item));
        state.mark_known(peer, item);
        assert!(state.knows(&peer, &item));
        state.forget_peer(&peer);
        assert!(!state.knows(&peer, &item));
    }

    #[test]
    fn known_set_clears_on_overflow() {
        let mut state = GossipState::new();
        let peer = NodeId::from_public_bytes([2u8; 32]);
        for i in 0..KNOWN_SET_CAP {
            let mut hash = [0u8; 32];
            hash[0..4].copy_from_slice(&(i as u32).to_be_bytes());
            state.mark_known(peer, InventoryItem::tx(hash));
        }
        // At capacity the next insert clears first — the cap is a bound on
        // memory, not a no-drop guarantee.
        let overflow = InventoryItem::block([0xAB; 32]);
        state.mark_known(peer, overflow);
        assert!(state.knows(&peer, &overflow));
        // The set restarts: previously tracked items may be gone.
        assert!(state.known[&peer].len() <= KNOWN_SET_CAP);
    }
}
