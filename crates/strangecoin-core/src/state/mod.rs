mod inner;
pub mod verkle;
pub mod witness;

pub use inner::{apply_block, unapply_block, State};

use std::collections::HashMap;

use crate::types::AccountState;
use verkle::VerkleTrie;

pub fn root_after(state: &State, block: &crate::types::Block) -> Result<[u8; 32], crate::error::CoreError> {
    let new_state = apply_block(state, block)?;
    Ok(VerkleTrie::compute_root(&new_state.balances))
}

pub fn compute_state_root(accounts: &HashMap<String, AccountState>) -> [u8; 32] {
    VerkleTrie::compute_root(accounts)
}
