//! S2-P13 — Bitcoin-style gossip: INV/GET_DATA announce + broadcast.
//!
//! Wire flow (ARCHITECT3 §3.6, ROADMAP3 Этап 2 DoD «Gossip работает»):
//!
//! 1. a node with a new object broadcasts `INV [0x06]` to its connected,
//!    non-banned peers ([`broadcast::announce_tx`] /
//!    [`broadcast::announce_block`]);
//! 2. a peer that does not have the object answers `GET_DATA [0x07]`
//!    ([`broadcast::handle_inv`]);
//! 3. the owner answers `TX [0x08]` or `BLOCKS [0x04]` per known item and
//!    lumps the rest into one `NOTFOUND [0x0A]`
//!    ([`broadcast::handle_get_data`]).
//!
//! INV deduplication is per-peer ([`GossipState`]): the announcer skips
//! peers that already know an object, and `handle_inv` never requests an
//! object the local node already has.
//!
//! Compact blocks (BIP 152, S2-P14) and Erlay reconciliation (ADR-0021,
//! S2-P15) extend this module in the later Stage 2 prompts.
//!
//! **Determinism** (Stage 2 checklist, threat V-21 extension): codecs are
//! pure byte shuffling; announce iterates `node_id`-sorted connected peers
//! (determinism #4); no clock, no RNG, no I/O anywhere in this module.

pub mod broadcast;
pub mod get_data;
pub mod inv;

pub use broadcast::{announce_block, announce_tx, handle_get_data, handle_inv, GossipState};
pub use get_data::{GetData, NotFound};
pub use inv::{Inv, InvKind, InventoryItem};

use crate::error::NetError;
use strangecoin_core::types::{Block, Transaction};

/// One gossip-layer message this node can send or receive.
///
/// No `PartialEq`: `Block` does not implement it — compare via
/// [`GossipMessage::encode`] round-trips instead.
#[derive(Debug, Clone)]
pub enum GossipMessage {
    /// `INV [0x06]` — announce objects we have.
    Inv(Inv),
    /// `GET_DATA [0x07]` — request objects announced in an INV.
    GetData(GetData),
    /// `TX [0x08]` — one full signed transaction.
    Tx(Transaction),
    /// `BLOCKS [0x04]` with `count = 1` — one full block (the existing
    /// sync codec, reused so there is exactly one block wire format).
    Block(Block),
    /// `NOTFOUND [0x0A]` — requested objects we do not have.
    NotFound(NotFound),
}

impl GossipMessage {
    /// Encode to the wire form (tag byte + payload).
    pub fn encode(&self) -> Result<Vec<u8>, NetError> {
        match self {
            GossipMessage::Inv(msg) => msg.encode(),
            GossipMessage::GetData(msg) => msg.encode(),
            GossipMessage::Tx(tx) => inv::encode_tx(tx),
            GossipMessage::Block(block) => {
                crate::protocol::encode_blocks(std::slice::from_ref(block))
            }
            GossipMessage::NotFound(msg) => msg.encode(),
        }
    }

    /// Parse one wire payload produced by [`GossipMessage::encode`]. Unknown
    /// tags surface as [`NetError::Protocol`] — the caller scores them as a
    /// protocol violation (ADR-0022).
    pub fn parse(bytes: &[u8]) -> Result<Self, NetError> {
        let Some(&tag) = bytes.first() else {
            return Err(NetError::Protocol("gossip: empty payload".to_string()));
        };
        match tag {
            inv::MSG_INV => Ok(GossipMessage::Inv(Inv::parse(bytes)?)),
            get_data::MSG_GET_DATA => Ok(GossipMessage::GetData(GetData::parse(bytes)?)),
            inv::MSG_TX => Ok(GossipMessage::Tx(inv::parse_tx(bytes)?)),
            crate::protocol::MSG_BLOCKS => {
                let mut blocks = crate::protocol::parse_blocks(bytes)?;
                if blocks.len() != 1 {
                    return Err(NetError::Protocol(format!(
                        "gossip block message: count {} != 1",
                        blocks.len()
                    )));
                }
                Ok(GossipMessage::Block(blocks.pop().expect("len checked")))
            }
            get_data::MSG_NOTFOUND => Ok(GossipMessage::NotFound(NotFound::parse(bytes)?)),
            other => Err(NetError::Protocol(format!(
                "gossip: unknown message tag {other:#04x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strangecoin_core::types::TransferTx;

    fn sample_tx() -> Transaction {
        Transaction::Transfer(TransferTx {
            from: "rsc1sender".into(),
            to: "rsc1receiver".into(),
            amount: 7,
            fee: 0,
            nonce: 1,
            chain_id: 3,
            signature: vec![1u8; 65],
            is_coinbase: false,
        })
    }

    fn sample_block() -> Block {
        let mut block = Block {
            index: 3,
            timestamp: 1_700_000_001,
            transactions: vec![sample_tx()],
            previous_hash: "bb".repeat(32),
            hash: String::new(),
            nonce: 11,
            target: "ff".repeat(32),
            consensus_version: 1,
            state_root: [1u8; 32],
            tx_root: [2u8; 32],
            receipts_root: [0u8; 32],
        };
        block.hash = hex::encode(strangecoin_core::serialize::block_hash(&block));
        block
    }

    #[test]
    fn gossip_message_roundtrips_each_variant() {
        let messages = vec![
            GossipMessage::Inv(Inv {
                items: vec![InventoryItem::tx([1u8; 32])],
            }),
            GossipMessage::GetData(GetData {
                items: vec![InventoryItem::block([2u8; 32])],
            }),
            GossipMessage::Tx(sample_tx()),
            GossipMessage::Block(sample_block()),
            GossipMessage::NotFound(NotFound {
                items: vec![InventoryItem::tx([3u8; 32])],
            }),
        ];
        for message in messages {
            let encoded = message.encode().unwrap();
            let decoded = GossipMessage::parse(&encoded).unwrap();
            // `Block` has no PartialEq — compare by re-encoding.
            assert_eq!(decoded.encode().unwrap(), encoded);
        }
    }

    #[test]
    fn gossip_block_message_requires_single_block() {
        // A two-block BLOCKS payload is valid sync wire but not gossip.
        let block = sample_block();
        let two = crate::protocol::encode_blocks(&[block.clone(), block]).unwrap();
        assert!(GossipMessage::parse(&two).is_err());
    }

    #[test]
    fn gossip_rejects_empty_and_unknown_tags() {
        assert!(GossipMessage::parse(&[]).is_err());
        assert!(GossipMessage::parse(&[0x7f, 0, 0, 0, 0]).is_err());
    }
}
