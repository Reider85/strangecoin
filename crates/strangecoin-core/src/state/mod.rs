mod inner;
pub mod sparse_merkle;
pub mod witness;

pub use inner::{apply_block, unapply_block, State};

use std::collections::HashMap;

use crate::types::AccountState;
use sparse_merkle::SparseMerkleTrie;

/// Compute the post-block state root and check it against `block.state_root`
/// (invariant #19, SCIP-0002). `chain_id` pins the validating network
/// (BUG-S1-004) — same rule as [`apply_block`].
///
/// A zero `state_root` is accepted only for the genesis block (`index == 0`)
/// or when `allow_zero_state_root` is explicitly enabled (legacy regtest
/// chains produced before BUG-S1-002). Everywhere else a block must commit
/// to its post-state.
pub fn root_after(
    state: &State,
    block: &crate::types::Block,
    chain_id: u32,
    allow_zero_state_root: bool,
) -> Result<[u8; 32], crate::error::CoreError> {
    let new_state = apply_block(state, block, chain_id)?;
    let computed = SparseMerkleTrie::compute_root(&new_state.balances);
    if block.state_root == [0u8; 32] {
        let zero_allowed = block.index == 0 || allow_zero_state_root;
        if !zero_allowed {
            return Err(crate::error::CoreError::StateRootMismatch {
                expected: [0u8; 32],
                got: computed,
            });
        }
    } else if block.state_root != computed {
        return Err(crate::error::CoreError::StateRootMismatch {
            expected: block.state_root,
            got: computed,
        });
    }
    Ok(computed)
}

pub fn compute_state_root(accounts: &HashMap<String, AccountState>) -> [u8; 32] {
    SparseMerkleTrie::compute_root(accounts)
}
