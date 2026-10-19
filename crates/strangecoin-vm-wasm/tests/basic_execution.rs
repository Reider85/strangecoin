use std::collections::HashMap;

use strangecoin_core::abi::{AbiEntry, ContractAbi};
use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext, Opcode, TxStatus, VmExecutor, VmError};
use strangecoin_vm_wasm::{WasmiError, WasmiExecutor};

const RETURN_42_WAT: &str = r#"(module (func (export "main") (result i32) i32.const 42))"#;
const NO_RESULT_WAT: &str = r#"(module (func (export "main")))"#;
const INFINITE_LOOP_WAT: &str = r#"(module (func (export "main") (loop $l (br $l))))"#;

fn block_header() -> BlockHeader {
    BlockHeader {
        index: 1,
        timestamp: 1_700_000_000,
        previous_hash: "0".repeat(64),
        hash: "0".repeat(64),
        nonce: 0,
        target: "ffff".to_string(),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
    }
}

fn execution_context(callee: &str, gas_limit: u64) -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert(callee.to_string(), AccountState::default());
    ExecutionContext {
        block: block_header(),
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
            height: 1,
            timestamp: 1_700_000_000,
            coinbase: "sc1coinbase".to_string(),
            block_gas_limit: 1_000_000,
        },
        storage: Default::default(),
        storage_overlay: HashMap::new(),
        storage_address: callee.to_string(),
        input: Vec::new(),
    }
}

#[test]
fn execute_wasm_returns_42_and_consumes_gas() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let result = executor
        .execute_wasm(RETURN_42_WAT.as_bytes(), &ctx)
        .expect("module should execute");

    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, 42i32.to_le_bytes());
    assert!(result.gas_used > 0);
    assert!(result.gas_used <= ctx.gas_limit);
    assert!(result.logs.is_empty());
}

#[test]
fn execute_wasm_void_entry_point() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let result = executor
        .execute_wasm(NO_RESULT_WAT.as_bytes(), &ctx)
        .expect("module should execute");

    assert_eq!(result.status, TxStatus::Success);
    assert!(result.return_data.is_empty());
    assert!(result.gas_used > 0);
}

#[test]
fn trait_execute_via_code_registry() {
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1contract", RETURN_42_WAT.as_bytes().to_vec());

    let ctx = execution_context("sc1contract", 100_000);
    let result: Result<_, WasmiError> = VmExecutor::execute(&executor, ctx);
    let result = result.expect("registered code should execute");

    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, 42i32.to_le_bytes());
    assert!(result.gas_used > 0);
}

#[test]
fn missing_code_maps_to_vm_error() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1unknown", 100_000);
    let err: VmError = VmExecutor::execute(&executor, ctx)
        .expect_err("unregistered callee must fail")
        .into();

    assert!(matches!(
        err,
        VmError::HostFunctionError { ref name, .. } if name == "execute"
    ));
}

#[test]
fn infinite_loop_runs_out_of_gas() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 10_000);
    let err = executor
        .execute_wasm(INFINITE_LOOP_WAT.as_bytes(), &ctx)
        .expect_err("infinite loop must be halted by fuel");

    assert!(matches!(err, WasmiError::OutOfFuel));
    let vm_err: VmError = err.into();
    assert_eq!(vm_err, VmError::OutOfGas);
}

#[test]
fn missing_main_export_is_error() {
    let executor = WasmiExecutor::new();
    let wat = r#"(module (func (export "other")))"#;
    let ctx = execution_context("sc1contract", 100_000);
    let err = executor
        .execute_wasm(wat.as_bytes(), &ctx)
        .expect_err("module without main export must fail");
    assert!(matches!(err, WasmiError::MissingExport(_)));
}

#[test]
fn gas_usage_is_deterministic() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let first = executor
        .execute_wasm(RETURN_42_WAT.as_bytes(), &ctx)
        .expect("first run");
    let second = executor
        .execute_wasm(RETURN_42_WAT.as_bytes(), &ctx)
        .expect("second run");
    assert_eq!(first.gas_used, second.gas_used);
    assert_eq!(first.return_data, second.return_data);
}

#[test]
fn gas_cost_matches_adr_0014_sketch() {
    let executor = WasmiExecutor::new();
    assert_eq!(executor.gas_cost(Opcode::Instruction), 1);
    assert_eq!(executor.gas_cost(Opcode::HostCall), 1);
    assert_eq!(executor.gas_cost(Opcode::MemoryGrow), 3);
    assert_eq!(executor.gas_cost(Opcode::Other), 1);
}

// ---------------------------------------------------------------------------
// Selector-based dispatch (S1.5-FBP03)
// ---------------------------------------------------------------------------

fn execution_context_with_input(callee: &str, gas_limit: u64, input: Vec<u8>) -> ExecutionContext {
    let mut ctx = execution_context(callee, gas_limit);
    ctx.input = input;
    ctx
}

fn abi_entry(name: &str) -> AbiEntry {
    AbiEntry::new(name, AbiEntry::selector_from_name(name), false)
}

/// Two dispatchable exports plus a legacy `main`; each export ignores its
/// arguments and returns a distinct marker.
const TWO_FUNCS_WAT: &str = r#"(module
    (memory (export "memory") 1)
    (func (export "foo") (param i32 i32) (result i32) i32.const 111)
    (func (export "bar") (param i32 i32) (result i32) i32.const 222)
    (func (export "main") (result i32) i32.const 0))"#;

fn two_funcs_executor() -> WasmiExecutor {
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1contract", TWO_FUNCS_WAT.as_bytes().to_vec());
    executor.insert_abi(
        "sc1contract",
        ContractAbi::new(vec![abi_entry("foo"), abi_entry("bar")]),
    );
    executor
}

#[test]
fn selector_dispatch_routes_to_matched_export() {
    let executor = two_funcs_executor();

    let ctx = execution_context_with_input(
        "sc1contract",
        100_000,
        AbiEntry::selector_from_name("foo").to_vec(),
    );
    let result = VmExecutor::execute(&executor, ctx).expect("foo dispatch");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, 111i32.to_le_bytes());

    let ctx = execution_context_with_input(
        "sc1contract",
        100_000,
        AbiEntry::selector_from_name("bar").to_vec(),
    );
    let result = VmExecutor::execute(&executor, ctx).expect("bar dispatch");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, 222i32.to_le_bytes());
}

#[test]
fn selector_dispatch_unknown_selector_falls_back_to_main() {
    let executor = two_funcs_executor();
    let ctx = execution_context_with_input("sc1contract", 100_000, vec![0xde, 0xad, 0xbe, 0xef]);
    let result = VmExecutor::execute(&executor, ctx).expect("fallback to main");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, 0i32.to_le_bytes());
}

#[test]
fn selector_dispatch_short_calldata_falls_back_to_main() {
    let executor = two_funcs_executor();
    let ctx = execution_context_with_input("sc1contract", 100_000, vec![0x01, 0x02]);
    let result = VmExecutor::execute(&executor, ctx).expect("fallback to main");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, 0i32.to_le_bytes());
}

#[test]
fn selector_dispatch_missing_export_is_error() {
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1contract", TWO_FUNCS_WAT.as_bytes().to_vec());
    executor.insert_abi("sc1contract", ContractAbi::new(vec![abi_entry("baz")]));
    let ctx = execution_context_with_input(
        "sc1contract",
        100_000,
        AbiEntry::selector_from_name("baz").to_vec(),
    );
    let err = VmExecutor::execute(&executor, ctx).expect_err("entry without export must fail");
    assert!(matches!(err, WasmiError::MissingExport(name) if name == "baz"));
}

/// Legacy zero-argument entry reachable through selector dispatch: reads
/// the frame calldata via `sc::input_read` and returns its length.
const INPUT_READ_WAT: &str = r#"(module
    (import "sc" "input_read" (func $ir (param i32 i32) (result i32)))
    (memory (export "memory") 1)
    (func (export "main") (export "readcalldata") (result i32)
      (call $ir (i32.const 0) (i32.const 64))))"#;

#[test]
fn input_read_exposes_calldata_to_legacy_main() {
    let executor = WasmiExecutor::new();
    let input = vec![7u8; 9];
    let ctx = execution_context_with_input("sc1contract", 100_000, input.clone());
    let result = executor
        .execute_wasm(INPUT_READ_WAT.as_bytes(), &ctx)
        .expect("input_read");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, (input.len() as i32).to_le_bytes());
}

#[test]
fn selector_dispatch_calls_zero_arg_export_with_input_read() {
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1contract", INPUT_READ_WAT.as_bytes().to_vec());
    executor.insert_abi("sc1contract", ContractAbi::new(vec![abi_entry("readcalldata")]));
    let mut input = AbiEntry::selector_from_name("readcalldata").to_vec();
    input.extend_from_slice(&[0xaa; 4]);
    let ctx = execution_context_with_input("sc1contract", 100_000, input.clone());
    let result = VmExecutor::execute(&executor, ctx).expect("dispatch to () export");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, (input.len() as i32).to_le_bytes());
}

/// Returns the first payload byte (calldata[4]) handed over via the
/// `(ptr, len)` argument pair — proves the host staged the bytes into
/// linear memory; a short calldata yields -1.
const PEEK_WAT: &str = r#"(module
    (memory (export "memory") 1)
    (func (export "peek") (param $ptr i32) (param $len i32) (result i32)
      (if (i32.lt_u (local.get $len) (i32.const 5))
        (then (return (i32.const -1))))
      (i32.load8_u (i32.add (local.get $ptr) (i32.const 4)))))"#;

fn peek_executor() -> WasmiExecutor {
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1contract", PEEK_WAT.as_bytes().to_vec());
    executor.insert_abi("sc1contract", ContractAbi::new(vec![abi_entry("peek")]));
    executor
}

#[test]
fn selector_dispatch_stages_calldata_into_memory() {
    let executor = peek_executor();
    let mut input = AbiEntry::selector_from_name("peek").to_vec();
    input.push(0xab);
    let ctx = execution_context_with_input("sc1contract", 100_000, input);
    let result = VmExecutor::execute(&executor, ctx).expect("peek dispatch");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, (0xabi32).to_le_bytes());
}

#[test]
fn selector_dispatch_large_calldata_grows_memory() {
    // Calldata crossing the 64 KiB page boundary: the host grows linear
    // memory before staging (copy is unmetered — calldata is bounded by
    // the transaction size, S1.5-FBP03). Reading past the original page
    // only succeeds if the grow + write landed.
    let executor = peek_executor();
    let mut input = AbiEntry::selector_from_name("peek").to_vec();
    input.extend_from_slice(&vec![0xabu8; 100_000]);
    let ctx = execution_context_with_input("sc1contract", 100_000, input);
    let result = VmExecutor::execute(&executor, ctx).expect("large calldata dispatch");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.return_data, (0xabi32).to_le_bytes());
}
