//! Snapshot file format, export/import and commitment verification
//! (S3-P13, ADR-0026).
//!
//! A single-file, canonical-binary snapshot with a commitment header, a
//! lexicographically sorted body and a whole-file blake3 trailer. Four-step
//! verification (checksum → network id → state-root recomputation →
//! tip-block hash match) runs **before** anything is written, closing the
//! forged-snapshot vector from Stage 2 residual §1 by construction.
//!
//! ## Format (ADR-0026 Decision + S3-P13 deviations)
//!
//! | Section | Contents |
//! |---|---|
//! | Header (117 B) | magic `SCSNAP01`, `format_version=1`, `network_id` u32 BE, `height` u64 BE, `block_hash` [u8;32], `state_root` [u8;32], `receipts_root` [u8;32] |
//! | Body — accounts | `count` u64 BE, then per record: `addr_len` u16 BE ‖ address ‖ `value_len` u32 BE ‖ value; sorted lexicographically by address (CF iteration order) |
//! | Body — contract storage | `count` u64 BE, then per record: `addr_len` u16 BE ‖ address ‖ slot [u8;32] ‖ `value_len` u32 BE ‖ value; sorted by (address, slot); `KeyPrefix::ContractStorageRoot` cache keys are **excluded** |
//! | Body — recent blocks | `count` u64 BE, then per block: `len` u32 BE ‖ canonical `serialize_block`; ascending heights covering `tip − min(tip, MTP_WINDOW−1) ..= tip` |
//! | Trailer (32 B) | blake3(header ‖ body) |
//!
//! **Deviations from the literal ADR-0026 body listing (FBP, fixed in the
//! S3-P13 commit):**
//!
//! 1. **Recent-blocks section.** The ADR body lists only accounts +
//!    contract storage. Applying block `height+1` after import requires
//!    (a) the parent block for `validate_position` and (b) the last
//!    `MEDIAN_TIME_WINDOW` (11) blocks for `median_time_past` — with only
//!    the tip block, the first post-import block fails
//!    `TimestampTooOld` whenever its timestamp equals the tip's (fast
//!    mining in the same second). The export therefore carries the tail
//!    window `tip−10 ..= tip` (or `0 ..= tip` for short chains);
//!    `chain_base_height` becomes that window's start. Step (d) of the
//!    verification (`tip_block.hash == header.block_hash`) is checked
//!    locally at import time rather than only after a network catch-up.
//! 2. **Account body encoding = raw state-CF bytes** (serde_json flat
//!    form, the same encoding `codec::encode_account` writes — S3-P07
//!    documented deviation: core has no binary Account codec). The ADR
//!    says "canonical binary from strangecoin-core"; a new binary codec
//!    would diverge from what the DB actually stores without improving
//!    integrity (the blake3 trailer covers the bytes either way). Verify
//!    step (c) decodes these bytes into `core::Account` for
//!    `SparseMerkleTrie::compute_root`.
//!
//! ## Determinism (Stage 3 checklist)
//!
//! No `SystemTime`, no RNG, no wall-clock anywhere in this module. Body
//! order comes from the engine's sorted `iterate_prefix` contract. Two
//! exports of the same state at the same height produce **byte-identical**
//! files (header + body + trailer). Cadence (`--every N` on the CLI) is
//! height-triggered.
//!
//! ## Transport
//!
//! Stage 3 is file-based only (`sc snapshot export` / `sc snapshot
//! import`). P2P `GET_SNAPSHOT` is a Stage 4+ residual (ADR-0026
//! §Transport).

use std::collections::HashMap;
use std::path::Path;

use strangecoin_core::consensus::MEDIAN_TIME_WINDOW;
use strangecoin_core::serialize::serialize_block;
use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::types::{Account, Block};

use crate::engine::{Cf, StorageEngine, StorageError, WriteBatchOp};
use crate::schema::{
    block_by_height_key, block_hash_index_key, meta_key, KeyPrefix, META_CHAIN_BASE, META_TIP_HASH,
    META_TIP_HEIGHT,
};

/// Magic bytes at offset 0 of every `.scsnap` file (ADR-0026).
pub const SNAPSHOT_MAGIC: &[u8; 8] = b"SCSNAP01";
/// Current format version; unknown versions are rejected fail-closed
/// (same rail as ADR-0024 schema versioning).
pub const SNAPSHOT_FORMAT_VERSION: u8 = 1;
/// Fixed header size: 8 + 1 + 4 + 8 + 32 + 32 + 32.
pub const SNAPSHOT_HEADER_LEN: usize = 117;
/// Trailer size (blake3 output).
pub const SNAPSHOT_TRAILER_LEN: usize = 32;

/// Errors surfaced by snapshot export / verify / import.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("snapshot corrupt: {0}")]
    Corrupt(&'static str),

    #[error("snapshot checksum mismatch")]
    ChecksumMismatch,

    #[error("snapshot network id mismatch: file {found}, expected {expected}")]
    NetworkIdMismatch { found: u32, expected: u32 },

    #[error("snapshot state root mismatch: header {header:?}, recomputed {recomputed:?}")]
    StateRootMismatch {
        header: [u8; 32],
        recomputed: [u8; 32],
    },

    #[error("snapshot tip block hash mismatch: header {header}, block {block}")]
    TipBlockHashMismatch { header: String, block: String },

    #[error("snapshot format version {0} is not supported")]
    UnsupportedFormatVersion(u8),

    #[error("database already has a chain (tip meta present); import requires a fresh store")]
    NotFreshDatabase,

    #[error("no tip block in database")]
    NoTipBlock,
}

/// Fixed-size commitment header (ADR-0026 §Header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub format_version: u8,
    pub network_id: u32,
    pub height: u64,
    /// Hex-encoded `Block::hash` of the tip block (64 lowercase hex chars).
    pub block_hash: String,
    pub state_root: [u8; 32],
    pub receipts_root: [u8; 32],
}

impl SnapshotHeader {
    /// Encode the header as the fixed 117-byte on-disk form.
    pub fn encode(&self) -> [u8; SNAPSHOT_HEADER_LEN] {
        let mut out = [0u8; SNAPSHOT_HEADER_LEN];
        out[0..8].copy_from_slice(SNAPSHOT_MAGIC);
        out[8] = self.format_version;
        out[9..13].copy_from_slice(&self.network_id.to_be_bytes());
        out[13..21].copy_from_slice(&self.height.to_be_bytes());
        // block_hash is hex (64 chars) in the in-memory type; the file
        // stores the raw 32 bytes (ADR: `block_hash` = [u8; 32]).
        let raw = hex::decode(&self.block_hash).expect("block hash must be hex");
        let raw: [u8; 32] = raw.try_into().expect("block hash must be 32 bytes");
        out[21..53].copy_from_slice(&raw);
        out[53..85].copy_from_slice(&self.state_root);
        out[85..117].copy_from_slice(&self.receipts_root);
        out
    }

    /// Decode a header from the fixed 117-byte on-disk form.
    pub fn decode(bytes: &[u8]) -> Result<Self, SnapshotError> {
        if bytes.len() < SNAPSHOT_HEADER_LEN {
            return Err(SnapshotError::Corrupt("header too short"));
        }
        if &bytes[0..8] != SNAPSHOT_MAGIC {
            return Err(SnapshotError::Corrupt("bad magic"));
        }
        let format_version = bytes[8];
        if format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(SnapshotError::UnsupportedFormatVersion(format_version));
        }
        let network_id = u32::from_be_bytes(bytes[9..13].try_into().expect("4 bytes"));
        let height = u64::from_be_bytes(bytes[13..21].try_into().expect("8 bytes"));
        let block_hash = hex::encode(&bytes[21..53]);
        let state_root: [u8; 32] = bytes[53..85].try_into().expect("32 bytes");
        let receipts_root: [u8; 32] = bytes[85..117].try_into().expect("32 bytes");
        Ok(Self {
            format_version,
            network_id,
            height,
            block_hash,
            state_root,
            receipts_root,
        })
    }
}

/// Write a snapshot of `engine`'s current tip state to `out_path`.
///
/// `out_path` may be a directory (the ADR-0026 self-describing filename
/// `snapshot-{height}-{state_root_hex}.scsnap` is generated inside it) or
/// an explicit file path. Returns the header that was written.
pub fn export(
    engine: &dyn StorageEngine,
    out_path: &Path,
    network_id: u32,
) -> Result<SnapshotHeader, SnapshotError> {
    let tip_height = read_u64_meta(engine, META_TIP_HEIGHT)?.ok_or(SnapshotError::NoTipBlock)?;
    let tip_hash = read_hash_meta(engine, META_TIP_HASH)?.ok_or(SnapshotError::NoTipBlock)?;
    let tip_block = read_tip_block(engine, tip_height)?.ok_or(SnapshotError::NoTipBlock)?;
    if tip_block.hash != tip_hash {
        return Err(SnapshotError::Corrupt("tip hash does not match tip block"));
    }

    let header = SnapshotHeader {
        format_version: SNAPSHOT_FORMAT_VERSION,
        network_id,
        height: tip_height,
        block_hash: tip_block.hash.clone(),
        state_root: tip_block.state_root,
        receipts_root: tip_block.receipts_root,
    };

    // Body: accounts (sorted by CF contract), contract storage (sorted,
    // root-cache keys excluded), recent-block window for MTP + parent
    // linkage (deviation #1: tip−10 ..= tip, or 0 ..= tip if shorter).
    let accounts = engine.iterate_prefix(Cf::State, b"")?;
    let storage = engine.iterate_prefix(Cf::ContractStorage, b"")?;

    let window = MEDIAN_TIME_WINDOW as u64;
    let recent_from = tip_height.saturating_sub(window - 1);
    let mut recent_blocks: Vec<Block> = Vec::new();
    for h in recent_from..=tip_height {
        let blk =
            read_tip_block(engine, h)?.ok_or(SnapshotError::Corrupt("recent block missing"))?;
        recent_blocks.push(blk);
    }

    let mut body = Vec::new();
    body.extend_from_slice(&(accounts.len() as u64).to_be_bytes());
    for (key, value) in &accounts {
        let addr_len =
            u16::try_from(key.len()).map_err(|_| SnapshotError::Corrupt("address too long"))?;
        body.extend_from_slice(&addr_len.to_be_bytes());
        body.extend_from_slice(key);
        body.extend_from_slice(&(value.len() as u32).to_be_bytes());
        body.extend_from_slice(value);
    }
    let slot_count_src = storage
        .iter()
        .filter(|(k, _)| k.first() != Some(&KeyPrefix::ContractStorageRoot.byte()))
        .count();
    body.extend_from_slice(&(slot_count_src as u64).to_be_bytes());
    for (key, value) in storage
        .iter()
        .filter(|(k, _)| k.first() != Some(&KeyPrefix::ContractStorageRoot.byte()))
    {
        // key = address ‖ slot[32]
        if key.len() < 32 {
            return Err(SnapshotError::Corrupt("contract storage key too short"));
        }
        let (addr, slot) = key.split_at(key.len() - 32);
        let addr_len =
            u16::try_from(addr.len()).map_err(|_| SnapshotError::Corrupt("address too long"))?;
        body.extend_from_slice(&addr_len.to_be_bytes());
        body.extend_from_slice(addr);
        body.extend_from_slice(slot);
        body.extend_from_slice(&(value.len() as u32).to_be_bytes());
        body.extend_from_slice(value);
    }
    body.extend_from_slice(&(recent_blocks.len() as u64).to_be_bytes());
    for blk in &recent_blocks {
        let bytes = serialize_block(blk);
        body.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        body.extend_from_slice(&bytes);
    }

    let header_bytes = header.encode();
    let mut file = header_bytes.to_vec();
    file.extend_from_slice(&body);
    let checksum = blake3::hash(&file);
    file.extend_from_slice(checksum.as_bytes());

    let target = if out_path.is_dir() || (out_path.extension().is_none() && !out_path.exists()) {
        // Directory path (existing or to-be-created): generate the
        // self-describing filename inside it.
        std::fs::create_dir_all(out_path)?;
        out_path.join(default_filename(&header))
    } else {
        out_path.to_path_buf()
    };
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&target, &file)?;
    tracing::info!(
        height = header.height,
        path = %target.display(),
        accounts = accounts.len(),
        slots = slot_count_src,
        "snapshot exported"
    );
    Ok(header)
}

/// Verify a snapshot file against `expected_network_id` (ADR-0026 steps
/// a–c plus the local tip-block hash match of step d). Returns the decoded
/// header and the raw body bytes (header-encoded ‖ body) so `import` can
/// re-derive records without re-reading the file structure blindly.
pub fn verify_file(
    path: &Path,
    expected_network_id: u32,
) -> Result<(SnapshotHeader, Vec<u8>), SnapshotError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < SNAPSHOT_HEADER_LEN + SNAPSHOT_TRAILER_LEN {
        return Err(SnapshotError::Corrupt("file too short"));
    }
    let (rest, trailer) = bytes.split_at(bytes.len() - SNAPSHOT_TRAILER_LEN);
    let header = SnapshotHeader::decode(&rest[0..SNAPSHOT_HEADER_LEN])?;

    // (a) whole-file checksum over header ‖ body.
    let expected = blake3::hash(rest);
    if expected.as_bytes() != trailer {
        return Err(SnapshotError::ChecksumMismatch);
    }

    // (b) network id.
    if header.network_id != expected_network_id {
        return Err(SnapshotError::NetworkIdMismatch {
            found: header.network_id,
            expected: expected_network_id,
        });
    }

    let body = &rest[SNAPSHOT_HEADER_LEN..];
    let (accounts, _slots, recent_blocks) = parse_body(body)?;
    let tip_block = recent_blocks
        .last()
        .ok_or(SnapshotError::Corrupt("no recent blocks"))?;

    // (c) state root recomputation from body accounts (forged mitigation).
    let mut map: HashMap<String, Account> = HashMap::with_capacity(accounts.len());
    for (addr, value) in &accounts {
        let account: Account = serde_json::from_slice(value)
            .map_err(|_| SnapshotError::Corrupt("account decode failed"))?;
        map.insert(addr.clone(), account);
    }
    let recomputed = SparseMerkleTrie::compute_root(&map);
    if recomputed != header.state_root {
        return Err(SnapshotError::StateRootMismatch {
            header: header.state_root,
            recomputed,
        });
    }

    // (d-local) tip block hash matches the header commitment; the recent
    // window must be contiguous and link through previous_hash.
    let tip_hash = hex::encode(strangecoin_core::serialize::block_hash(tip_block));
    if tip_hash != header.block_hash || tip_block.index != header.height {
        return Err(SnapshotError::TipBlockHashMismatch {
            header: header.block_hash.clone(),
            block: tip_hash,
        });
    }
    for pair in recent_blocks.windows(2) {
        if pair[1].index != pair[0].index + 1 || pair[1].previous_hash != pair[0].hash {
            return Err(SnapshotError::Corrupt("recent block window not linked"));
        }
    }

    Ok((header, rest[SNAPSHOT_HEADER_LEN..].to_vec()))
}

/// Verify `path` then write its state into `engine` in one atomic batch.
///
/// Fail-closed: refuses to touch a database that already carries tip meta
/// (import is a fresh-bootstrap operation; overwriting a live chain is
/// never implicit). After a successful import the store holds
/// `chain_base_height` (window start), the recent-block window, tip meta
/// and the full current state — the node reopens as a
/// snapshot-bootstrapped chain and catches up from `height + 1`.
pub fn import(
    engine: &dyn StorageEngine,
    path: &Path,
    expected_network_id: u32,
) -> Result<SnapshotHeader, SnapshotError> {
    if engine.get(Cf::Meta, &meta_key(META_TIP_HEIGHT))?.is_some() {
        return Err(SnapshotError::NotFreshDatabase);
    }

    let (header, body) = verify_file(path, expected_network_id)?;
    let (accounts, slots, recent_blocks) = parse_body(&body)?;
    let chain_base = recent_blocks
        .first()
        .map(|b| b.index)
        .ok_or(SnapshotError::Corrupt("no recent blocks"))?;

    let mut ops = Vec::with_capacity(accounts.len() + slots.len() + recent_blocks.len() * 3 + 8);
    for (addr, value) in &accounts {
        ops.push(WriteBatchOp::Put {
            cf: Cf::State,
            key: addr.as_bytes().to_vec(),
            value: value.clone(),
        });
    }
    for (addr, slot, value) in &slots {
        let mut key = addr.as_bytes().to_vec();
        key.extend_from_slice(slot);
        ops.push(WriteBatchOp::Put {
            cf: Cf::ContractStorage,
            key,
            value: value.clone(),
        });
    }

    // Recent-block window + hash indices + meta (same atomic batch as the
    // state — otherwise load sees tip without blocks or vice versa).
    let height_bytes = header.height.to_be_bytes();
    for blk in &recent_blocks {
        let raw = strangecoin_core::serialize::block_hash(blk);
        ops.push(WriteBatchOp::Put {
            cf: Cf::Blocks,
            key: block_by_height_key(blk.index),
            value: serialize_block(blk),
        });
        ops.push(WriteBatchOp::Put {
            cf: Cf::Blocks,
            key: block_hash_index_key(&raw),
            value: blk.index.to_be_bytes().to_vec(),
        });
    }
    let hash_raw: [u8; 32] = hex::decode(&header.block_hash)
        .map_err(|_| SnapshotError::Corrupt("header block hash not hex"))?
        .try_into()
        .map_err(|_| SnapshotError::Corrupt("header block hash wrong length"))?;
    ops.push(WriteBatchOp::Put {
        cf: Cf::Meta,
        key: meta_key(META_TIP_HEIGHT),
        value: height_bytes.to_vec(),
    });
    ops.push(WriteBatchOp::Put {
        cf: Cf::Meta,
        key: meta_key(META_TIP_HASH),
        value: hash_raw.to_vec(),
    });
    // Partial-chain marker: blocks exist only from `chain_base` upward
    // (absent = 0, full chain from genesis).
    if chain_base > 0 {
        ops.push(WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_CHAIN_BASE),
            value: chain_base.to_be_bytes().to_vec(),
        });
    }

    engine.write_batch(ops)?;
    engine.flush()?;
    tracing::info!(
        height = header.height,
        chain_base,
        network_id = header.network_id,
        "snapshot imported"
    );
    Ok(header)
}

/// ADR-0026 self-describing filename.
pub fn default_filename(header: &SnapshotHeader) -> String {
    format!(
        "snapshot-{}-{}.scsnap",
        header.height,
        hex::encode(header.state_root)
    )
}

// ------------------------------------------------------------------ helpers

fn read_u64_meta(engine: &dyn StorageEngine, name: &str) -> Result<Option<u64>, SnapshotError> {
    match engine.get(Cf::Meta, &meta_key(name))? {
        None => Ok(None),
        Some(bytes) => {
            let arr: [u8; 8] = bytes
                .try_into()
                .map_err(|_| SnapshotError::Corrupt("u64 meta wrong length"))?;
            Ok(Some(u64::from_be_bytes(arr)))
        }
    }
}

fn read_hash_meta(engine: &dyn StorageEngine, name: &str) -> Result<Option<String>, SnapshotError> {
    match engine.get(Cf::Meta, &meta_key(name))? {
        None => Ok(None),
        Some(bytes) => {
            if bytes.len() != 32 {
                return Err(SnapshotError::Corrupt("hash meta wrong length"));
            }
            Ok(Some(hex::encode(bytes)))
        }
    }
}

fn read_tip_block(engine: &dyn StorageEngine, height: u64) -> Result<Option<Block>, SnapshotError> {
    let Some(bytes) = engine.get(Cf::Blocks, &block_by_height_key(height))? else {
        return Ok(None);
    };
    strangecoin_core::serialize::deserialize_block(&bytes)
        .map(Some)
        .map_err(|_| SnapshotError::Corrupt("tip block decode failed"))
}

/// Parsed body contents: accounts (address, value), contract slots
/// (address, slot, value), and the recent-block MTP window.
type ParsedBody = (
    Vec<(String, Vec<u8>)>,
    Vec<(String, [u8; 32], Vec<u8>)>,
    Vec<Block>,
);

/// Parse the body section into `(accounts, slots, recent_blocks)`.
///
/// `accounts` is `Vec<(address, value)>` in file order (already sorted);
/// `slots` is `Vec<(address, slot, value)>`; `recent_blocks` is the
/// ascending MTP window ending at the tip (deviation #1).
fn parse_body(body: &[u8]) -> Result<ParsedBody, SnapshotError> {
    let mut pos = 0usize;
    let read_u64 = |pos: &mut usize| -> Result<u64, SnapshotError> {
        let end = pos
            .checked_add(8)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        let v = u64::from_be_bytes(body[*pos..end].try_into().expect("8 bytes"));
        *pos = end;
        Ok(v)
    };
    let read_u32 = |pos: &mut usize| -> Result<u32, SnapshotError> {
        let end = pos
            .checked_add(4)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        let v = u32::from_be_bytes(body[*pos..end].try_into().expect("4 bytes"));
        *pos = end;
        Ok(v)
    };
    let read_u16 = |pos: &mut usize| -> Result<u16, SnapshotError> {
        let end = pos
            .checked_add(2)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        let v = u16::from_be_bytes(body[*pos..end].try_into().expect("2 bytes"));
        *pos = end;
        Ok(v)
    };

    let account_count = read_u64(&mut pos)?;
    let mut accounts = Vec::with_capacity(account_count as usize);
    for _ in 0..account_count {
        let addr_len = read_u16(&mut pos)? as usize;
        let addr_end = pos
            .checked_add(addr_len)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        let addr = std::str::from_utf8(&body[pos..addr_end])
            .map_err(|_| SnapshotError::Corrupt("address not utf-8"))?
            .to_string();
        pos = addr_end;
        let value_len = read_u32(&mut pos)? as usize;
        let value_end = pos
            .checked_add(value_len)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        accounts.push((addr, body[pos..value_end].to_vec()));
        pos = value_end;
    }

    let slot_count = read_u64(&mut pos)?;
    let mut slots = Vec::with_capacity(slot_count as usize);
    for _ in 0..slot_count {
        let addr_len = read_u16(&mut pos)? as usize;
        let addr_end = pos
            .checked_add(addr_len)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        let addr = std::str::from_utf8(&body[pos..addr_end])
            .map_err(|_| SnapshotError::Corrupt("address not utf-8"))?
            .to_string();
        pos = addr_end;
        let slot_end = pos
            .checked_add(32)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        let slot: [u8; 32] = body[pos..slot_end].try_into().expect("32 bytes");
        pos = slot_end;
        let value_len = read_u32(&mut pos)? as usize;
        let value_end = pos
            .checked_add(value_len)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        slots.push((addr, slot, body[pos..value_end].to_vec()));
        pos = value_end;
    }

    let block_count = read_u64(&mut pos)?;
    if block_count == 0 || block_count > MEDIAN_TIME_WINDOW as u64 {
        return Err(SnapshotError::Corrupt("bad recent block count"));
    }
    let mut recent_blocks = Vec::with_capacity(block_count as usize);
    for _ in 0..block_count {
        let len = read_u32(&mut pos)? as usize;
        let end = pos
            .checked_add(len)
            .filter(|&e| e <= body.len())
            .ok_or(SnapshotError::Corrupt("body truncated"))?;
        let blk = strangecoin_core::serialize::deserialize_block(&body[pos..end])
            .map_err(|_| SnapshotError::Corrupt("block decode failed"))?;
        recent_blocks.push(blk);
        pos = end;
    }
    if pos != body.len() {
        return Err(SnapshotError::Corrupt("trailing body bytes"));
    }

    Ok((accounts, slots, recent_blocks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Minimal sorted in-memory engine for snapshot unit tests (the
    /// storage crate's `MemoryEngine` is `#[cfg(test)]`-private in
    /// `engine.rs`; same pattern as `tests/pruning.rs`'s SweepEngine).
    struct SnapEngine {
        cfs: std::sync::Mutex<[BTreeMap<Vec<u8>, Vec<u8>>; 7]>,
    }

    impl SnapEngine {
        fn new() -> Self {
            Self {
                cfs: std::sync::Mutex::new(std::array::from_fn(|_| BTreeMap::new())),
            }
        }
    }

    impl StorageEngine for SnapEngine {
        fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(self.cfs.lock().expect("lock")[cf.ordinal()]
                .get(key)
                .cloned())
        }
        fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError> {
            let mut cfs = self.cfs.lock().expect("lock");
            for op in ops {
                match op {
                    WriteBatchOp::Put { cf, key, value } => {
                        cfs[cf.ordinal()].insert(key, value);
                    }
                    WriteBatchOp::Delete { cf, key } => {
                        cfs[cf.ordinal()].remove(&key);
                    }
                }
            }
            Ok(())
        }
        fn iterate_prefix(
            &self,
            cf: Cf,
            prefix: &[u8],
        ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError> {
            let cfs = self.cfs.lock().expect("lock");
            Ok(cfs[cf.ordinal()]
                .range(prefix.to_vec()..)
                .take_while(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect())
        }
        fn flush(&self) -> Result<(), StorageError> {
            Ok(())
        }
    }

    /// Seed a minimal tip-at-height-H database: one EOA account, no
    /// contract storage, and a linked block window `recent_from..=H`
    /// (the MTP window export reads) whose tip state_root matches the
    /// account map.
    fn seed(engine: &SnapEngine, height: u64, balance: u64, network_id: u32) -> SnapshotHeader {
        let addr = "sc1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let mut accounts = HashMap::new();
        accounts.insert(addr.to_string(), Account::eoa(balance, 1));
        let state_root = SparseMerkleTrie::compute_root(&accounts);

        let window = MEDIAN_TIME_WINDOW as u64;
        let recent_from = height.saturating_sub(window - 1);

        // Build a linked chain recent_from..=height (previous_hash
        // linkage; only the tip carries the real state_root).
        let mut blocks: Vec<Block> = Vec::new();
        let mut prev_hash = "0".repeat(64);
        for h in recent_from..=height {
            let mut blk = Block {
                index: h,
                timestamp: 1_700_000_000 + h,
                transactions: vec![],
                previous_hash: prev_hash.clone(),
                hash: String::new(),
                nonce: 0,
                target: "ff".repeat(32),
                consensus_version: 1,
                state_root: if h == height { state_root } else { [0u8; 32] },
                tx_root: [0u8; 32],
                receipts_root: [0u8; 32],
            };
            let raw = strangecoin_core::serialize::block_hash(&blk);
            blk.hash = hex::encode(raw);
            prev_hash = blk.hash.clone();
            blocks.push(blk);
        }
        let tip = blocks.last().expect("window non-empty").clone();
        let tip_hash_raw = strangecoin_core::serialize::block_hash(&tip);

        let account_json = serde_json::to_vec(&Account::eoa(balance, 1)).expect("json");
        let mut ops = vec![WriteBatchOp::Put {
            cf: Cf::State,
            key: addr.as_bytes().to_vec(),
            value: account_json,
        }];
        for blk in &blocks {
            ops.push(WriteBatchOp::Put {
                cf: Cf::Blocks,
                key: block_by_height_key(blk.index),
                value: serialize_block(blk),
            });
        }
        ops.push(WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_TIP_HEIGHT),
            value: height.to_be_bytes().to_vec(),
        });
        ops.push(WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_TIP_HASH),
            value: tip_hash_raw.to_vec(),
        });
        if recent_from > 0 {
            ops.push(WriteBatchOp::Put {
                cf: Cf::Meta,
                key: meta_key(META_CHAIN_BASE),
                value: recent_from.to_be_bytes().to_vec(),
            });
        }
        let _ = network_id; // network_id lives in the file header, not the DB
        engine.write_batch(ops).expect("seed");

        SnapshotHeader {
            format_version: SNAPSHOT_FORMAT_VERSION,
            network_id,
            height,
            block_hash: hex::encode(tip_hash_raw),
            state_root,
            receipts_root: tip.receipts_root,
        }
    }

    fn tmp_file(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("sc_snap_{tag}_{}.scsnap", std::process::id()));
        p
    }

    #[test]
    fn header_round_trip() {
        let h = SnapshotHeader {
            format_version: 1,
            network_id: 3,
            height: 42,
            block_hash: hex::encode([0x11; 32]),
            state_root: [0x22; 32],
            receipts_root: [0x33; 32],
        };
        let bytes = h.encode();
        assert_eq!(bytes.len(), SNAPSHOT_HEADER_LEN);
        assert_eq!(&bytes[0..8], SNAPSHOT_MAGIC);
        let back = SnapshotHeader::decode(&bytes).expect("decode");
        assert_eq!(back, h);
    }

    #[test]
    fn export_verify_import_round_trip() {
        let engine = SnapEngine::new();
        // Height 15 > MTP window (11) so recent_from = 5 > 0 and the
        // partial-chain marker is written.
        let seeded = seed(&engine, 15, 1000, 3);
        let path = tmp_file("roundtrip");
        let _ = std::fs::remove_file(&path);

        let exported = export(&engine, &path, 3).expect("export");
        assert_eq!(exported.height, 15);
        assert_eq!(exported.state_root, seeded.state_root);

        let (header, _body) = verify_file(&path, 3).expect("verify");
        assert_eq!(header, exported);

        // Import into a fresh engine.
        let fresh = SnapEngine::new();
        let imported = import(&fresh, &path, 3).expect("import");
        assert_eq!(imported.height, 15);

        // State present.
        let addr = "sc1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let got = fresh
            .get(Cf::State, addr.as_bytes())
            .expect("get")
            .expect("account present");
        let account: Account = serde_json::from_slice(&got).expect("decode");
        assert_eq!(account.balance(), 1000);

        // Tip meta + chain base (recent window starts at 15-10 = 5).
        let tip = fresh
            .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))
            .expect("get")
            .expect("tip present");
        assert_eq!(tip, 15u64.to_be_bytes().to_vec());
        let base = fresh
            .get(Cf::Meta, &meta_key(META_CHAIN_BASE))
            .expect("get")
            .expect("base present");
        assert_eq!(base, 5u64.to_be_bytes().to_vec());

        // Re-import refuses (not fresh).
        let err = import(&fresh, &path, 3).expect_err("second import");
        assert!(matches!(err, SnapshotError::NotFreshDatabase));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_is_byte_identical_across_runs() {
        let engine = SnapEngine::new();
        seed(&engine, 3, 500, 3);
        let p1 = tmp_file("det1");
        let p2 = tmp_file("det2");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
        export(&engine, &p1, 3).expect("e1");
        export(&engine, &p2, 3).expect("e2");
        let b1 = std::fs::read(&p1).expect("r1");
        let b2 = std::fs::read(&p2).expect("r2");
        assert_eq!(b1, b2, "double export must be byte-identical");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn corrupt_checksum_is_rejected() {
        let engine = SnapEngine::new();
        seed(&engine, 1, 100, 3);
        let path = tmp_file("corrupt");
        let _ = std::fs::remove_file(&path);
        export(&engine, &path, 3).expect("export");

        let mut bytes = std::fs::read(&path).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&path, &bytes).expect("write");

        let err = verify_file(&path, 3).expect_err("must reject");
        assert!(matches!(err, SnapshotError::ChecksumMismatch), "{err:?}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wrong_network_id_is_rejected() {
        let engine = SnapEngine::new();
        seed(&engine, 1, 100, 3);
        let path = tmp_file("netid");
        let _ = std::fs::remove_file(&path);
        export(&engine, &path, 3).expect("export");

        let err = verify_file(&path, 1).expect_err("must reject");
        assert!(
            matches!(err, SnapshotError::NetworkIdMismatch { .. }),
            "{err:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn forged_state_root_is_rejected() {
        let engine = SnapEngine::new();
        seed(&engine, 5, 1000, 3);
        let path = tmp_file("forged");
        let _ = std::fs::remove_file(&path);
        export(&engine, &path, 3).expect("export");

        // Flip a balance digit inside the account JSON value, then
        // recompute the trailer so step (a) passes and step (c) fires.
        let mut bytes = std::fs::read(&path).expect("read");
        // Find `"balance":1000` (or similar) and bump the last digit.
        let body_start = SNAPSHOT_HEADER_LEN;
        let body_end = bytes.len() - SNAPSHOT_TRAILER_LEN;
        let needle = b"\"balance\":";
        let region = &bytes[body_start..body_end];
        let idx = region
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("balance field present");
        // The value starts right after the needle; flip its last digit.
        let value_start = body_start + idx + needle.len();
        let value_end = value_start
            + region[idx + needle.len()..]
                .iter()
                .position(|&b| b == b'}' || b == b',')
                .expect("value end");
        // Increment the last ASCII digit (wraps 9→0, good enough).
        let d = &mut bytes[value_end - 1];
        if d.is_ascii_digit() {
            *d = if *d == b'9' { b'0' } else { *d + 1 };
        } else {
            panic!("expected digit at {value_end}");
        }
        // Recompute trailer over header ‖ body.
        let (rest, _) = bytes.split_at(bytes.len() - SNAPSHOT_TRAILER_LEN);
        let checksum = blake3::hash(rest);
        let n = bytes.len();
        bytes[n - SNAPSHOT_TRAILER_LEN..].copy_from_slice(checksum.as_bytes());
        std::fs::write(&path, &bytes).expect("write");

        let err = verify_file(&path, 3).expect_err("must reject");
        assert!(
            matches!(err, SnapshotError::StateRootMismatch { .. }),
            "{err:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let engine = SnapEngine::new();
        seed(&engine, 1, 1, 3);
        let path = tmp_file("magic");
        let _ = std::fs::remove_file(&path);
        export(&engine, &path, 3).expect("export");
        let mut bytes = std::fs::read(&path).expect("read");
        bytes[0] = b'X';
        // Keep trailer consistent so we fail on magic, not checksum.
        let (rest, _) = bytes.split_at(bytes.len() - SNAPSHOT_TRAILER_LEN);
        let checksum = blake3::hash(rest);
        let n = bytes.len();
        bytes[n - SNAPSHOT_TRAILER_LEN..].copy_from_slice(checksum.as_bytes());
        std::fs::write(&path, &bytes).expect("write");
        let err = verify_file(&path, 3).expect_err("must reject");
        assert!(
            matches!(err, SnapshotError::Corrupt("bad magic")),
            "{err:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
