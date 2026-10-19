// Binaries that reach into `common` only for `vm_helpers` leave the glob
// unused; the allow keeps `clippy -D warnings` green (BUG-S1-012 pattern).
#[allow(unused_imports)]
pub use strangecoin::test_support::*;

// S1.5-P17 VM integration helpers: compiled into every test binary that
// declares `mod common;`, but only the `vm_*` binaries reach them — the
// allow mirrors the existing walk_state/craft_child pattern (BUG-S1-012).
#[allow(dead_code)]
#[cfg(feature = "vm-wasm")]
pub mod vm_helpers;

/// Replay `prefix` from an empty state with the pure core executor — the same
/// reconstruction `StateCache::rebuild_from_chain` performs on adoption.
// Called by craft_child below; the allow covers the test binaries that
// compile `mod common;` but never reach it (P21 / BUG-S1-012).
#[allow(dead_code)]
pub fn walk_state(prefix: &[strangecoin::Block]) -> strangecoin_core::state::State {
    let mut state = strangecoin_core::state::State::new();
    for block in prefix {
        state = strangecoin_core::state::apply_block(
            &state,
            block,
            strangecoin::consensus::CHAIN_ID_REGTEST,
        )
        .expect("prefix chain must apply");
    }
    state
}

/// A zero-amount coinbase — every non-genesis block needs one.
// Used by tests/events.rs and tests/state_root.rs; the allow covers the
// remaining test binaries that compile `mod common;` (P21 / BUG-S1-012).
#[allow(dead_code)]
pub fn coinbase_tx(receiver: &str, amount: u64) -> strangecoin::Transaction {
    strangecoin::Transaction::coinbase(
        "coinbase",
        receiver,
        amount,
        strangecoin::consensus::CHAIN_ID_REGTEST,
    )
}

/// Craft the valid child of `prefix.last()` whose header commits to the real
/// post-state root: canonical `tx_root` first, then `state_root` over the
/// applied state, then the header hash (S1-P19; the genesis target is maximal
/// and inherited until the first retarget, so the proof of work passes as-is).
// Used by tests/events.rs and tests/state_root.rs; the allow covers the
// remaining test binaries that compile `mod common;` (P21 / BUG-S1-012).
#[allow(dead_code)]
pub fn craft_child(
    prefix: &[strangecoin::Block],
    transactions: Vec<strangecoin::Transaction>,
) -> strangecoin::Block {
    let parent = prefix.last().expect("prefix must not be empty");
    let height = parent.index + 1;
    let mut block = strangecoin::Block {
        index: height,
        timestamp: std::cmp::max(
            strangecoin::blockchain::block_executor::now_secs(),
            parent.timestamp + 1,
        ),
        transactions,
        previous_hash: parent.hash.clone(),
        hash: String::new(),
        nonce: 0,
        target: parent.target.clone(),
        consensus_version: strangecoin::blockchain::ConsensusManager::new()
            .expected_version(height),
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
    let post = strangecoin_core::state::apply_block(
        &walk_state(prefix),
        &block,
        strangecoin::consensus::CHAIN_ID_REGTEST,
    )
    .expect("crafted block must apply to the prefix state");
    block.state_root = strangecoin_core::state::compute_state_root(&post.balances);
    // S1.5-FBP07: crafted blocks commit to the receipts SMT root like mined
    // ones — validate_and_apply rejects a zero receipts_root past genesis.
    block.receipts_root = strangecoin_core::state::receipts_root(&post.receipts);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    block
}
