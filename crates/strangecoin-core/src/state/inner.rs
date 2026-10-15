use std::collections::HashMap;

use crate::economics::emission::block_reward_at_height_for_chain;
use crate::error::CoreError;
use crate::types::{Account, Block, Transaction, TransferTx};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub balances: HashMap<String, Account>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_balance(&self, addr: &str) -> u64 {
        self.balances.get(addr).map(|a| a.balance()).unwrap_or(0)
    }

    pub fn get_nonce(&self, addr: &str) -> u64 {
        self.balances.get(addr).map(|a| a.nonce()).unwrap_or(0)
    }

    pub fn get_account(&self, addr: &str) -> Option<&Account> {
        self.balances.get(addr)
    }

    pub fn total_supply(&self) -> u64 {
        self.balances.values().map(|a| a.balance()).sum()
    }

    /// Set the balance, preserving the account variant: crediting or
    /// debiting a contract account must not turn it into an EOA (S1.5-P07).
    pub fn set_balance(&mut self, addr: &str, balance: u64) {
        self.balances
            .entry(addr.to_string())
            .or_default()
            .set_balance(balance);
        self.prune_if_empty(addr);
    }

    pub fn set_nonce(&mut self, addr: &str, nonce: u64) {
        self.balances
            .entry(addr.to_string())
            .or_default()
            .set_nonce(nonce);
        self.prune_if_empty(addr);
    }

    /// Install `account` at `addr` verbatim (contract deploy path, S1.5-P07);
    /// an empty account removes the entry.
    pub fn set_account(&mut self, addr: &str, account: Account) {
        if account.is_empty() {
            self.balances.remove(addr);
        } else {
            self.balances.insert(addr.to_string(), account);
        }
    }

    /// An empty account ([`Account::is_empty`]: EOA `(0, 0)`, phantom
    /// contract) carries no information: every read path defaults a missing
    /// account to the empty EOA. Keeping the state free of empty accounts is
    /// what makes `apply`/`unapply` an exact round-trip, which reorg and
    /// state-cache invalidation depend on. Deployed contract accounts are
    /// never empty — their code_hash pins them in the state.
    fn prune_if_empty(&mut self, addr: &str) {
        let empty = self
            .balances
            .get(addr)
            .is_some_and(|account| account.is_empty());
        if empty {
            self.balances.remove(addr);
        }
    }
}

/// Apply `block` on top of `state`. `chain_id` pins the network the block is
/// validated for (BUG-S1-004): every transaction must carry it, and the
/// coinbase reward schedule is the one configured for that chain.
pub fn apply_block(state: &State, block: &Block, chain_id: u32) -> Result<State, CoreError> {
    let mut new_state = state.clone();

    if block.index == 0 {
        apply_genesis(&mut new_state, block, chain_id)?;
    } else {
        apply_non_genesis(&mut new_state, block, chain_id)?;
    }

    Ok(new_state)
}

fn apply_genesis(state: &mut State, block: &Block, chain_id: u32) -> Result<(), CoreError> {
    for tx in &block.transactions {
        validate_tx_chain_id(tx, chain_id)?;
        match tx {
            Transaction::Transfer(transfer) => credit_receiver(state, transfer)?,
            Transaction::Create(_) | Transaction::Call(_) => {
                return Err(CoreError::UnsupportedTxType {
                    reason: "create/call transactions are not supported until S1.5-P15"
                        .to_string(),
                })
            }
        }
    }
    Ok(())
}

fn validate_tx_chain_id(tx: &Transaction, chain_id: u32) -> Result<(), CoreError> {
    if tx.chain_id() != chain_id {
        return Err(CoreError::InvalidChainId {
            expected: chain_id,
            got: tx.chain_id(),
        });
    }
    Ok(())
}

fn apply_non_genesis(state: &mut State, block: &Block, chain_id: u32) -> Result<(), CoreError> {
    for tx in &block.transactions {
        validate_tx_chain_id(tx, chain_id)?;
    }

    let total_supply = state.total_supply();
    let expected_reward = block_reward_at_height_for_chain(block.index, total_supply, chain_id);

    let coinbase = block
        .transactions
        .iter()
        .find(|tx| tx.is_coinbase())
        .ok_or(CoreError::InvalidCoinbaseAmount {
            expected: expected_reward,
            got: 0,
        })?;

    let coinbase_transfer = match coinbase {
        Transaction::Transfer(transfer) => transfer,
        _ => {
            return Err(CoreError::UnsupportedTxType {
                reason: "coinbase must be a transfer transaction".to_string(),
            })
        }
    };

    if coinbase_transfer.amount > expected_reward {
        return Err(CoreError::InvalidCoinbaseAmount {
            expected: expected_reward,
            got: coinbase_transfer.amount,
        });
    }

    credit_receiver(state, coinbase_transfer)?;

    for tx in &block.transactions {
        if tx.is_coinbase() {
            continue;
        }
        match tx {
            Transaction::Transfer(transfer) => {
                deduct_sender(state, transfer)?;
                credit_receiver(state, transfer)?;
                increment_nonce(state, transfer)?;
            }
            Transaction::Create(_) | Transaction::Call(_) => {
                return Err(CoreError::UnsupportedTxType {
                    reason: "create/call transactions are not supported until S1.5-P15"
                        .to_string(),
                })
            }
        }
    }

    Ok(())
}

fn deduct_sender(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let sender_balance = state.get_balance(&tx.from);
    if sender_balance < tx.amount {
        return Err(CoreError::InsufficientBalance {
            sender: tx.from.clone(),
            available: sender_balance,
            required: tx.amount,
        });
    }
    let new_balance = sender_balance
        .checked_sub(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.from, new_balance);
    Ok(())
}

fn credit_receiver(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let receiver_balance = state.get_balance(&tx.to);
    let new_balance = receiver_balance
        .checked_add(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.to, new_balance);
    Ok(())
}

fn increment_nonce(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let nonce = state.get_nonce(&tx.from);
    state.set_nonce(&tx.from, nonce + 1);
    Ok(())
}

pub fn unapply_block(state: &State, block: &Block) -> Result<State, CoreError> {
    let mut new_state = state.clone();

    if block.index == 0 {
        unapply_genesis(&mut new_state, block)?;
    } else {
        unapply_non_genesis(&mut new_state, block)?;
    }

    Ok(new_state)
}

fn unapply_genesis(state: &mut State, block: &Block) -> Result<(), CoreError> {
    for tx in block.transactions.iter().rev() {
        match tx {
            Transaction::Transfer(transfer) => debit_receiver(state, transfer)?,
            Transaction::Create(_) | Transaction::Call(_) => {
                return Err(CoreError::UnsupportedTxType {
                    reason: "create/call transactions are not supported until S1.5-P15"
                        .to_string(),
                })
            }
        }
    }
    Ok(())
}

fn unapply_non_genesis(state: &mut State, block: &Block) -> Result<(), CoreError> {
    for tx in block.transactions.iter().rev() {
        match tx {
            Transaction::Transfer(transfer) => {
                if transfer.is_coinbase {
                    debit_receiver(state, transfer)?;
                    continue;
                }
                decrement_nonce(state, transfer)?;
                debit_receiver(state, transfer)?;
                credit_sender(state, transfer)?;
            }
            Transaction::Create(_) | Transaction::Call(_) => {
                return Err(CoreError::UnsupportedTxType {
                    reason: "create/call transactions are not supported until S1.5-P15"
                        .to_string(),
                })
            }
        }
    }
    Ok(())
}

fn debit_receiver(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let receiver_balance = state.get_balance(&tx.to);
    let new_balance = receiver_balance
        .checked_sub(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.to, new_balance);
    Ok(())
}

fn credit_sender(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let sender_balance = state.get_balance(&tx.from);
    let new_balance = sender_balance
        .checked_add(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.from, new_balance);
    Ok(())
}

fn decrement_nonce(state: &mut State, tx: &TransferTx) -> Result<(), CoreError> {
    let nonce = state.get_nonce(&tx.from);
    state.set_nonce(&tx.from, nonce - 1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::CHAIN_ID_REGTEST;

    /// All fixtures below are regtest chains (chain_id 3, reward 0).
    fn apply_regtest(state: &State, block: &Block) -> Result<State, CoreError> {
        apply_block(state, block, CHAIN_ID_REGTEST)
    }

    fn test_state_with_balance(addr: &str, balance: u64) -> State {
        let mut state = State::new();
        state.set_balance(addr, balance);
        state
    }

    fn coinbase_tx(receiver: &str, amount: u64) -> Transaction {
        Transaction::coinbase("coinbase", receiver, amount, 3)
    }

    fn transfer_tx(sender: &str, receiver: &str, amount: u64, nonce: u64) -> Transaction {
        Transaction::transfer(sender, receiver, amount, nonce, 3)
    }

    fn test_block(index: u64, transactions: Vec<Transaction>) -> Block {
        let txs = if index > 0 {
            let mut v = vec![coinbase_tx("miner", 0)];
            v.extend(transactions);
            v
        } else {
            transactions
        };
        Block {
            index,
            timestamp: 1000 + index * 600,
            transactions: txs,
            previous_hash: "prev_hash".to_string(),
            hash: format!("block_hash_{}", index),
            nonce: 0,
            target: "ff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        }
    }

    #[test]
    fn apply_genesis_creates_balances() {
        let state = State::new();
        let tx = coinbase_tx("alice", 1000);
        let block = test_block(0, vec![tx]);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 1000);
    }

    #[test]
    fn apply_genesis_multiple_recipients() {
        let state = State::new();
        let txs = vec![coinbase_tx("alice", 500), coinbase_tx("bob", 300)];
        let block = test_block(0, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 500);
        assert_eq!(new_state.get_balance("bob"), 300);
    }

    #[test]
    fn apply_block_deducts_sender() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![transfer_tx("alice", "bob", 400, 1)];
        let block = test_block(1, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 600);
        assert_eq!(new_state.get_balance("bob"), 400);
    }

    #[test]
    fn apply_block_increments_nonce() {
        let mut state = test_state_with_balance("alice", 1000);
        state.set_nonce("alice", 0);
        let txs = vec![transfer_tx("alice", "bob", 100, 1)];
        let block = test_block(1, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_nonce("alice"), 1);
    }

    #[test]
    fn apply_block_rejects_insufficient_balance() {
        let state = test_state_with_balance("alice", 100);
        let txs = vec![transfer_tx("alice", "bob", 200, 1)];
        let block = test_block(1, txs);
        assert!(apply_regtest(&state, &block).is_err());
    }

    #[test]
    fn apply_block_coinbase_no_sender_check() {
        let state = State::new();
        let block = Block {
            index: 1,
            timestamp: 1600,
            transactions: vec![coinbase_tx("miner", 0)],
            previous_hash: "prev_hash".to_string(),
            hash: "block_hash_1".to_string(),
            nonce: 0,
            target: "ff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("miner"), 0);
    }

    #[test]
    fn unapply_genesis_removes_balances() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![coinbase_tx("alice", 1000)];
        let block = test_block(0, txs);
        let applied = apply_regtest(&state, &block).unwrap();
        let restored = unapply_block(&applied, &block).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn unapply_block_restores_nonce() {
        let mut state = test_state_with_balance("alice", 1000);
        state.set_nonce("alice", 0);
        let txs = vec![transfer_tx("alice", "bob", 100, 1)];
        let block = test_block(1, txs);
        let applied = apply_regtest(&state, &block).unwrap();
        assert_eq!(applied.get_nonce("alice"), 1);
        let restored = unapply_block(&applied, &block).unwrap();
        assert_eq!(restored.get_nonce("alice"), 0);
        assert_eq!(restored.get_balance("alice"), 1000);
        assert_eq!(restored.get_balance("bob"), 0);
    }

    #[test]
    fn apply_unapply_roundtrip() {
        let mut state = State::new();
        state.set_balance("alice", 5000);
        state.set_balance("bob", 2000);
        state.set_nonce("alice", 0);
        state.set_nonce("bob", 0);

        let genesis_txs = vec![coinbase_tx("alice", 5000), coinbase_tx("bob", 2000)];
        let genesis = test_block(0, genesis_txs);
        let after_genesis = apply_regtest(&state, &genesis).unwrap();

        let txs = vec![
            transfer_tx("alice", "bob", 300, 1),
            transfer_tx("bob", "alice", 100, 1),
        ];
        let block1 = test_block(1, txs);
        let after_block1 = apply_regtest(&after_genesis, &block1).unwrap();

        let txs2 = vec![transfer_tx("alice", "bob", 500, 2)];
        let block2 = test_block(2, txs2);
        let after_block2 = apply_regtest(&after_block1, &block2).unwrap();

        let restored2 = unapply_block(&after_block2, &block2).unwrap();
        assert_eq!(restored2, after_block1);

        let restored1 = unapply_block(&restored2, &block1).unwrap();
        assert_eq!(restored1, after_genesis);

        let restored_genesis = unapply_block(&restored1, &genesis).unwrap();
        assert_eq!(restored_genesis, state);
    }

    #[test]
    fn total_supply_tracks_correctly() {
        let state = State::new();
        let txs = vec![coinbase_tx("miner", 100)];
        let block = test_block(0, txs);
        let new_state = apply_regtest(&state, &block).unwrap();
        assert_eq!(new_state.total_supply(), 100);
    }

    #[test]
    fn multiple_transactions_in_block() {
        let mut state = State::new();
        state.set_balance("alice", 10000);
        state.set_nonce("alice", 0);

        let txs = vec![
            transfer_tx("alice", "bob", 200, 1),
            transfer_tx("alice", "charlie", 300, 2),
        ];
        let block = test_block(1, txs);
        let new_state = apply_regtest(&state, &block).unwrap();

        assert_eq!(new_state.get_balance("alice"), 9500);
        assert_eq!(new_state.get_balance("bob"), 200);
        assert_eq!(new_state.get_balance("charlie"), 300);
        assert_eq!(new_state.get_nonce("alice"), 2);
    }
}
