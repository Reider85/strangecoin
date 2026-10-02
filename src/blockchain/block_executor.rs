//! # Block executor (ARCHITECT3 §3.4, component 2)
//!
//! «Validate + apply» for a single block: everything that can be decided from
//! the block itself, its parent state and the chain prefix behind it. The
//! executor is pure — it never selects a tip and never writes to storage;
//! both belong to the caller.
//!
//! Checks collected here (they used to live inline in `Blockchain::validate_chain`):
//!
//! 1. position in the chain (`index`, `previous_hash`);
//! 2. header hash (`block.hash` over the serialized header);
//! 3. `consensus_version`;
//! 4. `tx_root` (merkle commitment to the transaction set);
//! 5. timestamp (MTP + future bound);
//! 6. proof of work and the retarget schedule for `block.target`;
//! 7. transaction signatures (with the opt-in grant-block exemption);
//! 8. state transition: coinbase emission, balances, nonces (`core::state`);
//! 9. `state_root` commitment, when the header carries one.

use std::time::{SystemTime, UNIX_EPOCH};

use strangecoin_core::state::{apply_block, compute_state_root, State};
use strangecoin_core::types::Block;
use tracing::warn;

use crate::error::StrangecoinError;
use crate::GRANT_BLOCK_INDEX;

/// Everything a block is validated against besides its parent state.
///
/// Pure data: no locks, no I/O, so the executor can be unit-tested without a
/// running node.
pub struct BlockView<'a> {
    /// Chain prefix ending in the parent block. For a well-formed chain
    /// `chain.len() == block.index`.
    pub chain: &'a [Block],
    /// Wall-clock seconds used for the future-timestamp bound.
    pub now: u64,
    /// Opt-in primary-issuance grant block (height 1) may carry an unsigned
    /// transfer from the genesis address, which has no key to sign with.
    pub allow_grant_blocks: bool,
}

impl<'a> BlockView<'a> {
    pub fn new(chain: &'a [Block], now: u64, allow_grant_blocks: bool) -> Self {
        Self {
            chain,
            now,
            allow_grant_blocks,
        }
    }

    /// `BlockView` for a block about to be appended to `chain`.
    pub fn next(chain: &'a [Block], allow_grant_blocks: bool) -> Self {
        Self::new(chain, now_secs(), allow_grant_blocks)
    }
}

/// Current wall-clock time in seconds (the clock every timestamp bound uses).
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Validate `block` against `parent_state` and `view.chain`, then apply it.
///
/// On success the returned state is the state *after* the block; on failure
/// nothing is mutated and the caller keeps its previous state untouched.
pub fn validate_and_apply(
    parent_state: &State,
    block: &Block,
    view: &BlockView<'_>,
) -> Result<State, StrangecoinError> {
    validate_position(block, view.chain)?;

    let computed_hash = hex::encode(strangecoin_core::serialize::block_hash(block));
    if block.hash != computed_hash {
        return Err(invalid(
            block,
            format!("header hash mismatch: stored {}, computed {}", block.hash, computed_hash),
        ));
    }

    let expected_version = strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION;
    if block.consensus_version != expected_version {
        return Err(invalid(
            block,
            format!(
                "consensus_version mismatch: got {}, expected {}",
                block.consensus_version, expected_version
            ),
        ));
    }

    strangecoin_core::consensus::validate_tx_root(block)?;
    strangecoin_core::consensus::validate_timestamp(block, view.chain, view.now)?;

    // The genesis target is a network parameter, not a puzzle the miner solved,
    // so proof of work starts at height 1 — same as validate_chain always did.
    if block.index > 0 {
        strangecoin_core::consensus::validate_difficulty(block)?;
        validate_target(block, view.chain)?;
    }

    let is_opt_in_grant_block = view.allow_grant_blocks && block.index == GRANT_BLOCK_INDEX;
    if !is_opt_in_grant_block {
        for tx in &block.transactions {
            strangecoin_core::consensus::verify_transaction(tx)?;
        }
    }

    let new_state = apply_block(parent_state, block)?;

    // A zero state_root means the header does not commit to one yet; a non-zero
    // value must match the state this block produces.
    if block.state_root != [0u8; 32] && block.state_root != compute_state_root(&new_state.balances)
    {
        return Err(invalid(
            block,
            format!(
                "state root mismatch: stored {:x?}, computed {:x?}",
                block.state_root,
                compute_state_root(&new_state.balances)
            ),
        ));
    }

    Ok(new_state)
}

/// `block.index` and `block.previous_hash` must line up with the prefix.
fn validate_position(block: &Block, chain: &[Block]) -> Result<(), StrangecoinError> {
    if block.index == 0 {
        if !chain.is_empty() {
            return Err(invalid(block, "genesis block applied on a non-empty chain"));
        }
        if block.previous_hash != "0".repeat(64) {
            return Err(invalid(block, "genesis previous_hash must be 64 zeroes"));
        }
        return Ok(());
    }

    let parent = chain
        .last()
        .ok_or_else(|| invalid(block, "parent block is missing"))?;
    if block.index as usize != chain.len() {
        return Err(invalid(
            block,
            format!(
                "index {} does not follow parent height {}",
                block.index, parent.index
            ),
        ));
    }
    if block.previous_hash != parent.hash {
        return Err(invalid(
            block,
            format!(
                "previous_hash {} does not match parent hash {}",
                block.previous_hash, parent.hash
            ),
        ));
    }
    Ok(())
}

/// `block.target` must be inherited from the parent, or recomputed on a
/// retarget height.
fn validate_target(block: &Block, chain: &[Block]) -> Result<(), StrangecoinError> {
    if block.index.is_multiple_of(crate::consensus::RETARGET_INTERVAL) {
        let expected = hex::encode(strangecoin_core::consensus::compute_target(chain));
        if block.target != expected {
            return Err(invalid(
                block,
                format!("target {} at retarget height, expected {}", block.target, expected),
            ));
        }
        return Ok(());
    }

    let parent = chain
        .last()
        .ok_or_else(|| invalid(block, "parent block is missing"))?;
    if block.target != parent.target {
        return Err(invalid(
            block,
            format!(
                "target {} changed outside a retarget height, expected {}",
                block.target, parent.target
            ),
        ));
    }
    Ok(())
}

fn invalid(block: &Block, reason: impl std::fmt::Display) -> StrangecoinError {
    let message = format!("block {}: {}", block.index, reason);
    warn!(block_index = block.index, reason = %reason, "Block rejected");
    StrangecoinError::InvalidBlock(message)
}
