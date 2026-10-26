//! S2-P13 — INV inventory announcement (Bitcoin-style gossip).
//!
//! Wire message `INV [0x06]` continues the binary tag space of `protocol.rs`
//! (0x01..0x04 sync, 0x05 ADDR): `[0x06][count: u32 BE][item]…` where
//! `item = [kind: u8][hash: 32]`. The count is bounded by
//! [`MAX_INV_BATCH`] and checked **before** any allocation (invariant #16).
//!
//! The same item shape is shared with `GET_DATA`/`NOTFOUND`
//! ([`super::get_data`]) and with the gossip responses: a `TX` is a
//! single-tag signed-transaction message, a `BLOCK` reuses the existing
//! `BLOCKS [0x04]` codec with `count = 1`.
//!
//! **Determinism** (Stage 2 checklist): pure byte shuffling — no clock, no
//! RNG, no I/O. Same input → same output on every node and every run.

use crate::error::NetError;

/// First payload byte of an INV message.
pub const MSG_INV: u8 = 0x06;

/// First payload byte of a `TX` gossip response (one signed transaction).
pub const MSG_TX: u8 = 0x08;

/// Upper bound of inventory items per INV/GET_DATA/NOTFOUND message —
/// enforced on decode before any allocation and on encode (invariant #16).
pub const MAX_INV_BATCH: usize = 512;

/// Wire size of one inventory item: `kind(1) + hash(32)`.
pub const INV_ITEM_WIRE_LEN: usize = 33;

/// What an inventory hash refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InvKind {
    /// A pending transaction, hash = txid.
    Tx,
    /// A canonical block, hash = block hash (32 raw bytes of the hex hash).
    Block,
}

impl InvKind {
    pub const WIRE_TX: u8 = 0x01;
    pub const WIRE_BLOCK: u8 = 0x02;

    pub fn to_wire(self) -> u8 {
        match self {
            InvKind::Tx => Self::WIRE_TX,
            InvKind::Block => Self::WIRE_BLOCK,
        }
    }

    pub fn from_wire(byte: u8) -> Option<Self> {
        match byte {
            Self::WIRE_TX => Some(InvKind::Tx),
            Self::WIRE_BLOCK => Some(InvKind::Block),
            _ => None,
        }
    }
}

/// One announced object: its kind plus the 32-byte hash identifying it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InventoryItem {
    pub kind: InvKind,
    pub hash: [u8; 32],
}

impl InventoryItem {
    pub fn tx(txid: [u8; 32]) -> Self {
        InventoryItem {
            kind: InvKind::Tx,
            hash: txid,
        }
    }

    pub fn block(block_hash: [u8; 32]) -> Self {
        InventoryItem {
            kind: InvKind::Block,
            hash: block_hash,
        }
    }
}

/// `INV [0x06] [count: u32 BE] [item]…` — announce the objects we have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inv {
    pub items: Vec<InventoryItem>,
}

impl Inv {
    pub fn encode(&self) -> Result<Vec<u8>, NetError> {
        encode_inv_items(MSG_INV, &self.items)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, NetError> {
        Ok(Inv {
            items: parse_inv_items(MSG_INV, bytes, "INV")?,
        })
    }
}

/// Encode `[tag][count][items]…` — the shared INV/GET_DATA/NOTFOUND shape.
pub(crate) fn encode_inv_items(tag: u8, items: &[InventoryItem]) -> Result<Vec<u8>, NetError> {
    if items.len() > MAX_INV_BATCH {
        return Err(NetError::Protocol(format!(
            "inv batch: {} items > {MAX_INV_BATCH}",
            items.len()
        )));
    }
    let mut out = Vec::with_capacity(5 + items.len() * INV_ITEM_WIRE_LEN);
    out.push(tag);
    out.extend_from_slice(&(items.len() as u32).to_be_bytes());
    for item in items {
        out.push(item.kind.to_wire());
        out.extend_from_slice(&item.hash);
    }
    Ok(out)
}

/// Decode `[tag][count][items]…` with a tag/batch/truncation discipline
/// mirroring `parse_addr` (invariant #16: bound the count before reserving).
pub(crate) fn parse_inv_items(
    expected_tag: u8,
    bytes: &[u8],
    what: &'static str,
) -> Result<Vec<InventoryItem>, NetError> {
    let malformed = |detail: &'static str| NetError::Protocol(format!("{what}: {detail}"));
    if bytes.len() < 5 || bytes[0] != expected_tag {
        return Err(malformed("bad tag or missing count"));
    }
    let count = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    if count > MAX_INV_BATCH {
        return Err(NetError::Protocol(format!(
            "{what}: count {count} > {MAX_INV_BATCH}"
        )));
    }
    if count * INV_ITEM_WIRE_LEN != bytes.len() - 5 {
        return Err(malformed("count exceeds payload length"));
    }
    let mut items = Vec::with_capacity(count);
    let mut offset = 5;
    for _ in 0..count {
        let kind =
            InvKind::from_wire(bytes[offset]).ok_or_else(|| malformed("unknown item kind"))?;
        offset += 1;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes[offset..offset + 32]);
        offset += 32;
        items.push(InventoryItem { kind, hash });
    }
    Ok(items)
}

/// `TX [0x08] [len: u32 BE] [signed tx bytes]` — one gossip transaction
/// response. Length is checked against [`crate::protocol::MAX_TX_SIZE`]
/// before the payload is allocated (invariant #16).
pub fn encode_tx(tx: &strangecoin_core::types::Transaction) -> Result<Vec<u8>, NetError> {
    let body = strangecoin_core::serialize::serialize_transaction_signed(tx);
    if body.len() > crate::protocol::MAX_TX_SIZE {
        return Err(NetError::Protocol(format!(
            "tx size {} > {}",
            body.len(),
            crate::protocol::MAX_TX_SIZE
        )));
    }
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(MSG_TX);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn parse_tx(bytes: &[u8]) -> Result<strangecoin_core::types::Transaction, NetError> {
    let malformed = |detail: &'static str| NetError::Protocol(format!("tx message: {detail}"));
    if bytes.len() < 5 || bytes[0] != MSG_TX {
        return Err(malformed("bad tag or missing length"));
    }
    let len = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    if len > crate::protocol::MAX_TX_SIZE {
        return Err(NetError::Protocol(format!(
            "tx message: length {len} > {}",
            crate::protocol::MAX_TX_SIZE
        )));
    }
    if len != bytes.len() - 5 {
        return Err(malformed("length exceeds payload length"));
    }
    strangecoin_core::serialize::deserialize_transaction_signed(&bytes[5..])
        .map_err(|_| malformed("tx decode"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inv_roundtrip() {
        let inv = Inv {
            items: vec![
                InventoryItem::tx([7u8; 32]),
                InventoryItem::block([9u8; 32]),
            ],
        };
        let encoded = inv.encode().unwrap();
        assert_eq!(encoded[0], MSG_INV);
        assert_eq!(Inv::parse(&encoded).unwrap(), inv);
    }

    #[test]
    fn inv_rejects_oversized_count_before_allocation() {
        let mut payload = vec![MSG_INV];
        payload.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(Inv::parse(&payload).is_err());
    }

    #[test]
    fn inv_rejects_unknown_kind_and_trailing_garbage() {
        let mut payload = vec![MSG_INV, 0, 0, 0, 1, 0x7f];
        payload.extend_from_slice(&[0u8; 32]);
        assert!(Inv::parse(&payload).is_err());

        let mut good = Inv {
            items: vec![InventoryItem::tx([1u8; 32])],
        }
        .encode()
        .unwrap();
        good.push(0xde);
        assert!(Inv::parse(&good).is_err());
        good.pop();
        good.pop(); // cut one hash byte → count/payload mismatch
        assert!(Inv::parse(&good).is_err());
    }

    #[test]
    fn inv_rejects_wrong_tag() {
        let mut payload = vec![MSG_TX, 0, 0, 0, 0];
        assert!(Inv::parse(&payload).is_err());
        payload[0] = MSG_INV;
        assert!(Inv::parse(&payload).is_ok());
    }
}
