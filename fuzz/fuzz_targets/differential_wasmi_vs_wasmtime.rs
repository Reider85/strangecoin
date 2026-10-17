//! S1.5-P16: differential testing — wasmi (our consensus runtime, pinned
//! =2.0.0) vs wasmtime (reference JIT interpreter). The same generated WAT
//! module must produce the same outcome class in both engines: both succeed
//! with the same `i32`, or both fail. A success/failure split is a real bug
//! (in wasmi or in our integration) and aborts the run.
//!
//! Fuel semantics intentionally differ between engines (wasmi charges 1 per
//! operator; wasmtime uses a weighted table), so gas amounts are NOT
//! compared — only values and the Ok/Err split. Generated programs are
//! bounded (loops ≤ 1000 iterations, small bodies) so neither engine is
//! expected to hit its fuel budget.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

const FUEL: u64 = 10_000_000;

#[derive(Arbitrary, Debug)]
struct DiffInput {
    kind: u8,
    /// Constants mixed into the generated program.
    a: i32,
    b: i32,
    /// Loop bound source (clamped to ≤ 1000).
    n: u16,
}

/// Generate a small, bounded, import-free WAT module exporting
/// `main () -> i32`. Every family is valid MVP WAT so both engines must
/// parse it; some families trap deliberately (division by zero,
/// `unreachable`) to exercise the failure-class comparison.
fn gen_wat(input: &DiffInput) -> String {
    let a = input.a;
    let b = input.b;
    let n = (input.n % 1001) as i32;
    match input.kind % 8 {
        0 => format!("(module (func (export \"main\") (result i32) i32.const {a}))"),
        1 => format!(
            "(module (func (export \"main\") (result i32) \
             (i32.add (i32.const {a}) (i32.const {b}))))"
        ),
        2 => format!(
            "(module (func (export \"main\") (result i32) \
             (i32.mul (i32.sub (i32.const {a}) (i32.const {b})) \
                      (i32.div_s (i32.const {a}) \
                                 (i32.const {b})))))"
        ),
        3 => format!(
            "(module (func (export \"main\") (result i32) \
             (local $i i32) (local $s i32) \
             (local.set $i (i32.const {n})) \
             (block $done \
               (loop $l \
                 (br_if $done (i32.eqz (local.get $i))) \
                 (local.set $s (i32.add (local.get $s) (local.get $i))) \
                 (local.set $i (i32.sub (local.get $i) (i32.const 1))) \
                 (br $l))) \
             (local.get $s)))"
        ),
        4 => format!(
            "(module (memory 1) (func (export \"main\") (result i32) \
             (i32.store (i32.const 0) (i32.const {a})) \
             (i32.load (i32.const 0))))"
        ),
        5 => "(module (func (export \"main\") (result i32) unreachable))".to_string(),
        6 => format!(
            "(module (func (export \"main\") (result i32) \
             (i32.rem_u (i32.const {a}) (i32.const {b}))))"
        ),
        _ => format!(
            "(module (func (export \"main\") (result i32) \
             (local $i i32) \
             (local.set $i (i32.const {a})) \
             (block $out \
               (loop $l \
                 (local.set $i (i32.add (local.get $i) (i32.const 1))) \
                 (br_if $out (i32.eq (local.get $i) (i32.const {b}))) \
                 (br_if $out (i32.eq (local.get $i) (i32.const 1000))) \
                 (br $l))) \
             (local.get $i)))"
        ),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Success(i32),
    Failed,
}

/// Engines are expensive to build (wasmtime parses its Cranelift config),
/// modules are per-iteration — reuse the engines across fuzz iterations.
fn wasmi_engine() -> &'static wasmi::Engine {
    static ENGINE: std::sync::OnceLock<wasmi::Engine> = std::sync::OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = wasmi::Config::default();
        config.consume_fuel(true);
        wasmi::Engine::new(&config)
    })
}

fn wasmtime_engine() -> &'static wasmtime::Engine {
    static ENGINE: std::sync::OnceLock<wasmtime::Engine> = std::sync::OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        wasmtime::Engine::new(&config).expect("wasmtime engine config is valid")
    })
}

fn run_wasmi(wat: &str, fuel: u64) -> Outcome {
    let engine = wasmi_engine();
    let module = match wasmi::Module::new(engine, wat.as_bytes()) {
        Ok(m) => m,
        Err(_) => return Outcome::Failed,
    };
    let mut store = wasmi::Store::new(engine, ());
    if store.set_fuel(fuel).is_err() {
        return Outcome::Failed;
    }
    let linker = <wasmi::Linker<()>>::new(engine);
    let instance = match linker.instantiate_and_start(&mut store, &module) {
        Ok(i) => i,
        Err(_) => return Outcome::Failed,
    };
    let func = match instance.get_typed_func::<(), i32>(&store, "main") {
        Ok(f) => f,
        Err(_) => return Outcome::Failed,
    };
    match func.call(&mut store, ()) {
        Ok(v) => Outcome::Success(v),
        Err(_) => Outcome::Failed,
    }
}

fn run_wasmtime(wat: &str, fuel: u64) -> Outcome {
    let engine = wasmtime_engine();
    let module = match wasmtime::Module::new(engine, wat) {
        Ok(m) => m,
        Err(_) => return Outcome::Failed,
    };
    let mut store = wasmtime::Store::new(engine, ());
    if store.set_fuel(fuel).is_err() {
        return Outcome::Failed;
    }
    let linker = wasmtime::Linker::new(engine);
    let instance = match linker.instantiate(&mut store, &module) {
        Ok(i) => i,
        Err(_) => return Outcome::Failed,
    };
    let func = match instance.get_typed_func::<(), i32>(&mut store, "main") {
        Ok(f) => f,
        Err(_) => return Outcome::Failed,
    };
    match func.call(&mut store, ()) {
        Ok(v) => Outcome::Success(v),
        Err(_) => Outcome::Failed,
    }
}

fuzz_target!(|input: DiffInput| {
    let wat = gen_wat(&input);
    let wasmi_outcome = run_wasmi(&wat, FUEL);
    let wasmtime_outcome = run_wasmtime(&wat, FUEL);
    assert_eq!(
        wasmi_outcome, wasmtime_outcome,
        "differential mismatch on module:\n{wat}"
    );
});
