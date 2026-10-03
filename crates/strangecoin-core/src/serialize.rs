use crate::types::{Block, BlockHeader, Transaction};
use blake3;
use hex;

pub const FORMAT_VERSION: u8 = 4;

pub fn serialize_transaction(tx: &Transaction) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    write_string(&mut out, &tx.sender);
    write_string(&mut out, &tx.receiver);
    out.extend_from_slice(&tx.amount.to_be_bytes());
    out.extend_from_slice(&tx.nonce.to_be_bytes());
    out.extend_from_slice(&tx.chain_id.to_be_bytes());
    out.push(tx.is_coinbase as u8);
    out
}

pub fn serialize_transaction_signed(tx: &Transaction) -> Vec<u8> {
    let mut out = serialize_transaction(tx);
    out.extend_from_slice(&(tx.signature.len() as u32).to_be_bytes());
    out.extend_from_slice(&tx.signature);
    out
}

pub fn txid(tx: &Transaction) -> [u8; 32] {
    *blake3::hash(&serialize_transaction_signed(tx)).as_bytes()
}

pub fn serialize_block_header(block: &Block) -> Vec<u8> {
    serialize_header(&block.header())
}

/// Canonical header encoding (identical to the block-header prefix of
/// [`serialize_block`]): the `hash` field is excluded because it is derived
/// from exactly these bytes.
pub fn serialize_header(header: &BlockHeader) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&header.index.to_be_bytes());
    out.extend_from_slice(&header.timestamp.to_be_bytes());
    write_bytes32(&mut out, &header.previous_hash);
    out.extend_from_slice(&header.tx_root);
    let target_bytes = hex::decode(&header.target).expect("Invalid target hex");
    assert_eq!(target_bytes.len(), 32, "Target must be 32 bytes");
    out.extend_from_slice(&target_bytes);
    out.extend_from_slice(&header.nonce.to_be_bytes());
    out.extend_from_slice(&header.consensus_version.to_be_bytes());
    out.extend_from_slice(&header.state_root);
    out
}

/// Byte length of [`serialize_header`] output.
pub const HEADER_WIRE_LEN: usize = 1 + 8 + 8 + 32 + 32 + 32 + 8 + 4 + 32;

/// PoW hash of a header: `blake3(serialize_header(header))`.
pub fn header_hash(header: &BlockHeader) -> [u8; 32] {
    *blake3::hash(&serialize_header(header)).as_bytes()
}

/// Decode a canonical header. The `hash` field is recomputed from the
/// encoded bytes, never trusted from the wire.
pub fn deserialize_header(bytes: &[u8]) -> Result<BlockHeader, &'static str> {
    if bytes.len() != HEADER_WIRE_LEN {
        return Err("Header length mismatch");
    }
    let mut offset = 0usize;
    if bytes[offset] != FORMAT_VERSION {
        return Err("Unsupported format version");
    }
    offset += 1;
    let index = read_u64_be(bytes, &mut offset);
    let timestamp = read_u64_be(bytes, &mut offset);
    let mut previous_hash_bytes = [0u8; 32];
    previous_hash_bytes.copy_from_slice(&bytes[offset..offset + 32]);
    offset += 32;
    let previous_hash = hex::encode(previous_hash_bytes);
    let mut tx_root = [0u8; 32];
    tx_root.copy_from_slice(&bytes[offset..offset + 32]);
    offset += 32;
    let mut target_bytes = [0u8; 32];
    target_bytes.copy_from_slice(&bytes[offset..offset + 32]);
    offset += 32;
    let target = hex::encode(target_bytes);
    let nonce = read_u64_be(bytes, &mut offset);
    let consensus_version = read_u32_be(bytes, &mut offset);
    let mut state_root = [0u8; 32];
    state_root.copy_from_slice(&bytes[offset..offset + 32]);
    offset += 32;
    debug_assert_eq!(offset, HEADER_WIRE_LEN);

    let mut header = BlockHeader {
        index,
        timestamp,
        previous_hash,
        hash: String::new(),
        nonce,
        target,
        consensus_version,
        state_root,
        tx_root,
    };
    header.hash = hex::encode(header_hash(&header));
    Ok(header)
}

pub fn merkle_root(txids: &[[u8; 32]]) -> [u8; 32] {
    if txids.is_empty() {
        return [0u8; 32];
    }
    if txids.len() == 1 {
        // Same odd-node duplication rule as every other level: a lone txid is
        // hashed with itself instead of being promoted as-is.
        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(&txids[0]);
        combined.extend_from_slice(&txids[0]);
        return *blake3::hash(&combined).as_bytes();
    }
    let mut hashes: Vec<[u8; 32]> = txids.to_vec();
    while hashes.len() > 1 {
        let mut next = Vec::new();
        for chunk in hashes.chunks(2) {
            let mut combined = Vec::new();
            combined.extend_from_slice(&chunk[0]);
            if chunk.len() > 1 {
                combined.extend_from_slice(&chunk[1]);
            } else {
                combined.extend_from_slice(&chunk[0]);
            }
            next.push(*blake3::hash(&combined).as_bytes());
        }
        hashes = next;
    }
    hashes[0]
}

pub fn compute_tx_root(transactions: &[Transaction]) -> [u8; 32] {
    if transactions.is_empty() {
        return [0u8; 32];
    }
    let txids: Vec<[u8; 32]> = transactions.iter().map(|tx| txid(tx)).collect();
    merkle_root(&txids)
}

pub fn block_hash(block: &Block) -> [u8; 32] {
    *blake3::hash(&serialize_block_header(block)).as_bytes()
}

pub fn serialize_block(block: &Block) -> Vec<u8> {
    let mut out = serialize_block_header(block);
    out.extend_from_slice(&(block.transactions.len() as u32).to_be_bytes());
    for tx in &block.transactions {
        let tx_bytes = serialize_transaction_signed(tx);
        out.extend_from_slice(&(tx_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&tx_bytes);
    }
    out
}

/// Inverse of [`serialize_block`]: canonical header prefix + length-prefixed
/// signed transactions. Rejects trailing bytes and absurd transaction counts
/// before any allocation (invariant #7).
pub fn deserialize_block(bytes: &[u8]) -> Result<Block, &'static str> {
    if bytes.len() < HEADER_WIRE_LEN + 4 {
        return Err("Block too short");
    }
    let header = deserialize_header(&bytes[..HEADER_WIRE_LEN])?;
    let mut offset = HEADER_WIRE_LEN;
    let tx_count = read_u32_be(bytes, &mut offset) as usize;
    // Every transaction needs at least its length prefix, so a count larger
    // than the remaining bytes can divide by 4 is garbage: bail before
    // reserving capacity for it.
    let remaining = bytes.len() - offset;
    if tx_count > remaining / 4 {
        return Err("Transaction count exceeds buffer");
    }
    let mut transactions = Vec::with_capacity(tx_count);
    for _ in 0..tx_count {
        let tx_len = read_u32_be(bytes, &mut offset) as usize;
        if tx_len > remaining || offset + tx_len > bytes.len() {
            return Err("Transaction length exceeds buffer");
        }
        let tx = deserialize_transaction_signed(&bytes[offset..offset + tx_len])?;
        offset += tx_len;
        transactions.push(tx);
    }
    if offset != bytes.len() {
        return Err("Trailing bytes after block");
    }
    Ok(Block::from_header(header, transactions))
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn write_bytes32(out: &mut Vec<u8>, s: &str) {
    let bytes = hex::decode(s).expect("Invalid hex hash");
    assert_eq!(bytes.len(), 32, "Expected 32-byte hash");
    out.extend_from_slice(&bytes);
}

pub fn read_u32_be(bytes: &[u8], offset: &mut usize) -> u32 {
    let val = u32::from_be_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
    ]);
    *offset += 4;
    val
}

pub fn read_u64_be(bytes: &[u8], offset: &mut usize) -> u64 {
    let val = u64::from_be_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
        bytes[*offset + 4],
        bytes[*offset + 5],
        bytes[*offset + 6],
        bytes[*offset + 7],
    ]);
    *offset += 8;
    val
}

pub fn read_string(bytes: &[u8], offset: &mut usize) -> Result<String, &'static str> {
    let len = read_u32_be(bytes, offset) as usize;
    if *offset + len > bytes.len() {
        return Err("Buffer too short for string");
    }
    let s = std::str::from_utf8(&bytes[*offset..*offset + len])
        .map_err(|_| "Invalid UTF-8 in string")?
        .to_string();
    *offset += len;
    Ok(s)
}

pub fn deserialize_transaction(bytes: &[u8]) -> Result<Transaction, &'static str> {
    let mut offset = 0;

    if offset >= bytes.len() {
        return Err("Empty buffer");
    }
    let format_version = bytes[offset];
    offset += 1;
    if format_version != FORMAT_VERSION {
        return Err("Unsupported format version");
    }

    let sender = read_string(bytes, &mut offset)?;
    let receiver = read_string(bytes, &mut offset)?;
    let amount = read_u64_be(bytes, &mut offset);
    let nonce = read_u64_be(bytes, &mut offset);
    let chain_id = read_u32_be(bytes, &mut offset) as u32;
    let is_coinbase = if offset < bytes.len() {
        let b = bytes[offset];
        offset += 1;
        b != 0
    } else {
        false
    };

    Ok(Transaction {
        sender,
        receiver,
        amount,
        nonce,
        chain_id,
        signature: Vec::new(),
        is_coinbase,
    })
}

/// Full round-trip parser for [`serialize_transaction_signed`]: all fields
/// plus the signature, rejecting truncated buffers and trailing bytes.
pub fn deserialize_transaction_signed(bytes: &[u8]) -> Result<Transaction, &'static str> {
    let mut offset = 0;

    if offset >= bytes.len() {
        return Err("Empty buffer");
    }
    let format_version = bytes[offset];
    offset += 1;
    if format_version != FORMAT_VERSION {
        return Err("Unsupported format version");
    }

    let sender = read_string(bytes, &mut offset)?;
    let receiver = read_string(bytes, &mut offset)?;
    let amount = read_u64_be(bytes, &mut offset);
    let nonce = read_u64_be(bytes, &mut offset);
    let chain_id = read_u32_be(bytes, &mut offset);
    if offset >= bytes.len() {
        return Err("Buffer too short for is_coinbase");
    }
    let is_coinbase = bytes[offset] != 0;
    offset += 1;

    let sig_len = read_u32_be(bytes, &mut offset) as usize;
    if offset + sig_len != bytes.len() {
        return Err("Signature length mismatch");
    }
    let signature = bytes[offset..].to_vec();

    Ok(Transaction {
        sender,
        receiver,
        amount,
        nonce,
        chain_id,
        signature,
        is_coinbase,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Block, Transaction};

    #[test]
    fn test_serialize_transaction_deterministic() {
        let tx = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4],
            is_coinbase: false,
        };
        let bytes1 = serialize_transaction(&tx);
        let bytes2 = serialize_transaction(&tx);
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_serialize_transaction_different() {
        let tx1 = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4],
            is_coinbase: false,
        };
        let tx2 = Transaction {
            sender: "sender2".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4],
            is_coinbase: false,
        };
        let bytes1 = serialize_transaction(&tx1);
        let bytes2 = serialize_transaction(&tx2);
        assert_ne!(bytes1, bytes2);
    }

    #[test]
    fn test_txid_deterministic() {
        let tx = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
                45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
            ],
            is_coinbase: false,
        };
        let id1 = txid(&tx);
        let id2 = txid(&tx);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_serialize_block_header_deterministic() {
        let block = Block {
            index: 1,
            timestamp: 1234567890,
            transactions: vec![],
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            hash: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            nonce: 42,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        let bytes1 = serialize_block_header(&block);
        let bytes2 = serialize_block_header(&block);
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_block_hash_deterministic() {
        let block = Block {
            index: 1,
            timestamp: 1234567890,
            transactions: vec![],
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            hash: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            nonce: 42,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        let h1 = block_hash(&block);
        let h2 = block_hash(&block);
        assert_eq!(h1, h2);
    }
}
