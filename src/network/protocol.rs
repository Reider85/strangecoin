use crate::error::StrangecoinError;
use std::io::Read;
use strangecoin_core::serialize::{
    deserialize_block, deserialize_header, serialize_block, serialize_header, HEADER_WIRE_LEN,
};
use strangecoin_core::types::{Block, BlockHeader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;
pub const MAX_BLOCK_SIZE: usize = 4 * 1024 * 1024;
pub const MAX_TX_SIZE: usize = 256 * 1024;

pub const HELLO_PREFIX: &str = "HELLO:";

// ---------------------------------------------------------------- S1-P16
// Binary message tags (first payload byte). Text messages ("HELLO:",
// "GET_BLOCKCHAIN", "UPDATE_BLOCKCHAIN:…") start with ASCII letters, so the
// tag space 0x01..=0x04 is unambiguous and a pre-P16 peer silently ignores
// an unknown binary request (no response → read timeout → fallback).

pub const MSG_GET_HEADERS: u8 = 0x01;
pub const MSG_HEADERS: u8 = 0x02;
pub const MSG_GET_BLOCKS: u8 = 0x03;
pub const MSG_BLOCKS: u8 = 0x04;

/// Max headers per HEADERS message / per GET_HEADERS round (invariant #7:
/// the batch bound is enforced before any `Vec::with_capacity`).
pub const MAX_HEADERS_BATCH: usize = 2000;
/// Max blocks per GET_BLOCKS / BLOCKS message.
pub const MAX_BLOCKS_BATCH: usize = 128;

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn take_u32(bytes: &[u8], offset: &mut usize) -> Result<u32, StrangecoinError> {
    if *offset + 4 > bytes.len() {
        return Err(StrangecoinError::SizeLimitExceeded("u32 truncation"));
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

fn take_u64(bytes: &[u8], offset: &mut usize) -> Result<u64, StrangecoinError> {
    if *offset + 8 > bytes.len() {
        return Err(StrangecoinError::SizeLimitExceeded("u64 truncation"));
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

pub fn parse_get_headers(bytes: &[u8]) -> Result<u64, StrangecoinError> {
    if bytes.len() != 9 || bytes[0] != MSG_GET_HEADERS {
        return Err(StrangecoinError::SizeLimitExceeded("GET_HEADERS"));
    }
    let mut offset = 1;
    take_u64(bytes, &mut offset)
}

// ---------------------------------------------------------------- HEADERS

/// `HEADERS [0x02] [count: u32] [len: u32][header bytes]…` — headers in
/// ascending order; each item is the canonical encoding of one header
/// (hash excluded, recomputed by the decoder).
pub fn encode_headers(headers: &[BlockHeader]) -> Result<Vec<u8>, StrangecoinError> {
    if headers.len() > MAX_HEADERS_BATCH {
        return Err(StrangecoinError::SizeLimitExceeded("headers batch"));
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
        return Err(StrangecoinError::SizeLimitExceeded("headers message"));
    }
    Ok(out)
}

pub fn parse_headers(bytes: &[u8]) -> Result<Vec<BlockHeader>, StrangecoinError> {
    if bytes.is_empty() || bytes[0] != MSG_HEADERS {
        return Err(StrangecoinError::SizeLimitExceeded("HEADERS tag"));
    }
    let mut offset = 1;
    let count = take_u32(bytes, &mut offset)? as usize;
    if count > MAX_HEADERS_BATCH {
        return Err(StrangecoinError::SizeLimitExceeded("headers batch"));
    }
    // Count can only be plausible if the buffer has room for at least one
    // length prefix per item — check before reserving capacity (invariant #7).
    if count > (bytes.len() - offset) / 4 {
        return Err(StrangecoinError::SizeLimitExceeded("headers truncation"));
    }
    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take_u32(bytes, &mut offset)? as usize;
        if len != HEADER_WIRE_LEN || offset + len > bytes.len() {
            return Err(StrangecoinError::SizeLimitExceeded("header item"));
        }
        let header = deserialize_header(&bytes[offset..offset + len])
            .map_err(|_| StrangecoinError::SizeLimitExceeded("header decode"))?;
        offset += len;
        headers.push(header);
    }
    if offset != bytes.len() {
        return Err(StrangecoinError::SizeLimitExceeded("HEADERS trailing"));
    }
    Ok(headers)
}

// ------------------------------------------------------------- GET_BLOCKS

/// `GET_BLOCKS [0x03] [count: u32] [hash: 32 bytes]…` — request full blocks
/// by hash (order of the response follows the request order).
pub fn encode_get_blocks(hashes: &[[u8; 32]]) -> Result<Vec<u8>, StrangecoinError> {
    if hashes.len() > MAX_BLOCKS_BATCH {
        return Err(StrangecoinError::SizeLimitExceeded("blocks batch"));
    }
    let mut out = Vec::with_capacity(5 + hashes.len() * 32);
    out.push(MSG_GET_BLOCKS);
    put_u32(&mut out, hashes.len() as u32);
    for hash in hashes {
        out.extend_from_slice(hash);
    }
    Ok(out)
}

pub fn parse_get_blocks(bytes: &[u8]) -> Result<Vec<[u8; 32]>, StrangecoinError> {
    if bytes.len() < 5 || bytes[0] != MSG_GET_BLOCKS {
        return Err(StrangecoinError::SizeLimitExceeded("GET_BLOCKS"));
    }
    let mut offset = 1;
    let count = take_u32(bytes, &mut offset)? as usize;
    if count > MAX_BLOCKS_BATCH {
        return Err(StrangecoinError::SizeLimitExceeded("blocks batch"));
    }
    if count * 32 != bytes.len() - offset {
        return Err(StrangecoinError::SizeLimitExceeded("GET_BLOCKS payload"));
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
pub fn encode_blocks(blocks: &[Block]) -> Result<Vec<u8>, StrangecoinError> {
    if blocks.len() > MAX_BLOCKS_BATCH {
        return Err(StrangecoinError::SizeLimitExceeded("blocks batch"));
    }
    let mut out = Vec::new();
    out.push(MSG_BLOCKS);
    put_u32(&mut out, blocks.len() as u32);
    for block in blocks {
        let bytes = serialize_block(block);
        if bytes.len() > MAX_BLOCK_SIZE {
            return Err(StrangecoinError::SizeLimitExceeded("block size"));
        }
        put_u32(&mut out, bytes.len() as u32);
        out.extend_from_slice(&bytes);
    }
    if out.len() > MAX_MESSAGE_SIZE {
        return Err(StrangecoinError::SizeLimitExceeded("blocks message"));
    }
    Ok(out)
}

pub fn parse_blocks(bytes: &[u8]) -> Result<Vec<Block>, StrangecoinError> {
    if bytes.is_empty() || bytes[0] != MSG_BLOCKS {
        return Err(StrangecoinError::SizeLimitExceeded("BLOCKS tag"));
    }
    let mut offset = 1;
    let count = take_u32(bytes, &mut offset)? as usize;
    if count > MAX_BLOCKS_BATCH {
        return Err(StrangecoinError::SizeLimitExceeded("blocks batch"));
    }
    if count > (bytes.len() - offset) / 4 {
        return Err(StrangecoinError::SizeLimitExceeded("blocks truncation"));
    }
    let mut blocks = Vec::with_capacity(count);
    for _ in 0..count {
        let len = take_u32(bytes, &mut offset)? as usize;
        if len > MAX_BLOCK_SIZE || offset + len > bytes.len() {
            return Err(StrangecoinError::SizeLimitExceeded("block item"));
        }
        let block = deserialize_block(&bytes[offset..offset + len])
            .map_err(|_| StrangecoinError::SizeLimitExceeded("block decode"))?;
        offset += len;
        blocks.push(block);
    }
    if offset != bytes.len() {
        return Err(StrangecoinError::SizeLimitExceeded("BLOCKS trailing"));
    }
    Ok(blocks)
}

// ------------------------------------------------------------ existing API

pub fn encode_hello(network_id: u32) -> Vec<u8> {
    let hello_msg = format!("{}{}", HELLO_PREFIX, network_id);
    let length = hello_msg.len() as u32;
    let mut data = length.to_be_bytes().to_vec();
    data.extend_from_slice(hello_msg.as_bytes());
    data
}

pub fn parse_hello(bytes: &[u8]) -> Result<u32, StrangecoinError> {
    let msg = String::from_utf8_lossy(bytes);
    if !msg.starts_with(HELLO_PREFIX) {
        return Err(StrangecoinError::InvalidHelloMessage);
    }
    let network_id_str = &msg[HELLO_PREFIX.len()..];
    network_id_str
        .parse()
        .map_err(|_| StrangecoinError::InvalidHelloMessage)
}

pub fn read_length_prefixed<R: Read>(reader: &mut R) -> Result<Vec<u8>, StrangecoinError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).map_err(|_| {
        StrangecoinError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "length prefix",
        ))
    })?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(StrangecoinError::SizeLimitExceeded("message"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|_| {
        StrangecoinError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "message body",
        ))
    })?;
    Ok(buf)
}

/// Write a payload with the length-prefix framing every message uses.
pub fn write_length_prefixed<W: std::io::Write>(
    writer: &mut W,
    payload: &[u8],
) -> Result<(), StrangecoinError> {
    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(StrangecoinError::SizeLimitExceeded("message"));
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
) -> Result<Vec<u8>, StrangecoinError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await.map_err(|_| {
        StrangecoinError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "length prefix",
        ))
    })?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(StrangecoinError::SizeLimitExceeded("message"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await.map_err(|_| {
        StrangecoinError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "message body",
        ))
    })?;
    Ok(buf)
}

/// Async variant of [`write_length_prefixed`] over tokio I/O (ADR-0011).
pub async fn write_length_prefixed_async<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> Result<(), StrangecoinError> {
    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(StrangecoinError::SizeLimitExceeded("message"));
    }
    let mut data = (payload.len() as u32).to_be_bytes().to_vec();
    data.extend_from_slice(payload);
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use strangecoin_core::types::Transaction;

    fn sample_block() -> Block {
        let mut block = Block {
            index: 7,
            timestamp: 1_700_000_000,
            transactions: vec![Transaction {
                sender: "rsc1sender".into(),
                receiver: "rsc1receiver".into(),
                amount: 42,
                nonce: 1,
                chain_id: 3,
                signature: vec![1u8; 65],
                is_coinbase: false,
            }],
            previous_hash: "aa".repeat(32),
            hash: String::new(),
            nonce: 99,
            target: "ff".repeat(32),
            consensus_version: 1,
            state_root: [3u8; 32],
            tx_root: [7u8; 32],
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
        assert_eq!(got.transactions[0].amount, 42);
        assert_eq!(got.transactions[0].signature, vec![1u8; 65]);
        assert_eq!(got.transactions[0].sender, "rsc1sender");
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
    fn text_messages_never_collide_with_binary_tags() {
        assert!(b"HELLO"[0] != MSG_GET_HEADERS && b"HELLO"[0] != MSG_HEADERS);
        assert!(
            b"GET_BLOCKCHAIN"[0] != MSG_GET_BLOCKS && b"GET_BLOCKCHAIN"[0] != MSG_BLOCKS
        );
        assert!(b"UPDATE_BLOCKCHAIN"[0] > 0x04);
    }
}
