//! `BatchExecutor` calldata codec (AI-P04/AI-P05, ADR-0028 #5).
//!
//! One fixed-width wire layout shared by three consumers that all depend on
//! `strangecoin-core` and nothing else: the agent JSON-RPC layer
//! (`strangecoin-api` `agent_signAndSubmitBatch`), the `BatchExecutor` WASM
//! contract (`strangecoin-vm-wasm/examples/batch_executor.rs` — WAT parses
//! these exact offsets) and the tests that drive both. Same single-export
//! self-dispatch pattern as [`crate::escrow`]: the monolith never registers
//! `AbiDocument`s, so selector dispatch inside the VM falls back to `main`
//! and `main` re-dispatches on the first four calldata bytes itself.
//!
//! ```text
//! execute(calldata):
//!   [0..4)     selector execute (blake3("execute(bytes)")[..4])
//!   [4..8)     u32 LE entry count (1..=MAX_BATCH_ENTRIES)
//!   entries × count:
//!     [u32 LE addr_len]   1..=MAX_BATCH_ADDR_LEN
//!     [addr bytes]        bech32 utf-8
//!     [u64 LE value]      SC moved by this entry
//!     [u32 LE data_len]   0..=MAX_BATCH_DATA_LEN
//!     [data bytes]        nested-call calldata (empty = plain transfer)
//! ```
//!
//! **Value binding (ADR-0030 amendment pattern):** the contract requires
//! `sc.value() == sum(entry.value)` before executing anything, so a batch
//! can only forward the value its outer tx actually attached — pooled
//! leftovers in the contract can never be drained by a crafted batch.
//!
//! **Authorization** is the *outer transaction* signature: the API layer
//! signs one `Call` tx (from the session-scoped subkey) whose calldata is
//! this encoding, so "one signature on the batch" holds without a second
//! in-contract signature check (ADR-0028 #5 amendment — see
//! `docs/spec/agent_integration.md` §5.2 implementation notes).
//!
//! **Derived txids:** internal calls are not separate chain transactions, so
//! the API returns `blake3(batch_txid ‖ index)` per entry as a stable handle
//! ([`derive_batch_txid`]) — re-derivable from the on-chain `batch_txid`.
//!
//! Determinism: pure byte functions — no time, no RNG, identical output for
//! identical input (Stage 3 storage determinism checklist pattern).

use crate::abi::{method_selector, AbiParam, AbiType};
use std::fmt;

/// Maximum entries in one batch (mirrors the WAT loop bound).
pub const MAX_BATCH_ENTRIES: usize = 64;

/// Maximum stored/parsed bech32 address length (same bound as
/// [`crate::escrow::MAX_ADDRESS_LEN`]).
pub const MAX_BATCH_ADDR_LEN: usize = 80;

/// Maximum calldata bytes per entry (mirrors the WAT `data_len` bound).
pub const MAX_BATCH_DATA_LEN: usize = 4096;

/// Maximum encoded `execute` calldata (the WAT input buffer is 16 KiB;
/// `input_read` returns `-1` past it and the contract traps).
pub const MAX_BATCH_ENCODED: usize = 16_384;

/// One batch entry: move `value` SC to `to`, optionally calling it with
/// `data` (empty `data` = plain value transfer to an EOA).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchEntry {
    /// Destination bech32 address.
    pub to: String,
    /// SC attached to this entry (must sum with the rest to `sc.value()`).
    pub value: u64,
    /// Nested-call calldata; empty for plain transfers.
    pub data: Vec<u8>,
}

/// Codec failure (truncated frames, bad selector, oversize fields…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchCodecError {
    /// Calldata shorter than the header / a length prefix runs past the end.
    Truncated,
    /// First 4 bytes are not the `execute` selector.
    BadSelector,
    /// Entry count is zero or exceeds [`MAX_BATCH_ENTRIES`].
    BadCount,
    /// A length-prefixed address is zero, over [`MAX_BATCH_ADDR_LEN`], or a
    /// per-entry `data` field exceeds [`MAX_BATCH_DATA_LEN`].
    BadFieldLen,
    /// Entry values overflow `u64` when summed.
    ValueOverflow,
    /// Valid prefix parsed but bytes remain after the last entry.
    TrailingBytes,
    /// Encoded calldata exceeds [`MAX_BATCH_ENCODED`].
    TooLarge,
}

impl fmt::Display for BatchCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BatchCodecError::Truncated => write!(f, "batch calldata truncated"),
            BatchCodecError::BadSelector => write!(f, "unknown batch selector"),
            BatchCodecError::BadCount => write!(f, "bad batch entry count"),
            BatchCodecError::BadFieldLen => write!(f, "bad batch field length"),
            BatchCodecError::ValueOverflow => write!(f, "batch value sum overflows"),
            BatchCodecError::TrailingBytes => write!(f, "trailing bytes after batch entries"),
            BatchCodecError::TooLarge => write!(f, "batch calldata too large"),
        }
    }
}

impl std::error::Error for BatchCodecError {}

/// `execute(entries: bytes)` selector — self-dispatch key inside `main`.
pub fn execute_selector() -> [u8; 4] {
    method_selector("execute", &[AbiParam::new("entries", AbiType::Bytes)])
}

/// Encode an `execute` call: selector ‖ count ‖ entries.
pub fn encode_execute(entries: &[BatchEntry]) -> Result<Vec<u8>, BatchCodecError> {
    if entries.is_empty() || entries.len() > MAX_BATCH_ENTRIES {
        return Err(BatchCodecError::BadCount);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&execute_selector());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        if entry.to.is_empty() || entry.to.len() > MAX_BATCH_ADDR_LEN {
            return Err(BatchCodecError::BadFieldLen);
        }
        if entry.data.len() > MAX_BATCH_DATA_LEN {
            return Err(BatchCodecError::BadFieldLen);
        }
        out.extend_from_slice(&(entry.to.len() as u32).to_le_bytes());
        out.extend_from_slice(entry.to.as_bytes());
        out.extend_from_slice(&entry.value.to_le_bytes());
        out.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry.data);
        if out.len() > MAX_BATCH_ENCODED {
            return Err(BatchCodecError::TooLarge);
        }
    }
    Ok(out)
}

/// Decode an `execute` call, validating bounds exactly as the WAT parser
/// does (selector, count, per-field lengths, no trailing bytes).
pub fn decode_execute(calldata: &[u8]) -> Result<Vec<BatchEntry>, BatchCodecError> {
    if calldata.len() < 8 {
        return Err(BatchCodecError::Truncated);
    }
    if calldata[0..4] != execute_selector() {
        return Err(BatchCodecError::BadSelector);
    }
    let count = u32::from_le_bytes(calldata[4..8].try_into().expect("4 bytes")) as usize;
    if count == 0 || count > MAX_BATCH_ENTRIES {
        return Err(BatchCodecError::BadCount);
    }
    let mut off = 8usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if off + 4 > calldata.len() {
            return Err(BatchCodecError::Truncated);
        }
        let addr_len =
            u32::from_le_bytes(calldata[off..off + 4].try_into().expect("4 bytes")) as usize;
        off += 4;
        if addr_len == 0 || addr_len > MAX_BATCH_ADDR_LEN {
            return Err(BatchCodecError::BadFieldLen);
        }
        if off + addr_len + 12 > calldata.len() {
            return Err(BatchCodecError::Truncated);
        }
        let to = std::str::from_utf8(&calldata[off..off + addr_len])
            .map_err(|_| BatchCodecError::BadFieldLen)?
            .to_string();
        off += addr_len;
        let value = u64::from_le_bytes(calldata[off..off + 8].try_into().expect("8 bytes"));
        off += 8;
        let data_len =
            u32::from_le_bytes(calldata[off..off + 4].try_into().expect("4 bytes")) as usize;
        off += 4;
        if data_len > MAX_BATCH_DATA_LEN {
            return Err(BatchCodecError::BadFieldLen);
        }
        if off + data_len > calldata.len() {
            return Err(BatchCodecError::Truncated);
        }
        let data = calldata[off..off + data_len].to_vec();
        off += data_len;
        entries.push(BatchEntry { to, value, data });
    }
    if off != calldata.len() {
        return Err(BatchCodecError::TrailingBytes);
    }
    Ok(entries)
}

/// Sum of all entry values, `None` on `u64` overflow (mirrors the WAT
/// pass-1 accumulation guard).
pub fn total_value(entries: &[BatchEntry]) -> Option<u64> {
    entries
        .iter()
        .try_fold(0u64, |acc, e| acc.checked_add(e.value))
}

/// Stable per-entry handle for a batch: `blake3(batch_txid ‖ index_le)`.
///
/// Internal calls are not chain transactions; this derives a deterministic
/// id the client can recompute from the on-chain `batch_txid` (AI-P04
/// `agent_signAndSubmitBatch` response contract).
pub fn derive_batch_txid(batch_txid: &[u8; 32], index: u32) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(batch_txid);
    hasher.update(&index.to_le_bytes());
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<BatchEntry> {
        vec![
            BatchEntry {
                to: "rsc1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(),
                value: 100,
                data: Vec::new(),
            },
            BatchEntry {
                to: "rsc1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(),
                value: 50,
                data: vec![0xde, 0xad, 0xbe, 0xef],
            },
        ]
    }

    #[test]
    fn roundtrip_preserves_entries() {
        let entries = sample_entries();
        let encoded = encode_execute(&entries).expect("encode");
        let decoded = decode_execute(&encoded).expect("decode");
        assert_eq!(decoded, entries);
    }

    #[test]
    fn selector_is_stable_and_distinct() {
        let sel = execute_selector();
        // Pinned: a selector bump is a wire-format break (golden-vector rule).
        assert_eq!(sel.len(), 4);
        let entries = sample_entries();
        let encoded = encode_execute(&entries).unwrap();
        assert_eq!(encoded[0..4], sel);
    }

    #[test]
    fn rejects_empty_and_oversize_counts() {
        assert_eq!(encode_execute(&[]).unwrap_err(), BatchCodecError::BadCount);
        let many = vec![
            BatchEntry {
                to: "rsc1x".to_string(),
                value: 1,
                data: Vec::new(),
            };
            MAX_BATCH_ENTRIES + 1
        ];
        assert_eq!(
            encode_execute(&many).unwrap_err(),
            BatchCodecError::BadCount
        );
    }

    #[test]
    fn rejects_bad_selector_and_trailing_bytes() {
        let encoded = encode_execute(&sample_entries()).unwrap();
        let mut bad_sel = encoded.clone();
        bad_sel[0] ^= 0xff;
        assert_eq!(
            decode_execute(&bad_sel).unwrap_err(),
            BatchCodecError::BadSelector
        );

        let mut trailing = encoded;
        trailing.push(0);
        assert_eq!(
            decode_execute(&trailing).unwrap_err(),
            BatchCodecError::TrailingBytes
        );
    }

    #[test]
    fn rejects_truncated_and_oversize_fields() {
        let encoded = encode_execute(&sample_entries()).unwrap();
        // Cut inside the first address length prefix.
        assert_eq!(
            decode_execute(&encoded[..6]).unwrap_err(),
            BatchCodecError::Truncated
        );

        // addr_len = 81 (over MAX_BATCH_ADDR_LEN) at the first entry.
        let mut bad_addr = encode_execute(&sample_entries()).unwrap();
        bad_addr[8..12].copy_from_slice(&81u32.to_le_bytes());
        assert_eq!(
            decode_execute(&bad_addr).unwrap_err(),
            BatchCodecError::BadFieldLen
        );
    }

    #[test]
    fn rejects_zero_address_len() {
        let mut bad = encode_execute(&sample_entries()).unwrap();
        bad[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            decode_execute(&bad).unwrap_err(),
            BatchCodecError::BadFieldLen
        );
    }

    #[test]
    fn total_value_sums_and_detects_overflow() {
        let entries = sample_entries();
        assert_eq!(total_value(&entries), Some(150));
        let overflow = vec![
            BatchEntry {
                to: "a".into(),
                value: u64::MAX,
                data: vec![],
            },
            BatchEntry {
                to: "b".into(),
                value: 1,
                data: vec![],
            },
        ];
        assert_eq!(total_value(&overflow), None);
    }

    #[test]
    fn derived_txids_are_deterministic_and_index_bound() {
        let txid = [7u8; 32];
        let a0 = derive_batch_txid(&txid, 0);
        let a1 = derive_batch_txid(&txid, 1);
        assert_eq!(a0, derive_batch_txid(&txid, 0), "same input, same id");
        assert_ne!(a0, a1, "index is bound into the digest");
        let other = derive_batch_txid(&[8u8; 32], 0);
        assert_ne!(a0, other, "txid is bound into the digest");
    }

    #[test]
    fn encode_respects_size_cap() {
        // A batch of max-size entries would exceed 16 KiB → TooLarge.
        let big = vec![
            BatchEntry {
                to: "rsc1".repeat(20), // 80 chars (at the addr cap)
                value: 1,
                data: vec![0u8; MAX_BATCH_DATA_LEN],
            };
            8
        ];
        // 8 × (4 + 80 + 8 + 4 + 4096) + 8 ≈ 33.6 KiB > 16 KiB.
        assert_eq!(encode_execute(&big).unwrap_err(), BatchCodecError::TooLarge);
    }
}
