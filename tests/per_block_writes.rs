//! Per-block incremental writes (S3-P07, ADR-0024 §Decision "Write path").
//!
//! Two acceptance checks of the prompt:
//!
//! 1. **O(changed records) on adopt** — a counting engine observes every
//!    `write_batch` op: adopting a one-block extension of a 30+ block
//!    chain must write a handful of records (the new block, the meta tip,
//!    the changed accounts, their pre-apply history), never one put per
//!    chain height or per account.
//! 2. **`state_history` on adopt** — the `state_history` CF holds the
//!    pre-apply values of the accounts the adopted tip touches
//!    (ADR-0024/ADR-0025), verifiable against the balances the adopting
//!    node held *before* the adoption.

mod common;

use common::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use strangecoin::storage::{Cf, KvPair, StorageEngine, StorageError, WriteBatchOp};

/// In-memory `StorageEngine` that counts mutations: one `BTreeMap` per
/// CF (sorted, mirroring the trait contract) plus put/delete counters.
/// Integration tests cannot reach the storage crate's `#[cfg(test)]`
/// `MemoryEngine`, so the mock lives here — same shape, counting added.
struct CountingEngine {
    cfs: Mutex<[BTreeMap<Vec<u8>, Vec<u8>>; 7]>,
    puts: AtomicUsize,
    deletes: AtomicUsize,
}

impl CountingEngine {
    fn new() -> Self {
        Self {
            cfs: Mutex::new(std::array::from_fn(|_| BTreeMap::new())),
            puts: AtomicUsize::new(0),
            deletes: AtomicUsize::new(0),
        }
    }

    fn reset_counters(&self) {
        self.puts.store(0, Ordering::SeqCst);
        self.deletes.store(0, Ordering::SeqCst);
    }

    fn puts(&self) -> usize {
        self.puts.load(Ordering::SeqCst)
    }

    fn deletes(&self) -> usize {
        self.deletes.load(Ordering::SeqCst)
    }
}

impl StorageEngine for CountingEngine {
    fn get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        let cfs = self.cfs.lock().expect("counting engine lock poisoned");
        Ok(cfs[cf.ordinal()].get(key).cloned())
    }

    fn write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError> {
        let mut cfs = self.cfs.lock().expect("counting engine lock poisoned");
        for op in ops {
            match op {
                WriteBatchOp::Put { cf, key, value } => {
                    self.puts.fetch_add(1, Ordering::SeqCst);
                    cfs[cf.ordinal()].insert(key, value);
                }
                WriteBatchOp::Delete { cf, key } => {
                    self.deletes.fetch_add(1, Ordering::SeqCst);
                    cfs[cf.ordinal()].remove(&key);
                }
            }
        }
        Ok(())
    }

    fn iterate_prefix(&self, cf: Cf, prefix: &[u8]) -> Result<Vec<KvPair>, StorageError> {
        let cfs = self.cfs.lock().expect("counting engine lock poisoned");
        Ok(cfs[cf.ordinal()]
            .range(prefix.to_vec()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }
}

/// Build a chain of `MIN_BLOCKS`+ blocks on `source`, extend it by one
/// block on a side facade, then adopt that extension back and assert the
/// write count stays O(changed records).
#[test]
fn adopt_writes_only_changed_records_not_the_whole_chain() {
    const MIN_BLOCKS: usize = 30;
    /// One block put + tip meta (height/hash/difficulty) + the touched
    /// accounts (~3) + their pre-apply history (~3) + margin. A full
    /// rewrite would need ≥ MIN_BLOCKS puts (blocks alone).
    const MAX_OPS: usize = 16;

    let engine = Arc::new(CountingEngine::new());
    let dir = TestDir::new("per_block_writes");
    let source = create_test_blockchain_with_engine(dir.path(), engine.clone());

    // Fund a wallet through the grant block, then transfer it around so
    // every block touches a known, non-trivial set of accounts.
    let (wallet, wallet_sk) = generate_keypair();
    assert!(source
        .grant_initial_balance_to_first_wallet(&wallet)
        .expect("grant must be enabled in tests"));
    let (alice, _) = generate_keypair();
    while source.chain_len() < MIN_BLOCKS {
        create_and_mine_tx(&source, &wallet, &alice, 7, &wallet_sk);
    }

    // Side facade: adopt the whole chain (its own genesis is byte-equal,
    // fork choice accepts the longer chain), then mine exactly one more
    // block on top of it.
    let side_dir = TestDir::new("per_block_writes_side");
    let side = create_test_blockchain(side_dir.path());
    assert!(side
        .adopt_candidate(source.chain_snapshot(), None, vec![], source.difficulty())
        .expect("side facade must adopt"));
    assert_eq!(side.chain_len(), source.chain_len());
    create_and_mine_tx(&side, &wallet, &alice, 7, &wallet_sk);
    assert_eq!(side.chain_len(), source.chain_len() + 1);

    // Balances the adopting node holds BEFORE the adoption — the values
    // the tip block's `state_history` entries must reproduce.
    let pre_wallet = source.get_balance(&wallet);
    let pre_alice = source.get_balance(&alice);

    engine.reset_counters();
    let adopted = source
        .adopt_candidate(side.chain_snapshot(), None, vec![], side.difficulty())
        .expect("extension must be adoptable");
    assert!(adopted, "strictly longer chain wins fork choice");

    let chain_len = source.chain_len();
    let ops = engine.puts() + engine.deletes();
    assert!(
        chain_len >= MIN_BLOCKS,
        "test premise: long chain, got {chain_len}"
    );
    assert!(
        ops <= MAX_OPS,
        "adopt must be O(changed records): {ops} puts+deletes \
         (puts {}, deletes {}) for a {chain_len}-block chain — \
         a full rewrite would need ≥ {chain_len} block puts alone",
        engine.puts(),
        engine.deletes()
    );

    // --- state_history: pre-apply snapshot of the adopted tip ---
    let tip_height = (chain_len - 1) as u64;
    let entries = engine
        .iterate_prefix(Cf::StateHistory, &tip_height.to_be_bytes())
        .expect("history iterate");
    assert!(
        !entries.is_empty(),
        "state_history must hold entries for the adopted tip height {tip_height}"
    );

    let history_value = |address: &str| -> Option<strangecoin::AccountState> {
        entries.iter().find_map(|(key, value)| {
            if key.len() > 8 && &key[8..] == address.as_bytes() {
                Some(serde_json::from_slice(value).expect("history account decodes"))
            } else {
                None
            }
        })
    };

    let wallet_pre = history_value(&wallet).expect("wallet pre-state recorded");
    assert_eq!(
        wallet_pre.balance(),
        pre_wallet,
        "state_history must store the wallet balance from BEFORE the adopted tip"
    );
    let alice_pre = history_value(&alice).expect("alice pre-state recorded");
    assert_eq!(
        alice_pre.balance(),
        pre_alice,
        "state_history must store the receiver balance from BEFORE the adopted tip"
    );
    // The adopted block moved the balances — so the post-state differs:
    // proves the recorded values are pre-apply, not post-apply.
    assert_ne!(
        source.get_balance(&wallet),
        pre_wallet,
        "the tip transfer must change the wallet balance"
    );
}
