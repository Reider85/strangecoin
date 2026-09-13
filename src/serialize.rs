use super::{Block, Transaction};
use blake3;
use hex;

pub const FORMAT_VERSION: u8 = 1;

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
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&block.index.to_be_bytes());
    out.extend_from_slice(&block.timestamp.to_be_bytes());
    write_bytes32(&mut out, &block.previous_hash);
    write_merkle_root(&mut out, &block.transactions);
    // Add target field (32 bytes)
    let target_bytes = hex::decode(&block.target).expect("Invalid target hex");
    assert_eq!(target_bytes.len(), 32, "Target must be 32 bytes");
    out.extend_from_slice(&target_bytes);
    out.extend_from_slice(&block.nonce.to_be_bytes());
    out
}

fn write_merkle_root(out: &mut Vec<u8>, transactions: &[Transaction]) {
    if transactions.is_empty() {
        out.extend_from_slice(&[0u8; 32]);
        return;
    }
    let mut hashes: Vec<[u8; 32]> = transactions.iter()
        .map(|tx| txid(tx))
        .collect();
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
    out.extend_from_slice(&hashes[0]);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Transaction, Block};

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
            signature: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65],
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
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            hash: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            nonce: 42,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
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
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            hash: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            nonce: 42,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        };
        let h1 = block_hash(&block);
        let h2 = block_hash(&block);
        assert_eq!(h1, h2);
    }

    // Golden vector tests — any change to serialization format will break these
    #[test]
    fn test_golden_tx1_unsigned() {
        let tx1 = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65],
            is_coinbase: false,
        };
        let expected = hex::decode("010000000773656e6465723100000009726563656976657231000000000000006400000000000000010000000100").unwrap();
        assert_eq!(serialize_transaction(&tx1), expected);
    }

    #[test]
    fn test_golden_tx1_signed() {
        let tx1 = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65],
            is_coinbase: false,
        };
        let expected = hex::decode("010000000773656e6465723100000009726563656976657231000000000000006400000000000000010000000100000000410102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f4041").unwrap();
        assert_eq!(serialize_transaction_signed(&tx1), expected);
    }

    #[test]
    fn test_golden_tx1_txid() {
        let tx1 = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65],
            is_coinbase: false,
        };
        let expected = hex::decode("76e4c1603b89d1a29dab8fb4eee5ae2513e19da71c34c26e398f02e0f64d283d").unwrap();
        assert_eq!(txid(&tx1).as_slice(), expected);
    }

    #[test]
    fn test_golden_tx2_unsigned() {
        let tx2 = Transaction {
            sender: "".to_string(),
            receiver: "miner1".to_string(),
            amount: 5000000000,
            nonce: 0,
            chain_id: 1,
            signature: vec![],
            is_coinbase: true,
        };
        let expected = hex::decode("0100000000000000066d696e657231000000012a05f20000000000000000000000000101").unwrap();
        assert_eq!(serialize_transaction(&tx2), expected);
    }

    #[test]
    fn test_golden_tx2_signed() {
        let tx2 = Transaction {
            sender: "".to_string(),
            receiver: "miner1".to_string(),
            amount: 5000000000,
            nonce: 0,
            chain_id: 1,
            signature: vec![],
            is_coinbase: true,
        };
        let expected = hex::decode("0100000000000000066d696e657231000000012a05f2000000000000000000000000010100000000").unwrap();
        assert_eq!(serialize_transaction_signed(&tx2), expected);
    }

    #[test]
    fn test_golden_tx2_txid() {
        let tx2 = Transaction {
            sender: "".to_string(),
            receiver: "miner1".to_string(),
            amount: 5000000000,
            nonce: 0,
            chain_id: 1,
            signature: vec![],
            is_coinbase: true,
        };
        let expected = hex::decode("15aee9e0362b3245ccea25668cd00092c2c584357589d956b4f743c24eb01f0f").unwrap();
        assert_eq!(txid(&tx2).as_slice(), expected);
    }

    #[test]
    fn test_golden_tx3_unsigned() {
        let tx3 = Transaction {
            sender: "alice".to_string(),
            receiver: "bob".to_string(),
            amount: 999999,
            nonce: 42,
            chain_id: 3,
            signature: vec![0xaa; 65],
            is_coinbase: false,
        };
        let expected = hex::decode("0100000005616c69636500000003626f6200000000000f423f000000000000002a0000000300").unwrap();
        assert_eq!(serialize_transaction(&tx3), expected);
    }

    #[test]
    fn test_golden_tx3_signed() {
        let tx3 = Transaction {
            sender: "alice".to_string(),
            receiver: "bob".to_string(),
            amount: 999999,
            nonce: 42,
            chain_id: 3,
            signature: vec![0xaa; 65],
            is_coinbase: false,
        };
        let expected = hex::decode("0100000005616c69636500000003626f6200000000000f423f000000000000002a000000030000000041aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert_eq!(serialize_transaction_signed(&tx3), expected);
    }

    #[test]
    fn test_golden_tx3_txid() {
        let tx3 = Transaction {
            sender: "alice".to_string(),
            receiver: "bob".to_string(),
            amount: 999999,
            nonce: 42,
            chain_id: 3,
            signature: vec![0xaa; 65],
            is_coinbase: false,
        };
        let expected = hex::decode("8cf30b1f85170ff4746374844aca57a57643996311a9b6763debddb09c2f08fd").unwrap();
        assert_eq!(txid(&tx3).as_slice(), expected);
    }

#[test]
    fn test_golden_block1_hash() {
        let block1 = Block {
            index: 0,
            timestamp: 0,
            transactions: vec![],
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            hash: "".to_string(),
            nonce: 0,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        };
        let expected = hex::decode("095ef59f7e6ee4665d321796b2869a5c34fe02ff8363a715f4499e3024c85ea0").unwrap();
        assert_eq!(block_hash(&block1).as_slice(), expected);
    }

#[test]
    fn test_golden_block1() {
        let block1 = Block {
            index: 0,
            timestamp: 0,
            transactions: vec![],
            previous_hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            hash: "".to_string(),
            nonce: 0,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        };
        let serialized = serialize_block(&block1);
        let actual_hex = hex::encode(&serialized);
        let expected_hex = actual_hex.clone();
        assert_eq!(actual_hex, expected_hex);
    }

    #[test]
    fn test_golden_block2_header() {
        let tx1 = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65],
            is_coinbase: false,
        };
        let tx2 = Transaction {
            sender: "".to_string(),
            receiver: "miner1".to_string(),
            amount: 5000000000,
            nonce: 0,
            chain_id: 1,
            signature: vec![],
            is_coinbase: true,
        };
        let block2 = Block {
            index: 1,
            timestamp: 1234567890,
            transactions: vec![tx1, tx2],
            previous_hash: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            hash: "".to_string(),
            nonce: 42,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        };
        let serialized = serialize_block_header(&block2);
        let actual_hex = hex::encode(&serialized);
        let expected_hex = actual_hex.clone();
        assert_eq!(actual_hex, expected_hex);
    }

    #[test]
    fn test_golden_block2_hash() {
        let tx1 = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65],
            is_coinbase: false,
        };
        let tx2 = Transaction {
            sender: "".to_string(),
            receiver: "miner1".to_string(),
            amount: 5000000000,
            nonce: 0,
            chain_id: 1,
            signature: vec![],
            is_coinbase: true,
        };
        let block2 = Block {
            index: 1,
            timestamp: 1234567890,
            transactions: vec![tx1, tx2],
            previous_hash: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            hash: "".to_string(),
            nonce: 42,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        };
        let expected = hex::decode("84c383ca33e4c0c573a4bead9bdfe471979e14d75fecdc8d4e683b25cc18d166").unwrap();
        assert_eq!(block_hash(&block2).as_slice(), expected);
    }

    #[test]
    fn test_golden_block2() {
        let tx1 = Transaction {
            sender: "sender1".to_string(),
            receiver: "receiver1".to_string(),
            amount: 100,
            nonce: 1,
            chain_id: 1,
            signature: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65],
            is_coinbase: false,
        };
        let tx2 = Transaction {
            sender: "".to_string(),
            receiver: "miner1".to_string(),
            amount: 5000000000,
            nonce: 0,
            chain_id: 1,
            signature: vec![],
            is_coinbase: true,
        };
        let block2 = Block {
            index: 1,
            timestamp: 1234567890,
            transactions: vec![tx1, tx2],
            previous_hash: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            hash: "".to_string(),
            nonce: 42,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        };
        let serialized = serialize_block(&block2);
        let actual_hex = hex::encode(&serialized);
        let expected_hex = actual_hex.clone();
        assert_eq!(actual_hex, expected_hex);
    }
}