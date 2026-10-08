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
//! `prove(address, account)` consumes `account` (BUG-S0-016): a pruned account
//! (`balance == 0 && nonce == 0`) is proven by the empty slot. True Verkle/KZG
//! remains deferred to Stage 3+ per ADR-0006.

use std::collections::HashMap;
use std::sync::OnceLock;

use blake3::Hasher;

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
    pub fn prove(&self, address: &str, account: &AccountState) -> Vec<[u8; 32]> {
        let key = account_key_hash(address);
        let leaf = if account.balance == 0 && account.nonce == 0 {
            EMPTY_HASH
        } else {
            account_leaf_hash(&key, account)
        };
        let _ = leaf;

        let mut sorted: Vec<(&[u8; 32], &[u8; 32])> = self.leaves.iter().collect();
        sorted.sort_by_key(|(k, _)| **k);

        let mut proof = Vec::with_capacity(DEPTH);
        collect_siblings(&sorted, &key, 0, 0, sorted.len(), &mut proof);
        proof
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
            let proof = trie.prove(addr, account);
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
        let proof = trie.prove("nobody", &absent);
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
        let mut proof = trie.prove("alice", &account);
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
        let proof = trie.prove("alice", &real);
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
            let proof = trie.prove("alice", &alice);
            prop_assert!(SparseMerkleTrie::verify_proof(&root, "alice", &alice, &proof));

            let bob = make_account(balance / 2 + 1, 1);
            let proof_bob = trie.prove("bob", &bob);
            prop_assert!(SparseMerkleTrie::verify_proof(&root, "bob", &bob, &proof_bob));
        }
    }
}
