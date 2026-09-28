use std::collections::HashMap;

use crate::economics::emission::block_reward_at_height;
use crate::error::CoreError;
use crate::types::{AccountState, Block, Transaction};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub balances: HashMap<String, AccountState>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_balance(&self, addr: &str) -> u64 {
        self.balances.get(addr).map(|a| a.balance).unwrap_or(0)
    }

    pub fn get_nonce(&self, addr: &str) -> u64 {
        self.balances.get(addr).map(|a| a.nonce).unwrap_or(0)
    }

    pub fn total_supply(&self) -> u64 {
        self.balances.values().map(|a| a.balance).sum()
    }

    pub fn set_balance(&mut self, addr: &str, balance: u64) {
        self.balances
            .entry(addr.to_string())
            .or_default()
            .balance = balance;
    }

    pub fn set_nonce(&mut self, addr: &str, nonce: u64) {
        self.balances
            .entry(addr.to_string())
            .or_default()
            .nonce = nonce;
    }
}

pub fn apply_block(state: &State, block: &Block) -> Result<State, CoreError> {
    let mut new_state = state.clone();

    if block.index == 0 {
        apply_genesis(&mut new_state, block)?;
    } else {
        apply_non_genesis(&mut new_state, block)?;
    }

    Ok(new_state)
}

fn apply_genesis(state: &mut State, block: &Block) -> Result<(), CoreError> {
    for tx in &block.transactions {
        credit_receiver(state, tx)?;
    }
    Ok(())
}

fn apply_non_genesis(state: &mut State, block: &Block) -> Result<(), CoreError> {
    let total_supply = state.total_supply();
    let expected_reward = block_reward_at_height(block.index, total_supply);

    let coinbase = block
        .transactions
        .iter()
        .find(|tx| tx.is_coinbase)
        .ok_or(CoreError::InvalidCoinbaseAmount {
            expected: expected_reward,
            got: 0,
        })?;

    if coinbase.amount > expected_reward {
        return Err(CoreError::InvalidCoinbaseAmount {
            expected: expected_reward,
            got: coinbase.amount,
        });
    }

    credit_receiver(state, coinbase)?;

    for tx in &block.transactions {
        if tx.is_coinbase {
            continue;
        }
        deduct_sender(state, tx)?;
        credit_receiver(state, tx)?;
        increment_nonce(state, tx)?;
    }

    Ok(())
}

fn deduct_sender(state: &mut State, tx: &Transaction) -> Result<(), CoreError> {
    let sender_balance = state.get_balance(&tx.sender);
    if sender_balance < tx.amount {
        return Err(CoreError::InsufficientBalance {
            sender: tx.sender.clone(),
            available: sender_balance,
            required: tx.amount,
        });
    }
    let new_balance = sender_balance
        .checked_sub(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.sender, new_balance);
    Ok(())
}

fn credit_receiver(state: &mut State, tx: &Transaction) -> Result<(), CoreError> {
    let receiver_balance = state.get_balance(&tx.receiver);
    let new_balance = receiver_balance
        .checked_add(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.receiver, new_balance);
    Ok(())
}

fn increment_nonce(state: &mut State, tx: &Transaction) -> Result<(), CoreError> {
    let nonce = state.get_nonce(&tx.sender);
    state.set_nonce(&tx.sender, nonce + 1);
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
        debit_receiver(state, tx)?;
    }
    Ok(())
}

fn unapply_non_genesis(state: &mut State, block: &Block) -> Result<(), CoreError> {
    for tx in block.transactions.iter().rev() {
        if tx.is_coinbase {
            debit_receiver(state, tx)?;
            continue;
        }
        decrement_nonce(state, tx)?;
        debit_receiver(state, tx)?;
        credit_sender(state, tx)?;
    }
    Ok(())
}

fn debit_receiver(state: &mut State, tx: &Transaction) -> Result<(), CoreError> {
    let receiver_balance = state.get_balance(&tx.receiver);
    let new_balance = receiver_balance
        .checked_sub(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.receiver, new_balance);
    Ok(())
}

fn credit_sender(state: &mut State, tx: &Transaction) -> Result<(), CoreError> {
    let sender_balance = state.get_balance(&tx.sender);
    let new_balance = sender_balance
        .checked_add(tx.amount)
        .ok_or(CoreError::StateOverflow)?;
    state.set_balance(&tx.sender, new_balance);
    Ok(())
}

fn decrement_nonce(state: &mut State, tx: &Transaction) -> Result<(), CoreError> {
    let nonce = state.get_nonce(&tx.sender);
    state.set_nonce(&tx.sender, nonce - 1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state_with_balance(addr: &str, balance: u64) -> State {
        let mut state = State::new();
        state.set_balance(addr, balance);
        state
    }

    fn coinbase_tx(receiver: &str, amount: u64) -> Transaction {
        Transaction {
            sender: "coinbase".to_string(),
            receiver: receiver.to_string(),
            amount,
            nonce: 0,
            chain_id: 3,
            signature: Vec::new(),
            is_coinbase: true,
        }
    }

    fn transfer_tx(sender: &str, receiver: &str, amount: u64, nonce: u64) -> Transaction {
        Transaction {
            sender: sender.to_string(),
            receiver: receiver.to_string(),
            amount,
            nonce,
            chain_id: 3,
            signature: Vec::new(),
            is_coinbase: false,
        }
    }

    fn test_block(index: u64, transactions: Vec<Transaction>) -> Block {
        let mut txs = if index > 0 {
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
        }
    }

    #[test]
    fn apply_genesis_creates_balances() {
        let state = State::new();
        let tx = coinbase_tx("alice", 1000);
        let block = test_block(0, vec![tx]);
        let new_state = apply_block(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 1000);
    }

    #[test]
    fn apply_genesis_multiple_recipients() {
        let state = State::new();
        let txs = vec![
            coinbase_tx("alice", 500),
            coinbase_tx("bob", 300),
        ];
        let block = test_block(0, txs);
        let new_state = apply_block(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 500);
        assert_eq!(new_state.get_balance("bob"), 300);
    }

    #[test]
    fn apply_block_deducts_sender() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![transfer_tx("alice", "bob", 400, 1)];
        let block = test_block(1, txs);
        let new_state = apply_block(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("alice"), 600);
        assert_eq!(new_state.get_balance("bob"), 400);
    }

    #[test]
    fn apply_block_increments_nonce() {
        let mut state = test_state_with_balance("alice", 1000);
        state.set_nonce("alice", 0);
        let txs = vec![transfer_tx("alice", "bob", 100, 1)];
        let block = test_block(1, txs);
        let new_state = apply_block(&state, &block).unwrap();
        assert_eq!(new_state.get_nonce("alice"), 1);
    }

    #[test]
    fn apply_block_rejects_insufficient_balance() {
        let state = test_state_with_balance("alice", 100);
        let txs = vec![transfer_tx("alice", "bob", 200, 1)];
        let block = test_block(1, txs);
        assert!(apply_block(&state, &block).is_err());
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
        };
        let new_state = apply_block(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("miner"), 0);
    }

    #[test]
    fn unapply_genesis_removes_balances() {
        let state = test_state_with_balance("alice", 1000);
        let txs = vec![coinbase_tx("alice", 1000)];
        let block = test_block(0, txs);
        let applied = apply_block(&state, &block).unwrap();
        let restored = unapply_block(&applied, &block).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn unapply_block_restores_nonce() {
        let mut state = test_state_with_balance("alice", 1000);
        state.set_nonce("alice", 0);
        let txs = vec![transfer_tx("alice", "bob", 100, 1)];
        let block = test_block(1, txs);
        let applied = apply_block(&state, &block).unwrap();
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
        let after_genesis = apply_block(&state, &genesis).unwrap();

        let txs = vec![
            transfer_tx("alice", "bob", 300, 1),
            transfer_tx("bob", "alice", 100, 1),
        ];
        let block1 = test_block(1, txs);
        let after_block1 = apply_block(&after_genesis, &block1).unwrap();

        let txs2 = vec![transfer_tx("alice", "bob", 500, 2)];
        let block2 = test_block(2, txs2);
        let after_block2 = apply_block(&after_block1, &block2).unwrap();

        let restored2 = unapply_block(&after_block2, &block2).unwrap();
        assert_eq!(restored2, after_block1);

        let restored1 = unapply_block(&restored2, &block1).unwrap();
        assert_eq!(restored1, after_genesis);

        let restored_genesis = unapply_block(&restored1, &genesis).unwrap();
        assert_eq!(restored_genesis, state);
    }

    #[test]
    fn total_supply tracks_correctly() {
        let state = State::new();
        let txs = vec![coinbase_tx("miner", 100)];
        let block = test_block(0, txs);
        let new_state = apply_block(&state, &block).unwrap();
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
        let new_state = apply_block(&state, &block).unwrap();

        assert_eq!(new_state.get_balance("alice"), 9500);
        assert_eq!(new_state.get_balance("bob"), 200);
        assert_eq!(new_state.get_balance("charlie"), 300);
        assert_eq!(new_state.get_nonce("alice"), 2);
    }
}
