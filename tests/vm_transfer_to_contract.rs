//! S1.5-P17: transfer SC to a contract address, then call a contract method
//! that *uses* the contract's balance (S1.5-P10 nested `sc.call` value).
//!
//! S1.5-FBP05: nested value transfers now **settle** — they move inside the
//! VM frame against the on-entry-credited account view, land in
//! `ExecutionResult::value_transfers`, and `apply_call` commits the whole
//! trail to `State` on `Success` (audit trail mirrored into the receipt,
//! undo record for `unapply`). The contract sees incoming value before its
//! code runs, so the withdraw pattern (receive then forward) works.

#![cfg(feature = "vm-wasm")]

mod common;

use std::collections::HashMap;

use common::vm_helpers::{
    child_block, commit_block, derive, init_chain, keypair, sign, wat,
};
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::{unapply_block, State};
use strangecoin_core::types::Transaction;
use strangecoin_core::vm::{TxStatus, ValueTransfer};

const STATUS_KEY: [u8; 32] = [0x75u8; 32];

/// Attempts to forward `value` to the bech32 target at offset 0 and persists
/// the nested-call status byte (0 = covered by the contract balance,
/// 2 = insufficient) under [`STATUS_KEY`].
///
/// Memory layout: target address at 0 (bech32 strings are ~60 chars), the
/// status slot key at 128 (S1.5-FBP05: previously at 32, which clobbered
/// the tail of the address — harmless before value settled, corrupting the
/// committed transfer target after), scratch at 96, out-buffer at 256.
fn forward_status_wat(target: &str, value: u64) -> String {
    let len = target.len();
    wat(
        &[(0, target.as_bytes()), (128, &STATUS_KEY)],
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
          (drop (call $w (i32.const 128) (i32.const 96) (i32.const 1)))
          (local.get $status))"#
        ),
    )
}

/// Forwards `value` to the bech32 target at offset 0 and returns 0 —
/// no status persistence (S1.5-FBP05 chain tests).
fn forward_wat(target: &str, value: u64) -> String {
    let len = target.len();
    wat(
        &[(0, target.as_bytes())],
        &format!(
            r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (drop (call $c (i32.const 0) (i32.const {len}) (i64.const {value})
                   (i32.const 0) (i32.const 0)
                   (i32.const 128) (i32.const 32)))
          (i32.const 0))"#
        ),
    )
}

/// Forwards `value` to the target, then traps unconditionally — used by the
/// failing-chain test so a failed child poisons every parent frame.
fn forward_then_trap_wat(target: &str, value: u64) -> String {
    let len = target.len();
    wat(
        &[(0, target.as_bytes())],
        &format!(
            r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (drop (call $c (i32.const 0) (i32.const {len}) (i64.const {value})
                   (i32.const 0) (i32.const 0)
                   (i32.const 128) (i32.const 32)))
          (unreachable))"#
        ),
    )
}

/// Trivial `main` that returns 0 — a terminal leg of a value chain.
fn noop_wat() -> String {
    wat(&[], r#"(func (export "main") (result i32) (i32.const 0))"#)
}

fn status_slot(state: &State, contract: &str) -> u8 {
    state
        .contract_storage
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    // S1.5-FBP05: the plain transfer receipt carries its single move.
    assert_eq!(
        state.receipts[1].value_transfers,
        vec![ValueTransfer {
            from: alice.clone(),
            to: spender.clone(),
            amount: 500,
        }]
    );

    // --- Block 3: method forwards value — the nested transfer settles ----
    // Outer value 10 moves EOA → contract on entry; the contract then
    // forwards 50 to bob from the balance it just received plus its own.
    let call = sign(
        Transaction::call(&alice, &spender, vec![], 10, 100_000, 3, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call.clone()]);
    state = commit_block(&mut chain, &state, call_block, &codes);

    assert_eq!(state.receipts[1].status, TxStatus::Success);
    assert_eq!(
        status_slot(&state, &spender),
        0,
        "forward of 50 is covered by the contract's 500 balance"
    );
    assert_eq!(
        state.get_balance(&bob),
        50,
        "nested value transfer settles: bob receives the forward (S1.5-FBP05)"
    );
    assert_eq!(
        state.get_account(&spender).unwrap().balance(),
        460,
        "contract keeps 500 + 10 (entry) - 50 (nested forward)"
    );
    assert_eq!(state.get_balance(&alice), 9_490);

    // Audit trail: the receipt lists every committed move in commit order,
    // and the undo map carries the same list for `unapply`.
    let call_txid = strangecoin_core::serialize::txid(&call);
    assert_eq!(
        state.receipts[1].value_transfers,
        vec![
            ValueTransfer {
                from: alice.clone(),
                to: spender.clone(),
                amount: 10,
            },
            ValueTransfer {
                from: spender.clone(),
                to: bob.clone(),
                amount: 50,
            },
        ]
    );
    assert_eq!(
        state.value_transfers_undo.get(&call_txid),
        Some(&state.receipts[1].value_transfers),
        "undo record mirrors the receipt trail"
    );

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
    assert!(probe_receipt.value_transfers.is_empty());
    assert_eq!(state.get_balance(&bob), 50, "no further value moved");
    assert_eq!(state.get_account(&broke).unwrap().balance(), 100);
}

/// S1.5-FBP05: EOA → A → B → C, each leg `value = 50`. The withdraw
/// pattern works: A and B forward what they receive on entry; every leg
/// lands in the receipt trail; `unapply` mirrors the whole chain back.
#[test]
fn nested_value_chain_a_to_b_to_c_settles_and_unapplies() {
    let (alice, alice_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes: HashMap<String, Vec<u8>> = HashMap::new();

    let contract_a = derive(&alice, 0);
    let contract_b = derive(&alice, 1);
    let contract_c = derive(&alice, 2);

    // --- Block 1: deploy the chain (A forwards to B, B forwards to C) ----
    let a_wat = forward_wat(&contract_b, 50);
    let b_wat = forward_wat(&contract_c, 50);
    let c_wat = noop_wat(); // C is the terminal leg — it never forwards
    let creates: Vec<Transaction> = [a_wat.as_bytes(), b_wat.as_bytes(), c_wat.as_bytes()]
        .iter()
        .enumerate()
        .map(|(i, wasm)| {
            sign(
                Transaction::create(
                    &alice,
                    wasm.to_vec(),
                    vec![],
                    0,
                    i as u64,
                    CHAIN_ID_REGTEST,
                ),
                &alice_sk,
            )
        })
        .collect();
    let deploy_block = child_block(chain.last().unwrap(), creates);
    state = commit_block(&mut chain, &state, deploy_block, &codes);
    codes.insert(contract_a.clone(), a_wat.into_bytes());
    codes.insert(contract_b.clone(), b_wat.into_bytes());
    codes.insert(contract_c.clone(), c_wat.into_bytes());

    // --- Block 2: EOA calls A with value 50; the chain forwards onward ---
    let call = sign(
        Transaction::call(&alice, &contract_a, vec![], 50, 200_000, 3, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call.clone()]);
    state = commit_block(&mut chain, &state, call_block, &codes);

    assert_eq!(state.receipts[1].status, TxStatus::Success);
    assert_eq!(state.get_balance(&alice), 9_950, "EOA pays the entry value");
    assert_eq!(state.get_balance(&contract_a), 0, "A: 50 in, 50 out");
    assert_eq!(state.get_balance(&contract_b), 0, "B: 50 in, 50 out");
    assert_eq!(state.get_balance(&contract_c), 50, "C keeps the last leg");

    let call_txid = strangecoin_core::serialize::txid(&call);
    let trail = vec![
        ValueTransfer {
            from: alice.clone(),
            to: contract_a.clone(),
            amount: 50,
        },
        ValueTransfer {
            from: contract_a.clone(),
            to: contract_b.clone(),
            amount: 50,
        },
        ValueTransfer {
            from: contract_b.clone(),
            to: contract_c.clone(),
            amount: 50,
        },
    ];
    assert_eq!(state.receipts[1].value_transfers, trail, "commit-order trail");
    assert_eq!(state.value_transfers_undo.get(&call_txid), Some(&trail));

    // --- Unapply: the whole chain unwinds in mirror order ----------------
    let restored = unapply_block(&state, chain.last().expect("chain not empty"), CHAIN_ID_REGTEST)
        .expect("call block must unapply");
    assert_eq!(restored.get_balance(&alice), 10_000);
    assert_eq!(restored.get_balance(&contract_a), 0);
    assert_eq!(restored.get_balance(&contract_b), 0);
    assert_eq!(restored.get_balance(&contract_c), 0);
    assert!(
        !restored.value_transfers_undo.contains_key(&call_txid),
        "undo record is consumed on unapply"
    );
}

/// S1.5-FBP05: a failed grandchild poisons every parent frame — no leg of
/// the value chain commits, the receipt carries no trail, and the undo map
/// has no record.
#[test]
fn failed_value_chain_commits_nothing() {
    let (alice, alice_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes: HashMap<String, Vec<u8>> = HashMap::new();

    let contract_a = derive(&alice, 0);
    let contract_b = derive(&alice, 1);

    // A forwards 50 to B then traps; B tries to forward 60 (more than the
    // 50 it receives — insufficient) then traps.
    let a_wat = forward_then_trap_wat(&contract_b, 50);
    let b_wat = forward_then_trap_wat(&contract_a, 60);
    let a_create = sign(
        Transaction::create(
            &alice,
            a_wat.clone().into_bytes(),
            vec![],
            0,
            0,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let b_create = sign(
        Transaction::create(
            &alice,
            b_wat.clone().into_bytes(),
            vec![],
            0,
            1,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let deploy_block = child_block(chain.last().unwrap(), vec![a_create, b_create]);
    state = commit_block(&mut chain, &state, deploy_block, &codes);
    codes.insert(contract_a.clone(), a_wat.into_bytes());
    codes.insert(contract_b.clone(), b_wat.into_bytes());

    let call = sign(
        Transaction::call(&alice, &contract_a, vec![], 50, 100_000, 2, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call.clone()]);
    state = commit_block(&mut chain, &state, call_block, &codes);

    let receipt = &state.receipts[1];
    assert_eq!(receipt.status, TxStatus::Failed, "trap surfaces as Failed");
    assert_eq!(receipt.gas_used, 100_000, "full budget burned");
    assert!(receipt.value_transfers.is_empty(), "no trail on failure");
    assert_eq!(state.get_balance(&alice), 10_000, "entry value not committed");
    assert_eq!(state.get_balance(&contract_a), 0);
    assert_eq!(state.get_balance(&contract_b), 0);
    let call_txid = strangecoin_core::serialize::txid(&call);
    assert!(
        !state.value_transfers_undo.contains_key(&call_txid),
        "no undo record for a failed frame"
    );
}
