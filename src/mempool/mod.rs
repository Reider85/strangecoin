use crate::consensus::{current_chain_id, verify_transaction};
use crate::error::StrangecoinError;
use strangecoin_core::serialize::txid;
use crate::{AccountState, Transaction};
use std::collections::{BTreeMap, HashMap};

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

    pub fn insert(
        &mut self,
        tx: Transaction,
        account_state: &AccountState,
    ) -> Result<(), StrangecoinError> {
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

        // Nonce должен продолжать подтверждённую последовательность с учётом
        // уже ожидающих транзакций отправителя: иначе две транзакции с одним
        // nonce (двойной спенд) обе пройдут проверку.
        let pending_count = self
            .by_sender
            .get(&tx.sender)
            .map(|m| m.len() as u64)
            .unwrap_or(0);
        let expected_nonce = account_state.nonce + 1 + pending_count;
        if tx.nonce != expected_nonce {
            return Err(StrangecoinError::InvalidNonce {
                expected: expected_nonce,
                got: tx.nonce,
            });
        }

        // Доступный баланс — это подтверждённый баланс минус сумма уже
        // ожидающих расходов отправителя (иначе мемпул пропустит overspend).
        let pending_spent: u64 = self
            .by_sender
            .get(&tx.sender)
            .map(|m| {
                m.values()
                    .filter_map(|id| self.txs.get(id))
                    .map(|t| t.amount)
                    .sum()
            })
            .unwrap_or(0);
        let available = account_state.balance.saturating_sub(pending_spent);
        if tx.amount > available {
            return Err(StrangecoinError::InsufficientBalance {
                available,
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
        self.txs.values().take(max_count).cloned().collect()
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
