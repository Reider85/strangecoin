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
    #[serde(default)]
    pub consensus_version: u32,
    #[serde(default)]
    pub state_root: [u8; 32],
    #[serde(default)]
    pub tx_root: [u8; 32],
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountState {
    pub balance: u64,
    pub nonce: u64,
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
