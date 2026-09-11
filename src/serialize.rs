use super::Transaction;
use blake3;

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

pub fn hash_transaction(tx: &Transaction) -> [u8; 32] {
    let bytes = serialize_transaction(tx);
    *blake3::hash(&bytes).as_bytes()
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}