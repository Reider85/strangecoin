//! Component tests for `block_executor`: a matrix of valid and invalid blocks
//! applied to a parent state (S1-P12, D01 scenarios at component level).

use strangecoin::blockchain::block_executor::{now_secs, validate_and_apply, BlockView};
use strangecoin::consensus::CURRENT_CONSENSUS_VERSION;
use strangecoin::error::StrangecoinError;
use strangecoin::serialize::{block_hash, compute_tx_root};
use strangecoin::test_support::{generate_keypair, sign_transaction};
use strangecoin::{Block, Transaction};
use strangecoin_core::consensus::{MAX_FUTURE_TIME, RETARGET_INTERVAL};
use strangecoin_core::state::{apply_block, compute_state_root, State};

const TARGET_MAX: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const TARGET_EASY: &str = "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const ZERO_TARGET: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn coinbase(receiver: &str, amount: u64) -> Transaction {
    Transaction {
        sender: "coinbase".to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce: 0,
        chain_id: strangecoin::consensus::current_chain_id(),
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
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: false,
    }
}

fn rehash(mut block: Block) -> Block {
    block.hash = hex::encode(block_hash(&block));
    block
}

fn seal(mut block: Block) -> Block {
    block.tx_root = compute_tx_root(&block.transactions);
    rehash(block)
}

fn genesis_with(target: &str, funded: &str) -> Block {
    seal(Block {
        index: 0,
        timestamp: 0,
        transactions: vec![coinbase(funded, 1000)],
        previous_hash: "0".repeat(64),
        hash: String::new(),
        nonce: 0,
        target: target.to_string(),
        consensus_version: CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    })
}

fn child_of(parent: &Block, transactions: Vec<Transaction>, timestamp: u64) -> Block {
    seal(Block {
        index: parent.index + 1,
        timestamp,
        transactions,
        previous_hash: parent.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: parent.target.clone(),
        consensus_version: CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    })
}

/// Standard fixture: genesis granting `funded` 1000, plus the state after it.
fn fixture_for(funded: &str) -> (Block, State) {
    let genesis = genesis_with(TARGET_MAX, funded);
    let state = validate_and_apply(&State::new(), &genesis, &BlockView::new(&[], now_secs(), false))
        .expect("genesis must apply");
    (genesis, state)
}

/// Same, with a well-known literal account that no test signs for.
fn fixture() -> (Block, State) {
    fixture_for("alice")
}

/// Apply `block` on top of a caller-supplied genesis block.
fn apply_block_on(genesis: &Block, block: &Block, allow_grant_blocks: bool) -> Result<State, StrangecoinError> {
    let parent_state =
        validate_and_apply(&State::new(), genesis, &BlockView::new(&[], now_secs(), allow_grant_blocks))
            .expect("genesis must apply");
    let view = BlockView::new(std::slice::from_ref(genesis), now_secs(), allow_grant_blocks);
    validate_and_apply(&parent_state, block, &view)
}

#[test]
fn valid_block_is_applied_to_parent_state() {
    let (alice, alice_sk) = generate_keypair();
    let (receiver, _) = generate_keypair();
    let (genesis, parent_state) = fixture_for(&alice);
    let mut tx = transfer(&alice, &receiver, 100, 1);
    sign_transaction(&mut tx, &alice_sk);

    let block = child_of(&genesis, vec![coinbase("miner", 0), tx], 600);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let state = validate_and_apply(&parent_state, &block, &view).expect("valid block must apply");

    assert_eq!(state.get_balance(&alice), 900);
    assert_eq!(state.get_balance(&receiver), 100);
    assert_eq!(state.get_nonce(&alice), 1);
}

#[test]
fn genesis_is_applied_to_an_empty_chain() {
    let genesis = genesis_with(TARGET_MAX, "alice");
    let view = BlockView::new(&[], now_secs(), false);
    let state = validate_and_apply(&State::new(), &genesis, &view).expect("genesis applies");
    assert_eq!(state.get_balance("alice"), 1000);
}

#[test]
fn rejects_genesis_on_a_non_empty_chain() {
    let (genesis, _) = fixture();
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&State::new(), &genesis_with(TARGET_MAX, "alice"), &view)
        .expect_err("genesis cannot follow a block");
    assert!(matches!(err, StrangecoinError::InvalidBlock(_)), "{err:?}");
}

#[test]
fn rejects_index_that_does_not_follow_the_parent() {
    let (genesis, parent_state) = fixture();
    let mut block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    block.index = 7;
    let block = rehash(block);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("height 7 cannot follow height 0");
    assert!(matches!(err, StrangecoinError::InvalidBlock(_)), "{err:?}");
}

#[test]
fn rejects_broken_previous_hash() {
    let (genesis, parent_state) = fixture();
    let mut block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    block.previous_hash = "11".repeat(32);
    let block = rehash(block);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("previous_hash must match the parent");
    assert!(matches!(err, StrangecoinError::InvalidBlock(_)), "{err:?}");
}

#[test]
fn rejects_tampered_header_hash() {
    let (genesis, parent_state) = fixture();
    let mut block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    block.hash = "00".repeat(32);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("stored hash must match the recomputed one");
    assert!(matches!(err, StrangecoinError::InvalidBlock(_)), "{err:?}");
}

#[test]
fn rejects_stale_consensus_version() {
    let (genesis, parent_state) = fixture();
    let mut block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    block.consensus_version = CURRENT_CONSENSUS_VERSION + 1;
    let block = rehash(block);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("consensus rules are height-locked");
    assert!(matches!(err, StrangecoinError::InvalidBlock(_)), "{err:?}");
}

#[test]
fn rejects_wrong_tx_root() {
    let (genesis, parent_state) = fixture();
    let mut block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    block.tx_root = [0xff; 32];
    let block = rehash(block);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    assert!(
        validate_and_apply(&parent_state, &block, &view).is_err(),
        "tx_root must commit to the transaction set"
    );
}

#[test]
fn rejects_timestamp_not_after_median_time_past() {
    let (genesis, parent_state) = fixture();
    // Median time past of the prefix is 0, so the child must be strictly newer.
    let block = child_of(&genesis, vec![coinbase("miner", 0)], 0);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("timestamp must be > median time past");
    assert!(matches!(err, StrangecoinError::TimestampTooOld), "{err:?}");
}

#[test]
fn rejects_timestamp_too_far_in_the_future() {
    let (genesis, parent_state) = fixture();
    let block = child_of(
        &genesis,
        vec![coinbase("miner", 0)],
        now_secs() + MAX_FUTURE_TIME + 60,
    );
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("timestamp must be <= now + MAX_FUTURE_TIME");
    assert!(matches!(err, StrangecoinError::TimestampInFuture), "{err:?}");
}

#[test]
fn rejects_proof_of_work_above_target() {
    let genesis = genesis_with(ZERO_TARGET, "alice");
    let block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    let err = apply_block_on(&genesis, &block, false)
        .expect_err("a hash above the target must not validate");
    assert!(matches!(err, StrangecoinError::InvalidDifficulty), "{err:?}");
}

#[test]
fn rejects_target_changed_outside_a_retarget_height() {
    // A parent target the miner could actually reach, so the child's proof of
    // work passes and the target comparison is what rejects the block.
    let genesis = genesis_with(TARGET_EASY, "alice");
    let parent_state =
        validate_and_apply(&State::new(), &genesis, &BlockView::new(&[], now_secs(), false))
            .expect("genesis must apply");

    let mut block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    block.target = TARGET_MAX.to_string();
    let block = rehash(block);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("target only changes at a retarget height");
    assert!(matches!(err, StrangecoinError::InvalidBlock(_)), "{err:?}");
}

#[test]
fn accepts_the_target_computed_at_a_retarget_height() {
    let (genesis, parent_state) = fixture();
    // Height 1 is not a retarget height (RETARGET_INTERVAL is far away), so the
    // child must inherit the parent's target byte for byte.
    let block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    assert_eq!(block.target, genesis.target);
    assert_eq!(1 % RETARGET_INTERVAL, 1);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    validate_and_apply(&parent_state, &block, &view).expect("inherited target is valid");
}

#[test]
fn rejects_unsigned_transfer() {
    let (genesis, parent_state) = fixture();
    let (receiver, _secret_key) = generate_keypair();
    let block = child_of(
        &genesis,
        vec![coinbase("miner", 0), transfer("alice", &receiver, 100, 1)],
        600,
    );
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("a transfer without a valid signature must be rejected");
    assert!(matches!(err, StrangecoinError::InvalidSignature), "{err:?}");
}

#[test]
fn rejects_transfer_without_funds() {
    let (alice, alice_sk) = generate_keypair();
    let (receiver, _) = generate_keypair();
    let (genesis, parent_state) = fixture_for(&alice);
    let mut tx = transfer(&alice, &receiver, 5000, 1);
    sign_transaction(&mut tx, &alice_sk);
    let block = child_of(&genesis, vec![coinbase("miner", 0), tx], 600);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("spending more than the balance must be rejected");
    assert!(matches!(err, StrangecoinError::InsufficientBalance { .. }), "{err:?}");
}

#[test]
fn rejects_coinbase_above_the_block_reward() {
    let (genesis, parent_state) = fixture();
    // Regtest emission is zero, so any non-zero coinbase is inflation.
    let block = child_of(&genesis, vec![coinbase("miner", 1)], 600);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("coinbase must not exceed the emission schedule");
    assert!(matches!(err, StrangecoinError::InvalidCoinbaseAmount { .. }), "{err:?}");
}

#[test]
fn rejects_block_without_a_coinbase() {
    let (alice, alice_sk) = generate_keypair();
    let (receiver, _) = generate_keypair();
    let (genesis, parent_state) = fixture_for(&alice);
    let mut tx = transfer(&alice, &receiver, 100, 1);
    sign_transaction(&mut tx, &alice_sk);
    let block = child_of(&genesis, vec![tx], 600);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("every non-genesis block needs a coinbase");
    assert!(matches!(err, StrangecoinError::InvalidCoinbaseAmount { .. }), "{err:?}");
}

#[test]
fn rejects_state_root_that_does_not_match_the_applied_state() {
    let (genesis, parent_state) = fixture();
    let mut block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    block.state_root = [0xff; 32];
    let block = rehash(block);
    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    let err = validate_and_apply(&parent_state, &block, &view)
        .expect_err("a wrong state_root must be rejected");
    assert!(matches!(err, StrangecoinError::InvalidBlock(_)), "{err:?}");
}

#[test]
fn accepts_state_root_matching_the_applied_state() {
    let (genesis, parent_state) = fixture();
    let block = child_of(&genesis, vec![coinbase("miner", 0)], 600);
    let post = apply_block(&parent_state, &block).expect("block applies");
    let root = compute_state_root(&post.balances);

    let mut committed = block;
    committed.state_root = root;
    let committed = rehash(committed);

    let view = BlockView::new(std::slice::from_ref(&genesis), now_secs(), false);
    validate_and_apply(&parent_state, &committed, &view)
        .expect("a matching state_root must be accepted");
}

#[test]
fn grant_block_needs_the_opt_in_flag() {
    let genesis = seal(Block {
        index: 0,
        timestamp: 0,
        transactions: vec![coinbase("initial_wallet_address", 10_000)],
        previous_hash: "0".repeat(64),
        hash: String::new(),
        nonce: 0,
        target: TARGET_MAX.to_string(),
        consensus_version: CURRENT_CONSENSUS_VERSION,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    });
    // The transfer away from the genesis address cannot be signed: that address
    // has no key. It is only valid while allow_grant_blocks is on, at height 1.
    let grant = child_of(
        &genesis,
        vec![
            coinbase("grant_wallet", 0),
            transfer("initial_wallet_address", "grant_wallet", 10_000, 0),
        ],
        600,
    );

    let err = apply_block_on(&genesis, &grant, false)
        .expect_err("grant block must be rejected while the flag is off");
    assert!(matches!(err, StrangecoinError::InvalidSignature), "{err:?}");

    let state = apply_block_on(&genesis, &grant, true).expect("opted-in grant block applies");
    assert_eq!(state.get_balance("grant_wallet"), 10_000);
    assert_eq!(state.get_balance("initial_wallet_address"), 0);
}
