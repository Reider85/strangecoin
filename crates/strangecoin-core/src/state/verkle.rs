use std::collections::HashMap;

use blake3::Hasher;

use crate::types::AccountState;

const EMPTY_HASH: [u8; 32] = [
    0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x9f, 0xb0, 0x2d, 0xc6, 0x03, 0x1e,
    0x89, 0x25, 0x81, 0x64, 0xb6, 0xb8, 0x73, 0x0a, 0x4b, 0x48, 0x02, 0x6d, 0x1e, 0x94, 0x78,
    0xf5, 0x46,
];

fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut data = [0u8; 64];
    data[..32].copy_from_slice(left);
    data[32..].copy_from_slice(right);
    hash_bytes(&data)
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

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Leaf {
        key_hash: [u8; 32],
        account: AccountState,
    },
    Branch {
        children: Box<[[u8; 32]; 256]>,
    },
}

#[derive(Clone, Debug)]
pub struct VerkleTrie {
    nodes: Box<[[u8; 32]; 256]>,
    depth: usize,
}

impl VerkleTrie {
    pub fn new() -> Self {
        Self {
            nodes: Box::new([EMPTY_HASH; 256]),
            depth: 0,
        }
    }

    pub fn insert(&mut self, address: &str, account: &AccountState) {
        let key = account_key_hash(address);
        let leaf = account_leaf_hash(&key, account);
        self.insert_at_depth(&key, leaf, 0);
    }

    fn insert_at_depth(&mut self, key: &[u8; 32], value: [u8; 32], depth: usize) {
        if depth >= 32 {
            return;
        }
        let idx = key[depth] as usize;
        self.nodes[idx] = value;
    }

    pub fn root(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();
        for child in self.nodes.iter() {
            hasher.update(child);
        }
        *hasher.finalize().as_bytes()
    }

    pub fn compute_root(accounts: &HashMap<String, AccountState>) -> [u8; 32] {
        if accounts.is_empty() {
            return Self::empty_root();
        }

        let mut sorted: Vec<(&String, &AccountState)> = accounts.iter().collect();
        sorted.sort_by_key(|(addr, _)| account_key_hash(addr));

        let mut trie = Self::new();
        for (addr, account) in sorted {
            trie.insert(addr, account);
        }
        trie.root()
    }

    pub fn empty_root() -> [u8; 32] {
        let mut hasher = Hasher::new();
        for _ in 0..256 {
            hasher.update(&EMPTY_HASH);
        }
        *hasher.finalize().as_bytes()
    }
}

impl Default for VerkleTrie {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(balance: u64, nonce: u64) -> AccountState {
        AccountState { balance, nonce }
    }

    #[test]
    fn empty_root_deterministic() {
        let r1 = VerkleTrie::empty_root();
        let r2 = VerkleTrie::empty_root();
        assert_eq!(r1, r2);
        assert_ne!(r1, [0u8; 32]);
    }

    #[test]
    fn single_account_root_deterministic() {
        let mut accounts = HashMap::new();
        accounts.insert("alice".to_string(), make_account(1000, 0));
        let r1 = VerkleTrie::compute_root(&accounts);
        let r2 = VerkleTrie::compute_root(&accounts);
        assert_eq!(r1, r2);
        assert_ne!(r1, VerkleTrie::empty_root());
    }

    #[test]
    fn two_accounts_order_independent() {
        let mut accounts1 = HashMap::new();
        accounts1.insert("alice".to_string(), make_account(1000, 0));
        accounts1.insert("bob".to_string(), make_account(2000, 1));

        let mut accounts2 = HashMap::new();
        accounts2.insert("bob".to_string(), make_account(2000, 1));
        accounts2.insert("alice".to_string(), make_account(1000, 0));

        assert_eq!(
            VerkleTrie::compute_root(&accounts1),
            VerkleTrie::compute_root(&accounts2)
        );
    }

    #[test]
    fn different_accounts_different_roots() {
        let mut accounts1 = HashMap::new();
        accounts1.insert("alice".to_string(), make_account(1000, 0));

        let mut accounts2 = HashMap::new();
        accounts2.insert("alice".to_string(), make_account(2000, 0));

        assert_ne!(
            VerkleTrie::compute_root(&accounts1),
            VerkleTrie::compute_root(&accounts2)
        );
    }

    #[test]
    fn add_account_changes_root() {
        let mut accounts1 = HashMap::new();
        accounts1.insert("alice".to_string(), make_account(1000, 0));
        let root1 = VerkleTrie::compute_root(&accounts1);

        let mut accounts2 = accounts1.clone();
        accounts2.insert("bob".to_string(), make_account(500, 0));
        let root2 = VerkleTrie::compute_root(&accounts2);

        assert_ne!(root1, root2);
    }

    #[test]
    fn nonce_change_changes_root() {
        let mut accounts1 = HashMap::new();
        accounts1.insert("alice".to_string(), make_account(1000, 0));
        let root1 = VerkleTrie::compute_root(&accounts1);

        let mut accounts2 = HashMap::new();
        accounts2.insert("alice".to_string(), make_account(1000, 1));
        let root2 = VerkleTrie::compute_root(&accounts2);

        assert_ne!(root1, root2);
    }

    #[test]
    fn many_accounts_deterministic() {
        let mut accounts = HashMap::new();
        for i in 0..100 {
            accounts.insert(format!("addr_{}", i), make_account(i * 100, i));
        }
        let r1 = VerkleTrie::compute_root(&accounts);
        let r2 = VerkleTrie::compute_root(&accounts);
        assert_eq!(r1, r2);
        assert_ne!(r1, [0u8; 32]);
    }
}
