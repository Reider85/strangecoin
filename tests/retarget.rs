//! BUG-S1-011 — retarget at the real interval height (2016).
//!
//! `compute_target` implements the sliding-window retarget, but the existing
//! component test only exercises height 1 (inherited target) and the proptest
//! clamp test recomputes the formula without calling `compute_target`. These
//! tests build a real `RETARGET_INTERVAL`-long chain and compare the result
//! against literal hex expectations (no formula duplication).

use strangecoin::consensus::{
    compute_target, MAX_TARGET_CHANGE_FACTOR, RETARGET_INTERVAL, TARGET_BLOCK_TIME,
};
use strangecoin::Block;

/// Powers of two chosen so every ratio lands on an exact integer target:
/// `prev * actual / expected` never loses bits to truncation, and
/// `prev * 4` never overflows u256 — the expected values below are literals.
const TARGET_2_200: &str = "0000000000000100000000000000000000000000000000000000000000000000";
const TARGET_2_201: &str = "0000000000000200000000000000000000000000000000000000000000000000";
const TARGET_2_202: &str = "0000000000000400000000000000000000000000000000000000000000000000";
const TARGET_2_199: &str = "0000000000000080000000000000000000000000000000000000000000000000";
const TARGET_2_198: &str = "0000000000000040000000000000000000000000000000000000000000000000";

/// A chain of `count` blocks with evenly spaced timestamps
/// `[spacing, 2*spacing, .., count*spacing]`, all carrying `target`.
/// `compute_target` reads only `timestamp` and `target`, so hashes, PoW and
/// bodies stay empty — the retarget math is the unit under test.
fn chain_with_spacing(count: usize, spacing: u64, target: &str) -> Vec<Block> {
    (0..count)
        .map(|i| Block {
            index: i as u64,
            timestamp: (i as u64 + 1) * spacing,
            transactions: Vec::new(),
            previous_hash: String::new(),
            hash: String::new(),
            nonce: 0,
            target: target.to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        })
        .collect()
}

#[test]
fn compute_target_at_height_2016() {
    assert_eq!(RETARGET_INTERVAL, 2016);
    assert_eq!(TARGET_BLOCK_TIME, 600);
    assert_eq!(MAX_TARGET_CHANGE_FACTOR, 4);

    // 2016 blocks at exactly TARGET_BLOCK_TIME: actual_time == expected_time,
    // so the retargeted target must equal the previous one, byte for byte.
    let chain = chain_with_spacing(RETARGET_INTERVAL as usize, 600, TARGET_2_200);
    let target = compute_target(&chain);
    assert_eq!(hex::encode(target), TARGET_2_200);
}

#[test]
fn retarget_doubles_target_when_blocks_twice_as_slow() {
    // actual = 2015 * 1200 = 2 * expected → new = prev * 2 (inside the clamp).
    let chain = chain_with_spacing(RETARGET_INTERVAL as usize, 1200, TARGET_2_200);
    let target = compute_target(&chain);
    assert_eq!(hex::encode(target), TARGET_2_201);
}

#[test]
fn retarget_halves_target_when_blocks_twice_as_fast() {
    // actual = 2015 * 300 = expected / 2 → new = prev / 2 (inside the clamp).
    let chain = chain_with_spacing(RETARGET_INTERVAL as usize, 300, TARGET_2_200);
    let target = compute_target(&chain);
    assert_eq!(hex::encode(target), TARGET_2_199);
}

#[test]
fn retarget_clamps_at_max_factor() {
    // actual = 2015 * 6000 = 10 * expected → raw = prev * 10, clamped to
    // prev * MAX_TARGET_CHANGE_FACTOR.
    let chain = chain_with_spacing(RETARGET_INTERVAL as usize, 6000, TARGET_2_200);
    let target = compute_target(&chain);
    assert_eq!(hex::encode(target), TARGET_2_202);
}

#[test]
fn retarget_clamps_at_min_factor() {
    // actual = 2015 * 75 = expected / 8 → raw = prev / 8, clamped to
    // prev / MAX_TARGET_CHANGE_FACTOR.
    let chain = chain_with_spacing(RETARGET_INTERVAL as usize, 75, TARGET_2_200);
    let target = compute_target(&chain);
    assert_eq!(hex::encode(target), TARGET_2_198);
}

#[test]
fn no_retarget_below_interval() {
    // One block short of a full window: the target is inherited unchanged,
    // no matter how far off the block times are.
    let chain = chain_with_spacing(RETARGET_INTERVAL as usize - 1, 75, TARGET_2_200);
    let target = compute_target(&chain);
    assert_eq!(hex::encode(target), TARGET_2_200);
}
