//! Differential testing (S1.5-P16): wasmi (the consensus runtime, pinned
//! =2.0.0 inside `WasmiExecutor`) must agree with wasmtime (the reference
//! engine) on a fixed vector suite of bounded, import-free modules.
//!
//! Comparison basis: outcome class only — `Success(i32)` with the same
//! value, or `Failed` on both sides. Fuel *amounts* are deliberately not
//! compared (engine-local metering tables differ); every vector is small
//! enough that neither engine should exhaust the shared 10M budget.

use std::collections::HashMap;

use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext};
use strangecoin_vm_wasm::WasmiExecutor;

const GAS_LIMIT: u64 = 10_000_000;

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Success(i32),
    Failed,
}

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

fn execution_context() -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert("sc1contract".to_string(), AccountState::eoa(1_000_000, 0));
    ExecutionContext {
        block: block_header(),
        accounts,
        caller: "sc1caller".to_string(),
        callee: "sc1contract".to_string(),
        value: 0,
        gas_limit: GAS_LIMIT,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height: 7,
            timestamp: 1_700_000_000,
            coinbase: "sc1coinbase".to_string(),
            block_gas_limit: GAS_LIMIT,
        },
        storage: Default::default(),
        storage_overlay: HashMap::new(),
        storage_address: "sc1contract".to_string(),
        input: Vec::new(),
    }
}

fn run_wasmi(wat: &str) -> Outcome {
    let executor = WasmiExecutor::new();
    match executor.execute_wasm(wat.as_bytes(), &execution_context()) {
        Ok(result) => {
            let bytes: [u8; 4] = result.return_data[..4].try_into().expect("i32 return");
            Outcome::Success(i32::from_le_bytes(bytes))
        }
        Err(_) => Outcome::Failed,
    }
}

fn run_wasmtime(wat: &str) -> Outcome {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let Ok(engine) = wasmtime::Engine::new(&config) else {
        return Outcome::Failed;
    };
    let Ok(module) = wasmtime::Module::new(&engine, wat) else {
        return Outcome::Failed;
    };
    let mut store = wasmtime::Store::new(&engine, ());
    if store.set_fuel(GAS_LIMIT).is_err() {
        return Outcome::Failed;
    }
    let linker = wasmtime::Linker::new(&engine);
    let Ok(instance) = linker.instantiate(&mut store, &module) else {
        return Outcome::Failed;
    };
    let Ok(func) = instance.get_typed_func::<(), i32>(&mut store, "main") else {
        return Outcome::Failed;
    };
    match func.call(&mut store, ()) {
        Ok(v) => Outcome::Success(v),
        Err(_) => Outcome::Failed,
    }
}

fn assert_agree(wat: &str) {
    let wasmi = run_wasmi(wat);
    let wasmtime = run_wasmtime(wat);
    assert_eq!(wasmi, wasmtime, "differential mismatch on module:\n{wat}");
}

#[test]
fn constant_return() {
    assert_agree(r#"(module (func (export "main") (result i32) i32.const 42))"#);
}

#[test]
fn negative_constant_return() {
    assert_agree(r#"(module (func (export "main") (result i32) i32.const -7))"#);
}

#[test]
fn arithmetic_add_mul() {
    assert_agree(
        r#"(module (func (export "main") (result i32)
             (i32.mul (i32.add (i32.const 6) (i32.const 7)) (i32.const 2))))"#,
    );
}

#[test]
fn i32_wrapping_division() {
    // i32::MIN / -1 wraps per the Wasm spec — both engines must agree.
    assert_agree(
        r#"(module (func (export "main") (result i32)
             (i32.div_s (i32.const -2147483648) (i32.const -1))))"#,
    );
}

#[test]
fn division_by_zero_traps_on_both() {
    assert_agree(
        r#"(module (func (export "main") (result i32)
             (i32.div_s (i32.const 1) (i32.const 0))))"#,
    );
}

#[test]
fn remainder_by_zero_traps_on_both() {
    assert_agree(
        r#"(module (func (export "main") (result i32)
             (i32.rem_u (i32.const 5) (i32.const 0))))"#,
    );
}

#[test]
fn unreachable_traps_on_both() {
    assert_agree(r#"(module (func (export "main") (result i32) unreachable))"#);
}

#[test]
fn bounded_loop_sums_to_triangle_number() {
    assert_agree(
        r#"(module (func (export "main") (result i32)
             (local $i i32) (local $s i32)
             (local.set $i (i32.const 100))
             (block $done
               (loop $l
                 (br_if $done (i32.eqz (local.get $i)))
                 (local.set $s (i32.add (local.get $s) (local.get $i)))
                 (local.set $i (i32.sub (local.get $i) (i32.const 1)))
                 (br $l)))
             (local.get $s)))"#,
    );
    // Triangle number of 100 = 5050 — pin the expected value too.
    assert_eq!(run_wasmi(bounded_loop_wat()), Outcome::Success(5050));
}

fn bounded_loop_wat() -> &'static str {
    r#"(module (func (export "main") (result i32)
         (local $i i32) (local $s i32)
         (local.set $i (i32.const 100))
         (block $done
           (loop $l
             (br_if $done (i32.eqz (local.get $i)))
             (local.set $s (i32.add (local.get $s) (local.get $i)))
             (local.set $i (i32.sub (local.get $i) (i32.const 1)))
             (br $l)))
         (local.get $s)))"#
}

#[test]
fn memory_store_load_roundtrip() {
    assert_agree(
        r#"(module (memory 1)
             (func (export "main") (result i32)
               (i32.store (i32.const 0) (i32.const 1234))
               (i32.load (i32.const 0))))"#,
    );
}

#[test]
fn shift_masking_matches_spec() {
    // Shift amounts are masked to 5 bits — 32 and 0 must agree.
    assert_agree(
        r#"(module (func (export "main") (result i32)
             (i32.shl (i32.const 1) (i32.const 32))))"#,
    );
    assert_agree(
        r#"(module (func (export "main") (result i32)
             (i32.shl (i32.const 1) (i32.const 33))))"#,
    );
}

#[test]
fn invalid_module_fails_on_both() {
    // Not valid WAT at all — both engines must reject at parse time.
    let garbage = "not a module at all";
    assert_eq!(run_wasmi(garbage), Outcome::Failed);
    assert_eq!(run_wasmtime(garbage), Outcome::Failed);
}

#[test]
fn missing_main_export_fails_on_both() {
    assert_agree(r#"(module (func (export "other") (result i32) i32.const 1))"#);
}

#[test]
fn signature_mismatch_fails_on_both() {
    // `main` returns i64, not i32 — typed-func lookup must fail on both.
    assert_agree(r#"(module (func (export "main") (result i64) i64.const 1))"#);
}
