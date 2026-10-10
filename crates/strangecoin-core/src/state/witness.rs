use std::collections::HashMap;

use crate::error::CoreError;
use crate::types::{AccountState, Block};

use super::inner::State;
use super::sparse_merkle::{LeafUpdate, SparseMerkleTrie};

#[derive(Clone, Debug)]
pub struct AccountProof {
    pub balance: u64,
    pub nonce: u64,
    pub key_hash: [u8; 32],
    pub proof: Vec<[u8; 32]>,
}

#[derive(Clone, Debug)]
pub struct StateWitness {
    pub pre_state_root: [u8; 32],
    pub proofs: HashMap<String, AccountProof>,
}

pub fn build_witness(pre_state: &State, block: &Block) -> Result<StateWitness, CoreError> {
    let pre_state_root = SparseMerkleTrie::compute_root(&pre_state.balances);
    let trie = build_trie(pre_state);

    let mut proofs = HashMap::new();
    let touched = collect_touched_addresses(block);

    for addr in &touched {
        let account = pre_state.balances.get(addr).cloned().unwrap_or_default();
        let key_hash = *blake3::hash(addr.as_bytes()).as_bytes();
        let proof = trie.prove(addr, &account)?;
        proofs.insert(
            addr.clone(),
            AccountProof {
                balance: account.balance,
                nonce: account.nonce,
                key_hash,
                proof,
            },
        );
    }

    Ok(StateWitness {
        pre_state_root,
        proofs,
    })
}

pub fn verify_block_stateless(
    parent_state_root: &[u8; 32],
    block: &Block,
    witness: &StateWitness,
) -> Result<(), CoreError> {
    if witness.pre_state_root != *parent_state_root {
        return Err(CoreError::WitnessVerificationFailed);
    }

    for (addr, account_proof) in &witness.proofs {
        let account = AccountState {
            balance: account_proof.balance,
            nonce: account_proof.nonce,
        };
        if !SparseMerkleTrie::verify_proof(parent_state_root, addr, &account, &account_proof.proof) {
            return Err(CoreError::WitnessVerificationFailed);
        }
    }

    // apply_block on a partial state defaults a missing account to (0, 0):
    // the witness must cover every address the block touches, or a stripped
    // proof would silently invent an absent pre-image.
    for addr in collect_touched_addresses(block) {
        if !witness.proofs.contains_key(&addr) {
            return Err(CoreError::WitnessVerificationFailed);
        }
    }

    let mut reconstructed = State::new();
    for (addr, account_proof) in &witness.proofs {
        reconstructed.balances.insert(
            addr.clone(),
            AccountState {
                balance: account_proof.balance,
                nonce: account_proof.nonce,
            },
        );
    }

    let post_state = super::inner::apply_block(&reconstructed, block)?;

    // Recompute the post-state root from the parent root, the witness proofs
    // and the applied block (BUG-S1-003, S1-P07 КГ, invariant #19), then
    // compare it with the commitment in the header.
    let updates: Vec<LeafUpdate<'_>> = witness
        .proofs
        .iter()
        .map(|(addr, account_proof)| LeafUpdate {
            address: addr,
            pre: AccountState {
                balance: account_proof.balance,
                nonce: account_proof.nonce,
            },
            proof: &account_proof.proof,
            post: post_state
                .balances
                .get(addr)
                .cloned()
                .unwrap_or_default(),
        })
        .collect();
    let computed = SparseMerkleTrie::root_after_updates(parent_state_root, &updates)?;

    // Genesis (index 0) is exempt from the commitment (invariant #19, same
    // rule as `root_after` / SCIP-0002); a non-genesis zero root can never
    // match the recomputed value and is rejected.
    if block.index == 0 && block.state_root == [0u8; 32] {
        return Ok(());
    }
    if block.state_root != computed {
        return Err(CoreError::PostStateRootMismatch {
            expected: block.state_root,
            got: computed,
        });
    }

    Ok(())
}

fn build_trie(state: &State) -> SparseMerkleTrie {
    let mut trie = SparseMerkleTrie::new();
    let mut sorted: Vec<(&String, &AccountState)> = state.balances.iter().collect();
    sorted.sort_by_key(|(addr, _)| *blake3::hash(addr.as_bytes()).as_bytes());
    for (addr, account) in sorted {
        trie.insert(addr, account);
    }
    trie
}

fn collect_touched_addresses(block: &Block) -> Vec<String> {
    let mut addresses = Vec::new();
    for tx in &block.transactions {
        if tx.is_coinbase {
            if !addresses.contains(&tx.receiver) {
                addresses.push(tx.receiver.clone());
            }
        } else {
            if !addresses.contains(&tx.sender) {
                addresses.push(tx.sender.clone());
            }
            if !addresses.contains(&tx.receiver) {
                addresses.push(tx.receiver.clone());
            }
        }
    }
    addresses
}
