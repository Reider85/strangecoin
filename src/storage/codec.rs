//! Value codecs at the monolith edge (ADR-0024 §Decision "Values").
//!
//! The engine stores opaque bytes ([`strangecoin_storage::StorageEngine`]);
//! encoding/decoding of consensus types happens here, one layer above the
//! trait — the storage crate never sees a `Block` / `AccountState` /
//! `Transaction`.
//!
//! | Value | Codec | Why |
//! |---|---|---|
//! | block | [`serialize_block`](strangecoin_core::serialize) | canonical binary (project constraint) |
//! | mempool tx | `serialize_transaction_signed` | canonical, carries the signature (txid preimage) |
//! | account (state + state_history) | serde_json flat form | *Deviation (S3-P07):* ADR-0024 says "canonical bytes", but core has no binary `Account` codec; the flat `{balance,nonce,code_hash?,storage_root?}` form is a fixed-order struct encoding — deterministic, and the value is derived state (invariant #1), never a consensus preimage |
//! | contract slot | raw bytes | the value *is* the slot; the key carries `address ‖ slot` |
//! | tip height / difficulty / chain id | big-endian integers | fixed width, matches the schema's BE rule |
//! | tip hash | 32 raw bytes of the hex `Block::hash` | ADR-0024 meta table: "32-byte value" |
//!
//! Determinism (Stage 3 checklist #5): every function here is a pure
//! byte transform — no clock, no RNG, no I/O. Decode failures map to
//! [`StorageError::Corrupt`] (fail-closed, never "repair").

use strangecoin_core::types::{Block, Transaction};
use strangecoin_storage::StorageError;

use crate::AccountState;

fn corrupt(msg: &'static str) -> StorageError {
    StorageError::Corrupt(msg)
}

// ------------------------------------------------------------------ blocks

pub(crate) fn encode_block(block: &Block) -> Vec<u8> {
    strangecoin_core::serialize::serialize_block(block)
}

pub(crate) fn decode_block(bytes: &[u8]) -> Result<Block, StorageError> {
    strangecoin_core::serialize::deserialize_block(bytes)
        .map_err(|_| corrupt("block decode failed"))
}

// ------------------------------------------------------------ transactions

pub(crate) fn encode_tx(tx: &Transaction) -> Vec<u8> {
    strangecoin_core::serialize::serialize_transaction_signed(tx)
}

pub(crate) fn decode_tx(bytes: &[u8]) -> Result<Transaction, StorageError> {
    strangecoin_core::serialize::deserialize_transaction_signed(bytes)
        .map_err(|_| corrupt("transaction decode failed"))
}

// ---------------------------------------------------------------- accounts

/// serde_json of a fixed-order struct: encoding a primitive-field enum
/// struct cannot fail (documented in the module table).
pub(crate) fn encode_account(account: &AccountState) -> Vec<u8> {
    serde_json::to_vec(account).expect("AccountState serialization cannot fail")
}

pub(crate) fn decode_account(bytes: &[u8]) -> Result<AccountState, StorageError> {
    serde_json::from_slice(bytes).map_err(|_| corrupt("account decode failed"))
}

// ----------------------------------------------------------- meta scalars

pub(crate) fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

pub(crate) fn decode_u64(bytes: &[u8]) -> Result<u64, StorageError> {
    let arr: [u8; 8] = bytes
        .try_into()
        .map_err(|_| corrupt("u64 value wrong length"))?;
    Ok(u64::from_be_bytes(arr))
}

pub(crate) fn encode_u32(value: u32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

pub(crate) fn decode_u32(bytes: &[u8]) -> Result<u32, StorageError> {
    let arr: [u8; 4] = bytes
        .try_into()
        .map_err(|_| corrupt("u32 value wrong length"))?;
    Ok(u32::from_be_bytes(arr))
}

/// `Block::hash` (hex blake3) → the ADR-0024 32-byte meta value.
pub(crate) fn encode_hash(hash: &str) -> Result<Vec<u8>, StorageError> {
    hex::decode(hash).map_err(|_| corrupt("tip hash is not hex"))
}

pub(crate) fn decode_hash(bytes: &[u8]) -> Result<String, StorageError> {
    if bytes.len() != 32 {
        return Err(corrupt("tip hash wrong length"));
    }
    Ok(hex::encode(bytes))
}
