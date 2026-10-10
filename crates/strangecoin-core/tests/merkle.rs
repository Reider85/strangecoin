use proptest::prelude::*;
use strangecoin_core::serialize::{compute_tx_root, merkle_root, txid};
use strangecoin_core::types::Transaction;

fn make_tx(sender: &str, receiver: &str, amount: u64, nonce: u64) -> Transaction {
    Transaction {
        sender: sender.to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce,
        chain_id: 3,
        signature: vec![],
        is_coinbase: false,
    }
}

fn make_coinbase(receiver: &str, amount: u64) -> Transaction {
    Transaction {
        sender: "coinbase".to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce: 0,
        chain_id: 3,
        signature: vec![],
        is_coinbase: true,
    }
}

#[test]
fn empty_txids_returns_zero() {
    let root = merkle_root(&[]);
    assert_eq!(root, [0u8; 32]);
}

#[test]
fn single_txid_duplicated() {
    let tx = make_tx("alice", "bob", 100, 1);
    let id = txid(&tx);
    let root = merkle_root(&[id]);
    let expected = *blake3::hash(&{
        let mut combined = Vec::new();
        combined.extend_from_slice(&id);
        combined.extend_from_slice(&id);
        combined
    })
    .as_bytes();
    assert_eq!(root, expected);
}

#[test]
fn two_txids_pair_hash() {
    let tx1 = make_tx("alice", "bob", 100, 1);
    let tx2 = make_tx("bob", "charlie", 200, 2);
    let id1 = txid(&tx1);
    let id2 = txid(&tx2);
    let root = merkle_root(&[id1, id2]);
    let expected = *blake3::hash(&{
        let mut combined = Vec::new();
        combined.extend_from_slice(&id1);
        combined.extend_from_slice(&id2);
        combined
    })
    .as_bytes();
    assert_eq!(root, expected);
}

#[test]
fn three_txids_odd_duplication() {
    let tx1 = make_tx("alice", "bob", 100, 1);
    let tx2 = make_tx("bob", "charlie", 200, 2);
    let tx3 = make_tx("charlie", "dave", 300, 3);
    let id1 = txid(&tx1);
    let id2 = txid(&tx2);
    let id3 = txid(&tx3);

    let root = merkle_root(&[id1, id2, id3]);

    let left = *blake3::hash(&{
        let mut c = Vec::new();
        c.extend_from_slice(&id1);
        c.extend_from_slice(&id2);
        c
    })
    .as_bytes();
    let right = *blake3::hash(&{
        let mut c = Vec::new();
        c.extend_from_slice(&id3);
        c.extend_from_slice(&id3);
        c
    })
    .as_bytes();
    let expected = *blake3::hash(&{
        let mut c = Vec::new();
        c.extend_from_slice(&left);
        c.extend_from_slice(&right);
        c
    })
    .as_bytes();
    assert_eq!(root, expected);
}

#[test]
fn seven_txids_multi_level() {
    let txs: Vec<Transaction> = (0..7)
        .map(|i| {
            make_tx(
                "sender",
                &format!("recv_{}", i),
                (i + 1) * 100,
                i + 1,
            )
        })
        .collect();
    let ids: Vec<[u8; 32]> = txs.iter().map(txid).collect();

    let root = merkle_root(&ids);

    assert_ne!(root, [0u8; 32]);
    assert_ne!(root, ids[0]);
    assert_ne!(root, ids[6]);
}

#[test]
fn deterministic_same_input() {
    let txs = [
        make_tx("a", "b", 10, 1),
        make_tx("c", "d", 20, 2),
        make_tx("e", "f", 30, 3),
    ];
    let ids: Vec<[u8; 32]> = txs.iter().map(txid).collect();

    let root1 = merkle_root(&ids);
    let root2 = merkle_root(&ids);
    assert_eq!(root1, root2);
}

#[test]
fn different_inputs_different_roots() {
    let txs1 = [make_tx("a", "b", 10, 1), make_tx("c", "d", 20, 2)];
    let txs2 = [make_tx("a", "b", 10, 1), make_tx("c", "d", 20, 3)];
    let ids1: Vec<[u8; 32]> = txs1.iter().map(txid).collect();
    let ids2: Vec<[u8; 32]> = txs2.iter().map(txid).collect();

    assert_ne!(merkle_root(&ids1), merkle_root(&ids2));
}

#[test]
fn compute_tx_root_matches_manual() {
    let txs = vec![
        make_coinbase("miner", 5000),
        make_tx("alice", "bob", 100, 1),
    ];
    let ids: Vec<[u8; 32]> = txs.iter().map(txid).collect();

    let from_compute = compute_tx_root(&txs);
    let from_manual = merkle_root(&ids);
    assert_eq!(from_compute, from_manual);
}

#[test]
fn compute_tx_root_empty() {
    let txs: Vec<Transaction> = vec![];
    assert_eq!(compute_tx_root(&txs), [0u8; 32]);
}

proptest! {
    #[test]
    fn proptest_deterministic_root(txs in proptest::collection::vec(
        proptest::arbitrary::any::<(String, String, u64, u64)>(), 0..20
    )) {
        let txs: Vec<Transaction> = txs.into_iter().map(|(s, r, a, n)| {
            Transaction {
                sender: s,
                receiver: r,
                amount: a,
                nonce: n,
                chain_id: 3,
                signature: vec![],
                is_coinbase: false,
            }
        }).collect();
        let ids: Vec<[u8; 32]> = txs.iter().map(txid).collect();
        let root1 = merkle_root(&ids);
        let root2 = merkle_root(&ids);
        prop_assert_eq!(root1, root2);
    }

    #[test]
    fn proptest_root_non_zero_for_non_empty(txs in proptest::collection::vec(
        proptest::arbitrary::any::<(String, String, u64, u64)>(), 1..20
    )) {
        let txs: Vec<Transaction> = txs.into_iter().map(|(s, r, a, n)| {
            Transaction {
                sender: s,
                receiver: r,
                amount: a,
                nonce: n,
                chain_id: 3,
                signature: vec![],
                is_coinbase: false,
            }
        }).collect();
        let ids: Vec<[u8; 32]> = txs.iter().map(txid).collect();
        let root = merkle_root(&ids);
        prop_assert_ne!(root, [0u8; 32]);
    }
}
