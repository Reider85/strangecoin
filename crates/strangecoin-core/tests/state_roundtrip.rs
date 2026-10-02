use strangecoin_core::state::{apply_block, unapply_block, State};
use strangecoin_core::types::{Block, Transaction};

use proptest::prelude::*;

fn arbitrary_address() -> impl Strategy<Value = String> {
    "[a-z]{3,8}".prop_map(|s| format!("addr_{}", s))
}

fn make_state(balances: Vec<(String, u64)>) -> State {
    let mut state = State::new();
    for (addr, bal) in balances {
        state.set_balance(&addr, bal);
    }
    state
}

fn genesis_block(txs: Vec<Transaction>) -> Block {
    Block {
        index: 0,
        timestamp: 0,
        transactions: txs,
        previous_hash: String::new(),
        hash: "genesis_hash".to_string(),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn transfer_block(index: u64, txs: Vec<Transaction>) -> Block {
    let mut all_txs = vec![coinbase("miner", 0)];
    all_txs.extend(txs);
    Block {
        index,
        timestamp: index * 600,
        transactions: all_txs,
        previous_hash: format!("prev_{}", index - 1),
        hash: format!("hash_{}", index),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn coinbase(receiver: &str, amount: u64) -> Transaction {
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

fn transfer(sender: &str, receiver: &str, amount: u64, nonce: u64) -> Transaction {
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

proptest! {
    #[test]
    fn apply_unapply_roundtrip_single_tx(
        sender_bal in 1000u64..1_000_000u64,
        amount in 1u64..500u64,
    ) {
        let state = make_state(vec![
            ("alice".to_string(), sender_bal),
            ("bob".to_string(), 0),
        ]);
        state.clone().set_nonce("alice", 0);

        let genesis = genesis_block(vec![coinbase("alice", sender_bal)]);
        let after_genesis = apply_block(&state, &genesis).unwrap();

        let txs = vec![transfer("alice", "bob", amount, 1)];
        let block = transfer_block(1, txs);
        let after_block = apply_block(&after_genesis, &block).unwrap();

        let restored = unapply_block(&after_block, &block).unwrap();
        prop_assert_eq!(restored, after_genesis);
    }

    #[test]
    fn apply_unapply_roundtrip_multiple_blocks(
        bal1 in 5000u64..100_000u64,
        bal2 in 5000u64..100_000u64,
        amt1 in 100u64..2000u64,
        amt2 in 100u64..2000u64,
        amt3 in 100u64..2000u64,
    ) {
        let mut state = State::new();
        state.set_balance("alice", bal1);
        state.set_balance("bob", bal2);
        state.set_nonce("alice", 0);
        state.set_nonce("bob", 0);

        let genesis = genesis_block(vec![
            coinbase("alice", bal1),
            coinbase("bob", bal2),
        ]);
        let s0 = apply_block(&state, &genesis).unwrap();

        let txs1 = vec![transfer("alice", "bob", amt1, 1)];
        let b1 = transfer_block(1, txs1);
        let s1 = apply_block(&s0, &b1).unwrap();

        let txs2 = vec![transfer("bob", "alice", amt2, 1)];
        let b2 = transfer_block(2, txs2);
        let s2 = apply_block(&s1, &b2).unwrap();

        let txs3 = vec![transfer("alice", "bob", amt3.min(s2.get_balance("alice")), 2)];
        let b3 = transfer_block(3, txs3);

        if let Ok(s3) = apply_block(&s2, &b3) {
            let r2 = unapply_block(&s3, &b3).unwrap();
            prop_assert_eq!(&r2, &s2);
            let r1 = unapply_block(&r2, &b2).unwrap();
            prop_assert_eq!(&r1, &s1);
            let r0 = unapply_block(&r1, &b1).unwrap();
            prop_assert_eq!(&r0, &s0);
        }
    }

    #[test]
    fn apply_preserves_total_supply(
        sender_bal in 1000u64..1_000_000u64,
        amount in 1u64..500u64,
    ) {
        let state = make_state(vec![("alice".to_string(), sender_bal)]);
        let genesis = genesis_block(vec![coinbase("alice", sender_bal)]);
        let after_genesis = apply_block(&state, &genesis).unwrap();
        let supply_before = after_genesis.total_supply();

        let txs = vec![transfer("alice", "bob", amount, 1)];
        let block = transfer_block(1, txs);
        let after_block = apply_block(&after_genesis, &block).unwrap();

        prop_assert_eq!(after_block.total_supply(), supply_before);
    }

    #[test]
    fn unapply_restores_state_exactly(
        sender_bal in 1000u64..1_000_000u64,
        amount in 1u64..500u64,
    ) {
        let mut state = State::new();
        state.set_balance("alice", sender_bal);
        state.set_nonce("alice", 0);

        let genesis = genesis_block(vec![coinbase("alice", sender_bal)]);
        let after_genesis = apply_block(&state, &genesis).unwrap();

        let txs = vec![transfer("alice", "bob", amount, 1)];
        let block = transfer_block(1, txs);
        let after_block = apply_block(&after_genesis, &block).unwrap();

        let restored = unapply_block(&after_block, &block).unwrap();
        prop_assert_eq!(restored.get_balance("alice"), after_genesis.get_balance("alice"));
        prop_assert_eq!(restored.get_balance("bob"), after_genesis.get_balance("bob"));
        prop_assert_eq!(restored.get_nonce("alice"), after_genesis.get_nonce("alice"));
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn genesis_creates_value() {
        let state = State::new();
        let block = genesis_block(vec![coinbase("miner", 100_000)]);
        let new_state = apply_block(&state, &block).unwrap();
        assert_eq!(new_state.get_balance("miner"), 100_000);
        assert_eq!(new_state.total_supply(), 100_000);
    }

    #[test]
    fn unapply_genesis_removes_value() {
        let state = State::new();
        let block = genesis_block(vec![coinbase("miner", 100_000)]);
        let applied = apply_block(&state, &block).unwrap();
        let restored = unapply_block(&applied, &block).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn chain_of_blocks_roundtrip() {
        let state = State::new();

        let genesis = genesis_block(vec![
            coinbase("alice", 10_000),
            coinbase("bob", 5_000),
        ]);
        let s0 = apply_block(&state, &genesis).unwrap();

        let b1 = transfer_block(1, vec![transfer("alice", "bob", 1000, 1)]);
        let s1 = apply_block(&s0, &b1).unwrap();

        let b2 = transfer_block(2, vec![transfer("bob", "alice", 500, 1)]);
        let s2 = apply_block(&s1, &b2).unwrap();

        let b3 = transfer_block(3, vec![transfer("alice", "bob", 2000, 2)]);
        let s3 = apply_block(&s2, &b3).unwrap();

        assert_eq!(s3.get_balance("alice"), 7500);
        assert_eq!(s3.get_balance("bob"), 7500);
        assert_eq!(s3.total_supply(), 15_000);

        let r3 = unapply_block(&s3, &b3).unwrap();
        assert_eq!(r3, s2);
        let r2 = unapply_block(&r3, &b2).unwrap();
        assert_eq!(r2, s1);
        let r1 = unapply_block(&r2, &b1).unwrap();
        assert_eq!(r1, s0);
        let r0 = unapply_block(&r1, &genesis).unwrap();
        assert_eq!(r0, state);
    }

    #[test]
    fn nonce_tracking() {
        let mut state = State::new();
        state.set_balance("alice", 10_000);
        state.set_nonce("alice", 0);

        let genesis = genesis_block(vec![coinbase("alice", 10_000)]);
        let s0 = apply_block(&state, &genesis).unwrap();

        let b1 = transfer_block(1, vec![transfer("alice", "bob", 100, 1)]);
        let s1 = apply_block(&s0, &b1).unwrap();
        assert_eq!(s1.get_nonce("alice"), 1);

        let b2 = transfer_block(2, vec![transfer("alice", "bob", 100, 2)]);
        let s2 = apply_block(&s1, &b2).unwrap();
        assert_eq!(s2.get_nonce("alice"), 2);

        let r2 = unapply_block(&s2, &b2).unwrap();
        assert_eq!(r2.get_nonce("alice"), 1);
        let r1 = unapply_block(&r2, &b1).unwrap();
        assert_eq!(r1.get_nonce("alice"), 0);
    }

    #[test]
    fn insufficient_balance_rejected() {
        let state = make_state(vec![("alice".to_string(), 100)]);
        let block = transfer_block(1, vec![transfer("alice", "bob", 200, 1)]);
        assert!(apply_block(&state, &block).is_err());
    }

    #[test]
    fn zero_amount_transfer() {
        let state = State::new();

        let genesis = genesis_block(vec![coinbase("alice", 1000)]);
        let s0 = apply_block(&state, &genesis).unwrap();

        let block = transfer_block(1, vec![transfer("alice", "bob", 0, 1)]);
        let s1 = apply_block(&s0, &block).unwrap();

        assert_eq!(s1.get_balance("alice"), 1000);
        assert_eq!(s1.get_balance("bob"), 0);
    }

    #[test]
    fn self_transfer() {
        let state = State::new();

        let genesis = genesis_block(vec![coinbase("alice", 1000)]);
        let s0 = apply_block(&state, &genesis).unwrap();

        let block = transfer_block(1, vec![transfer("alice", "alice", 500, 1)]);
        let s1 = apply_block(&s0, &block).unwrap();

        assert_eq!(s1.get_balance("alice"), 1000);
    }
}
