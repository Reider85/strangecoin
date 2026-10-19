//! Events/logs → receipts integration (S1.5-P14, invariant #18):
//! `emit_log` output lands in a `Receipt`; failed frames discard logs;
//! receipts are deterministic and verifiable against the receipts sub-trie.

use std::collections::HashMap;

use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::state::sparse_merkle::SparseMerkleTrie;
use strangecoin_core::state::{
    apply_block_with_vm, prove_receipt, receipts_subroot, unapply_block, verify_receipt_proof,
};
use strangecoin_core::types::{Block, Transaction};
use strangecoin_core::vm::{ExecutionResult, Receipt, TxStatus, VmError};
use strangecoin_vm_wasm::{WasmiError, WasmiExecutor};

const TOPIC: [u8; 32] = [0xaau8; 32];
const INFINITE_LOOP_WAT: &str = r#"(module (func (export "main") (loop $l (br $l))))"#;
const TRAP_AFTER_LOG_BODY: &str = r#"
    (import "sc" "emit_log" (func $l (param i32 i32 i32 i32) (result i32)))
    (func (export "main") (result i32)
      (drop (call $l (i32.const 0) (i32.const 1) (i32.const 32) (i32.const 4)))
      (unreachable))"#;

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

const EMIT_LOG_WAT_BODY: &str = r#"
    (import "sc" "emit_log" (func $l (param i32 i32 i32 i32) (result i32)))
    (func (export "main") (result i32)
      (call $l (i32.const 0) (i32.const 1) (i32.const 32) (i32.const 4)))"#;

fn emit_log_wat() -> String {
    wat(&[(0, &TOPIC), (32, b"abcd")], EMIT_LOG_WAT_BODY)
}

#[test]
fn emit_log_lands_in_receipt_via_from_execution() {
    let executor = WasmiExecutor::new();
    let ctx = crate_context("sc1contract", 100_000);
    let result = executor
        .execute_wasm(emit_log_wat().as_bytes(), &ctx)
        .expect("emit_log module must run");
    assert_eq!(result.status, TxStatus::Success);

    let receipt = Receipt::from_execution([9u8; 32], 4, 2, &result);
    assert_eq!(receipt.logs.len(), 1);
    let log = &receipt.logs[0];
    assert_eq!(log.address, "sc1contract");
    assert_eq!(log.topics, vec![TOPIC]);
    assert_eq!(log.data, b"abcd");
    assert_eq!(receipt.gas_used, result.gas_used);
    assert_eq!(receipt.status, TxStatus::Success);
}

#[test]
fn max_topics_four_enforced_no_log_in_error_receipt() {
    let executor = WasmiExecutor::new();
    let five_topics = wat(
        &[(0, &TOPIC)],
        r#"
        (import "sc" "emit_log" (func $l (param i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $l (i32.const 0) (i32.const 5) (i32.const 32) (i32.const 0)))"#,
    );
    let ctx = crate_context("sc1contract", 100_000);
    let err = executor
        .execute_wasm(five_topics.as_bytes(), &ctx)
        .expect_err("5 topics must abort the frame");
    if let WasmiError::Engine(msg) = &err {
        assert!(msg.contains("at most 4 topics"), "msg: {msg}");
    } else {
        panic!("expected engine error, got {err:?}");
    }
    let vm_error: VmError = err.into();
    let receipt = Receipt::from_vm_error([1u8; 32], 1, 0, &vm_error, 100_000);
    assert!(receipt.logs.is_empty(), "aborted frame must not emit logs");
}

#[test]
fn out_of_gas_receipt_burns_full_budget() {
    let executor = WasmiExecutor::new();
    let ctx = crate_context("sc1contract", 10_000);
    let err = executor
        .execute_wasm(INFINITE_LOOP_WAT.as_bytes(), &ctx)
        .expect_err("infinite loop must be halted by fuel");
    assert!(matches!(err, WasmiError::OutOfFuel));
    let vm_error: VmError = err.into();
    let receipt = Receipt::from_vm_error([2u8; 32], 1, 0, &vm_error, 10_000);
    assert_eq!(receipt.status, TxStatus::OutOfGas);
    assert_eq!(receipt.gas_used, 10_000);
    assert!(receipt.logs.is_empty());
}

#[test]
fn trap_after_emit_discards_logs() {
    let executor = WasmiExecutor::new();
    let wasm = wat(&[(0, &TOPIC), (32, b"abcd")], TRAP_AFTER_LOG_BODY);
    let ctx = crate_context("sc1contract", 100_000);
    let err = executor
        .execute_wasm(wasm.as_bytes(), &ctx)
        .expect_err("unreachable must abort the frame");
    let vm_error: VmError = err.into();
    let receipt = Receipt::from_vm_error([3u8; 32], 1, 0, &vm_error, 100_000);
    assert!(receipt.logs.is_empty(), "failed frame discards logs");
    assert_ne!(receipt.status, TxStatus::Success);
}

#[test]
fn receipts_are_deterministic() {
    let executor = WasmiExecutor::new();
    let ctx = crate_context("sc1contract", 100_000);
    let r1 = executor
        .execute_wasm(emit_log_wat().as_bytes(), &ctx)
        .expect("first run");
    let r2 = executor
        .execute_wasm(emit_log_wat().as_bytes(), &ctx)
        .expect("second run");
    assert_eq!(r1, r2);
    let receipt1 = Receipt::from_execution([4u8; 32], 1, 0, &r1);
    let receipt2 = Receipt::from_execution([4u8; 32], 1, 0, &r2);
    assert_eq!(receipt1, receipt2);
}

// ---------------------------------------------------------------------------
// end-to-end through core: Create + Call(emit_log) in one block (S1.5-P14)
// ---------------------------------------------------------------------------

fn crate_context(callee: &str, gas_limit: u64) -> strangecoin_core::vm::ExecutionContext {
    use strangecoin_core::types::{AccountState, BlockHeader};
    use strangecoin_core::vm::{BlockContext, ExecutionContext};

    let mut accounts = HashMap::new();
    accounts.insert(callee.to_string(), AccountState::eoa(1_000, 0));
    ExecutionContext {
        block: BlockHeader {
            index: 1,
            timestamp: 1_700_000_000,
            previous_hash: "0".repeat(64),
            hash: "0".repeat(64),
            nonce: 0,
            target: "ffff".to_string(),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        },
        accounts,
        caller: "sc1caller".to_string(),
        callee: callee.to_string(),
        value: 0,
        gas_limit,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height: 7,
            timestamp: 1_700_000_000,
            coinbase: "sc1coinbase".to_string(),
            block_gas_limit: strangecoin_core::vm::BLOCK_GAS_LIMIT,
        },
        storage: Default::default(),
        storage_overlay: HashMap::new(),
        storage_address: callee.to_string(),
        input: Vec::new(),
    }
}

fn test_block(index: u64, transactions: Vec<Transaction>) -> Block {
    Block {
        index,
        timestamp: 1000 + index * 600,
        transactions,
        previous_hash: "prev_hash".to_string(),
        hash: format!("block_hash_{index}"),
        nonce: 0,
        target: "ff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

#[test]
fn apply_block_with_vm_collects_log_receipts_end_to_end() {
    let mut state = strangecoin_core::state::State::new();
    state.set_balance("sc1alice", 1_000);

    let wasm = emit_log_wat();
    let create = Transaction::create("sc1alice", wasm.into_bytes(), vec![], 100, 0, CHAIN_ID_REGTEST);
    let contract_addr = strangecoin_core::address::derive_contract_address(
        "sc1alice",
        0,
        CHAIN_ID_REGTEST,
    )
    .expect("derive address");
    let call = Transaction::call(
        "sc1alice",
        &contract_addr,
        vec![],
        10,
        100_000,
        1,
        CHAIN_ID_REGTEST,
    );
    let block = test_block(
        1,
        vec![
            Transaction::coinbase("coinbase", "sc1miner", 0, CHAIN_ID_REGTEST),
            create,
            call.clone(),
        ],
    );

    let mut vm = WasmiExecutor::new();
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm)
        .expect("block with create+call must apply");

    assert_eq!(applied.receipts.len(), 3, "coinbase + create + call");
    let call_receipt = &applied.receipts[2];
    assert_eq!(call_receipt.txid, strangecoin_core::serialize::txid(&call));
    assert_eq!(call_receipt.status, TxStatus::Success);
    assert!(
        call_receipt.gas_used > 0,
        "emit_log must consume gas in the receipt"
    );
    assert_eq!(call_receipt.logs.len(), 1);
    assert_eq!(call_receipt.logs[0].address, contract_addr);
    assert_eq!(call_receipt.logs[0].topics, vec![TOPIC]);
    assert_eq!(call_receipt.logs[0].data, b"abcd");

    let contract = applied.get_account(&contract_addr).expect("deployed");
    assert!(contract.is_contract());
    assert_eq!(contract.balance(), 110, "endowment 100 + value 10 on success");
    assert_eq!(applied.get_balance("sc1alice"), 890);

    let root = receipts_subroot(&applied.receipts);
    let proof = prove_receipt(&applied.receipts, &call_receipt.txid).expect("proof");
    assert!(verify_receipt_proof(&root, call_receipt, &proof));
    assert_eq!(root, receipts_subroot(&applied.receipts));
    assert_ne!(root, SparseMerkleTrie::empty_root());
}

#[test]
fn apply_block_with_vm_failed_call_receipt_and_value_not_transferred() {
    let mut state = strangecoin_core::state::State::new();
    state.set_balance("sc1alice", 1_000);

    let create = Transaction::create(
        "sc1alice",
        INFINITE_LOOP_WAT.as_bytes().to_vec(),
        vec![],
        0,
        0,
        CHAIN_ID_REGTEST,
    );
    let contract_addr = strangecoin_core::address::derive_contract_address(
        "sc1alice",
        0,
        CHAIN_ID_REGTEST,
    )
    .expect("derive address");
    let call = Transaction::call(
        "sc1alice",
        &contract_addr,
        vec![],
        25,
        5_000,
        1,
        CHAIN_ID_REGTEST,
    );
    let block = test_block(
        1,
        vec![
            Transaction::coinbase("coinbase", "sc1miner", 0, CHAIN_ID_REGTEST),
            create,
            call.clone(),
        ],
    );

    let mut vm = WasmiExecutor::new();
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm)
        .expect("failed call still produces a receipt");

    let call_receipt = &applied.receipts[2];
    assert_eq!(call_receipt.status, TxStatus::OutOfGas);
    assert_eq!(call_receipt.gas_used, 5_000);
    assert!(call_receipt.logs.is_empty());

    let contract = applied.get_account(&contract_addr).expect("deployed");
    assert_eq!(contract.balance(), 0, "value must not move on failure");
    assert_eq!(applied.get_balance("sc1alice"), 1_000);
}

#[test]
fn unapply_block_reverses_create_and_call() {
    let mut state = strangecoin_core::state::State::new();
    state.set_balance("sc1alice", 1_000);

    let create = Transaction::create(
        "sc1alice",
        emit_log_wat().into_bytes(),
        vec![],
        100,
        0,
        CHAIN_ID_REGTEST,
    );
    let contract_addr = strangecoin_core::address::derive_contract_address(
        "sc1alice",
        0,
        CHAIN_ID_REGTEST,
    )
    .expect("derive address");
    let call = Transaction::call(
        "sc1alice",
        &contract_addr,
        vec![],
        10,
        100_000,
        1,
        CHAIN_ID_REGTEST,
    );
    let block = test_block(
        1,
        vec![
            Transaction::coinbase("coinbase", "sc1miner", 0, CHAIN_ID_REGTEST),
            create,
            call,
        ],
    );

    let mut vm = WasmiExecutor::new();
    let applied = apply_block_with_vm(&state, &block, CHAIN_ID_REGTEST, &mut vm).expect("apply");
    assert!(applied.get_account(&contract_addr).is_some());
    assert!(!applied.receipts.is_empty());

    let restored = unapply_block(&applied, &block, CHAIN_ID_REGTEST).expect("unapply");
    assert_eq!(restored, state, "balances round-trip exactly");
    assert!(
        restored.get_account(&contract_addr).is_none(),
        "contract account removed on unapply"
    );
    assert!(restored.receipts.is_empty(), "tip receipts cleared on unapply");
}

#[test]
fn apply_block_without_vm_rejects_create() {
    let state = strangecoin_core::state::State::new();
    let create = Transaction::create("sc1alice", vec![0x00], vec![], 0, 0, CHAIN_ID_REGTEST);
    let block = test_block(
        1,
        vec![
            Transaction::coinbase("coinbase", "sc1miner", 0, CHAIN_ID_REGTEST),
            create,
        ],
    );
    let err = strangecoin_core::state::apply_block(&state, &block, CHAIN_ID_REGTEST)
        .expect_err("create without VM must be rejected");
    assert!(matches!(
        err,
        strangecoin_core::CoreError::UnsupportedTxType { .. }
    ));
}

#[test]
fn register_code_rejects_invalid_wasm() {
    use strangecoin_core::vm::VmExecutor;
    let mut vm = WasmiExecutor::new();
    let err = vm
        .register_code("sc1bad", b"not wasm at all".to_vec())
        .expect_err("invalid wasm must be rejected");
    assert!(matches!(err, VmError::HostFunctionError { .. }));
}

#[test]
fn execution_result_logs_clone_into_receipt_without_borrow() {
    let result = ExecutionResult {
        gas_used: 1,
        logs: vec![],
        status: TxStatus::Success,
        return_data: vec![],
        storage_overlay: HashMap::new(),
        value_transfers: Vec::new(),
    };
    let receipt = Receipt::from_execution([0u8; 32], 0, 0, &result);
    assert_eq!(receipt, Receipt::from_execution([0u8; 32], 0, 0, &result));
}
