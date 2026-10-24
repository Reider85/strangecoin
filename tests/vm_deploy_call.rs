//! S1.5-P17: end-to-end deploy → call → event → receipt → storage on the
//! full component pipeline (signatures, gas, VM, receipts, state root).
//!
//! Regression targets: invariant #18 (event log / Receipt), invariant #19
//! extension (contract storage folds into the state root, S1.5-P15).

#![cfg(feature = "vm-wasm")]

mod common;

use std::collections::HashMap;

use common::vm_helpers::{
    child_block, commit_block, derive, execution_context, init_chain, keypair, sign, wat,
};
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::serialize::txid;
use strangecoin_core::state::compute_state_root;
use strangecoin_core::types::Transaction;
use strangecoin_core::vm::TxStatus;

const KEY: [u8; 32] = [0x01u8; 32];
const INIT_KEY: [u8; 32] = [0x07u8; 32];
const TOPIC: [u8; 32] = [0xaau8; 32];

/// Storage-writing, event-emitting contract that returns 42.
fn storage_log_ret42_wat() -> String {
    wat(
        &[(0, &KEY), (32, b"val!"), (64, &TOPIC), (96, b"evnt")],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (import "sc" "emit_log" (func $l (param i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (drop (call $w (i32.const 0) (i32.const 32) (i32.const 4)))
          (drop (call $l (i32.const 64) (i32.const 1) (i32.const 96) (i32.const 4)))
          (i32.const 42))"#,
    )
}

#[test]
fn deploy_call_event_receipt_storage_end_to_end() {
    let (alice, alice_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes: HashMap<String, Vec<u8>> = HashMap::new();

    // --- Block 1: deploy (CreateTx) ------------------------------------
    let init_code = storage_log_ret42_wat().into_bytes();
    let create = sign(
        Transaction::create(&alice, init_code.clone(), vec![], 100, 0, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let contract = derive(&alice, 0);
    let create_block = child_block(chain.last().unwrap(), vec![create.clone()]);
    let post_create = commit_block(&mut chain, &state, create_block, &codes);
    state = post_create;
    codes.insert(contract.clone(), init_code.clone());

    let account = state.get_account(&contract).expect("contract account");
    assert!(
        account.is_contract(),
        "Create must install a contract account"
    );
    let contract_acc = account.as_contract().unwrap();
    assert_eq!(
        contract_acc.code_hash,
        *blake3::hash(&init_code).as_bytes(),
        "code_hash must commit to the deployed init_code"
    );
    assert_eq!(contract_acc.balance, 100, "endowment credited on deploy");
    assert_eq!(state.get_balance(&alice), 9_900);
    assert_eq!(state.get_nonce(&alice), 1);

    let create_receipt = &state.receipts[1];
    assert_eq!(create_receipt.txid, txid(&create));
    assert_eq!(create_receipt.status, TxStatus::Success);
    assert_eq!(create_receipt.gas_used, 0, "deploy reserves no gas");
    assert!(create_receipt.logs.is_empty());

    // --- Block 2: call (CallTx) — event + storage + return value --------
    let root_before_call = compute_state_root(&state.balances);
    let call = sign(
        Transaction::call(&alice, &contract, vec![], 10, 200_000, 1, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call.clone()]);
    let post_call = commit_block(&mut chain, &state, call_block, &codes);
    state = post_call;

    let call_receipt = &state.receipts[1];
    assert_eq!(call_receipt.txid, txid(&call));
    assert_eq!(call_receipt.status, TxStatus::Success);
    assert!(
        call_receipt.gas_used > 0 && call_receipt.gas_used <= 200_000,
        "gas_used must be metered within the tx budget, got {}",
        call_receipt.gas_used
    );

    // Invariant #18: the emitted log lands in the receipt with matching
    // address, topics and data.
    assert_eq!(call_receipt.logs.len(), 1);
    let log = &call_receipt.logs[0];
    assert_eq!(log.address, contract);
    assert_eq!(log.topics, vec![TOPIC]);
    assert_eq!(log.data, b"evnt");

    // Balance movement: endowment 100 + call value 10.
    let account = state.get_account(&contract).expect("contract account");
    assert_eq!(account.balance(), 110);
    assert_eq!(state.get_balance(&alice), 9_890);

    // Invariant #19 extension: storage flushed, storage_root moved, and the
    // block header commits to the new global state root.
    let contract_acc = account.as_contract().unwrap();
    assert_ne!(
        contract_acc.storage_root, [0u8; 32],
        "storage_root must reflect the flushed overlay"
    );
    let stored = state
        .contract_storage
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&contract)
        .and_then(|storage| storage.get(&KEY))
        .cloned()
        .expect("slot must be persisted");
    assert_eq!(stored, b"val!");

    let block = chain.last().unwrap();
    assert_ne!(
        block.state_root, root_before_call,
        "state_root must change with contract storage (invariant #19)"
    );
    assert_eq!(
        block.state_root,
        compute_state_root(&state.balances),
        "header state_root must equal the post-state root"
    );

    // Return value: `main`'s i32 surfaces as LE `return_data` (the receipt
    // itself carries no return payload — pin the convention directly on the
    // executor with the same module).
    let wasm = String::from_utf8(init_code.clone()).expect("wat is utf-8");
    let vm = strangecoin_vm_wasm::WasmiExecutor::new();
    let result = vm
        .execute_wasm(
            wasm.as_bytes(),
            &execution_context(&contract, vec![], 200_000),
        )
        .expect("deployed module must run");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(
        i32::from_le_bytes(result.return_data[..4].try_into().unwrap()),
        42
    );
}

/// Constructor writes an "initial supply" under `INIT_KEY`; `main` reads
/// it back and returns the byte count (S1.5-FBP04 end-to-end deploy-init).
fn constructor_init_supply_wat() -> String {
    wat(
        &[(0, &INIT_KEY), (32, b"init")],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (import "sc" "storage_read" (func $r (param i32 i32 i32) (result i32)))
        (func (export "constructor") (param i32 i32) (result i32)
          (drop (call $w (i32.const 0) (i32.const 32) (i32.const 4)))
          (i32.const 0))
        (func (export "main") (result i32)
          (call $r (i32.const 0) (i32.const 128) (i32.const 32)))"#,
    )
}

#[test]
fn deploy_constructor_initializes_storage_then_call_reads_it() {
    let (alice, alice_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes: HashMap<String, Vec<u8>> = HashMap::new();

    // --- Block 1: deploy with a constructor budget ------------------------
    let init_code = constructor_init_supply_wat().into_bytes();
    let create = sign(
        Transaction::create_with(
            &alice,
            init_code.clone(),
            vec![],
            vec![],
            100,
            200_000,
            0,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let contract = derive(&alice, 0);
    let create_block = child_block(chain.last().unwrap(), vec![create]);
    state = commit_block(&mut chain, &state, create_block, &codes);
    codes.insert(contract.clone(), init_code.clone());

    let create_receipt = &state.receipts[1];
    assert_eq!(create_receipt.status, TxStatus::Success);
    assert!(
        create_receipt.gas_used > 0 && create_receipt.gas_used <= 200_000,
        "constructor is metered within the create budget, got {}",
        create_receipt.gas_used
    );

    let stored = state
        .contract_storage
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&contract)
        .and_then(|storage| storage.get(&INIT_KEY))
        .cloned()
        .expect("constructor storage must persist through the full pipeline");
    assert_eq!(stored, b"init");
    let contract_acc = state.get_account(&contract).unwrap().as_contract().unwrap();
    assert_ne!(contract_acc.storage_root, [0u8; 32]);
    assert_eq!(contract_acc.balance, 100, "endowment moved on Success");
    assert_eq!(state.get_balance(&alice), 9_900);

    // --- Block 2: call `main` (empty calldata -> legacy fallback) ---------
    let call = sign(
        Transaction::call(&alice, &contract, vec![], 0, 200_000, 1, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call]);
    state = commit_block(&mut chain, &state, call_block, &codes);
    assert_eq!(state.receipts[1].status, TxStatus::Success);

    // The read path returns the 4-byte length written by the constructor
    // (seed the probe context with the persisted storage snapshot).
    let wasm = String::from_utf8(init_code.clone()).expect("wat is utf-8");
    let vm = strangecoin_vm_wasm::WasmiExecutor::new();
    let mut probe = execution_context(&contract, vec![], 200_000);
    probe.storage = state.contract_storage.clone();
    let result = vm
        .execute_wasm(wasm.as_bytes(), &probe)
        .expect("deployed module must run");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(
        i32::from_le_bytes(result.return_data[..4].try_into().unwrap()),
        4,
        "main reads the constructor-initialized slot"
    );
}
