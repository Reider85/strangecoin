mod inner;
pub mod contract_storage;
pub mod sparse_merkle;
pub mod witness;

pub use contract_storage::{ContractStorage, ContractStorageMap, ContractStorageOverlay};
pub use inner::{apply_block, apply_block_with_vm, unapply_block, State};
pub use sparse_merkle::{contract_storage_root, prove_receipt, receipts_subroot, verify_receipt_proof};

use std::collections::HashMap;

use crate::types::AccountState;
use crate::vm::VmExecutor;
use sparse_merkle::SparseMerkleTrie;

/// Compute the post-block state root and check it against `block.state_root`
/// (invariant #19, SCIP-0002). `chain_id` pins the validating network
/// (BUG-S1-004) — same rule as [`apply_block`].
///
/// Transfer-only blocks: `Create`/`Call` transactions need
/// [`root_after_with_vm`].
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
    root_after_inner(state, block, allow_zero_state_root, |state, block| {
        apply_block(state, block, chain_id)
    })
}

/// [`root_after`] for blocks carrying `Create`/`Call` transactions: the
/// state transition runs through `vm`, so contract storage overlays flush
/// and `ContractAccount.storage_root` values fold into the computed global
/// root (S1.5-P15, invariant #19 extension to contract storage).
pub fn root_after_with_vm<V: VmExecutor>(
    state: &State,
    block: &crate::types::Block,
    chain_id: u32,
    allow_zero_state_root: bool,
    vm: &mut V,
) -> Result<[u8; 32], crate::error::CoreError> {
    root_after_inner(state, block, allow_zero_state_root, |state, block| {
        apply_block_with_vm(state, block, chain_id, vm)
    })
}

fn root_after_inner(
    state: &State,
    block: &crate::types::Block,
    allow_zero_state_root: bool,
    apply: impl FnOnce(
        &State,
        &crate::types::Block,
    ) -> Result<State, crate::error::CoreError>,
) -> Result<[u8; 32], crate::error::CoreError> {
    let new_state = apply(state, block)?;
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
