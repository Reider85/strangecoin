//! S1.5-P17: reentrancy attack end-to-end (threat V-18) — a malicious
//! contract re-enters a `nonReentrant` method through the real block
//! pipeline and is rejected with `ReentrancyDetected`.
//!
//! Core-level scenario: the node path does not propagate contract ABIs into
//! the executor yet, so the ABI is registered on the executor by hand
//! (residual gap, candidate for Stage 2 wiring).

#![cfg(feature = "vm-wasm")]

mod common;

use common::vm_helpers::{core_block, derive, execution_context, wat};
use strangecoin_core::abi::{AbiEntry, ContractAbi};
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::{apply_block_with_vm, State};
use strangecoin_core::types::Transaction;
use strangecoin_core::vm::{TxStatus, VmError, VmExecutor};
use strangecoin_vm_wasm::WasmiExecutor;

const SEL: [u8; 4] = [1, 2, 3, 4];
const ALICE: &str = "rsc1alice";

/// Storage-guarded self-caller: first run writes the guard key, then
/// re-enters itself with [`SEL`]; later frames return 0 without calling.
/// The bech32 target (~45 bytes) lives at 64..109, so the selector sits at
/// 160 to avoid overlapping data segments. Dual-exported `main`/`guarded`
/// so the nested call's selector dispatch (S1.5-FBP03) resolves the same
/// function body.
fn guarded_wat(target: &[u8]) -> String {
    let target_len = target.len();
    wat(
        &[(64, target), (160, &SEL)],
        &format!(
            r#"
        (import "sc" "storage_read" (func $read (param i32 i32 i32) (result i32)))
        (import "sc" "storage_write" (func $write (param i32 i32 i32) (result i32)))
        (import "sc" "call" (func $call (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (export "guarded") (result i32)
          (if (i32.eqz (call $read (i32.const 0) (i32.const 128) (i32.const 32)))
            (then
              (drop (call $write (i32.const 0) (i32.const 128) (i32.const 4)))
              (return (call $call (i32.const 64) (i32.const {target_len}) (i64.const 0)
                                  (i32.const 160) (i32.const 4)
                                  (i32.const 256) (i32.const 32)))))
          (i32.const 0))"#
        ),
    )
}

fn guarded_abi(non_reentrant: bool) -> ContractAbi {
    ContractAbi::new(vec![AbiEntry::new("guarded", SEL, non_reentrant)])
}

#[test]
fn reentrancy_attack_end_to_end_is_rejected() {
    let mut state = State::new();
    state.set_balance(ALICE, 10_000);
    let contract = derive(ALICE, 0);

    let create = Transaction::create(
        ALICE,
        guarded_wat(contract.as_bytes()).into_bytes(),
        vec![],
        100,
        0,
        CHAIN_ID_REGTEST,
    );
    let call = Transaction::call(
        ALICE,
        &contract,
        SEL.to_vec(),
        10,
        100_000,
        1,
        CHAIN_ID_REGTEST,
    );

    let mut vm = WasmiExecutor::new();
    vm.insert_abi(&contract, guarded_abi(true));

    let block = core_block(1, vec![create, call]);
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm)
        .expect("attack block must apply (the call fails, a receipt is produced)");

    // Receipt view of the abort: ReentrancyDetected is not OutOfGas, so
    // `from_vm_error` marks the tx Failed and burns the full budget.
    let receipt = &applied.receipts[2];
    assert_eq!(receipt.status, TxStatus::Failed);
    assert_eq!(receipt.gas_used, 100_000, "full gas budget is burned");
    assert!(receipt.logs.is_empty(), "aborted frame must not emit logs");

    // Value stays put: the contract keeps only its endowment (100), the
    // call's value of 10 is not moved on the abort.
    let contract_acc = applied.get_account(&contract).expect("deployed");
    assert_eq!(contract_acc.balance(), 100);
    assert_eq!(applied.get_balance(ALICE), 9_900);

    // Pin the exact error variant through the direct executor path.
    let err = vm
        .execute(execution_context(&contract, SEL.to_vec(), 100_000))
        .expect_err("guarded self-call must be rejected");
    assert_eq!(VmError::from(err), VmError::ReentrancyDetected);
}

#[test]
fn reentrancy_without_non_reentrant_flag_runs_and_terminates() {
    // The same attack code is legal when the ABI entry is not flagged —
    // the storage guard terminates the recursion and the tx succeeds.
    let mut state = State::new();
    state.set_balance(ALICE, 10_000);
    let contract = derive(ALICE, 0);

    let create = Transaction::create(
        ALICE,
        guarded_wat(contract.as_bytes()).into_bytes(),
        vec![],
        100,
        0,
        CHAIN_ID_REGTEST,
    );
    let call = Transaction::call(
        ALICE,
        &contract,
        SEL.to_vec(),
        10,
        100_000,
        1,
        CHAIN_ID_REGTEST,
    );

    let mut vm = WasmiExecutor::new();
    vm.insert_abi(&contract, guarded_abi(false));

    let block = core_block(1, vec![create, call]);
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm)
        .expect("unguarded self-call must apply");

    let receipt = &applied.receipts[2];
    assert_eq!(receipt.status, TxStatus::Success);
    assert!(
        receipt.gas_used < 100_000,
        "storage-guarded recursion terminates early, gas_used={}",
        receipt.gas_used
    );
    assert_eq!(applied.get_account(&contract).unwrap().balance(), 110);
}
