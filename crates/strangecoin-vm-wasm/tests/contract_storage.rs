//! Contract storage end-to-end through wasmi (S1.5-P15): `storage_write`
//! flushes into `State.contract_storage`, `ContractAccount.storage_root`
//! and the global state root move with it (invariant #19 extension),
//! `storage_read` falls through to the persisted snapshot seeded by
//! `apply_call`, nested calls write into their own contract scope, and
//! `unapply_block` restores the exact pre-call contents (invariant #4).

use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::state::{
    apply_block_with_vm, contract_storage_root, root_after_with_vm, unapply_block, State,
};
use strangecoin_core::types::{Block, Transaction};
use strangecoin_core::vm::TxStatus;
use strangecoin_vm_wasm::WasmiExecutor;

const KEY_A: [u8; 32] = [0x01u8; 32];
const KEY_B: [u8; 32] = [0x02u8; 32];

fn hex_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

fn wat(segments: &[(u32, &[u8])], body: &str) -> String {
    let mut imports = String::new();
    let mut rest = String::new();
    for line in body.lines() {
        if line.trim_start().starts_with("(import ") {
            imports.push_str(line.trim_start());
        } else {
            rest.push_str(line);
        }
        rest.push('\n');
    }
    let mut out = String::from("(module ");
    out.push_str(&imports);
    out.push_str("(memory (export \"memory\") 1)");
    for (offset, bytes) in segments {
        out.push_str(&format!(
            "(data (i32.const {offset}) \"{}\")",
            hex_escape(bytes)
        ));
    }
    out.push_str(&rest);
    out.push(')');
    out
}

/// Writes `value` under `KEY_A`, then reads `KEY_A` into a scratch buffer
/// and copies those bytes under `KEY_B` — the copy is empty on the first
/// call (base absent at read time) and equals `value` from the second call
/// on (read falls through to the persisted snapshot).
fn read_write_wat() -> String {
    wat(
        &[(0, &KEY_A), (32, &KEY_B), (64, b"hello")],
        r#"
        (import "sc" "storage_read" (func $r (param i32 i32 i32) (result i32)))
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (local $n i32)
          (local.set $n (call $r (i32.const 0) (i32.const 128) (i32.const 32)))
          (drop (call $w (i32.const 32) (i32.const 128) (local.get $n)))
          (drop (call $w (i32.const 0) (i32.const 64) (i32.const 5)))
          (local.get $n))"#,
    )
}

/// Writes `vec![7, 7, 7]` under `KEY_A` — the callee of the nested-call
/// scenario.
fn write_own_storage_wat() -> String {
    wat(
        &[(0, &KEY_A), (32, &[7u8, 7, 7])],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $w (i32.const 0) (i32.const 32) (i32.const 3)))"#,
    )
}

/// Calls the contract whose bech32 address sits in the data segment at 0,
/// forwarding an empty payload — used to drive the callee's
/// `write_own_storage` module from an outer contract frame.
fn nested_call_wat(target: &str) -> String {
    let len = target.len();
    wat(
        &[(0, target.as_bytes())],
        &format!(
            r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $c (i32.const 0) (i32.const {len}) (i64.const 0) (i32.const 0) (i32.const 0) (i32.const 256) (i32.const 0)))"#
        ),
    )
}

fn test_block(index: u64, transactions: Vec<Transaction>) -> Block {
    let mut all_txs = vec![Transaction::coinbase("coinbase", "sc1miner", 0, CHAIN_ID_REGTEST)];
    all_txs.extend(transactions);
    Block {
        index,
        timestamp: 1000 + index * 600,
        transactions: all_txs,
        previous_hash: "prev_hash".to_string(),
        hash: format!("block_hash_{index}"),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn funded_state() -> State {
    let mut state = State::new();
    state.set_balance("sc1alice", 10_000);
    state
}

fn derive(alice_nonce: u64) -> String {
    strangecoin_core::address::derive_contract_address("sc1alice", alice_nonce, CHAIN_ID_REGTEST)
        .expect("derive address")
}

#[test]
fn storage_write_flushes_and_moves_state_root() {
    let state = funded_state();
    let wasm = read_write_wat();
    let create = Transaction::create("sc1alice", wasm.into_bytes(), vec![], 0, 0, CHAIN_ID_REGTEST);
    let contract = derive(0);
    let call = Transaction::call("sc1alice", &contract, vec![], 0, 100_000, 1, CHAIN_ID_REGTEST);
    let block = test_block(1, vec![create, call]);

    let root_before = SparseMerkleTrie::compute_root(&state.balances);
    let mut vm = WasmiExecutor::new();
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm).expect("apply");

    let receipt = &applied.receipts[2];
    assert_eq!(receipt.status, TxStatus::Success);
    assert!(receipt.gas_used > 0);

    let storage = applied.contract_storage.get(&contract).expect("flushed");
    assert_eq!(storage[&KEY_A], b"hello");
    assert!(storage[&KEY_B].is_empty(), "first call copies an absent read");

    let expected_root = contract_storage_root(storage);
    assert_ne!(expected_root, [0u8; 32]);
    assert_eq!(
        applied
            .get_account(&contract)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root,
        expected_root
    );

    let root_after = SparseMerkleTrie::compute_root(&applied.balances);
    assert_ne!(root_after, root_before, "storage flush must move state root");

    // Invariant #19: the block committing to the computed root validates.
    let mut committed = block.clone();
    committed.state_root = root_after;
    let mut vm = WasmiExecutor::new();
    let computed = root_after_with_vm(&state, &committed, CHAIN_ID_REGTEST, false, &mut vm)
        .expect("root_after_with_vm");
    assert_eq!(computed, root_after);
}

#[test]
fn second_call_reads_persisted_storage_from_seeded_base() {
    let state = funded_state();
    let wasm = read_write_wat();
    let create = Transaction::create("sc1alice", wasm.into_bytes(), vec![], 0, 0, CHAIN_ID_REGTEST);
    let contract = derive(0);
    let block1 = test_block(
        1,
        vec![
            create,
            Transaction::call("sc1alice", &contract, vec![], 0, 100_000, 1, CHAIN_ID_REGTEST),
        ],
    );
    let after_first = {
        let mut vm = WasmiExecutor::new();
        apply_block_with_vm(&state, &block1, CHAIN_ID_REGTEST, &mut vm).expect("block1")
    };

    let block2 = test_block(
        2,
        vec![Transaction::call(
            "sc1alice",
            &contract,
            vec![],
            0,
            100_000,
            2,
            CHAIN_ID_REGTEST,
        )],
    );
    let mut vm = WasmiExecutor::new();
    // The code registry is per-executor; the node threads it via
    // `view.codes`, so re-register here for the second block.
    vm.insert_code(&contract, read_write_wat().into_bytes());
    let after_second =
        apply_block_with_vm(&after_first, &block2, CHAIN_ID_REGTEST, &mut vm).expect("block2");

    assert_eq!(after_second.receipts[1].status, TxStatus::Success);
    let storage = after_second.contract_storage.get(&contract).expect("flushed");
    assert_eq!(storage[&KEY_A], b"hello");
    assert_eq!(
        storage[&KEY_B],
        b"hello",
        "second frame must read the persisted KEY_A written by the first call"
    );
}

#[test]
fn unapply_restores_wasmi_storage_roundtrip() {
    let state = funded_state();
    let wasm = read_write_wat();
    let create = Transaction::create("sc1alice", wasm.into_bytes(), vec![], 0, 0, CHAIN_ID_REGTEST);
    let contract = derive(0);
    let block1 = test_block(
        1,
        vec![
            create,
            Transaction::call("sc1alice", &contract, vec![], 0, 100_000, 1, CHAIN_ID_REGTEST),
        ],
    );
    let after_first = {
        let mut vm = WasmiExecutor::new();
        apply_block_with_vm(&state, &block1, CHAIN_ID_REGTEST, &mut vm).expect("block1")
    };

    let block2 = test_block(
        2,
        vec![Transaction::call(
            "sc1alice",
            &contract,
            vec![],
            0,
            100_000,
            2,
            CHAIN_ID_REGTEST,
        )],
    );
    let after_second = {
        let mut vm = WasmiExecutor::new();
        vm.insert_code(&contract, read_write_wat().into_bytes());
        apply_block_with_vm(&after_first, &block2, CHAIN_ID_REGTEST, &mut vm).expect("block2")
    };
    assert_eq!(after_second.receipts[1].status, TxStatus::Success);
    assert_eq!(after_second.contract_storage[&contract][&KEY_B], b"hello");

    let restored_to_first = unapply_block(&after_second, &block2, CHAIN_ID_REGTEST).unwrap();
    assert_eq!(
        restored_to_first.contract_storage.get(&contract),
        after_first.contract_storage.get(&contract),
        "unapply must restore the exact pre-call storage map"
    );
    assert_eq!(
        SparseMerkleTrie::compute_root(&restored_to_first.balances),
        SparseMerkleTrie::compute_root(&after_first.balances)
    );

    let restored_to_genesis = unapply_block(&restored_to_first, &block1, CHAIN_ID_REGTEST).unwrap();
    assert_eq!(restored_to_genesis, state);
}

#[test]
fn nested_call_writes_into_callee_scope() {
    let state = funded_state();
    let callee = derive(0);
    let caller_contract = derive(1);

    let callee_wasm = write_own_storage_wat();
    let caller_wasm = nested_call_wat(&callee);

    let block = test_block(
        1,
        vec![
            Transaction::create(
                "sc1alice",
                callee_wasm.into_bytes(),
                vec![],
                0,
                0,
                CHAIN_ID_REGTEST,
            ),
            Transaction::create(
                "sc1alice",
                caller_wasm.into_bytes(),
                vec![],
                0,
                1,
                CHAIN_ID_REGTEST,
            ),
            Transaction::call(
                "sc1alice",
                &caller_contract,
                vec![],
                0,
                300_000,
                2,
                CHAIN_ID_REGTEST,
            ),
        ],
    );

    let mut vm = WasmiExecutor::new();
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm).expect("apply");

    let call_receipt = &applied.receipts[3];
    assert_eq!(
        call_receipt.status,
        TxStatus::Success,
        "nested call into the storage-writing callee must succeed"
    );

    let callee_storage = applied
        .contract_storage
        .get(&callee)
        .expect("callee storage flushed");
    assert_eq!(callee_storage[&KEY_A], vec![7, 7, 7]);
    assert_eq!(
        applied
            .get_account(&callee)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root,
        contract_storage_root(callee_storage)
    );

    assert!(
        applied
            .contract_storage
            .get(&caller_contract)
            .map(|storage| storage.is_empty())
            .unwrap_or(true),
        "the outer contract's own storage stays untouched"
    );
    assert_eq!(
        applied
            .get_account(&caller_contract)
            .unwrap()
            .as_contract()
            .unwrap()
            .storage_root,
        [0u8; 32]
    );
}

#[test]
fn storage_write_over_cap_rejected() {
    use strangecoin_core::vm::limits::MAX_STORAGE_PER_CONTRACT;

    // 64 pages of memory so the write can carry a > 1 MB value; the cap
    // trips inside `storage_write` before any allocation of that size
    // reaches the overlay.
    let big = MAX_STORAGE_PER_CONTRACT + 1;
    let wat = wat(
        &[],
        &format!(
            r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (memory (export "memory") 17)
        (func (export "main") (result i32)
          (call $w (i32.const 0) (i32.const 0) (i32.const {big})))"#
        ),
    );
    // The module body declares its own 17-page memory; strip the helper's
    // default single-page declaration to avoid a duplicate export clash.
    let wat = wat.replace("(memory (export \"memory\") 1)", "");

    let state = funded_state();
    let create = Transaction::create(
        "sc1alice",
        wat.into_bytes(),
        vec![],
        0,
        0,
        CHAIN_ID_REGTEST,
    );
    let contract = derive(0);
    let block = test_block(
        1,
        vec![
            create,
            Transaction::call(
                "sc1alice",
                &contract,
                vec![],
                0,
                2_000_000,
                1,
                CHAIN_ID_REGTEST,
            ),
        ],
    );

    let mut vm = WasmiExecutor::new();
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm)
        .expect("failed write still yields a receipt");
    assert_eq!(applied.receipts[2].status, TxStatus::Failed);
    assert!(
        !applied.contract_storage.contains_key(&contract),
        "capped write must not flush"
    );
}

#[test]
fn storage_roots_deterministic_across_replay() {
    let state = funded_state();
    let wasm = read_write_wat();
    let create = Transaction::create("sc1alice", wasm.into_bytes(), vec![], 0, 0, CHAIN_ID_REGTEST);
    let contract = derive(0);
    let block = test_block(
        1,
        vec![
            create,
            Transaction::call("sc1alice", &contract, vec![], 0, 100_000, 1, CHAIN_ID_REGTEST),
        ],
    );

    let first = {
        let mut vm = WasmiExecutor::new();
        apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm).unwrap()
    };
    let second = {
        let mut vm = WasmiExecutor::new();
        apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm).unwrap()
    };
    assert_eq!(
        SparseMerkleTrie::compute_root(&first.balances),
        SparseMerkleTrie::compute_root(&second.balances)
    );
    assert_eq!(first.contract_storage, second.contract_storage);
}
