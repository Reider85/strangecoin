use crate::error::NetError;
use crate::noise::transport::{
    MAX_ENCRYPTED_PART, MAX_TRANSPORT_PAYLOAD, NOISE_NONCE_LEN, NOISE_TAG_LEN,
};
use crate::noise::NoiseTransport;
use std::io::Read;
use strangecoin_core::serialize::{
    deserialize_block, deserialize_header, serialize_block, serialize_header, HEADER_WIRE_LEN,
};
use strangecoin_core::types::{Block, BlockHeader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Binary HELLO + capabilities negotiation (S2-P10) — sent over the
/// encrypted framing below, right after the Noise handshake finishes.
pub mod hello;

/// Framing/parse size violation, folded into the Stage 2 `NetError`
/// taxonomy (S2-P06 migration): a malformed or over-sized peer message is a
/// protocol violation. Keeps the legacy `size limit exceeded: {0}` payload
/// text so log lines stay greppable across the move.
fn size_err(what: &'static str) -> NetError {
    NetError::Protocol(format!("size limit exceeded: {}", what))
}

pub const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;
pub const MAX_BLOCK_SIZE: usize = 4 * 1024 * 1024;
pub const MAX_TX_SIZE: usize = 256 * 1024;

// ---------------------------------------------------------------- S1-P16
// Binary message tags (first payload byte). S2-P18 removed the v0.8.6 text
// protocol (`GET_BLOCKCHAIN` / `UPDATE_BLOCKCHAIN:`), so every defined
// message is now a binary tag: an ASCII-leading payload (or any tag outside
// `KNOWN_TAGS`) is an unknown *future* message type and decodes to
// [`InboundMessage::UnknownMessage`] — the caller logs and ignores it
// (forward-compat, never a crash and never a ban).

pub const MSG_GET_HEADERS: u8 = 0x01;
pub const MSG_HEADERS: u8 = 0x02;
pub const MSG_GET_BLOCKS: u8 = 0x03;
pub const MSG_BLOCKS: u8 = 0x04;

/// Max headers per HEADERS message / per GET_HEADERS round (invariant #7:
/// the batch bound is enforced before any `Vec::with_capacity`).
pub const MAX_HEADERS_BATCH: usize = 2000;
/// Max blocks per GET_BLOCKS / BLOCKS message.
pub const MAX_BLOCKS_BATCH: usize = 128;

// ------------------------------------------------- top-level dispatch (S2-P18)

/// Every first-byte tag the Stage 2 wire format defines: the sync codecs
/// `0x01..=0x04` (this module), `ADDR` gossip `0x05`, the gossip/compact
/// codecs `0x06..=0x0C` and the Erlay reconcile envelope `0x0D` (S2-P19).
/// Anything outside this set — including the removed v0.8.6 text protocol —
/// is an unknown *future* message type.
pub const KNOWN_TAGS: [u8; 13] = [
    MSG_GET_HEADERS,
    MSG_HEADERS,
    MSG_GET_BLOCKS,
    MSG_BLOCKS,
    crate::discovery::addr_gossip::MSG_ADDR,
    crate::gossip::inv::MSG_INV,
    crate::gossip::get_data::MSG_GET_DATA,
    crate::gossip::inv::MSG_TX,
    crate::gossip::compact_blocks::MSG_COMPACT_BLOCK,
    crate::gossip::get_data::MSG_NOTFOUND,
    crate::gossip::compact_blocks::MSG_GET_BLOCK_TXN,
    crate::gossip::compact_blocks::MSG_BLOCK_TXN,
    crate::gossip::erlay::MSG_ERLAY_RECONCILE,
];

/// Whether `tag` is a message type this build knows ([`KNOWN_TAGS`]).
pub fn is_known_tag(tag: u8) -> bool {
    KNOWN_TAGS.contains(&tag)
}

/// One inbound payload after top-level tag dispatch (S2-P18 forward-compat).
///
/// This is the single decode entry point for a connection's request loop:
/// the two request messages come fully decoded, every other *known* tag is
/// handed to its owning codec one layer up, and everything else — the
/// removed v0.8.6 text protocol (`GET_BLOCKCHAIN`, `UPDATE_BLOCKCHAIN:`) or
/// a future message type this build does not know — is an
/// [`InboundMessage::UnknownMessage`].
///
/// The forward-compat contract: a caller that receives `UnknownMessage`
/// **logs and ignores** it — never crashes, never scores a protocol
/// violation and never bans the peer (a future peer speaking a newer
/// protocol must not be punished for it; junk payloads still draw main-bucket
/// rate-limit tokens through [`MessageClass::Other`](crate::rate_limiter::MessageClass)).
/// Malformed *known* requests surface as `Err` — those stay protocol
/// violations (ADR-0022) and are scored by their owning codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundMessage {
    /// `GET_HEADERS [0x01]` — fully decoded `from_height`.
    GetHeaders(u64),
    /// `GET_BLOCKS [0x03]` — fully decoded requested hashes.
    GetBlocks(Vec<[u8; 32]>),
    /// A known tag whose body belongs to another codec (`HEADERS`/`BLOCKS`
    /// responses, `ADDR`, gossip `0x06..=0x0C`) — parse it one layer up.
    KnownTag(u8),
    /// Unrecognized payload: removed v0.8.6 text messages and any future
    /// message type. Log + ignore (forward-compat).
    UnknownMessage(Vec<u8>),
}

/// Decode one inbound payload (the request loop's top-level dispatch,
/// S2-P18). Never fails for unknown types — those are
/// [`InboundMessage::UnknownMessage`]; `Err` is reserved for malformed
/// *known* requests (`GET_HEADERS`/`GET_BLOCKS`), preserving the pre-S2-P18
/// handler behaviour of closing the connection on a broken sync request.
pub fn decode_message(bytes: &[u8]) -> Result<InboundMessage, NetError> {
    let Some(&tag) = bytes.first() else {
        return Ok(InboundMessage::UnknownMessage(bytes.to_vec()));
    };
    match tag {
        MSG_GET_HEADERS => parse_get_headers(bytes).map(InboundMessage::GetHeaders),
        MSG_GET_BLOCKS => parse_get_blocks(bytes).map(InboundMessage::GetBlocks),
        t if is_known_tag(t) => Ok(InboundMessage::KnownTag(t)),
        _ => Ok(InboundMessage::UnknownMessage(bytes.to_vec())),
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn take_u32(bytes: &[u8], offset: &mut usize) -> Result<u32, NetError> {
    if *offset + 4 > bytes.len() {
        return Err(size_err("u32 truncation"));
    }
    let v = u32::from_be_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
    ]);
    *offset += 4;
    Ok(v)
}

fn take_u64(bytes: &[u8], offset: &mut usize) -> Result<u64, NetError> {
    if *offset + 8 > bytes.len() {
        return Err(size_err("u64 truncation"));
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[*offset..*offset + 8]);
    *offset += 8;
    Ok(u64::from_be_bytes(buf))
}

// ------------------------------------------------------------ GET_HEADERS

/// `GET_HEADERS [0x01] [from_height: u64]` — request up to
/// [`MAX_HEADERS_BATCH`] headers with `index >= from_height`.
pub fn encode_get_headers(from_height: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.push(MSG_GET_HEADERS);
    put_u64(&mut out, from_height);
    out
}

pub fn parse_get_headers(bytes: &[u8]) -> Result<u64, NetError> {
    if bytes.len() != 9 || bytes[0] != MSG_GET_HEADERS {
        return Err(size_err("GET_HEADERS"));
    }
    let mut offset = 1;
    take_u64(bytes, &mut offset)
}

// ---------------------------------------------------------------- HEADERS

/// `HEADERS [0x02] [count: u32] [len: u32][header bytes]…` — headers in
/// ascending order; each item is the canonical encoding of one header
/// (hash excluded, recomputed by the decoder).
pub fn encode_headers(headers: &[BlockHeader]) -> Result<Vec<u8>, NetError> {
    if headers.len() > MAX_HEADERS_BATCH {
        return Err(size_err("headers batch"));
    }
    let mut out = Vec::new();
    out.push(MSG_HEADERS);
    put_u32(&mut out, headers.len() as u32);
    for header in headers {
        let bytes = serialize_header(header);
        put_u32(&mut out, bytes.len() as u32);
        out.extend_from_slice(&bytes);
    }
    if out.len() > MAX_MESSAGE_SIZE {
        return Err(size_err("headers message"));
    }
    Ok(out)
}

pub fn parse_headers(bytes: &[u8]) -> Result<Vec<BlockHeader>, NetError> {
    if bytes.is_empty() || bytes[0] != MSG_HEADERS {
        return Err(size_err("HEADERS tag"));
    }
    let mut offset = 1;
    let count = take_u32(bytes, &mut offset)? as usize;
    if count > MAX_HEADERS_BATCH {
        return Err(size_err("headers batch"));
    }
    // Count can only be plausible if the buffer has room for at least one
    // length prefix per item — check before reserving capacity (invariant #7).
    if count > (bytes.len() - offset) / 4 {
        return Err(size_err("headers truncation"));
    }
    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take_u32(bytes, &mut offset)? as usize;
        if len != HEADER_WIRE_LEN || offset + len > bytes.len() {
            return Err(size_err("header item"));
        }
        let header = deserialize_header(&bytes[offset..offset + len])
            .map_err(|_| size_err("header decode"))?;
        offset += len;
        headers.push(header);
    }
    if offset != bytes.len() {
        return Err(size_err("HEADERS trailing"));
    }
    Ok(headers)
}

// ------------------------------------------------------------- GET_BLOCKS

/// `GET_BLOCKS [0x03] [count: u32] [hash: 32 bytes]…` — request full blocks
/// by hash (order of the response follows the request order).
pub fn encode_get_blocks(hashes: &[[u8; 32]]) -> Result<Vec<u8>, NetError> {
    if hashes.len() > MAX_BLOCKS_BATCH {
        return Err(size_err("blocks batch"));
    }
    let mut out = Vec::with_capacity(5 + hashes.len() * 32);
    out.push(MSG_GET_BLOCKS);
    put_u32(&mut out, hashes.len() as u32);
    for hash in hashes {
        out.extend_from_slice(hash);
    }
    Ok(out)
}

pub fn parse_get_blocks(bytes: &[u8]) -> Result<Vec<[u8; 32]>, NetError> {
    if bytes.len() < 5 || bytes[0] != MSG_GET_BLOCKS {
        return Err(size_err("GET_BLOCKS"));
    }
    let mut offset = 1;
    let count = take_u32(bytes, &mut offset)? as usize;
    if count > MAX_BLOCKS_BATCH {
        return Err(size_err("blocks batch"));
    }
    if count * 32 != bytes.len() - offset {
        return Err(size_err("GET_BLOCKS payload"));
    }
    let mut hashes = Vec::with_capacity(count);
    for _ in 0..count {
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes[offset..offset + 32]);
        offset += 32;
        hashes.push(hash);
    }
    Ok(hashes)
}

// ----------------------------------------------------------------- BLOCKS

/// `BLOCKS [0x04] [count: u32] [len: u32][block bytes]…` — full blocks.
pub fn encode_blocks(blocks: &[Block]) -> Result<Vec<u8>, NetError> {
    if blocks.len() > MAX_BLOCKS_BATCH {
        return Err(size_err("blocks batch"));
    }
    let mut out = Vec::new();
    out.push(MSG_BLOCKS);
    put_u32(&mut out, blocks.len() as u32);
    for block in blocks {
        let bytes = serialize_block(block);
        if bytes.len() > MAX_BLOCK_SIZE {
            return Err(size_err("block size"));
        }
        put_u32(&mut out, bytes.len() as u32);
        out.extend_from_slice(&bytes);
    }
    if out.len() > MAX_MESSAGE_SIZE {
        return Err(size_err("blocks message"));
    }
    Ok(out)
}

pub fn parse_blocks(bytes: &[u8]) -> Result<Vec<Block>, NetError> {
    if bytes.is_empty() || bytes[0] != MSG_BLOCKS {
        return Err(size_err("BLOCKS tag"));
    }
    let mut offset = 1;
    let count = take_u32(bytes, &mut offset)? as usize;
    if count > MAX_BLOCKS_BATCH {
        return Err(size_err("blocks batch"));
    }
    if count > (bytes.len() - offset) / 4 {
        return Err(size_err("blocks truncation"));
    }
    let mut blocks = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take_u32(bytes, &mut offset)? as usize;
        if len > MAX_BLOCK_SIZE || offset + len > bytes.len() {
            return Err(size_err("block item"));
        }
        let block = deserialize_block(&bytes[offset..offset + len])
            .map_err(|_| size_err("block decode"))?;
        offset += len;
        blocks.push(block);
    }
    if offset != bytes.len() {
        return Err(size_err("BLOCKS trailing"));
    }
    Ok(blocks)
}

// ------------------------------------------------------------ existing API

// S2-P19 removed the plaintext `HELLO:<network_id>` codec
// (`encode_hello`/`parse_hello`/`HELLO_PREFIX`): every session now speaks
// the Noise-encrypted S2-P10 HELLO (`protocol::hello`) — the Stage 1.5
// plaintext greeting is gone with the rest of the pre-Noise wire.

pub fn read_length_prefixed<R: Read>(reader: &mut R) -> Result<Vec<u8>, NetError> {
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .map_err(|_| NetError::Transport("length prefix".to_string()))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(size_err("message"));
    }
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|_| NetError::Transport("message body".to_string()))?;
    Ok(buf)
}

/// Write a payload with the length-prefix framing every message uses.
pub fn write_length_prefixed<W: std::io::Write>(
    writer: &mut W,
    payload: &[u8],
) -> Result<(), NetError> {
    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(size_err("message"));
    }
    let mut data = (payload.len() as u32).to_be_bytes().to_vec();
    data.extend_from_slice(payload);
    writer.write_all(&data)?;
    writer.flush()?;
    Ok(())
}

/// Async variant of [`read_length_prefixed`] over tokio I/O (ADR-0011).
/// Same wire format: 4-byte BE length prefix + payload, `MAX_MESSAGE_SIZE` cap.
pub async fn read_length_prefixed_async<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<Vec<u8>, NetError> {
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .await
        .map_err(|_| NetError::Transport("length prefix".to_string()))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(size_err("message"));
    }
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|_| NetError::Transport("message body".to_string()))?;
    Ok(buf)
}

/// Async variant of [`write_length_prefixed`] over tokio I/O (ADR-0011).
pub async fn write_length_prefixed_async<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> Result<(), NetError> {
    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(size_err("message"));
    }
    let mut data = (payload.len() as u32).to_be_bytes().to_vec();
    data.extend_from_slice(payload);
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}

// ------------------------------------------- encrypted framing (S2-P08)

/// Wire format of one encrypted message: a 4-byte BE total length covering
/// everything after it, then consecutive Noise parts
/// `[nonce: u64 BE][ciphertext + AEAD tag]`:
///
/// ```text
/// [total_len: u32 BE][nonce: u64][ciphertext + tag][nonce][ciphertext + tag]…
/// ```
///
/// A single part below the Noise size limit is exactly the S2-P08 spec shape
/// `[length][nonce][ciphertext + AEAD tag: length - 8]`; larger payloads are
/// split into [`MAX_TRANSPORT_PAYLOAD`]-sized plaintext chunks because the
/// Noise transport caps one message at 65 535 bytes. Non-final parts are
/// therefore always exactly [`MAX_ENCRYPTED_PART`] bytes and the final part
/// carries the remainder — [`decrypt_frame`] splits on that rule.
///
/// Length-prefix discipline (invariant #16): [`decrypt_frame`] validates the
/// claimed length against [`MAX_MESSAGE_SIZE`] **before** it allocates.
pub const ENCRYPTED_FRAME_OVERHEAD: usize = NOISE_NONCE_LEN + NOISE_TAG_LEN;

/// Encrypt one complete message into a self-contained wire frame (the whole
/// `[length][nonce][ct…]` structure — send it with a plain write, do **not**
/// wrap it in another length prefix).
///
/// Payloads larger than [`MAX_TRANSPORT_PAYLOAD`] are chunked transparently;
/// every chunk consumes one transport nonce, so the peer reassembles in
/// order. The total length is patched in after the loop, once it is known.
///
/// Note: the S2-P08 spec types this as `msg: &Message`; this crate has no
/// `Message` type (messages are tagged byte payloads built by `encode_*` /
/// parsed by `parse_*`), so the function encrypts raw payload bytes.
pub fn encrypt_frame(
    transport: &mut NoiseTransport,
    msg: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), NetError> {
    let parts = msg.len().div_ceil(MAX_TRANSPORT_PAYLOAD).max(1);
    let wire_len = 4 + msg.len() + parts * ENCRYPTED_FRAME_OVERHEAD;
    if wire_len > MAX_MESSAGE_SIZE {
        return Err(size_err("encrypted message"));
    }
    out.clear();
    out.extend_from_slice(&0u32.to_be_bytes()); // total_len, patched below
    let mut part = vec![0u8; MAX_ENCRYPTED_PART];
    let mut start = 0;
    loop {
        let end = std::cmp::min(start + MAX_TRANSPORT_PAYLOAD, msg.len());
        let n = transport
            .encrypt(&msg[start..end], &mut part)
            .map_err(|e| NetError::Transport(e.to_string()))?;
        out.extend_from_slice(&part[..n]);
        start = end;
        if start >= msg.len() {
            break;
        }
    }
    let total = out.len() - 4;
    out[0..4].copy_from_slice(&(total as u32).to_be_bytes());
    Ok(())
}

/// Decrypt a complete wire frame produced by [`encrypt_frame`] (including its
/// leading length prefix), reassembling chunked payloads.
///
/// Order of checks — every one of them runs before the allocation or side
/// effect it protects (invariant #16):
///
/// 1. claimed `total_len` ≤ [`MAX_MESSAGE_SIZE`] → reject huge lengths
///    before `Vec::with_capacity`;
/// 2. `total_len` matches the buffer → reject truncation;
/// 3. per-part minimum size before the per-part scratch buffer;
/// 4. nonce check + AEAD tag inside `NoiseTransport::decrypt`.
///
/// Transport-layer failures (tampered tag, replayed nonce) surface as
/// [`NetError::Transport`], framing violations as [`NetError::Protocol`].
pub fn decrypt_frame(transport: &mut NoiseTransport, raw: &[u8]) -> Result<Vec<u8>, NetError> {
    if raw.len() < 4 {
        return Err(size_err("encrypted frame prefix"));
    }
    let total = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    if total > MAX_MESSAGE_SIZE {
        return Err(size_err("encrypted frame"));
    }
    if total != raw.len() - 4 {
        return Err(size_err("encrypted frame length"));
    }
    if total < ENCRYPTED_FRAME_OVERHEAD {
        return Err(size_err("encrypted frame"));
    }
    let blob = &raw[4..];
    let mut plaintext = Vec::with_capacity(total);
    let mut offset = 0;
    while offset < total {
        let remaining = total - offset;
        let take = remaining.min(MAX_ENCRYPTED_PART);
        let part = &blob[offset..offset + take];
        if part.len() < ENCRYPTED_FRAME_OVERHEAD {
            return Err(size_err("encrypted part"));
        }
        let mut chunk = vec![0u8; part.len() - ENCRYPTED_FRAME_OVERHEAD];
        let n = transport
            .decrypt(part, &mut chunk)
            .map_err(|e| NetError::Transport(e.to_string()))?;
        plaintext.extend_from_slice(&chunk[..n]);
        offset += take;
    }
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use strangecoin_core::types::Transaction;

    fn sample_block() -> Block {
        let mut block = Block {
            index: 7,
            timestamp: 1_700_000_000,
            transactions: vec![Transaction::Transfer(strangecoin_core::types::TransferTx {
                from: "rsc1sender".into(),
                to: "rsc1receiver".into(),
                amount: 42,
                fee: 0,
                nonce: 1,
                chain_id: 3,
                signature: vec![1u8; 65],
                is_coinbase: false,
            })],
            previous_hash: "aa".repeat(32),
            hash: String::new(),
            nonce: 99,
            target: "ff".repeat(32),
            consensus_version: 1,
            state_root: [3u8; 32],
            tx_root: [7u8; 32],
            receipts_root: [0u8; 32],
        };
        // The decoder recomputes the hash from the canonical encoding — the
        // sample must carry the same derived value to compare equal.
        block.hash = hex::encode(strangecoin_core::serialize::block_hash(&block));
        block
    }

    #[test]
    fn get_headers_roundtrip() {
        let encoded = encode_get_headers(1234);
        assert_eq!(encoded[0], MSG_GET_HEADERS);
        assert_eq!(parse_get_headers(&encoded).unwrap(), 1234);
    }

    #[test]
    fn get_headers_rejects_wrong_len_and_tag() {
        assert!(parse_get_headers(&[MSG_GET_HEADERS, 0, 0]).is_err());
        let mut bad_tag = encode_get_headers(5);
        bad_tag[0] = 0x7f;
        assert!(parse_get_headers(&bad_tag).is_err());
    }

    #[test]
    fn headers_roundtrip() {
        let block = sample_block();
        let mut header = block.header();
        header.hash = hex::encode(strangecoin_core::serialize::header_hash(&header));
        let encoded = encode_headers(&[header.clone(), header.clone()]).unwrap();
        let decoded = parse_headers(&encoded).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].hash, header.hash);
        assert_eq!(decoded[0].nonce, header.nonce);
        assert_eq!(decoded[1].index, header.index);
    }

    #[test]
    fn headers_reject_oversized_count_before_allocation() {
        // count = u32::MAX with a tiny buffer must not allocate
        let mut payload = vec![MSG_HEADERS];
        payload.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse_headers(&payload).is_err());
    }

    #[test]
    fn headers_reject_trailing_garbage() {
        let block = sample_block();
        let mut header = block.header();
        header.hash = hex::encode(strangecoin_core::serialize::header_hash(&header));
        let mut encoded = encode_headers(&[header]).unwrap();
        encoded.push(0xde);
        assert!(parse_headers(&encoded).is_err());
    }

    #[test]
    fn get_blocks_roundtrip() {
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        let encoded = encode_get_blocks(&[hash1, hash2]).unwrap();
        let decoded = parse_get_blocks(&encoded).unwrap();
        assert_eq!(decoded, vec![hash1, hash2]);
    }

    #[test]
    fn get_blocks_rejects_count_mismatch_and_oversize() {
        let mut payload = vec![MSG_GET_BLOCKS];
        payload.extend_from_slice(&2u32.to_be_bytes());
        payload.extend_from_slice(&[0u8; 32]); // claims 2 hashes, has 1
        assert!(parse_get_blocks(&payload).is_err());

        let mut over = vec![MSG_GET_BLOCKS];
        over.extend_from_slice(&((MAX_BLOCKS_BATCH + 1) as u32).to_be_bytes());
        assert!(parse_get_blocks(&over).is_err());
    }

    #[test]
    fn blocks_roundtrip_preserves_transactions() {
        let block = sample_block();
        let encoded = encode_blocks(std::slice::from_ref(&block)).unwrap();
        let decoded = parse_blocks(&encoded).unwrap();
        assert_eq!(decoded.len(), 1);
        let got = &decoded[0];
        assert_eq!(got.index, block.index);
        assert_eq!(got.hash, block.hash);
        assert_eq!(got.nonce, block.nonce);
        assert_eq!(got.target, block.target);
        assert_eq!(got.state_root, block.state_root);
        assert_eq!(got.tx_root, block.tx_root);
        assert_eq!(got.transactions.len(), 1);
        assert_eq!(got.transactions[0].value(), 42);
        assert_eq!(got.transactions[0].signature(), vec![1u8; 65].as_slice());
        assert_eq!(got.transactions[0].sender(), "rsc1sender");
        assert_eq!(
            strangecoin_core::serialize::serialize_block(got),
            strangecoin_core::serialize::serialize_block(&block),
            "re-encoded block must be byte-identical"
        );
    }

    #[test]
    fn blocks_reject_oversized_count() {
        let mut payload = vec![MSG_BLOCKS];
        payload.extend_from_slice(&((MAX_BLOCKS_BATCH + 1) as u32).to_be_bytes());
        assert!(parse_blocks(&payload).is_err());
    }

    #[test]
    fn ascii_leading_payloads_are_never_known_tags() {
        // S2-P18: text messages (the removed v0.8.6 protocol started with
        // ASCII letters) can never be confused with binary tags.
        for byte in b'A'..=b'Z' {
            assert!(!is_known_tag(byte), "{:#04x} must not be a tag", byte);
        }
    }

    #[test]
    fn decode_message_roundtrips_sync_requests() {
        assert_eq!(
            decode_message(&encode_get_headers(1234)).unwrap(),
            InboundMessage::GetHeaders(1234)
        );
        let hashes = [[1u8; 32], [2u8; 32]];
        assert_eq!(
            decode_message(&encode_get_blocks(&hashes).unwrap()).unwrap(),
            InboundMessage::GetBlocks(hashes.to_vec())
        );
    }

    #[test]
    fn decode_message_marks_known_non_request_tags_as_known() {
        for tag in KNOWN_TAGS {
            if tag == MSG_GET_HEADERS || tag == MSG_GET_BLOCKS {
                continue; // fully decoded above / malformed → Err
            }
            assert_eq!(
                decode_message(&[tag]).unwrap(),
                InboundMessage::KnownTag(tag)
            );
        }
    }

    #[test]
    fn decode_message_rejects_malformed_known_requests() {
        // Truncated GET_HEADERS payload: a protocol violation, not unknown.
        assert!(decode_message(&[MSG_GET_HEADERS, 0]).is_err());
        assert!(decode_message(&[MSG_GET_BLOCKS, 0, 0, 0, 1, 0xaa]).is_err());
    }

    #[test]
    fn decode_message_treats_unknown_and_empty_payloads_as_unknown_message() {
        assert_eq!(
            decode_message(b"PING").unwrap(),
            InboundMessage::UnknownMessage(b"PING".to_vec())
        );
        assert_eq!(
            decode_message(&[0x7f, 0xde, 0xad]).unwrap(),
            InboundMessage::UnknownMessage(vec![0x7f, 0xde, 0xad])
        );
        assert_eq!(
            decode_message(&[]).unwrap(),
            InboundMessage::UnknownMessage(Vec::new())
        );
    }
}
