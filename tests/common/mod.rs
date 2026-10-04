use std::time::Duration;
use strangecoin::events::{EventBus, NodeEvent};
pub use strangecoin::test_support::*;

#[allow(dead_code)]
pub fn wait_for_event(
    bus: &EventBus,
    predicate: impl Fn(&NodeEvent) -> bool,
    timeout: Duration,
) -> Option<NodeEvent> {
    let rx = bus.subscribe();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match rx.recv_timeout(remaining) {
            Ok(event) => {
                if predicate(&event) {
                    return Some(event);
                }
            }
            Err(_) => return None,
        }
    }
}

#[allow(dead_code)]
pub fn wait_for_block_applied(bus: &EventBus, timeout: Duration) -> Option<NodeEvent> {
    wait_for_event(
        bus,
        |e| matches!(e, NodeEvent::BlockApplied { .. }),
        timeout,
    )
}

/// Replay `prefix` from an empty state with the pure core executor — the same
/// reconstruction `StateCache::rebuild_from_chain` performs on adoption.
#[allow(dead_code)]
pub fn walk_state(prefix: &[strangecoin::Block]) -> strangecoin_core::state::State {
    let mut state = strangecoin_core::state::State::new();
    for block in prefix {
        state = strangecoin_core::state::apply_block(&state, block)
            .expect("prefix chain must apply");
    }
    state
}

/// A zero-amount coinbase — every non-genesis block needs one.
#[allow(dead_code)]
pub fn coinbase_tx(receiver: &str, amount: u64) -> strangecoin::Transaction {
    strangecoin::Transaction {
        sender: "coinbase".to_string(),
        receiver: receiver.to_string(),
        amount,
        nonce: 0,
        chain_id: strangecoin::consensus::current_chain_id(),
        signature: Vec::new(),
        is_coinbase: true,
    }
}

/// Craft the valid child of `prefix.last()` whose header commits to the real
/// post-state root: canonical `tx_root` first, then `state_root` over the
/// applied state, then the header hash (S1-P19; the genesis target is maximal
/// and inherited until the first retarget, so the proof of work passes as-is).
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
    };
    block.tx_root = strangecoin::serialize::compute_tx_root(&block.transactions);
    let post = strangecoin_core::state::apply_block(&walk_state(prefix), &block)
        .expect("crafted block must apply to the prefix state");
    block.state_root = strangecoin_core::state::compute_state_root(&post.balances);
    block.hash = hex::encode(strangecoin::serialize::block_hash(&block));
    block
}
