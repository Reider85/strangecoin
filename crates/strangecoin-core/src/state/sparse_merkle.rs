//! # Sparse Merkle Tree — state commitment (ADR-0006 amended, BUG-S0-011)
//!
//! Replaces the former flat 256-slot `VerkleTrie`: that structure branched
//! only on `key[0]`, collided once state exceeded 256 accounts (BUG-S0-014)
//! and was not a trie at all (BUG-S0-011).
//!
//! Construction:
//! * key = `blake3(address)` (32 bytes = 256 bits)
//! * binary tree, depth 256: level `i` branches on bit `i` (MSB of byte 0 first)
//! * leaf hash = `blake3(key ‖ balance_le ‖ nonce_le)`; empty slot = `EMPTY_HASH`
//! * internal node = `blake3(left ‖ right)`
//! * proof = 256 sibling hashes (8 KiB), one per level
//!
//! `prove(address, account)` consumes `account` (BUG-S0-016): the claimed
//! account is validated against the trie state; a pruned account
//! (`balance == 0 && nonce == 0`) is proven by the empty slot (`EMPTY_HASH`).
//! A mismatch between the claimed account and the trie returns
//! `CoreError::ProofAccountMismatch`. True Verkle/KZG remains deferred to
//! Stage 3+ per ADR-0006.

use std::collections::HashMap;
use std::sync::OnceLock;

use blake3::Hasher;

use crate::error::CoreError;
use crate::types::AccountState;

const DEPTH: usize = 256;

const EMPTY_HASH: [u8; 32] = [
    0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x9f, 0xb0, 0x2d, 0xc6, 0x03, 0x1e, 0x89,
    0x25, 0x81, 0x64, 0xb6, 0xb8, 0x73, 0x0a, 0x4b, 0x48, 0x02, 0x6d, 0x1e, 0x94, 0x78, 0xf5, 0x46,
];

fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

fn account_key_hash(address: &str) -> [u8; 32] {
    hash_bytes(address.as_bytes())
}

fn account_leaf_hash(key_hash: &[u8; 32], account: &AccountState) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(key_hash);
    hasher.update(&account.balance.to_le_bytes());
    hasher.update(&account.nonce.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Bit `bit_index` of `key`, MSB-first (byte 0, bit 7 → index 0).
fn bit_at(key: &[u8; 32], bit_index: usize) -> u8 {
    let byte = key[bit_index / 8];
    let shift = 7 - (bit_index % 8);
    (byte >> shift) & 1
}

/// Copy of `key` with every bit at index `>= depth` zeroed — the path prefix
/// identifying the node at `depth` on `key`'s path (root at 0, leaf at 256).
fn path_prefix(key: &[u8; 32], depth: usize) -> [u8; 32] {
    let mut prefix = [0u8; 32];
    let whole = depth / 8;
    prefix[..whole].copy_from_slice(&key[..whole]);
    let rem = depth % 8;
    if rem > 0 {
        prefix[whole] = key[whole] & (0xffu8 << (8 - rem));
    }
    prefix
}

/// Path prefix of the sibling node at depth `level + 1` next to the on-path
/// node of `key` — the prefix of length `level` with bit `level` flipped.
fn sibling_path(key: &[u8; 32], level: usize) -> [u8; 32] {
    let mut path = path_prefix(key, level);
    if bit_at(key, level) == 0 {
        path[level / 8] |= 1 << (7 - level % 8);
    }
    path
}

/// Leaf hash of `account` at `key`, or [`EMPTY_HASH`] when the account is
/// pruned (`balance == 0 && nonce == 0` — absent from the trie).
fn leaf_hash_for(key: &[u8; 32], account: &AccountState) -> [u8; 32] {
    if account.balance == 0 && account.nonce == 0 {
        EMPTY_HASH
    } else {
        account_leaf_hash(key, account)
    }
}

/// One leaf replacement for [`SparseMerkleTrie::root_after_updates`]: the
/// account as it stands in the pre-state (`pre` + its inclusion proof against
/// the pre-root) and the account the block writes in its place (`post`).
#[derive(Clone, Debug)]
pub struct LeafUpdate<'a> {
    pub address: &'a str,
    pub pre: AccountState,
    pub proof: &'a [[u8; 32]],
    pub post: AccountState,
}

/// Precomputed hash of an empty subtree rooted at `depth`
/// (bits already consumed = `depth`; remaining levels = `DEPTH - depth`).
fn empty_subtree(depth: usize) -> [u8; 32] {
    static TABLE: OnceLock<[[u8; 32]; DEPTH + 1]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [[0u8; 32]; DEPTH + 1];
        table[DEPTH] = EMPTY_HASH;
        for d in (0..DEPTH).rev() {
            table[d] = hash_pair(&table[d + 1], &table[d + 1]);
        }
        table
    });
    table[depth]
}

/// Root hash of the subtree covering `sorted[start..end]`, branching on bits
/// from `bit` onward. Empty range → precomputed empty-subtree hash.
fn hash_range(sorted: &[(&[u8; 32], &[u8; 32])], bit: usize, start: usize, end: usize) -> [u8; 32] {
    if start >= end {
        return empty_subtree(bit);
    }
    if bit == DEPTH {
        return *sorted[start].1;
    }
    let mid_rel = sorted[start..end].partition_point(|(k, _)| bit_at(k, bit) == 0);
    let mid = start + mid_rel;
    let left = if mid == start {
        empty_subtree(bit + 1)
    } else {
        hash_range(sorted, bit + 1, start, mid)
    };
    let right = if mid == end {
        empty_subtree(bit + 1)
    } else {
        hash_range(sorted, bit + 1, mid, end)
    };
    hash_pair(&left, &right)
}

/// Sibling subtree hash at level `bit` on the path of `key`, then recurse.
fn collect_siblings(
    sorted: &[(&[u8; 32], &[u8; 32])],
    key: &[u8; 32],
    bit: usize,
    start: usize,
    end: usize,
    proof: &mut Vec<[u8; 32]>,
) {
    if bit == DEPTH {
        return;
    }
    let mid_rel = sorted[start..end].partition_point(|(k, _)| bit_at(k, bit) == 0);
    let mid = start + mid_rel;
    let bit_val = bit_at(key, bit);

    let (sibling_start, sibling_end, next_start, next_end) = if bit_val == 0 {
        (mid, end, start, mid)
    } else {
        (start, mid, mid, end)
    };

    let sibling = if sibling_start >= sibling_end {
        empty_subtree(bit + 1)
    } else {
        hash_range(sorted, bit + 1, sibling_start, sibling_end)
    };
    proof.push(sibling);
    collect_siblings(sorted, key, bit + 1, next_start, next_end, proof);
}

#[derive(Clone, Debug, Default)]
pub struct SparseMerkleTrie {
    leaves: HashMap<[u8; 32], [u8; 32]>,
}

impl SparseMerkleTrie {
    pub fn new() -> Self {
        Self {
            leaves: HashMap::new(),
        }
    }

    pub fn insert(&mut self, address: &str, account: &AccountState) {
        let key = account_key_hash(address);
        if account.balance == 0 && account.nonce == 0 {
            self.leaves.remove(&key);
            return;
        }
        let leaf = account_leaf_hash(&key, account);
        self.leaves.insert(key, leaf);
    }

    pub fn root(&self) -> [u8; 32] {
        let mut sorted: Vec<(&[u8; 32], &[u8; 32])> = self.leaves.iter().collect();
        sorted.sort_by_key(|(k, _)| **k);
        hash_range(&sorted, 0, 0, sorted.len())
    }

    pub fn compute_root(accounts: &HashMap<String, AccountState>) -> [u8; 32] {
        if accounts.is_empty() {
            return Self::empty_root();
        }
        let mut sorted: Vec<([u8; 32], [u8; 32])> = accounts
            .iter()
            .filter(|(_, a)| !(a.balance == 0 && a.nonce == 0))
            .map(|(addr, account)| {
                let key = account_key_hash(addr);
                (key, account_leaf_hash(&key, account))
            })
            .collect();
        sorted.sort_by_key(|(k, _)| *k);
        let refs: Vec<(&[u8; 32], &[u8; 32])> = sorted.iter().map(|(k, v)| (k, v)).collect();
        hash_range(&refs, 0, 0, refs.len())
    }

    pub fn empty_root() -> [u8; 32] {
        empty_subtree(0)
    }

    /// Sibling proof for `account` (or its absence) against `self.root()`.
    ///
    /// Returns `DEPTH` (256) sibling hashes, level 0 (near root) first.
    ///
    /// Validates that the claimed `account` matches the trie state
    /// (BUG-S0-016): a non-empty account must be present with the matching
    /// leaf hash; a pruned account (`balance == 0 && nonce == 0`) must be
    /// absent. Mismatch → [`CoreError::ProofAccountMismatch`].
    pub fn prove(
        &self,
        address: &str,
        account: &AccountState,
    ) -> Result<Vec<[u8; 32]>, CoreError> {
        let key = account_key_hash(address);
        let is_pruned = account.balance == 0 && account.nonce == 0;
        let expected_leaf = if is_pruned {
            EMPTY_HASH
        } else {
            account_leaf_hash(&key, account)
        };

        match self.leaves.get(&key) {
            None if is_pruned => {}
            Some(leaf) if !is_pruned && *leaf == expected_leaf => {}
            _ => return Err(CoreError::ProofAccountMismatch),
        }

        let mut sorted: Vec<(&[u8; 32], &[u8; 32])> = self.leaves.iter().collect();
        sorted.sort_by_key(|(k, _)| **k);

        let mut proof = Vec::with_capacity(DEPTH);
        collect_siblings(&sorted, &key, 0, 0, sorted.len(), &mut proof);
        Ok(proof)
    }

    /// Verify `account` (or its absence) against `root` using `proof`.
    ///
    /// Accounts with `balance == 0 && nonce == 0` are pruned from the state:
    /// such an account is proven by the empty slot (`EMPTY_HASH` leaf).
    pub fn verify_proof(
        root: &[u8; 32],
        address: &str,
        account: &AccountState,
        proof: &[[u8; 32]],
    ) -> bool {
        if proof.len() != DEPTH {
            return false;
        }
        let key = account_key_hash(address);
        let leaf = if account.balance == 0 && account.nonce == 0 {
            EMPTY_HASH
        } else {
            account_leaf_hash(&key, account)
        };

        let mut acc = leaf;
        for i in (0..DEPTH).rev() {
            if bit_at(&key, i) == 0 {
                acc = hash_pair(&acc, &proof[i]);
            } else {
                acc = hash_pair(&proof[i], &acc);
            }
        }
        acc == *root
    }

    /// Recompute the tree root after replacing the leaves of every key in
    /// `updates`, anchored at `pre_root` (BUG-S1-003, S1-P07 КГ).
    ///
    /// Each update carries the pre-image account with its `DEPTH`-sibling
    /// inclusion proof against `pre_root` (absence is proven by the empty
    /// slot) and the post-image account the block writes. The function:
    ///
    /// 1. verifies every pre-proof against `pre_root`;
    /// 2. seeds the node map from the proofs — the pre-tree frontier;
    /// 3. overwrites the updated leaves with their post-image leaf hashes;
    /// 4. recomputes the ancestor chain of every updated key bottom-up.
    ///
    /// Untouched sibling subtrees keep their pre-image hashes, which are
    /// already anchored to `pre_root`, so the returned root commits to
    /// exactly the tree obtained from the pre-state by applying `updates` —
    /// an untouched leaf cannot diverge without breaking a pre-proof.
    ///
    /// Returns [`CoreError::WitnessVerificationFailed`] when any pre-proof
    /// does not verify against `pre_root`.
    pub fn root_after_updates(
        pre_root: &[u8; 32],
        updates: &[LeafUpdate<'_>],
    ) -> Result<[u8; 32], CoreError> {
        if updates.is_empty() {
            return Ok(*pre_root);
        }

        let mut nodes: HashMap<(usize, [u8; 32]), [u8; 32]> = HashMap::new();

        for update in updates {
            let key = account_key_hash(update.address);
            if !Self::verify_proof(pre_root, update.address, &update.pre, update.proof) {
                return Err(CoreError::WitnessVerificationFailed);
            }
            nodes.insert((DEPTH, key), leaf_hash_for(&key, &update.pre));
            for level in 0..DEPTH {
                nodes.insert((level + 1, sibling_path(&key, level)), update.proof[level]);
            }
        }

        for update in updates {
            let key = account_key_hash(update.address);
            nodes.insert((DEPTH, key), leaf_hash_for(&key, &update.post));
        }

        for update in updates {
            let key = account_key_hash(update.address);
            for level in (0..DEPTH).rev() {
                let on_path = path_prefix(&key, level + 1);
                let off_path = sibling_path(&key, level);
                let (left, right) = if bit_at(&key, level) == 0 {
                    (on_path, off_path)
                } else {
                    (off_path, on_path)
                };
                let left_hash = *nodes
                    .get(&(level + 1, left))
                    .ok_or(CoreError::WitnessVerificationFailed)?;
                let right_hash = *nodes
                    .get(&(level + 1, right))
                    .ok_or(CoreError::WitnessVerificationFailed)?;
                nodes.insert(
                    (level, path_prefix(&key, level)),
                    hash_pair(&left_hash, &right_hash),
                );
            }
        }

        let root = *nodes
            .get(&(0, [0u8; 32]))
            .ok_or(CoreError::WitnessVerificationFailed)?;
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn make_account(balance: u64, nonce: u64) -> AccountState {
        AccountState { balance, nonce }
    }

    fn accounts_map(pairs: &[(&str, u64, u64)]) -> HashMap<String, AccountState> {
        pairs
            .iter()
            .map(|(a, b, n)| (a.to_string(), make_account(*b, *n)))
            .collect()
    }

    #[test]
    fn empty_root_deterministic() {
        let r1 = SparseMerkleTrie::empty_root();
        let r2 = SparseMerkleTrie::empty_root();
        assert_eq!(r1, r2);
        assert_ne!(r1, [0u8; 32]);
    }

    #[test]
    fn single_account_root_deterministic() {
        let accounts = accounts_map(&[("alice", 1000, 0)]);
        let r1 = SparseMerkleTrie::compute_root(&accounts);
        let r2 = SparseMerkleTrie::compute_root(&accounts);
        assert_eq!(r1, r2);
        assert_ne!(r1, SparseMerkleTrie::empty_root());
    }

    #[test]
    fn two_accounts_order_independent() {
        let a1 = accounts_map(&[("alice", 1000, 0), ("bob", 2000, 1)]);
        let a2 = accounts_map(&[("bob", 2000, 1), ("alice", 1000, 0)]);
        assert_eq!(
            SparseMerkleTrie::compute_root(&a1),
            SparseMerkleTrie::compute_root(&a2)
        );
    }

    #[test]
    fn different_accounts_different_roots() {
        let a1 = accounts_map(&[("alice", 1000, 0)]);
        let a2 = accounts_map(&[("alice", 2000, 0)]);
        assert_ne!(
            SparseMerkleTrie::compute_root(&a1),
            SparseMerkleTrie::compute_root(&a2)
        );
    }

    #[test]
    fn add_account_changes_root() {
        let a1 = accounts_map(&[("alice", 1000, 0)]);
        let root1 = SparseMerkleTrie::compute_root(&a1);
        let mut a2 = a1.clone();
        a2.insert("bob".to_string(), make_account(500, 0));
        let root2 = SparseMerkleTrie::compute_root(&a2);
        assert_ne!(root1, root2);
    }

    #[test]
    fn nonce_change_changes_root() {
        let a1 = accounts_map(&[("alice", 1000, 0)]);
        let root1 = SparseMerkleTrie::compute_root(&a1);
        let a2 = accounts_map(&[("alice", 1000, 1)]);
        let root2 = SparseMerkleTrie::compute_root(&a2);
        assert_ne!(root1, root2);
    }

    #[test]
    fn many_accounts_deterministic() {
        let mut accounts = HashMap::new();
        for i in 0..100u64 {
            accounts.insert(format!("addr_{}", i), make_account(i * 100, i));
        }
        let r1 = SparseMerkleTrie::compute_root(&accounts);
        let r2 = SparseMerkleTrie::compute_root(&accounts);
        assert_eq!(r1, r2);
        assert_ne!(r1, [0u8; 32]);
    }

    /// BUG-S0-014: 256..512 accounts must not collide (old flat trie lost data).
    #[test]
    fn sweep_256_to_512_accounts_unique_roots() {
        for n in [256usize, 300, 400, 512] {
            let mut accounts = HashMap::new();
            for i in 0..n {
                accounts.insert(format!("user_{}", i), make_account(i as u64 + 1, 1));
            }
            let root = SparseMerkleTrie::compute_root(&accounts);
            assert_ne!(root, SparseMerkleTrie::empty_root(), "n={n}");

            let mut without_last = accounts.clone();
            without_last.remove(&format!("user_{}", n - 1));
            let root_less = SparseMerkleTrie::compute_root(&without_last);
            assert_ne!(root, root_less, "removing one of n={n} accounts must change root");

            let mut tweaked = accounts.clone();
            tweaked.insert(format!("user_{}", n - 1), make_account(999_999, 2));
            let root_tweaked = SparseMerkleTrie::compute_root(&tweaked);
            assert_ne!(root, root_tweaked, "balance change at n={n} must change root");
        }
    }

    #[test]
    fn zero_accounts_are_pruned() {
        let mut accounts = HashMap::new();
        accounts.insert("empty".to_string(), make_account(0, 0));
        accounts.insert("alice".to_string(), make_account(10, 0));
        let root = SparseMerkleTrie::compute_root(&accounts);
        let only_alice = accounts_map(&[("alice", 10, 0)]);
        assert_eq!(root, SparseMerkleTrie::compute_root(&only_alice));
    }

    #[test]
    fn prove_verify_roundtrip_present_account() {
        let accounts = accounts_map(&[("alice", 1000, 3), ("bob", 50, 1), ("carol", 7, 0)]);
        let root = SparseMerkleTrie::compute_root(&accounts);

        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }
        assert_eq!(trie.root(), root);

        for (addr, account) in &accounts {
            let proof = trie.prove(addr, account).unwrap();
            assert_eq!(proof.len(), DEPTH);
            assert!(
                SparseMerkleTrie::verify_proof(&root, addr, account, &proof),
                "verify failed for {addr}"
            );
        }
    }

    #[test]
    fn prove_verify_absent_account() {
        let accounts = accounts_map(&[("alice", 1000, 0)]);
        let root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let absent = make_account(0, 0);
        let proof = trie.prove("nobody", &absent).unwrap();
        assert_eq!(proof.len(), DEPTH);
        assert!(SparseMerkleTrie::verify_proof(&root, "nobody", &absent, &proof));
    }

    #[test]
    fn tampered_proof_rejected() {
        let accounts = accounts_map(&[("alice", 1000, 0), ("bob", 2000, 0)]);
        let root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let account = make_account(1000, 0);
        let mut proof = trie.prove("alice", &account).unwrap();
        proof[10][0] ^= 0xff;
        assert!(!SparseMerkleTrie::verify_proof(&root, "alice", &account, &proof));
    }

    #[test]
    fn tampered_account_rejected() {
        let accounts = accounts_map(&[("alice", 1000, 0)]);
        let root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let real = make_account(1000, 0);
        let fake = make_account(999, 0);
        let proof = trie.prove("alice", &real).unwrap();
        assert!(!SparseMerkleTrie::verify_proof(&root, "alice", &fake, &proof));
    }

    #[test]
    fn wrong_length_proof_rejected() {
        let accounts = accounts_map(&[("alice", 1000, 0)]);
        let root = SparseMerkleTrie::compute_root(&accounts);
        let account = make_account(1000, 0);
        assert!(!SparseMerkleTrie::verify_proof(&root, "alice", &account, &[]));
        assert!(!SparseMerkleTrie::verify_proof(&root, "alice", &account, &[[0u8; 32]; 10]));
    }

    /// BUG-S0-016: prove() validates the claimed account against the trie.
    #[test]
    fn prove_wrong_balance_returns_err() {
        let mut trie = SparseMerkleTrie::new();
        trie.insert("alice", &make_account(1000, 0));
        let wrong = make_account(999, 0);
        assert!(matches!(
            trie.prove("alice", &wrong),
            Err(CoreError::ProofAccountMismatch)
        ));
    }

    /// BUG-S0-016: prove() validates the claimed account against the trie.
    #[test]
    fn prove_wrong_nonce_returns_err() {
        let mut trie = SparseMerkleTrie::new();
        trie.insert("alice", &make_account(1000, 0));
        let wrong = make_account(1000, 1);
        assert!(matches!(
            trie.prove("alice", &wrong),
            Err(CoreError::ProofAccountMismatch)
        ));
    }

    /// BUG-S0-016: a pruned (0,0) account must be absent from the trie.
    #[test]
    fn prove_pruned_when_present_returns_err() {
        let mut trie = SparseMerkleTrie::new();
        trie.insert("alice", &make_account(1000, 0));
        let pruned = make_account(0, 0);
        assert!(matches!(
            trie.prove("alice", &pruned),
            Err(CoreError::ProofAccountMismatch)
        ));
    }

    /// BUG-S0-016: a non-empty account must be present in the trie.
    #[test]
    fn prove_nonempty_when_absent_returns_err() {
        let trie = SparseMerkleTrie::new();
        let claimed = make_account(100, 0);
        assert!(matches!(
            trie.prove("nobody", &claimed),
            Err(CoreError::ProofAccountMismatch)
        ));
    }

    /// BUG-S0-016: a pruned account for an absent address is valid.
    #[test]
    fn prove_absent_account_ok() {
        let mut trie = SparseMerkleTrie::new();
        trie.insert("alice", &make_account(1000, 0));
        let absent = make_account(0, 0);
        assert!(trie.prove("nobody", &absent).is_ok());
    }

    #[test]
    fn insert_remove_via_zero_account() {
        let mut trie = SparseMerkleTrie::new();
        let acc = make_account(100, 0);
        trie.insert("alice", &acc);
        let root_with = trie.root();
        trie.insert("alice", &make_account(0, 0));
        assert_eq!(trie.root(), SparseMerkleTrie::empty_root());
        assert_ne!(root_with, trie.root());
    }

    /// BUG-S1-003: one updated leaf — recomputed root equals a full recompute.
    #[test]
    fn root_after_updates_single_key() {
        let accounts = accounts_map(&[("alice", 1000, 0), ("bob", 2000, 1)]);
        let pre_root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let pre = make_account(1000, 0);
        let post = make_account(900, 1);
        let proof = trie.prove("alice", &pre).unwrap();
        let updates = [LeafUpdate {
            address: "alice",
            pre: pre.clone(),
            proof: &proof,
            post: post.clone(),
        }];

        let mut updated = accounts.clone();
        updated.insert("alice".to_string(), post);
        let expected = SparseMerkleTrie::compute_root(&updated);

        assert_eq!(
            SparseMerkleTrie::root_after_updates(&pre_root, &updates).unwrap(),
            expected
        );
    }

    /// BUG-S1-003: two keys sharing a long path prefix — the shared ancestors
    /// must be recomputed after both leaves, not taken stale from a proof.
    #[test]
    fn root_after_updates_overlapping_key_paths() {
        let mut pair = None;
        for i in 0..20_000u64 {
            let a = format!("overlap_a_{i}");
            let b = format!("overlap_b_{i}");
            if account_key_hash(&a)[0] == account_key_hash(&b)[0] {
                pair = Some((a, b));
                break;
            }
        }
        let (alice, bob) = pair.expect("no first-byte key collision found");

        let mut accounts = HashMap::new();
        accounts.insert(alice.clone(), make_account(1000, 0));
        accounts.insert(bob.clone(), make_account(2000, 0));
        let pre_root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let alice_pre = make_account(1000, 0);
        let alice_post = make_account(500, 1);
        let bob_pre = make_account(2000, 0);
        let bob_post = make_account(1500, 1);
        let alice_proof = trie.prove(&alice, &alice_pre).unwrap();
        let bob_proof = trie.prove(&bob, &bob_pre).unwrap();
        let updates = [
            LeafUpdate {
                address: &alice,
                pre: alice_pre.clone(),
                proof: &alice_proof,
                post: alice_post.clone(),
            },
            LeafUpdate {
                address: &bob,
                pre: bob_pre.clone(),
                proof: &bob_proof,
                post: bob_post.clone(),
            },
        ];

        let mut updated = accounts;
        updated.insert(alice.clone(), alice_post);
        updated.insert(bob.clone(), bob_post);
        let expected = SparseMerkleTrie::compute_root(&updated);

        assert_eq!(
            SparseMerkleTrie::root_after_updates(&pre_root, &updates).unwrap(),
            expected
        );
    }

    /// BUG-S1-003: a pruned post-image (0, 0) removes the leaf.
    #[test]
    fn root_after_updates_pruned_post_account() {
        let accounts = accounts_map(&[("alice", 1000, 0), ("bob", 42, 0)]);
        let pre_root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let pre = make_account(1000, 0);
        let proof = trie.prove("alice", &pre).unwrap();
        let updates = [LeafUpdate {
            address: "alice",
            pre: pre.clone(),
            proof: &proof,
            post: make_account(0, 0),
        }];

        let mut updated = accounts;
        updated.remove("alice");
        let expected = SparseMerkleTrie::compute_root(&updated);
        assert_eq!(expected, SparseMerkleTrie::compute_root(&accounts_map(&[("bob", 42, 0)])));

        assert_eq!(
            SparseMerkleTrie::root_after_updates(&pre_root, &updates).unwrap(),
            expected
        );
    }

    /// BUG-S1-003: an absent pre-image (empty slot proof) can gain a balance.
    #[test]
    fn root_after_updates_absent_to_present() {
        let accounts = accounts_map(&[("alice", 1000, 0)]);
        let pre_root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let absent = make_account(0, 0);
        let proof = trie.prove("nobody", &absent).unwrap();
        let updates = [LeafUpdate {
            address: "nobody",
            pre: absent.clone(),
            proof: &proof,
            post: make_account(500, 0),
        }];

        let mut updated = accounts;
        updated.insert("nobody".to_string(), make_account(500, 0));
        let expected = SparseMerkleTrie::compute_root(&updated);

        assert_eq!(
            SparseMerkleTrie::root_after_updates(&pre_root, &updates).unwrap(),
            expected
        );
    }

    /// BUG-S1-003: a proof that does not verify against the pre-root is rejected.
    #[test]
    fn root_after_updates_invalid_pre_proof_rejected() {
        let accounts = accounts_map(&[("alice", 1000, 0)]);
        let pre_root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let pre = make_account(1000, 0);
        let mut proof = trie.prove("alice", &pre).unwrap();
        proof[3][0] ^= 0xff;
        let updates = [LeafUpdate {
            address: "alice",
            pre,
            proof: &proof,
            post: make_account(900, 0),
        }];

        assert!(matches!(
            SparseMerkleTrie::root_after_updates(&pre_root, &updates),
            Err(CoreError::WitnessVerificationFailed)
        ));
    }

    /// BUG-S1-003: a claimed pre-image that disagrees with the trie is rejected.
    #[test]
    fn root_after_updates_wrong_pre_account_rejected() {
        let accounts = accounts_map(&[("alice", 1000, 0)]);
        let pre_root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let real = make_account(1000, 0);
        let fake = make_account(999_999, 0);
        let proof = trie.prove("alice", &real).unwrap();
        let updates = [LeafUpdate {
            address: "alice",
            pre: fake,
            proof: &proof,
            post: make_account(900, 0),
        }];

        assert!(matches!(
            SparseMerkleTrie::root_after_updates(&pre_root, &updates),
            Err(CoreError::WitnessVerificationFailed)
        ));
    }

    #[test]
    fn root_after_updates_empty_returns_pre_root() {
        let pre_root = SparseMerkleTrie::empty_root();
        assert_eq!(
            SparseMerkleTrie::root_after_updates(&pre_root, &[]).unwrap(),
            pre_root
        );
    }

    /// BUG-S1-003: scattered multi-key updates match a full recompute.
    #[test]
    fn root_after_updates_many_keys_matches_full_recompute() {
        let mut accounts = HashMap::new();
        for i in 0..64u64 {
            accounts.insert(format!("acct_{i}"), make_account(i * 10 + 1, i % 3));
        }
        let pre_root = SparseMerkleTrie::compute_root(&accounts);
        let mut trie = SparseMerkleTrie::new();
        for (addr, account) in &accounts {
            trie.insert(addr, account);
        }

        let targets = [0u64, 7, 13, 31, 63];
        let mut updated = accounts.clone();
        let mut owned: Vec<(String, AccountState, AccountState, Vec<[u8; 32]>)> = Vec::new();
        for &i in &targets {
            let addr = format!("acct_{i}");
            let pre = accounts[&addr].clone();
            let post = make_account(pre.balance + 1000, pre.nonce + 1);
            let proof = trie.prove(&addr, &pre).unwrap();
            updated.insert(addr.clone(), post.clone());
            owned.push((addr, pre, post, proof));
        }
        let updates: Vec<LeafUpdate<'_>> = owned
            .iter()
            .map(|(addr, pre, post, proof)| LeafUpdate {
                address: addr,
                pre: pre.clone(),
                proof,
                post: post.clone(),
            })
            .collect();

        let expected = SparseMerkleTrie::compute_root(&updated);
        assert_eq!(
            SparseMerkleTrie::root_after_updates(&pre_root, &updates).unwrap(),
            expected
        );
    }

    proptest! {
        #[test]
        fn proptest_thousand_accounts_unique_deterministic_root(
            seed in 0u64..10,
        ) {
            let mut accounts = HashMap::new();
            for i in 0..1000u64 {
                let balance = (i.wrapping_mul(seed.wrapping_add(1))).wrapping_add(i);
                accounts.insert(format!("acct_{i}"), make_account(balance, i % 7));
            }
            let r1 = SparseMerkleTrie::compute_root(&accounts);
            let r2 = SparseMerkleTrie::compute_root(&accounts);
            prop_assert_eq!(r1, r2);
            prop_assert_ne!(r1, SparseMerkleTrie::empty_root());
        }

        #[test]
        fn proptest_proof_roundtrip(
            balance in 1u64..1_000_000u64,
            nonce in 0u64..100u64,
        ) {
            let mut accounts = HashMap::new();
            accounts.insert("alice".to_string(), make_account(balance, nonce));
            accounts.insert("bob".to_string(), make_account(balance / 2 + 1, 1));
            let root = SparseMerkleTrie::compute_root(&accounts);

            let mut trie = SparseMerkleTrie::new();
            for (addr, account) in &accounts {
                trie.insert(addr, account);
            }
            prop_assert_eq!(trie.root(), root);

            let alice = make_account(balance, nonce);
            let proof = trie.prove("alice", &alice).unwrap();
            prop_assert!(SparseMerkleTrie::verify_proof(&root, "alice", &alice, &proof));

            let bob = make_account(balance / 2 + 1, 1);
            let proof_bob = trie.prove("bob", &bob).unwrap();
            prop_assert!(SparseMerkleTrie::verify_proof(&root, "bob", &bob, &proof_bob));
        }

        /// BUG-S1-003: mulproof update recompute equals a full root recompute.
        #[test]
        fn proptest_root_after_updates_matches_full_recompute(
            alice_bal in 1u64..1_000_000u64,
            bob_bal in 1u64..1_000_000u64,
            send in 1u64..500u64,
        ) {
            let mut accounts = HashMap::new();
            accounts.insert("alice".to_string(), make_account(alice_bal, 0));
            accounts.insert("bob".to_string(), make_account(bob_bal, 1));
            accounts.insert("carol".to_string(), make_account(77, 2));
            let pre_root = SparseMerkleTrie::compute_root(&accounts);

            let mut trie = SparseMerkleTrie::new();
            for (addr, account) in &accounts {
                trie.insert(addr, account);
            }

            let alice_pre = make_account(alice_bal, 0);
            let alice_post = make_account(alice_bal - send, 1);
            let bob_pre = make_account(bob_bal, 1);
            let bob_post = make_account(bob_bal + send, 1);
            let alice_proof = trie.prove("alice", &alice_pre).unwrap();
            let bob_proof = trie.prove("bob", &bob_pre).unwrap();
            let updates = [
                LeafUpdate { address: "alice", pre: alice_pre.clone(), proof: &alice_proof, post: alice_post.clone() },
                LeafUpdate { address: "bob", pre: bob_pre.clone(), proof: &bob_proof, post: bob_post.clone() },
            ];

            let mut updated = accounts;
            updated.insert("alice".to_string(), alice_post);
            updated.insert("bob".to_string(), bob_post);
            let expected = SparseMerkleTrie::compute_root(&updated);

            let got = SparseMerkleTrie::root_after_updates(&pre_root, &updates).unwrap();
            prop_assert_eq!(got, expected);
        }
    }
}
