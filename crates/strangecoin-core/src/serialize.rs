use crate::types::{Block, BlockHeader, CallTx, CreateTx, Transaction, TransferTx};
use blake3;
use hex;

pub const FORMAT_VERSION: u8 = 5;

pub const TX_TAG_TRANSFER: u8 = 0;
pub const TX_TAG_CREATE: u8 = 1;
pub const TX_TAG_CALL: u8 = 2;

pub fn serialize_transaction(tx: &Transaction) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    match tx {
        Transaction::Transfer(t) => {
            out.push(TX_TAG_TRANSFER);
            write_string(&mut out, &t.from);
            write_string(&mut out, &t.to);
            out.extend_from_slice(&t.amount.to_be_bytes());
            out.extend_from_slice(&t.fee.to_be_bytes());
            out.extend_from_slice(&t.nonce.to_be_bytes());
            out.extend_from_slice(&t.chain_id.to_be_bytes());
            out.push(t.is_coinbase as u8);
        }
        Transaction::Create(t) => {
            out.push(TX_TAG_CREATE);
            write_string(&mut out, &t.from);
            write_bytes(&mut out, &t.init_code);
            write_bytes(&mut out, &t.init_args);
            out.extend_from_slice(&t.endowment.to_be_bytes());
            out.extend_from_slice(&t.fee.to_be_bytes());
            out.extend_from_slice(&t.nonce.to_be_bytes());
            out.extend_from_slice(&t.chain_id.to_be_bytes());
        }
        Transaction::Call(t) => {
            out.push(TX_TAG_CALL);
            write_string(&mut out, &t.from);
            write_string(&mut out, &t.to);
            write_bytes(&mut out, &t.data);
            out.extend_from_slice(&t.value.to_be_bytes());
            out.extend_from_slice(&t.gas_limit.to_be_bytes());
            out.extend_from_slice(&t.fee.to_be_bytes());
            out.extend_from_slice(&t.nonce.to_be_bytes());
            out.extend_from_slice(&t.chain_id.to_be_bytes());
        }
    }
    out
}

pub fn serialize_transaction_signed(tx: &Transaction) -> Vec<u8> {
    let mut out = serialize_transaction(tx);
    let signature = tx.signature();
    out.extend_from_slice(&(signature.len() as u32).to_be_bytes());
    out.extend_from_slice(signature);
    out
}

pub fn txid(tx: &Transaction) -> [u8; 32] {
    *blake3::hash(&serialize_transaction_signed(tx)).as_bytes()
}

/// Canonical receipt encoding (S1.5-P14): the leaf preimage of the receipts
/// sub-trie. Field order is fixed and lengths are explicit, so identical
/// receipts always hash to the same leaf.
pub fn serialize_receipt(receipt: &crate::vm::Receipt) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&receipt.txid);
    out.extend_from_slice(&receipt.block_height.to_be_bytes());
    out.extend_from_slice(&receipt.tx_index.to_be_bytes());
    out.extend_from_slice(&receipt.gas_used.to_be_bytes());
    out.push(receipt.status.canonical_byte());
    out.extend_from_slice(&(receipt.logs.len() as u32).to_be_bytes());
    for log in &receipt.logs {
        write_string(&mut out, &log.address);
        out.extend_from_slice(&(log.topics.len() as u32).to_be_bytes());
        for topic in &log.topics {
            out.extend_from_slice(topic);
        }
        write_bytes(&mut out, &log.data);
    }
    out
}

/// Merkle commitment over a block's receipts (S1.5-P14): one blake3 leaf per
/// receipt, sorted for an order-independent set commitment, folded with the
/// same odd-node duplication rule as [`merkle_root`]. An empty receipt list
/// commits to `[0u8; 32]`.
pub fn compute_receipts_root(receipts: &[crate::vm::Receipt]) -> [u8; 32] {
    let mut leaves: Vec<[u8; 32]> = receipts
        .iter()
        .map(|r| *blake3::hash(&serialize_receipt(r)).as_bytes())
        .collect();
    leaves.sort_unstable();
    merkle_root(&leaves)
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
    let index = read_u64_be(bytes, &mut offset)?;
    let timestamp = read_u64_be(bytes, &mut offset)?;
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
    let nonce = read_u64_be(bytes, &mut offset)?;
    let consensus_version = read_u32_be(bytes, &mut offset)?;
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
    let txids: Vec<[u8; 32]> = transactions.iter().map(txid).collect();
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
    let tx_count = read_u32_be(bytes, &mut offset)? as usize;
    // Every transaction needs at least its length prefix, so a count larger
    // than the remaining bytes can divide by 4 is garbage: bail before
    // reserving capacity for it.
    let remaining = bytes.len() - offset;
    if tx_count > remaining / 4 {
        return Err("Transaction count exceeds buffer");
    }
    let mut transactions = Vec::with_capacity(tx_count);
    for _ in 0..tx_count {
        let tx_len = read_u32_be(bytes, &mut offset)? as usize;
        if offset + tx_len > bytes.len() {
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

fn write_bytes(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
}

fn write_bytes32(out: &mut Vec<u8>, s: &str) {
    let bytes = hex::decode(s).expect("Invalid hex hash");
    assert_eq!(bytes.len(), 32, "Expected 32-byte hash");
    out.extend_from_slice(&bytes);
}

pub fn read_u32_be(bytes: &[u8], offset: &mut usize) -> Result<u32, &'static str> {
    if *offset + 4 > bytes.len() {
        return Err("Buffer too short for u32");
    }
    let val = u32::from_be_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
    ]);
    *offset += 4;
    Ok(val)
}

pub fn read_u64_be(bytes: &[u8], offset: &mut usize) -> Result<u64, &'static str> {
    if *offset + 8 > bytes.len() {
        return Err("Buffer too short for u64");
    }
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
    Ok(val)
}

pub fn read_string(bytes: &[u8], offset: &mut usize) -> Result<String, &'static str> {
    let len = read_u32_be(bytes, offset)? as usize;
    if *offset + len > bytes.len() {
        return Err("Buffer too short for string");
    }
    let s = std::str::from_utf8(&bytes[*offset..*offset + len])
        .map_err(|_| "Invalid UTF-8 in string")?
        .to_string();
    *offset += len;
    Ok(s)
}

fn read_bytes(bytes: &[u8], offset: &mut usize) -> Result<Vec<u8>, &'static str> {
    let len = read_u32_be(bytes, offset)? as usize;
    if *offset + len > bytes.len() {
        return Err("Buffer too short for bytes");
    }
    let out = bytes[*offset..*offset + len].to_vec();
    *offset += len;
    Ok(out)
}

fn read_tx_tag(bytes: &[u8], offset: &mut usize) -> Result<u8, &'static str> {
    if *offset >= bytes.len() {
        return Err("Buffer too short for tx tag");
    }
    let tag = bytes[*offset];
    *offset += 1;
    match tag {
        TX_TAG_TRANSFER | TX_TAG_CREATE | TX_TAG_CALL => Ok(tag),
        _ => Err("Unknown transaction tag"),
    }
}

fn deserialize_unsigned_body(
    bytes: &[u8],
    offset: &mut usize,
) -> Result<Transaction, &'static str> {
    let tag = read_tx_tag(bytes, offset)?;
    match tag {
        TX_TAG_TRANSFER => {
            let from = read_string(bytes, offset)?;
            let to = read_string(bytes, offset)?;
            let amount = read_u64_be(bytes, offset)?;
            let fee = read_u64_be(bytes, offset)?;
            let nonce = read_u64_be(bytes, offset)?;
            let chain_id = read_u32_be(bytes, offset)?;
            if *offset >= bytes.len() {
                return Err("Buffer too short for is_coinbase");
            }
            let is_coinbase = bytes[*offset] != 0;
            *offset += 1;
            Ok(Transaction::Transfer(TransferTx {
                from,
                to,
                amount,
                fee,
                nonce,
                chain_id,
                signature: Vec::new(),
                is_coinbase,
            }))
        }
        TX_TAG_CREATE => {
            let from = read_string(bytes, offset)?;
            let init_code = read_bytes(bytes, offset)?;
            let init_args = read_bytes(bytes, offset)?;
            let endowment = read_u64_be(bytes, offset)?;
            let fee = read_u64_be(bytes, offset)?;
            let nonce = read_u64_be(bytes, offset)?;
            let chain_id = read_u32_be(bytes, offset)?;
            Ok(Transaction::Create(CreateTx {
                from,
                init_code,
                init_args,
                endowment,
                fee,
                nonce,
                chain_id,
                signature: Vec::new(),
            }))
        }
        TX_TAG_CALL => {
            let from = read_string(bytes, offset)?;
            let to = read_string(bytes, offset)?;
            let data = read_bytes(bytes, offset)?;
            let value = read_u64_be(bytes, offset)?;
            let gas_limit = read_u64_be(bytes, offset)?;
            let fee = read_u64_be(bytes, offset)?;
            let nonce = read_u64_be(bytes, offset)?;
            let chain_id = read_u32_be(bytes, offset)?;
            Ok(Transaction::Call(CallTx {
                from,
                to,
                data,
                value,
                gas_limit,
                fee,
                nonce,
                chain_id,
                signature: Vec::new(),
            }))
        }
        _ => Err("Unknown transaction tag"),
    }
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

    deserialize_unsigned_body(bytes, &mut offset)
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

    let mut tx = deserialize_unsigned_body(bytes, &mut offset)?;

    let sig_len = read_u32_be(bytes, &mut offset)? as usize;
    if offset + sig_len != bytes.len() {
        return Err("Signature length mismatch");
    }
    *tx.signature_mut() = bytes[offset..].to_vec();

    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Block, Transaction};

    fn sample_transfer(sender: &str) -> Transaction {
        let mut tx = Transaction::transfer(sender, "receiver1", 100, 1, 1);
        *tx.signature_mut() = vec![1, 2, 3, 4];
        tx
    }

    #[test]
    fn test_serialize_transaction_deterministic() {
        let tx = sample_transfer("sender1");
        let bytes1 = serialize_transaction(&tx);
        let bytes2 = serialize_transaction(&tx);
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_serialize_transaction_different() {
        let tx1 = sample_transfer("sender1");
        let tx2 = sample_transfer("sender2");
        let bytes1 = serialize_transaction(&tx1);
        let bytes2 = serialize_transaction(&tx2);
        assert_ne!(bytes1, bytes2);
    }

    #[test]
    fn test_txid_deterministic() {
        let mut tx = Transaction::transfer("sender1", "receiver1", 100, 1, 1);
        *tx.signature_mut() = (1..=65).collect();
        let id1 = txid(&tx);
        let id2 = txid(&tx);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_roundtrip_all_variants() {
        let transfer = Transaction::transfer("sender1", "receiver1", 100, 1, 1);
        let create = Transaction::create("sender1", vec![1, 2, 3], vec![4, 5], 50, 1, 1);
        let call = Transaction::call("sender1", "receiver1", vec![9], 7, 100_000, 1, 1);
        for tx in [transfer, create, call] {
            let signed = serialize_transaction_signed(&tx);
            let decoded = deserialize_transaction_signed(&signed).unwrap();
            assert_eq!(decoded, tx);
            let unsigned = serialize_transaction(&tx);
            let decoded_u = deserialize_transaction(&unsigned).unwrap();
            assert_eq!(decoded_u, tx);
        }
    }

    #[test]
    fn test_reject_unknown_tag() {
        let mut bytes = serialize_transaction(&Transaction::transfer("a", "b", 1, 1, 1));
        bytes[1] = 9;
        assert!(deserialize_transaction(&bytes).is_err());
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
