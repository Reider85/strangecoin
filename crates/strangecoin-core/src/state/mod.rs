mod inner;
pub mod verkle;
pub mod witness;

pub use inner::{apply_block, unapply_block, State};

use std::collections::HashMap;

use crate::types::AccountState;
use verkle::VerkleTrie;

pub fn root_after(
    state: &State,
    block: &crate::types::Block,
) -> Result<[u8; 32], crate::error::CoreError> {
    let new_state = apply_block(state, block)?;
    let computed = VerkleTrie::compute_root(&new_state.balances);
    // A zero state_root means the block does not commit to one yet
    // (blocks produced before S1-P06); a non-zero value must match.
    if block.state_root != [0u8; 32] && block.state_root != computed {
        return Err(crate::error::CoreError::StateRootMismatch {
            expected: block.state_root,
            got: computed,
        });
    }
    Ok(computed)
}

pub fn compute_state_root(accounts: &HashMap<String, AccountState>) -> [u8; 32] {
    VerkleTrie::compute_root(accounts)
}
