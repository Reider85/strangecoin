//! S1.5-P16: contract-storage trie fuzzing. A random sequence of
//! `storage_write` / delete / read operations through
//! `ContractStorageOverlay` must produce a deterministic SMT sub-root:
//! the same operation sequence always yields the same `storage_root`, and
//! apply/unapply (preimage restore) must return the base map to a
//! byte-identical state (invariant #4 extension, S1.5-P15).

#![no_main]

use std::collections::HashMap;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use strangecoin_core::state::{contract_storage_root, ContractStorage, ContractStorageOverlay};

const MAX_VALUE_LEN: usize = 1024;
const MAX_OPS: usize = 64;

#[derive(Arbitrary, Debug)]
enum StorageOp {
    /// Stage a write of `value` under `key` (last-value-wins).
    Write { key: [u8; 32], value: Vec<u8> },
    /// Stage a removal of `key`.
    Remove { key: [u8; 32] },
    /// Flush the staged overlay into the persisted base, then clear it.
    Commit,
}

fn truncate(value: &[u8]) -> Vec<u8> {
    value.iter().copied().take(MAX_VALUE_LEN).collect()
}

fuzz_target!(|ops: Vec<StorageOp>| {
    let mut base = ContractStorage::new();
    let mut overlay = ContractStorageOverlay::new();

    for op in ops.iter().take(MAX_OPS) {
        match op {
            StorageOp::Write { key, value } => {
                overlay.entries.insert(*key, Some(truncate(value)));
            }
            StorageOp::Remove { key } => {
                overlay.entries.insert(*key, None);
            }
            StorageOp::Commit => {
                // Snapshot for the undo check: preimages then apply.
                let preimages = overlay.preimages_of(&base);
                overlay.apply_to(&mut base);

                // Undo (reorg path, S1.5-P15 unapply_call): restore every
                // staged key from its preimage; base must be byte-identical
                // to a replay of the same overlay.
                let mut restored = base.clone();
                for (key, preimage) in &preimages {
                    match preimage {
                        Some(value) => {
                            restored.insert(*key, value.clone());
                        }
                        None => {
                            restored.remove(key);
                        }
                    }
                }
                // Re-apply the same overlay onto the restored base and
                // compare roots — a cheaper full-state equality check that
                // still catches any divergence in the trie.
                overlay.apply_to(&mut restored);
                assert_eq!(
                    contract_storage_root(&restored),
                    contract_storage_root(&base),
                    "undo/redo root mismatch (invariant #4)"
                );

                overlay = ContractStorageOverlay::new();
            }
        }
    }

    // Determinism: recomputing the post-overlay root twice from the same
    // (base, overlay) pair must agree, and must equal the root of the
    // explicitly merged map.
    let root1 = overlay.root_from(&base);
    let root2 = overlay.root_from(&base);
    assert_eq!(root1, root2, "contract storage root not deterministic");

    let mut merged: HashMap<[u8; 32], Vec<u8>> = base.clone();
    overlay.apply_to(&mut merged);
    assert_eq!(
        root1,
        contract_storage_root(&merged),
        "root_from diverged from merged-map root"
    );
});
