//! Contract storage in the state trie (S1.5-P15): overlay flush,
//! `storage_root`/state-root updates (invariant #19 extension) and the
//! `unapply` restore path (invariant #4). The executor is mocked — the
//! wasmi end-to-end path lives in `strangecoin-vm-wasm` tests.
//! S1.5-FBP06: the persisted snapshot is a shared handle — pointer
//! stability across calls proves the per-call setup is O(1).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use strangecoin_core::address::derive_contract_address;
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::state::{
    apply_block, apply_block_with_vm, contract_storage_root, root_after_with_vm, unapply_block,
    State,
};
use strangecoin_core::types::{Block, Transaction};
use strangecoin_core::vm::{
    ExecutionContext, ExecutionResult, Opcode, TxStatus, VmError, VmExecutor,
};

fn make_state(balances: Vec<(String, u64)>) -> State {
    let mut state = State::new();
    for (addr, bal) in balances {
        state.set_balance(&addr, bal);
    }
    state
}

/// Read access to a state's shared contract storage (S1.5-FBP06).
fn storage_of(
    state: &State,
) -> std::sync::RwLockReadGuard<'_, strangecoin_core::state::ContractStorageMap> {
    state
        .contract_storage
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn coinbase(receiver: &str, amount: u64) -> Transaction {
    Transaction::coinbase("coinbase", receiver, amount, 3)
}

fn test_block(index: u64, txs: Vec<Transaction>) -> Block {
    let mut all_txs = if index > 0 {
        vec![coinbase("miner", 0)]
    } else {
        Vec::new()
    };
    all_txs.extend(txs);
    Block {
        index,
        timestamp: index * 600,
        transactions: all_txs,
        previous_hash: format!("prev_{}", index.saturating_sub(1)),
        hash: format!("hash_{}", index),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

/// Executor stub that stages a fixed write-set under the callee's storage
/// address and records the `ctx.storage` snapshot it was seeded with.
struct WriteVm {
    writes: HashMap<[u8; 32], Vec<u8>>,
    seen_storage: Mutex<Vec<strangecoin_core::ContractStorageMap>>,
}

impl WriteVm {
    fn new(writes: Vec<([u8; 32], Vec<u8>)>) -> Self {
        Self {
            writes: writes.into_iter().collect(),
            seen_storage: Mutex::new(Vec::new()),
        }
    }
}

impl VmExecutor for WriteVm {
    type Error = VmError;

    fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        self.seen_storage.lock().unwrap().push(
            ctx.storage
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        );
        let mut storage_overlay = HashMap::new();
        storage_overlay.insert(ctx.storage_address.clone(), self.writes.clone());
        // S1.5-FBP05: mirror the executor's top-level value settlement so
        // `apply_call` commits the move on `Success`.
        let value_transfers = (ctx.value > 0)
            .then(|| strangecoin_core::ValueTransfer {
                from: ctx.caller.clone(),
                to: ctx.callee.clone(),
                amount: ctx.value,
            })
            .into_iter()
            .collect::<Vec<_>>();
        Ok(ExecutionResult {
            gas_used: 100,
            logs: Vec::new(),
            status: TxStatus::Success,
            return_data: Vec::new(),
            storage_overlay,
            value_transfers,
        })
    }

    fn gas_cost(&self, _opcode: Opcode) -> u64 {
        1
    }

    fn register_code(&mut self, _address: &str, _wasm: Vec<u8>) -> Result<(), VmError> {
        Ok(())
    }
}

struct FailVm;

impl VmExecutor for FailVm {
    type Error = VmError;

    fn execute(&self, _ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        Err(VmError::OutOfGas)
    }

    /// Deploy still succeeds (S1.5-FBP04): only `Call` frames fail — the
    /// scenarios below need an installed contract whose calls error out.
    fn execute_constructor(&self, _ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        Ok(ExecutionResult {
            gas_used: 0,
            logs: Vec::new(),
            status: TxStatus::Success,
            return_data: Vec::new(),
            storage_overlay: HashMap::new(),
            value_transfers: Vec::new(),
        })
    }

    fn gas_cost(&self, _opcode: Opcode) -> u64 {
        1
    }

    fn register_code(&mut self, _address: &str, _wasm: Vec<u8>) -> Result<(), VmError> {
        Ok(())
    }
}

/// Genesis + deploy, shared by the scenarios below.
fn setup_deploy() -> (State, Block, String) {
    let state = make_state(vec![("alice".to_string(), 5_000)]);
    let genesis = test_block(0, vec![coinbase("alice", 5_000)]);
    let after_genesis = apply_block(&state, &genesis, CHAIN_ID_REGTEST).unwrap();

    let create = Transaction::create("alice", vec![0x00, 0x61, 0x73, 0x6d], vec![], 100, 0, 3);
    let contract = derive_contract_address("alice", 0, CHAIN_ID_REGTEST).unwrap();
    let deploy = test_block(1, vec![create]);
    let after_deploy =
        apply_block_with_vm(&after_genesis, &deploy, CHAIN_ID_REGTEST, &mut FailVm).unwrap();

    assert!(after_deploy.get_account(&contract).unwrap().is_contract());
    assert_eq!(
        after_deploy
            .get_account(&contract)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root,
        [0u8; 32],
        "fresh deploy carries the empty-storage sentinel"
    );
    (after_deploy, deploy, contract)
}

#[test]
fn call_flushes_storage_and_updates_roots() {
    let (after_deploy, _deploy, contract) = setup_deploy();
    let key = [7u8; 32];
    let mut vm = WriteVm::new(vec![(key, vec![1, 2, 3])]);
    let call = Transaction::call("alice", &contract, vec![1], 0, 50_000, 1, 3);
    let block = test_block(2, vec![call]);

    let root_before = SparseMerkleTrie::compute_root(&after_deploy.balances);
    let applied =
        apply_block_with_vm(&after_deploy, &block, CHAIN_ID_REGTEST, &mut vm).unwrap();

    {
        let storage = storage_of(&applied);
        assert_eq!(storage[&contract][&key], vec![1, 2, 3]);
        let expected_storage_root = contract_storage_root(&storage[&contract]);
        assert_ne!(expected_storage_root, [0u8; 32]);
        assert_eq!(
            applied
                .get_account(&contract)
                .unwrap()
                .as_contract()
                .unwrap()
                .storage_root,
            expected_storage_root
        );
    }

    let root_after = SparseMerkleTrie::compute_root(&applied.balances);
    assert_ne!(root_after, root_before, "storage write must change state root (invariant #19)");

    // root_after_with_vm: matching root passes, tampered root fails.
    let mut committed = block.clone();
    committed.state_root = root_after;
    let computed = root_after_with_vm(
        &after_deploy,
        &committed,
        CHAIN_ID_REGTEST,
        false,
        &mut WriteVm::new(vec![(key, vec![1, 2, 3])]),
    )
    .unwrap();
    assert_eq!(computed, root_after);

    let mut tampered = block.clone();
    tampered.state_root = [9u8; 32];
    let err = root_after_with_vm(
        &after_deploy,
        &tampered,
        CHAIN_ID_REGTEST,
        false,
        &mut WriteVm::new(vec![(key, vec![1, 2, 3])]),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        strangecoin_core::CoreError::StateRootMismatch { .. }
    ));
}

#[test]
fn second_call_sees_persisted_storage_from_context() {
    let (after_deploy, _deploy, contract) = setup_deploy();
    let key = [7u8; 32];

    let block2 = test_block(
        2,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![1],
            0,
            50_000,
            1,
            3,
        )],
    );
    let after_first =
        apply_block_with_vm(&after_deploy, &block2, CHAIN_ID_REGTEST, &mut WriteVm::new(vec![(key, vec![42])]))
            .unwrap();

    let mut vm = WriteVm::new(vec![(key, vec![43])]);
    let block3 = test_block(
        3,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![2],
            0,
            50_000,
            2,
            3,
        )],
    );
    let after_second =
        apply_block_with_vm(&after_first, &block3, CHAIN_ID_REGTEST, &mut vm).unwrap();

    // The second frame was seeded with the persisted snapshot.
    let seen = vm.seen_storage.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0][&contract][&key], vec![42]);
    drop(seen);

    assert_eq!(storage_of(&after_second)[&contract][&key], vec![43]);
    assert_eq!(after_second.receipts[1].gas_used, 100);
}

#[test]
fn unapply_restores_storage_and_storage_root() {
    let (after_deploy, deploy, contract) = setup_deploy();
    let key = [7u8; 32];

    let block2 = test_block(
        2,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![1],
            0,
            50_000,
            1,
            3,
        )],
    );
    let after_first =
        apply_block_with_vm(&after_deploy, &block2, CHAIN_ID_REGTEST, &mut WriteVm::new(vec![(key, vec![42])]))
            .unwrap();
    let root_after_first = SparseMerkleTrie::compute_root(&after_first.balances);

    let block3 = test_block(
        3,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![2],
            0,
            50_000,
            2,
            3,
        )],
    );
    let after_second =
        apply_block_with_vm(&after_first, &block3, CHAIN_ID_REGTEST, &mut WriteVm::new(vec![(key, vec![43])]))
            .unwrap();

    // Roll back the tip call: contents and storage_root restored exactly.
    let restored_to_first = unapply_block(&after_second, &block3, CHAIN_ID_REGTEST).unwrap();
    assert_eq!(
        storage_of(&restored_to_first)[&contract][&key],
        vec![42]
    );
    assert_eq!(
        restored_to_first
            .get_account(&contract)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root,
        after_first
            .get_account(&contract)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root
    );
    assert_eq!(
        SparseMerkleTrie::compute_root(&restored_to_first.balances),
        root_after_first,
        "unapply must restore the exact state root (invariant #4)"
    );

    // Roll back the first storage call: storage empties to the sentinel.
    let restored_to_deploy = unapply_block(&restored_to_first, &block2, CHAIN_ID_REGTEST).unwrap();
    assert!(storage_of(&restored_to_deploy)
        .get(&contract)
        .map(HashMap::is_empty)
        .unwrap_or(true));
    assert_eq!(
        restored_to_deploy
            .get_account(&contract)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root,
        [0u8; 32]
    );

    // Roll back the deploy: account and storage entry gone.
    let restored_to_genesis = unapply_block(&restored_to_deploy, &deploy, CHAIN_ID_REGTEST).unwrap();
    assert!(restored_to_genesis.get_account(&contract).is_none());
    assert!(!storage_of(&restored_to_genesis).contains_key(&contract));
}

#[test]
fn failed_call_discards_storage_writes() {
    let (after_deploy, _deploy, contract) = setup_deploy();
    let call = Transaction::call("alice", &contract, vec![1], 0, 50_000, 1, 3);
    let block = test_block(2, vec![call]);

    let applied =
        apply_block_with_vm(&after_deploy, &block, CHAIN_ID_REGTEST, &mut FailVm).unwrap();

    assert_eq!(applied.receipts[1].status, TxStatus::OutOfGas);
    assert!(!storage_of(&applied).contains_key(&contract));
    assert_eq!(
        applied
            .get_account(&contract)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root,
        [0u8; 32],
        "failed frame must not move storage_root"
    );
    assert!(applied.storage_undo.is_empty());
}

#[test]
fn unapply_after_failed_call_leaves_storage_untouched() {
    let (after_deploy, _deploy, contract) = setup_deploy();
    let key = [7u8; 32];
    let block = test_block(
        2,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![1],
            0,
            50_000,
            1,
            3,
        )],
    );
    let after_ok =
        apply_block_with_vm(&after_deploy, &block, CHAIN_ID_REGTEST, &mut WriteVm::new(vec![(key, vec![9])]))
            .unwrap();

    let fail_block = test_block(
        3,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![2],
            0,
            50_000,
            2,
            3,
        )],
    );
    let after_fail =
        apply_block_with_vm(&after_ok, &fail_block, CHAIN_ID_REGTEST, &mut FailVm).unwrap();

    let restored = unapply_block(&after_fail, &fail_block, CHAIN_ID_REGTEST).unwrap();
    assert_eq!(storage_of(&restored)[&contract][&key], vec![9]);
}

#[test]
fn state_equality_covers_contract_storage() {
    let a = State::new();
    let b = State::new();
    assert_eq!(a, b);
    a.contract_storage
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert("sc1c".to_string(), [([1u8; 32], vec![1])].into());
    assert_ne!(a, b);
    b.contract_storage
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert("sc1c".to_string(), [([1u8; 32], vec![1])].into());
    assert_eq!(a, b);
}

#[test]
fn apply_is_deterministic_over_storage_roots() {
    let (after_deploy, _deploy, contract) = setup_deploy();
    let key = [7u8; 32];
    let block = test_block(
        2,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![1],
            0,
            50_000,
            1,
            3,
        )],
    );
    let first = apply_block_with_vm(
        &after_deploy,
        &block,
        CHAIN_ID_REGTEST,
        &mut WriteVm::new(vec![(key, vec![1]), ([8u8; 32], vec![2])]),
    )
    .unwrap();
    let second = apply_block_with_vm(
        &after_deploy,
        &block,
        CHAIN_ID_REGTEST,
        &mut WriteVm::new(vec![(key, vec![1]), ([8u8; 32], vec![2])]),
    )
    .unwrap();
    assert_eq!(
        SparseMerkleTrie::compute_root(&first.balances),
        SparseMerkleTrie::compute_root(&second.balances)
    );
    assert_eq!(first, second);
}

#[test]
fn flush_ignores_non_contract_scopes() {
    let (after_deploy, _deploy, contract) = setup_deploy();
    let key = [7u8; 32];
    let mut writes = HashMap::new();
    writes.insert(contract.clone(), {
        let mut slots = HashMap::new();
        slots.insert(key, vec![1]);
        slots
    });
    writes.insert("alice".to_string(), {
        let mut slots = HashMap::new();
        slots.insert(key, vec![6, 6, 6]);
        slots
    });
    struct ScopedVm {
        writes: HashMap<String, HashMap<[u8; 32], Vec<u8>>>,
    }
    impl VmExecutor for ScopedVm {
        type Error = VmError;
        fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
            let value_transfers = (ctx.value > 0)
                .then(|| strangecoin_core::ValueTransfer {
                    from: ctx.caller.clone(),
                    to: ctx.callee.clone(),
                    amount: ctx.value,
                })
                .into_iter()
                .collect::<Vec<_>>();
            Ok(ExecutionResult {
                gas_used: 1,
                logs: Vec::new(),
                status: TxStatus::Success,
                return_data: Vec::new(),
                storage_overlay: self.writes.clone(),
                value_transfers,
            })
        }
        fn gas_cost(&self, _opcode: Opcode) -> u64 {
            1
        }
        fn register_code(&mut self, _address: &str, _wasm: Vec<u8>) -> Result<(), VmError> {
            Ok(())
        }
    }
    let mut vm = ScopedVm { writes };
    let block = test_block(
        2,
        vec![Transaction::call(
            "alice",
            &contract,
            vec![1],
            0,
            50_000,
            1,
            3,
        )],
    );
    let applied =
        apply_block_with_vm(&after_deploy, &block, CHAIN_ID_REGTEST, &mut vm).unwrap();
    {
        let storage = storage_of(&applied);
        assert_eq!(storage[&contract][&key], vec![1]);
        assert!(
            !storage.contains_key("alice"),
            "EOA scopes must not gain storage"
        );
    }
    assert!(
        applied
            .get_account("alice")
            .unwrap()
            .as_contract()
            .is_none()
    );
}

// --- S1.5-FBP06: lazy contract storage access ------------------------------

/// КГ S1.5-FBP06: O(1) per-call setup. 100 sequential `CallTx`s against a
/// contract with 10 000 persisted keys all receive the **same** shared
/// storage handle (`Arc::as_ptr` stable across calls), proving the frame
/// is seeded with `Arc::clone` and not a deep copy of the map. The frames
/// stage no writes: the per-call flush cost is dominated by the `storage_root`
/// recompute over the whole contract map (O(storage), incremental SMT is
/// Stage 3+), which is outside this prompt's scope — the flush merge
/// itself is covered by the tests above.
#[test]
fn lazy_storage_setup_is_pointer_stable_across_calls() {
    const KEYS: usize = 10_000;
    const CALLS: usize = 100;

    let (after_deploy, _deploy, contract) = setup_deploy();

    // Seed 10 000 persisted keys directly (bypassing the VM).
    {
        let mut storage_map = after_deploy
            .contract_storage
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let slots = storage_map.entry(contract.clone()).or_default();
        for i in 0..KEYS {
            let mut key = [0u8; 32];
            key[..8].copy_from_slice(&u64::try_from(i).unwrap().to_le_bytes());
            slots.insert(key, vec![0xff]);
        }
    }

    /// Records the `Arc::as_ptr` of `ctx.storage` per call.
    struct PointerVm {
        seen: Mutex<Vec<usize>>,
    }

    impl VmExecutor for PointerVm {
        type Error = VmError;

        fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
            self.seen
                .lock()
                .unwrap()
                .push(Arc::as_ptr(&ctx.storage) as usize);
            Ok(ExecutionResult {
                gas_used: 10,
                logs: Vec::new(),
                status: TxStatus::Success,
                return_data: Vec::new(),
                storage_overlay: HashMap::new(),
                value_transfers: Vec::new(),
            })
        }

        fn gas_cost(&self, _opcode: Opcode) -> u64 {
            1
        }

        fn register_code(&mut self, _address: &str, _wasm: Vec<u8>) -> Result<(), VmError> {
            Ok(())
        }
    }

    let mut vm = PointerVm {
        seen: Mutex::new(Vec::new()),
    };

    let txs: Vec<Transaction> = (0..CALLS)
        .map(|i| {
            Transaction::call(
                "alice",
                &contract,
                vec![1],
                0,
                50_000,
                u64::try_from(i).unwrap(),
                3,
            )
        })
        .collect();
    let block = test_block(2, txs);

    let applied =
        apply_block_with_vm(&after_deploy, &block, CHAIN_ID_REGTEST, &mut vm).unwrap();

    let seen = vm.seen.lock().unwrap();
    assert_eq!(seen.len(), CALLS);
    assert!(
        seen.iter().all(|ptr| *ptr == seen[0]),
        "every call must receive the same shared storage handle (no per-call clone)"
    );
    drop(seen);

    let storage = storage_of(&applied);
    let contract_storage = storage.get(&contract).expect("seeded scope present");
    assert_eq!(
        contract_storage.len(),
        KEYS,
        "no-write frames leave the persisted map untouched"
    );
}
