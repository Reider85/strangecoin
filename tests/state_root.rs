//! S1-P19: the state-root line end-to-end — a chain whose headers commit to
//! real state roots passes full validation on a second node, while a tampered
//! root does not (invariant #19 through `adopt_candidate` → `rebuild_from_chain`
//! → `block_executor`; core-level coverage lives in `strangecoin-core`).

mod common;

use common::*;
use strangecoin::Transaction;

/// Grant on a source node, then craft a child that commits to its real state
/// root. Returns (chain prefix, crafted child, alice, bob).
fn committed_chain(tag: &str) -> (Vec<strangecoin::Block>, strangecoin::Block, String, String) {
    let dir = TestDir::new(tag);
    let bc = create_test_blockchain(dir.path());
    let keypairs = generate_keypairs(2);
    let alice = keypairs[0].0.clone();
    let bob = keypairs[1].0.clone();
    assert!(
        bc.grant_initial_balance_to_first_wallet(&alice).unwrap(),
        "grant block must be created"
    );

    let prefix = bc.chain_snapshot();
    let mut transfer = Transaction {
        sender: alice.clone(),
        receiver: bob.clone(),
        amount: 500,
        nonce: 1,
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: false,
    };
    sign_transaction(&mut transfer, &keypairs[0].1);
    let child = craft_child(&prefix, vec![coinbase_tx("miner", 0), transfer]);
    assert_ne!(
        child.state_root,
        [0u8; 32],
        "crafted block must commit to its state root"
    );
    (prefix, child, alice, bob)
}

#[test]
fn committed_state_root_chain_passes_on_a_second_node() {
    let dir_b = TestDir::new("sr_second");
    let bc_b = create_test_blockchain(dir_b.path());

    let (prefix, child, alice, bob) = committed_chain("sr_source");

    let mut candidate = prefix;
    candidate.push(child);
    let adopted = bc_b
        .adopt_candidate(candidate, None, Vec::new(), 0)
        .expect("chain with committed state roots must pass full validation");
    assert!(adopted, "second node must adopt the committed-root chain");
    assert_eq!(bc_b.chain_len(), 3, "genesis + grant + crafted child");
    assert_eq!(
        bc_b.get_balance(&alice),
        9500,
        "the committed transfer must apply on the second node"
    );
    assert_eq!(bc_b.get_balance(&bob), 500);
    assert!(bc_b.validate_chain(), "adopted chain must validate");
}

#[test]
fn tampered_state_root_is_rejected_by_the_second_node() {
    let dir_b = TestDir::new("sr_reject");
    let bc_b = create_test_blockchain(dir_b.path());

    let (prefix, mut child, _, _) = committed_chain("sr_tamper");
    // Flip a byte of the commitment and re-seal: PoW (maximal regtest target)
    // and tx_root still hold, so the state root is the only thing wrong.
    child.state_root[0] ^= 0xff;
    child.hash = hex::encode(strangecoin::serialize::block_hash(&child));

    let mut candidate = prefix;
    candidate.push(child);
    let adopted = bc_b
        .adopt_candidate(candidate, None, Vec::new(), 0)
        .expect("rejection is a fork-choice/validation answer, not an error");
    assert!(
        !adopted,
        "a wrong state_root must fail full validation on the second node"
    );
    assert_eq!(
        bc_b.chain_len(),
        1,
        "rejected candidate must not touch the second node's chain"
    );
}

#[test]
fn zero_state_root_is_tolerated_as_no_commitment() {
    // Opt-in semantics (block_executor): zero means "does not commit", so a
    // chain crafted without roots still adopts — the committed-root tests
    // above are what pin invariant #19.
    let dir_b = TestDir::new("sr_zero");
    let bc_b = create_test_blockchain(dir_b.path());

    let dir_a = TestDir::new("sr_zero_src");
    let bc_a = create_test_blockchain(dir_a.path());
    let keypairs = generate_keypairs(2);
    let alice = keypairs[0].0.clone();
    assert!(bc_a
        .grant_initial_balance_to_first_wallet(&alice)
        .unwrap());

    let mut prefix = bc_a.chain_snapshot();
    let parent = prefix.last().expect("genesis exists").clone();
    let mut child = strangecoin::Block {
        index: parent.index + 1,
        timestamp: std::cmp::max(
            strangecoin::blockchain::block_executor::now_secs(),
            parent.timestamp + 1,
        ),
        transactions: vec![coinbase_tx("miner", 0)],
        previous_hash: parent.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: parent.target.clone(),
        consensus_version: strangecoin::blockchain::ConsensusManager::new()
            .expected_version(parent.index + 1),
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    };
    child.tx_root = strangecoin::serialize::compute_tx_root(&child.transactions);
    child.hash = hex::encode(strangecoin::serialize::block_hash(&child));
    prefix.push(child);

    assert!(
        bc_b
            .adopt_candidate(prefix, None, Vec::new(), 0)
            .expect("zero state_root means no commitment and must validate"),
        "chain without state commitments must still adopt"
    );
    assert_eq!(bc_b.chain_len(), 3);
}
