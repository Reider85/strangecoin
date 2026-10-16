use std::collections::HashMap;
use std::time::{Duration, Instant};

use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{
    BlockContext, ExecutionContext, GasCounter, TxStatus, VmError,
};
use strangecoin_vm_wasm::gas::{FuelGasCounter, HostCallWeight};
use strangecoin_vm_wasm::{WasmiError, WasmiExecutor};

const RETURN_42_WAT: &str = r#"(module (func (export "main") (result i32) i32.const 42))"#;
const INFINITE_LOOP_WAT: &str = r#"(module (func (export "main") (loop $l (br $l))))"#;
const HEAVY_COMPUTE_WAT: &str = r#"(module
    (func (export "main") (result i32)
      (local $i i32)
      (local.set $i (i32.const 10000000))
      (loop $l
        (local.set $i (i32.sub (local.get $i) (i32.const 1)))
        (br_if $l (i32.ne (local.get $i) (i32.const 0))))
      (i32.const 1)))"#;

const GAS_LEFT_WAT: &str = r#"(module
    (import "sc" "gas_left" (func $gas_left (result i64)))
    (func (export "main") (result i32)
      (i32.wrap_i64 (call $gas_left))))"#;

const GAS_LEFT_DELTA_WAT: &str = r#"(module
    (import "sc" "gas_left" (func $gas_left (result i64)))
    (func (export "main") (result i32)
      (local $first i64)
      (local.set $first (call $gas_left))
      (i32.wrap_i64
        (i64.sub (local.get $first) (call $gas_left)))))"#;

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
            block_gas_limit: strangecoin_core::vm::BLOCK_GAS_LIMIT,
        },
        storage: HashMap::new(),
        storage_overlay: HashMap::new(),
        storage_address: callee.to_string(),
        input: Vec::new(),
    }
}

#[test]
fn infinite_loop_runs_out_of_gas_quickly() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 10_000);

    let started = Instant::now();
    let err = executor
        .execute_wasm(INFINITE_LOOP_WAT.as_bytes(), &ctx)
        .expect_err("infinite loop must be halted by fuel");
    let elapsed = started.elapsed();

    assert!(matches!(err, WasmiError::OutOfFuel));
    let vm_err: VmError = err.into();
    assert_eq!(vm_err, VmError::OutOfGas);
    // Fuel exhaustion must be near-instant; generous bound keeps CI stable
    // (local runs are sub-millisecond).
    assert!(
        elapsed < Duration::from_millis(50),
        "OutOfGas took {elapsed:?}"
    );
}

#[test]
fn heavy_compute_exceeding_gas_limit_runs_out_of_gas() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", strangecoin_core::vm::BLOCK_GAS_LIMIT);
    let err = executor
        .execute_wasm(HEAVY_COMPUTE_WAT.as_bytes(), &ctx)
        .expect_err("10M-iteration loop cannot finish under 1M gas");
    assert!(matches!(err, WasmiError::OutOfFuel));
}

#[test]
fn gas_left_returns_remaining_budget() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let result = executor
        .execute_wasm(GAS_LEFT_WAT.as_bytes(), &ctx)
        .expect("gas_left host call should execute");

    assert_eq!(result.status, TxStatus::Success);
    let remaining = i32::from_le_bytes(result.return_data.try_into().unwrap()) as u32 as u64;
    assert!(remaining > 0, "remaining gas must be positive");
    assert!(
        remaining < ctx.gas_limit,
        "host call must consume gas ({remaining})"
    );
    assert!(
        remaining > ctx.gas_limit - 1_000,
        "a single gas_left probe must stay cheap: {remaining}"
    );
}

#[test]
fn gas_left_decreases_across_calls() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let result = executor
        .execute_wasm(GAS_LEFT_DELTA_WAT.as_bytes(), &ctx)
        .expect("two gas_left probes should execute");

    let consumed = i32::from_le_bytes(result.return_data.try_into().unwrap()) as u32 as u64;
    // The delta covers the second call's 3-gas charge plus inter-call
    // instructions — strictly positive, bounded by the tiny probe body.
    assert!(
        (3..1_000).contains(&consumed),
        "unexpected consumption between probes: {consumed}"
    );
}

#[test]
fn gas_left_host_call_is_deterministic() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let first = executor
        .execute_wasm(GAS_LEFT_DELTA_WAT.as_bytes(), &ctx)
        .expect("first run");
    let second = executor
        .execute_wasm(GAS_LEFT_DELTA_WAT.as_bytes(), &ctx)
        .expect("second run");
    assert_eq!(first.gas_used, second.gas_used);
    assert_eq!(first.return_data, second.return_data);
}

#[test]
fn insufficient_fuel_for_host_call_traps_out_of_gas() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 1);
    let err = executor
        .execute_wasm(GAS_LEFT_WAT.as_bytes(), &ctx)
        .expect_err("1 gas cannot cover the gas_left charge");
    let vm_err: VmError = err.into();
    assert_eq!(vm_err, VmError::OutOfGas);
}

#[test]
fn fuel_gas_counter_spend_and_underflow() {
    let mut counter = FuelGasCounter::new(100);
    assert_eq!(counter.gas_left(), 100);
    counter.spend(40).expect("40 of 100 fits");
    assert_eq!(counter.gas_used(), 40);
    assert_eq!(counter.gas_left(), 60);
    counter.spend(61).expect_err("61 > 60 remaining");
    counter.spend(60).expect("exact remainder fits");
    assert_eq!(counter.gas_left(), 0);
    assert_eq!(
        counter.spend(1).expect_err("drained counter rejects"),
        VmError::OutOfGas
    );
}

#[test]
fn host_call_weights_follow_adr_0014() {
    assert_eq!(HostCallWeight::Secp256k1Verify.gas(), 3000);
    assert_eq!(HostCallWeight::BlsVerify.gas(), 12000);
    assert_eq!(HostCallWeight::Blake3 { bytes: 64 }.gas(), 56);
    assert_eq!(HostCallWeight::Blake3 { bytes: 65 }.gas(), 62);
    assert_eq!(HostCallWeight::Keccak { bytes: 32 }.gas(), 36);
    assert_eq!(HostCallWeight::Sha256 { bytes: 32 }.gas(), 62);
    assert_eq!(HostCallWeight::StorageRead.gas(), 200);
    assert_eq!(HostCallWeight::StorageWrite.gas(), 5000);
    assert_eq!(HostCallWeight::BlockContext.gas(), 3);
    assert_eq!(HostCallWeight::GasLeft.gas(), 3);
    assert_eq!(HostCallWeight::EmitLog { topics: 2 }.gas(), 1125);
    assert_eq!(HostCallWeight::MemoryGrow.gas(), 3);

    let mut counter = FuelGasCounter::new(2_999);
    counter
        .spend_host_call(HostCallWeight::Secp256k1Verify)
        .expect_err("3000 > 2999");
    counter.spend_host_call(HostCallWeight::GasLeft).expect("3 fits");
    assert_eq!(counter.gas_left(), 2_996);
}

#[test]
fn gas_reported_by_executor_is_deterministic_and_bounded() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let first = executor
        .execute_wasm(RETURN_42_WAT.as_bytes(), &ctx)
        .expect("first run");
    let second = executor
        .execute_wasm(RETURN_42_WAT.as_bytes(), &ctx)
        .expect("second run");
    assert_eq!(first.gas_used, second.gas_used);
    assert!(first.gas_used <= ctx.gas_limit);
    assert!(first.gas_used > 0);
}
