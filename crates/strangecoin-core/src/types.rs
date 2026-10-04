use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::consensus::U256;

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

/// The header half of a [`Block`]: everything the canonical header encoding
/// (and therefore the PoW hash) commits to, minus the transaction bodies.
///
/// `hash` is derived — the wire encoding excludes it and the decoder
/// recomputes it — so a peer cannot lie about a header's identity.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockHeader {
    pub index: u64,
    pub timestamp: u64,
    pub previous_hash: String,
    pub hash: String,
    pub nonce: u64,
    pub target: String,
    pub consensus_version: u32,
    pub state_root: [u8; 32],
    pub tx_root: [u8; 32],
}

impl Block {
    /// Header view of this block (fields only, transactions dropped).
    pub fn header(&self) -> BlockHeader {
        BlockHeader {
            index: self.index,
            timestamp: self.timestamp,
            previous_hash: self.previous_hash.clone(),
            hash: self.hash.clone(),
            nonce: self.nonce,
            target: self.target.clone(),
            consensus_version: self.consensus_version,
            state_root: self.state_root,
            tx_root: self.tx_root,
        }
    }

    /// Recover a full block from a header plus its transaction bodies.
    pub fn from_header(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Block {
            index: header.index,
            timestamp: header.timestamp,
            transactions,
            previous_hash: header.previous_hash,
            hash: header.hash,
            nonce: header.nonce,
            target: header.target,
            consensus_version: header.consensus_version,
            state_root: header.state_root,
            tx_root: header.tx_root,
        }
    }
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

/// Chain snapshot for sync channel and GUI updates
#[derive(Clone, Debug)]
pub struct ChainSnapshot {
    pub chain: Vec<Block>,
    pub balances: HashMap<String, AccountState>,
    pub difficulty: u32,
    pub mempool_txs: Vec<Transaction>,
    pub total_work: U256,
}
