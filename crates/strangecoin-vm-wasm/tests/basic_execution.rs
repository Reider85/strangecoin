use std::collections::HashMap;

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
        block_context: BlockContext {
            height: 1,
            timestamp: 1_700_000_000,
            coinbase: "sc1coinbase".to_string(),
            block_gas_limit: 1_000_000,
        },
        storage_overlay: HashMap::new(),
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
