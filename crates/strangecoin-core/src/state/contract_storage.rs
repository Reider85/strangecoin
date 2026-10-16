//! Contract storage (S1.5-P15): per-contract key→value maps committed to
//! the global state root through `ContractAccount.storage_root` — an SMT
//! sub-root computed by
//! [`contract_storage_root`](crate::state::sparse_merkle::contract_storage_root).
//!
//! A [`ContractStorageOverlay`] is the write-set of one contract call: it
//! stages new values and can flush itself into a [`ContractStorage`] map or
//! compute the post-flush sub-root without mutating the base. The pre-image
//! of every staged key (captured via [`ContractStorageOverlay::preimages_of`])
//! is what `apply_call` stores in `State::storage_undo` so an apply/unapply
//! round-trip restores the exact prior contents (invariant #4, reorg support).

use std::collections::HashMap;

use super::sparse_merkle::contract_storage_root;

/// Persisted storage of one contract: 32-byte slot key → value.
pub type ContractStorage = HashMap<[u8; 32], Vec<u8>>;

/// Chain-wide contract storage: bech32 contract address → slot map.
pub type ContractStorageMap = HashMap<String, ContractStorage>;

/// Write-set of one contract call (S1.5-P15): `entries[key] = Some(value)`
/// stages a write, `None` stages a removal. Keys absent from the map are
/// untouched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContractStorageOverlay {
    pub entries: HashMap<[u8; 32], Option<Vec<u8>>>,
}

impl ContractStorageOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stage a frame's raw write map (the executor's last-value-wins
    /// overlay) as a flushable overlay.
    pub fn from_writes(writes: &HashMap<[u8; 32], Vec<u8>>) -> Self {
        Self {
            entries: writes
                .iter()
                .map(|(key, value)| (*key, Some(value.clone())))
                .collect(),
        }
    }

    /// Pre-image of every staged key against `base` (`None` = the key was
    /// absent before the write) — the undo record `apply_call` keeps per
    /// txid in `State::storage_undo`.
    pub fn preimages_of(&self, base: &ContractStorage) -> Vec<([u8; 32], Option<Vec<u8>>)> {
        self.entries
            .keys()
            .map(|key| (*key, base.get(key).cloned()))
            .collect()
    }

    /// Flush the staged writes into `storage` in place.
    pub fn apply_to(&self, storage: &mut ContractStorage) {
        for (key, entry) in &self.entries {
            match entry {
                Some(value) => {
                    storage.insert(*key, value.clone());
                }
                None => {
                    storage.remove(key);
                }
            }
        }
    }

    /// Storage sub-root after flushing this overlay onto `base`, without
    /// mutating `base`.
    pub fn root_from(&self, base: &ContractStorage) -> [u8; 32] {
        let mut merged = base.clone();
        self.apply_to(&mut merged);
        contract_storage_root(&merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writes(pairs: &[([u8; 32], Vec<u8>)]) -> HashMap<[u8; 32], Vec<u8>> {
        pairs.iter().cloned().collect()
    }

    #[test]
    fn from_writes_apply_to_roundtrip() {
        let base = writes(&[([1u8; 32], vec![10]), ([2u8; 32], vec![20])]);
        let overlay = ContractStorageOverlay::from_writes(&writes(&[([2u8; 32], vec![99]), ([3u8; 32], vec![30])]));

        let mut merged = base.clone();
        overlay.apply_to(&mut merged);

        assert_eq!(merged[&[1u8; 32]], vec![10]);
        assert_eq!(merged[&[2u8; 32]], vec![99]);
        assert_eq!(merged[&[3u8; 32]], vec![30]);
    }

    #[test]
    fn removal_entry_deletes_key() {
        let mut overlay = ContractStorageOverlay::new();
        overlay.entries.insert([1u8; 32], None);
        let mut storage = writes(&[([1u8; 32], vec![1]), ([2u8; 32], vec![2])]);
        overlay.apply_to(&mut storage);
        assert!(!storage.contains_key(&[1u8; 32]));
        assert_eq!(storage[&[2u8; 32]], vec![2]);
    }

    #[test]
    fn preimages_of_captures_absent_and_present() {
        let base = writes(&[([1u8; 32], vec![10])]);
        let overlay =
            ContractStorageOverlay::from_writes(&writes(&[([1u8; 32], vec![11]), ([2u8; 32], vec![22])]));
        let mut preimages = overlay.preimages_of(&base);
        preimages.sort_by_key(|(k, _)| *k);
        assert_eq!(preimages[0], ([1u8; 32], Some(vec![10])));
        assert_eq!(preimages[1], ([2u8; 32], None));
    }

    #[test]
    fn root_from_equals_root_of_merged_map() {
        let base = writes(&[([1u8; 32], vec![10])]);
        let overlay =
            ContractStorageOverlay::from_writes(&writes(&[([1u8; 32], vec![11])]));
        let mut merged = base.clone();
        overlay.apply_to(&mut merged);
        assert_eq!(overlay.root_from(&base), contract_storage_root(&merged));
    }

    #[test]
    fn root_from_empty_overlay_unchanged() {
        let base = writes(&[([1u8; 32], vec![10])]);
        let overlay = ContractStorageOverlay::new();
        assert_eq!(overlay.root_from(&base), contract_storage_root(&base));
    }
}
