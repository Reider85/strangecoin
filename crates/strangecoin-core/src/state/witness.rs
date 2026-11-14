//! Stateless validation witnesses (S1-P07 baseline, S3-P11 wire layer).
//!
//! [`StateWitness`] carries, for every address a block touches, the
//! account's pre-image and a Sparse-Merkle sibling proof anchored at
//! `pre_state_root`; [`verify_block_stateless`] checks coverage, every
//! proof, and the recomputed post-state root against the header — so a
//! light client validates a block **without** holding the full state.
//!
//! **S3-P11 deviations (FBP, fixed here and in the commit):**
//!
//! - **SMT, not Verkle.** The proofs are over the current sorted-range SMT
//!   (`sparse_merkle.rs`), not a Verkle trie. Replacing SMT with Verkle is
//!   consensus-breaking and SCIP-gated (ADR-0006 + Stage 2 residual §3);
//!   Stage 3 ships the wire + node path over the existing SMT so the
//!   light-client DoD is met functionally, Verkle stays a separate SCIP.
//! - **Wire integration deferred.** The canonical codec below is *not*
//!   wired into the `strangecoin-net` message enum — carrying witnesses
//!   over gossip/sync is Stage 4+ residual. Stage 3 exposes the codec,
//!   the node-side API and tests only.
//! - **Pre-state comes from replay.** `build_witness` needs the *full*
//!   pre-state (the SMT proof spans every leaf), so the node-side
//!   `BlockchainFacade::build_witness_for_block` reconstructs it by
//!   replaying `chain[..height]` — expensive, documented at the facade.
//!   The `state_history` CF (touched deltas only) cannot rebuild the leaf
//!   set and is therefore not an optimization path.
//!
//! **Codec determinism** (Stage 3 determinism checklist, V-21 extension):
//! `serialize_state_witness` is a pure byte function — no time, no RNG,
//! no I/O — and sorts proofs by address bytes, so one `StateWitness`
//! always encodes to the same bytes regardless of `HashMap` iteration
//! order; decode rejects unsorted input, keeping the representation
//! canonical in both directions.

use std::collections::HashMap;

use crate::error::CoreError;
use crate::serialize::{read_string, read_u32_be, read_u64_be};
use crate::types::{Account, Block, ContractAccount};

use super::inner::State;
use super::sparse_merkle::{LeafUpdate, SparseMerkleTrie};

/// Wire format version of [`serialize_state_witness`]. Kept separate from
/// `serialize::FORMAT_VERSION` (block/tx/header wire): witness bytes travel
/// alongside a block, never inside its commitment, so the two versions
/// evolve independently. Bump on any incompatible layout change.
pub const WITNESS_FORMAT_VERSION: u8 = 1;

/// Minimum wire size of one encoded [`AccountProof`]: address length
/// prefix (4) + balance (8) + nonce (8) + code_hash tag (1) +
/// storage_root tag (1) + key_hash (32) + proof hash count (4).
const MIN_ACCOUNT_PROOF_WIRE_LEN: usize = 4 + 8 + 8 + 1 + 1 + 32 + 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountProof {
    pub balance: u64,
    pub nonce: u64,
    /// `Some` iff the proven account is a contract (S1.5-P07).
    pub code_hash: Option<[u8; 32]>,
    /// `Some` iff the proven account is a contract (S1.5-P07).
    pub storage_root: Option<[u8; 32]>,
    pub key_hash: [u8; 32],
    pub proof: Vec<[u8; 32]>,
}

impl AccountProof {
    pub fn account(&self) -> Account {
        match (self.code_hash, self.storage_root) {
            (Some(code_hash), Some(storage_root)) => Account::Contract(ContractAccount {
                code_hash,
                storage_root,
                nonce: self.nonce,
                balance: self.balance,
            }),
            _ => Account::eoa(self.balance, self.nonce),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateWitness {
    pub pre_state_root: [u8; 32],
    pub proofs: HashMap<String, AccountProof>,
}

/// Canonical binary encoding of `witness` (S3-P11, invariant-style like
/// `serialize_block`): `WITNESS_FORMAT_VERSION` byte, `pre_state_root`,
/// then the proofs **sorted by address bytes** — a `HashMap` iterates in
/// an arbitrary order, so sorting is what makes the encoding deterministic
/// (Stage 3 determinism checklist). Pure function: no time, no RNG, no I/O.
///
/// The encoded witness is *not* part of any block commitment; it is relayed
/// beside the block (wire integration — Stage 4+ residual, module docs).
pub fn serialize_state_witness(witness: &StateWitness) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(WITNESS_FORMAT_VERSION);
    out.extend_from_slice(&witness.pre_state_root);

    let mut proofs: Vec<(&String, &AccountProof)> = witness.proofs.iter().collect();
    proofs.sort_by(|(a, _), (b, _)| a.as_bytes().cmp(b.as_bytes()));

    out.extend_from_slice(&(proofs.len() as u32).to_be_bytes());
    for (address, proof) in proofs {
        out.extend_from_slice(&(address.len() as u32).to_be_bytes());
        out.extend_from_slice(address.as_bytes());
        serialize_account_proof(&mut out, proof);
    }
    out
}

/// Inverse of [`serialize_state_witness`]. Rejects unknown versions,
/// truncated buffers, proof counts that cannot fit the remainder (no
/// oversized allocation before the sanity check), addresses that are not
/// in strictly-ascending order (a duplicate or shuffled encoding is not
/// canonical) and trailing bytes — mirroring `deserialize_block`.
pub fn deserialize_state_witness(bytes: &[u8]) -> Result<StateWitness, &'static str> {
    if bytes.len() < 1 + 32 + 4 {
        return Err("Witness too short");
    }
    if bytes[0] != WITNESS_FORMAT_VERSION {
        return Err("Unknown witness format version");
    }
    let mut offset = 1;
    let mut pre_state_root = [0u8; 32];
    pre_state_root.copy_from_slice(&bytes[offset..offset + 32]);
    offset += 32;

    let count = read_u32_be(bytes, &mut offset)? as usize;
    let remaining = bytes.len() - offset;
    if count > remaining / MIN_ACCOUNT_PROOF_WIRE_LEN {
        return Err("Proof count exceeds buffer");
    }

    let mut proofs = HashMap::with_capacity(count);
    let mut prev_address: Option<Vec<u8>> = None;
    for _ in 0..count {
        let address = read_string(bytes, &mut offset)?;
        if let Some(prev) = &prev_address {
            if address.as_bytes() <= prev.as_slice() {
                return Err("Proof addresses not in strictly ascending order");
            }
        }
        prev_address = Some(address.as_bytes().to_vec());
        let proof = deserialize_account_proof(bytes, &mut offset)?;
        proofs.insert(address, proof);
    }
    if offset != bytes.len() {
        return Err("Trailing bytes after witness");
    }

    Ok(StateWitness {
        pre_state_root,
        proofs,
    })
}

fn serialize_account_proof(out: &mut Vec<u8>, proof: &AccountProof) {
    out.extend_from_slice(&proof.balance.to_be_bytes());
    out.extend_from_slice(&proof.nonce.to_be_bytes());
    match proof.code_hash {
        Some(hash) => {
            out.push(1);
            out.extend_from_slice(&hash);
        }
        None => out.push(0),
    }
    match proof.storage_root {
        Some(root) => {
            out.push(1);
            out.extend_from_slice(&root);
        }
        None => out.push(0),
    }
    out.extend_from_slice(&proof.key_hash);
    out.extend_from_slice(&(proof.proof.len() as u32).to_be_bytes());
    for hash in &proof.proof {
        out.extend_from_slice(hash);
    }
}

fn deserialize_account_proof(
    bytes: &[u8],
    offset: &mut usize,
) -> Result<AccountProof, &'static str> {
    let balance = read_u64_be(bytes, offset)?;
    let nonce = read_u64_be(bytes, offset)?;
    let code_hash = read_optional_hash32(bytes, offset, "Invalid code_hash tag")?;
    let storage_root = read_optional_hash32(bytes, offset, "Invalid storage_root tag")?;
    let key_hash = read_hash32(bytes, offset)?;
    let hash_count = read_u32_be(bytes, offset)? as usize;
    let remaining = bytes.len() - *offset;
    if hash_count > remaining / 32 {
        return Err("Proof hash count exceeds buffer");
    }
    let mut proof = Vec::with_capacity(hash_count);
    for _ in 0..hash_count {
        proof.push(read_hash32(bytes, offset)?);
    }
    Ok(AccountProof {
        balance,
        nonce,
        code_hash,
        storage_root,
        key_hash,
        proof,
    })
}

fn read_hash32(bytes: &[u8], offset: &mut usize) -> Result<[u8; 32], &'static str> {
    if *offset + 32 > bytes.len() {
        return Err("Buffer too short for 32-byte hash");
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes[*offset..*offset + 32]);
    *offset += 32;
    Ok(hash)
}

fn read_optional_hash32(
    bytes: &[u8],
    offset: &mut usize,
    what: &'static str,
) -> Result<Option<[u8; 32]>, &'static str> {
    if *offset >= bytes.len() {
        return Err("Buffer too short for optional hash tag");
    }
    match bytes[*offset] {
        0 => {
            *offset += 1;
            Ok(None)
        }
        1 => {
            *offset += 1;
            Ok(Some(read_hash32(bytes, offset)?))
        }
        _ => Err(what),
    }
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
        let (code_hash, storage_root) = match &account {
            Account::Contract(contract) => (Some(contract.code_hash), Some(contract.storage_root)),
            Account::Eoa(_) => (None, None),
        };
        proofs.insert(
            addr.clone(),
            AccountProof {
                balance: account.balance(),
                nonce: account.nonce(),
                code_hash,
                storage_root,
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

/// Stateless verification of `block` against `parent_state_root` with a
/// Merkle witness (BUG-S1-003). `chain_id` pins the validating network
/// (BUG-S1-004) — the caller supplies the expected network constant; the
/// state transition rejects foreign-chain transactions and applies the
/// chain's own coinbase reward schedule.
pub fn verify_block_stateless(
    parent_state_root: &[u8; 32],
    block: &Block,
    chain_id: u32,
    witness: &StateWitness,
) -> Result<(), CoreError> {
    if witness.pre_state_root != *parent_state_root {
        return Err(CoreError::WitnessVerificationFailed);
    }

    for (addr, account_proof) in &witness.proofs {
        let account = account_proof.account();
        if !SparseMerkleTrie::verify_proof(parent_state_root, addr, &account, &account_proof.proof)
        {
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
        reconstructed
            .balances
            .insert(addr.clone(), account_proof.account());
    }

    let post_state = super::inner::apply_block(&reconstructed, block, chain_id)?;

    // Recompute the post-state root from the parent root, the witness proofs
    // and the applied block (BUG-S1-003, S1-P07 КГ, invariant #19), then
    // compare it with the commitment in the header.
    let updates: Vec<LeafUpdate<'_>> = witness
        .proofs
        .iter()
        .map(|(addr, account_proof)| LeafUpdate {
            address: addr,
            pre: account_proof.account(),
            proof: &account_proof.proof,
            post: post_state.balances.get(addr).cloned().unwrap_or_default(),
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
    let mut sorted: Vec<(&String, &Account)> = state.balances.iter().collect();
    sorted.sort_by_key(|(addr, _)| *blake3::hash(addr.as_bytes()).as_bytes());
    for (addr, account) in sorted {
        trie.insert(addr, account);
    }
    trie
}

fn collect_touched_addresses(block: &Block) -> Vec<String> {
    let mut addresses = Vec::new();
    for tx in &block.transactions {
        if tx.is_coinbase() {
            if let Some(receiver) = tx.receiver() {
                if !addresses.contains(&receiver.to_string()) {
                    addresses.push(receiver.to_string());
                }
            }
        } else {
            if !addresses.contains(&tx.sender().to_string()) {
                addresses.push(tx.sender().to_string());
            }
            if let Some(receiver) = tx.receiver() {
                if !addresses.contains(&receiver.to_string()) {
                    addresses.push(receiver.to_string());
                }
            }
        }
    }
    addresses
}
