use std::collections::{HashMap, BTreeMap};
use crate::{Transaction, AccountState};
use crate::error::StrangecoinError;
use crate::serialize::txid;
use crate::consensus::{verify_transaction, current_chain_id};

pub const MAX_PENDING_TXS: usize = 10_000;

pub type TxId = [u8; 32];

#[derive(Clone)]
pub struct Mempool {
    txs: HashMap<TxId, Transaction>,
    by_sender: HashMap<String, BTreeMap<u64, TxId>>,
}

impl Mempool {
    pub fn new() -> Self {
        Mempool {
            txs: HashMap::new(),
            by_sender: HashMap::new(),
        }
    }

    pub fn insert(&mut self, tx: Transaction, account_state: &AccountState) -> Result<(), StrangecoinError> {
        if self.txs.len() >= MAX_PENDING_TXS {
            return Err(StrangecoinError::MempoolFull(MAX_PENDING_TXS));
        }

        let tx_id = txid(&tx);
        if self.txs.contains_key(&tx_id) {
            return Err(StrangecoinError::DuplicateTx);
        }

        verify_transaction(&tx)?;

        if tx.chain_id != current_chain_id() {
            return Err(StrangecoinError::InvalidChainId {
                expected: current_chain_id(),
                got: tx.chain_id,
            });
        }

        if tx.nonce != account_state.nonce + 1 {
            return Err(StrangecoinError::InvalidNonce {
                expected: account_state.nonce + 1,
                got: tx.nonce,
            });
        }

        if tx.amount > account_state.balance {
            return Err(StrangecoinError::InsufficientBalance {
                available: account_state.balance,
                required: tx.amount,
            });
        }

        self.txs.insert(tx_id, tx.clone());
        self.by_sender
            .entry(tx.sender.clone())
            .or_default()
            .insert(tx.nonce, tx_id);

        Ok(())
    }

    pub fn remove(&mut self, tx_id: &TxId) {
        if let Some(tx) = self.txs.remove(tx_id) {
            if let Some(sender_map) = self.by_sender.get_mut(&tx.sender) {
                sender_map.remove(&tx.nonce);
                if sender_map.is_empty() {
                    self.by_sender.remove(&tx.sender);
                }
            }
        }
    }

    pub fn get_pending(&self, max_count: usize) -> Vec<Transaction> {
        self.txs
            .values()
            .take(max_count)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.txs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    pub fn contains(&self, tx_id: &TxId) -> bool {
        self.txs.contains_key(tx_id)
    }

    pub fn transactions(&self) -> Vec<Transaction> {
        self.txs.values().cloned().collect()
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}