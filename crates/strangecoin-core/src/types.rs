use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Block {
    pub index: u64,
    pub timestamp: u64,
    pub transactions: Vec<Transaction>,
    pub previous_hash: String,
    pub hash: String,
    pub nonce: u64,
    pub target: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Transaction {
    pub sender: String,
    pub receiver: String,
    pub amount: u64,
    #[serde(default)]
    pub nonce: u64,
    #[serde(default)]
    pub chain_id: u32,
    #[serde(default)]
    pub signature: Vec<u8>,
    #[serde(default)]
    pub is_coinbase: bool,
}
