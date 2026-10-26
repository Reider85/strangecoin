//! S2-P13 — GET_DATA request + NOTFOUND response.
//!
//! `GET_DATA [0x07]` and `NOTFOUND [0x0A]` share the exact item shape of
//! `INV [0x06]` ([`super::inv`]): a peer that receives an INV for objects it
//! lacks answers with GET_DATA; the owner answers `TX [0x08]`/`BLOCKS [0x04]`
//! per known item and lumps everything it does not have into one NOTFOUND.
//!
//! **Determinism**: pure codec, no clock/RNG/I/O.

use super::inv::{encode_inv_items, parse_inv_items, InventoryItem};

use crate::error::NetError;

/// First payload byte of a GET_DATA message.
pub const MSG_GET_DATA: u8 = 0x07;

/// First payload byte of a NOTFOUND message.
pub const MSG_NOTFOUND: u8 = 0x0A;

/// `GET_DATA [0x07] [count: u32 BE] [item]…` — request full objects by
/// inventory hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetData {
    pub items: Vec<InventoryItem>,
}

impl GetData {
    pub fn encode(&self) -> Result<Vec<u8>, NetError> {
        encode_inv_items(MSG_GET_DATA, &self.items)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, NetError> {
        Ok(GetData {
            items: parse_inv_items(MSG_GET_DATA, bytes, "GET_DATA")?,
        })
    }
}

/// `NOTFOUND [0x0A] [count: u32 BE] [item]…` — the requested objects we do
/// not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotFound {
    pub items: Vec<InventoryItem>,
}

impl NotFound {
    pub fn encode(&self) -> Result<Vec<u8>, NetError> {
        encode_inv_items(MSG_NOTFOUND, &self.items)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, NetError> {
        Ok(NotFound {
            items: parse_inv_items(MSG_NOTFOUND, bytes, "NOTFOUND")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_data_roundtrip() {
        let msg = GetData {
            items: vec![InventoryItem::tx([3u8; 32])],
        };
        let encoded = msg.encode().unwrap();
        assert_eq!(encoded[0], MSG_GET_DATA);
        assert_eq!(GetData::parse(&encoded).unwrap(), msg);
    }

    #[test]
    fn notfound_roundtrip() {
        let msg = NotFound {
            items: vec![
                InventoryItem::tx([1u8; 32]),
                InventoryItem::block([2u8; 32]),
            ],
        };
        let encoded = msg.encode().unwrap();
        assert_eq!(encoded[0], MSG_NOTFOUND);
        assert_eq!(NotFound::parse(&encoded).unwrap(), msg);
    }

    #[test]
    fn get_data_and_notfound_do_not_cross_parse() {
        let msg = GetData {
            items: vec![InventoryItem::tx([3u8; 32])],
        };
        let encoded = msg.encode().unwrap();
        assert!(NotFound::parse(&encoded).is_err());
    }

    #[test]
    fn get_data_rejects_oversized_count() {
        let mut payload = vec![MSG_GET_DATA];
        payload.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(GetData::parse(&payload).is_err());
    }
}
