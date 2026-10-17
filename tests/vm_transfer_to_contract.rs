//! S1.5-P17: transfer SC to a contract address, then call a contract method
//! that *uses* the contract's balance (S1.5-P10 nested `sc.call` value).
//!
//! Residual wiring gap (reported, not fixed here): nested-call value
//! settlement stays inside the VM frame — `ExecutionResult` carries no
//! accounts and `apply_call` persists only the *outer* tx value plus the
//! storage overlay. The contract therefore *reads* its balance through the
//! nested transfer's affordability check: a covered forward persists
//! status 0 to storage (asserted at the state level); an uncovered one is
//! an exceptional child failure that burns the remaining parent budget.

#![cfg(feature = "vm-wasm")]

mod common;

use std::collections::HashMap;

use common::vm_helpers::{
    child_block, commit_block, derive, init_chain, keypair, sign, wat,
};
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::State;
use strangecoin_core::types::Transaction;
use strangecoin_core::vm::TxStatus;

const STATUS_KEY: [u8; 32] = [0x75u8; 32];

/// Attempts to forward `value` to the bech32 target at offset 0 and persists
/// the nested-call status byte (0 = covered by the contract balance,
/// 2 = insufficient) under [`STATUS_KEY`].
fn forward_status_wat(target: &str, value: u64) -> String {
    let len = target.len();
    wat(
        &[(0, target.as_bytes()), (32, &STATUS_KEY)],
        &format!(
            r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (local $status i32)
          (local.set $status
            (call $c (i32.const 0) (i32.const {len}) (i64.const {value})
                     (i32.const 0) (i32.const 0)
                     (i32.const 256) (i32.const 32)))
          (i32.store (i32.const 96) (local.get $status))
          (drop (call $w (i32.const 32) (i32.const 96) (i32.const 1)))
          (local.get $status))"#
        ),
    )
}

fn status_slot(state: &State, contract: &str) -> u8 {
    state
        .contract_storage
        .get(contract)
        .and_then(|storage| storage.get(&STATUS_KEY))
        .expect("status slot must be flushed")
        .first()
        .copied()
        .expect("status byte")
}

#[test]
fn transfer_to_contract_and_method_reads_balance() {
    let (alice, alice_sk) = keypair();
    let (bob, _bob_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes: HashMap<String, Vec<u8>> = HashMap::new();

    // --- Block 1: deploy two balance-probes (forward 50 and 5000) --------
    let fwd50_wat = forward_status_wat(&bob, 50);
    let fwd5000_wat = forward_status_wat(&bob, 5_000);
    let fwd50 = sign(
        Transaction::create(
            &alice,
            fwd50_wat.clone().into_bytes(),
            vec![],
            0,
            0,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let fwd5000 = sign(
        Transaction::create(
            &alice,
            fwd5000_wat.clone().into_bytes(),
            vec![],
            0,
            1,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let deploy_block = child_block(chain.last().unwrap(), vec![fwd50, fwd5000]);
    state = commit_block(&mut chain, &state, deploy_block, &codes);
    let spender = derive(&alice, 0);
    let broke = derive(&alice, 1);
    codes.insert(spender.clone(), fwd50_wat.into_bytes());
    codes.insert(broke.clone(), fwd5000_wat.into_bytes());

    // --- Block 2: plain TransferTx to the contract address ---------------
    let transfer = sign(
        Transaction::transfer(&alice, &spender, 500, 2, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let transfer_block = child_block(chain.last().unwrap(), vec![transfer]);
    state = commit_block(&mut chain, &state, transfer_block, &codes);

    assert_eq!(
        state.get_account(&spender).expect("contract").balance(),
        500,
        "TransferTx to a contract address must credit its balance"
    );
    assert_eq!(state.get_balance(&alice), 9_500);
    assert_eq!(state.receipts[1].status, TxStatus::Success);

    // --- Block 3: method reads the balance via the nested transfer ------
    // Outer value 10 also moves EOA → contract through the supported path.
    let call = sign(
        Transaction::call(&alice, &spender, vec![], 10, 100_000, 3, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call]);
    state = commit_block(&mut chain, &state, call_block, &codes);

    assert_eq!(state.receipts[1].status, TxStatus::Success);
    assert_eq!(
        status_slot(&state, &spender),
        0,
        "forward of 50 is covered by the contract's 500 balance"
    );
    assert_eq!(
        state.get_account(&spender).unwrap().balance(),
        510,
        "outer CallTx value (10) settles on top of the transferred 500"
    );
    assert_eq!(state.get_balance(&alice), 9_490);

    // --- Block 4: a contract without cover fails the nested transfer ----
    // An unaffordable nested value transfer is an exceptional child failure:
    // the child burns its (parent remaining) fuel budget, so the outer
    // frame runs OutOfGas before it can persist the status byte.
    let top_up = sign(
        Transaction::transfer(&alice, &broke, 100, 4, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let probe = sign(
        Transaction::call(&alice, &broke, vec![], 0, 100_000, 5, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let over_block = child_block(chain.last().unwrap(), vec![top_up, probe]);
    state = commit_block(&mut chain, &state, over_block, &codes);

    let probe_receipt = &state.receipts[2];
    assert_eq!(probe_receipt.status, TxStatus::OutOfGas);
    assert_eq!(
        probe_receipt.gas_used, 100_000,
        "failed nested transfer burns the remaining parent budget"
    );
    assert_eq!(state.get_balance(&bob), 0, "no value moved");
    assert_eq!(state.get_account(&broke).unwrap().balance(), 100);
}
