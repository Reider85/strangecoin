//! S1.5-P16 fallback soak runner: deterministic seeded stirring over the
//! same four surfaces the libFuzzer targets cover (host functions, gas
//! metering, storage trie, differential wasmi vs wasmtime), for hosts where
//! cargo-fuzz/libFuzzer cannot run (see `fuzz/README.md`). Same contract as
//! the fuzz targets: a panic aborts the process and fails the run.
//!
//! Phases (env `SOAK_PHASE=host|gas|storage|diff|all`, default `all`):
//! - host — generated WAT calling a random `sc.*` import twice; results must
//!   match; plus the precompile router on random payloads.
//! - gas — raw bytes / mutated valid WAT / bounded and infinite loops under
//!   a random gas budget; must terminate deterministically.
//! - storage — random write/delete/commit sequences through
//!   `ContractStorageOverlay`; roots must be deterministic and undo/redo
//!   must preserve the root.
//! - diff — generated bounded WAT run in wasmi (raw engine) and wasmtime;
//!   outcome classes must agree.
//!
//! Run: `SOAK_SECONDS=600 SOAK_PHASE=all cargo run -p strangecoin-vm-wasm --example vm_fuzz_soak`

use std::collections::HashMap;
use std::time::{Duration, Instant};

use strangecoin_core::state::{contract_storage_root, ContractStorage, ContractStorageOverlay};
use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext, TxStatus};
use strangecoin_vm_wasm::{dispatch_precompile, WasmiExecutor};

const MAX_DATA: usize = 4096;
const MAX_VALUE_LEN: usize = 1024;
const MAX_OPS: usize = 64;
const DIFF_FUEL: u64 = 10_000_000;

/// Xorshift64*: deterministic, dependency-free byte stirring.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
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

fn execution_context(gas_limit: u64) -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert("sc1contract".to_string(), AccountState::eoa(1_000_000, 0));
    ExecutionContext {
        block: block_header(),
        accounts,
        caller: "sc1caller".to_string(),
        callee: "sc1contract".to_string(),
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
            block_gas_limit: 1_000_000,
        },
        storage: HashMap::new(),
        storage_overlay: HashMap::new(),
        storage_address: "sc1contract".to_string(),
        input: Vec::new(),
    }
}

fn hex_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

const HOST_FNS: &[(&str, &str)] = &[
    ("blake3", "(param i32 i32 i32)"),
    ("keccak", "(param i32 i32 i32)"),
    ("sha256", "(param i32 i32 i32)"),
    ("secp256k1_verify", "(param i32 i32 i32)"),
    ("bls_verify", ""),
    ("storage_read", "(param i32 i32 i32)"),
    ("storage_write", "(param i32 i32 i32)"),
    ("block_context", "(param i32 i32)"),
    ("gas_left", ""),
    ("emit_log", "(param i32 i32 i32 i32)"),
    ("call", "(param i32 i32 i64 i32 i32 i32 i32)"),
    ("delegatecall", "(param i32 i32 i32 i32 i32 i32)"),
    ("staticcall", "(param i32 i32 i32 i32 i32 i32)"),
];

fn module_wat(name: &str, params: &str, ptrs: &[i32; 4], data: &[u8]) -> String {
    let args = match name {
        "call" => format!(
            "(i32.const {}) (i32.const {}) (i64.const {}) (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {})",
            ptrs[0], ptrs[1], ptrs[2] as i64, ptrs[3], ptrs[0], ptrs[1], ptrs[2]
        ),
        "delegatecall" | "staticcall" => format!(
            "(i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {}) (i32.const {})",
            ptrs[0], ptrs[1], ptrs[2], ptrs[3], ptrs[0], ptrs[1]
        ),
        "bls_verify" | "gas_left" => String::new(),
        "block_context" => format!("(i32.const {}) (i32.const {})", ptrs[0], ptrs[1]),
        _ => format!(
            "(i32.const {}) (i32.const {}) (i32.const {})",
            ptrs[0], ptrs[1], ptrs[2]
        ),
    };
    let data_seg = if data.is_empty() {
        String::new()
    } else {
        format!("(data (i32.const 0) \"{}\")", hex_escape(data))
    };
    format!(
        "(module (import \"sc\" \"{name}\" (func $h {params})) \
         (memory (export \"memory\") 1) {data_seg} \
         (func (export \"main\") (drop (call $h {args}))))"
    )
}

fn soak_host(rng: &mut Xorshift, deadline: Instant) -> u64 {
    let executor = WasmiExecutor::new();
    let mut iters = 0u64;
    while Instant::now() < deadline {
        let fn_id = rng.below(HOST_FNS.len() as u64) as usize;
        let (name, params) = HOST_FNS[fn_id];
        let ptrs = [
            rng.next() as i32,
            rng.next() as i32,
            rng.next() as i32,
            rng.next() as i32,
        ];
        let data_len = rng.below(MAX_DATA as u64) as usize;
        let data = rng.bytes(data_len);
        let wat = module_wat(name, params, &ptrs, &data);
        let first = executor.execute_wasm(wat.as_bytes(), &execution_context(1_000_000));
        let second = executor.execute_wasm(wat.as_bytes(), &execution_context(1_000_000));
        match (&first, &second) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "host call not deterministic: {name}"),
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
            _ => panic!("host call diverged: {name}"),
        }

        let callee = format!("0x{:02x}", 1 + rng.below(8));
        let p1 = dispatch_precompile(&callee, &data);
        let p2 = dispatch_precompile(&callee, &data);
        match (&p1, &p2) {
            (Ok(a), Ok(b)) => assert_eq!(a, b),
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("precompile diverged: {callee}"),
        }
        iters += 1;
    }
    iters
}

fn seed_wat(payload: &[u8]) -> String {
    let data: Vec<u8> = payload.iter().copied().take(256).collect();
    let data_seg = if data.is_empty() {
        String::new()
    } else {
        format!("(data (i32.const 0) \"{}\")", hex_escape(&data))
    };
    format!(
        "(module (import \"sc\" \"gas_left\" (func $gl (result i64))) \
         (memory (export \"memory\") 1) {data_seg} \
         (func (export \"main\") (result i32) \
           (drop (call $gl)) \
           (i32.add (i32.const 1) (i32.const 2))))"
    )
}

fn bounded_loop_wat(iterations: u32) -> String {
    format!(
        "(module (func (export \"main\") (result i32) \
           (local $i i32) \
           (local.set $i (i32.const {iterations})) \
           (block $done \
             (loop $l \
               (br_if $done (i32.eqz (local.get $i))) \
               (local.set $i (i32.sub (local.get $i) (i32.const 1))) \
               (br $l))) \
           (local.get $i)))"
    )
}

const INFINITE_LOOP_WAT: &str = r#"(module (func (export "main") (loop $l (br $l))))"#;

fn check_gas_run(wat: &[u8], gas_limit: u64) {
    let executor = WasmiExecutor::new();
    let first = executor.execute_wasm(wat, &execution_context(gas_limit));
    let second = executor.execute_wasm(wat, &execution_context(gas_limit));
    match (&first, &second) {
        (Ok(a), Ok(b)) => {
            assert!(a.gas_used <= gas_limit, "gas_used {} exceeds limit", a.gas_used);
            assert_eq!(a.status, TxStatus::Success);
            assert_eq!(a, b);
        }
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
        _ => panic!("gas run diverged"),
    }
}

fn soak_gas(rng: &mut Xorshift, deadline: Instant) -> u64 {
    let mut iters = 0u64;
    while Instant::now() < deadline {
        let gas_limit = rng.below(10_000_000);
        match rng.below(4) {
            0 => {
                let raw_len = rng.below(MAX_DATA as u64) as usize;
                let raw = rng.bytes(raw_len);
                check_gas_run(&raw, gas_limit);
            }
            1 => {
                let mut bytes = seed_wat(&rng.bytes(64)).into_bytes();
                let flips = 1 + rng.below(4) as usize;
                for _ in 0..flips {
                    if bytes.is_empty() {
                        break;
                    }
                    let idx = rng.below(bytes.len() as u64) as usize;
                    bytes[idx] ^= 0x20;
                }
                check_gas_run(&bytes, gas_limit);
            }
            2 => {
                let iterations = rng.below(100_000) as u32;
                check_gas_run(bounded_loop_wat(iterations).as_bytes(), gas_limit);
            }
            _ => check_gas_run(INFINITE_LOOP_WAT.as_bytes(), gas_limit),
        }
        iters += 1;
    }
    iters
}

fn soak_storage(rng: &mut Xorshift, deadline: Instant) -> u64 {
    let mut iters = 0u64;
    while Instant::now() < deadline {
        let mut base = ContractStorage::new();
        let mut overlay = ContractStorageOverlay::new();
        let ops = 1 + rng.below(MAX_OPS as u64);
        for _ in 0..ops {
            match rng.below(3) {
                0 => {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&rng.bytes(32));
                    let value_len = rng.below(MAX_VALUE_LEN as u64) as usize;
                    let value = rng.bytes(value_len);
                    overlay.entries.insert(key, Some(value));
                }
                1 => {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&rng.bytes(32));
                    overlay.entries.insert(key, None);
                }
                _ => {
                    let preimages = overlay.preimages_of(&base);
                    overlay.apply_to(&mut base);
                    let mut restored = base.clone();
                    for (key, preimage) in &preimages {
                        match preimage {
                            Some(value) => {
                                restored.insert(*key, value.clone());
                            }
                            None => {
                                restored.remove(key);
                            }
                        }
                    }
                    overlay.apply_to(&mut restored);
                    assert_eq!(
                        contract_storage_root(&restored),
                        contract_storage_root(&base),
                        "undo/redo root mismatch"
                    );
                    overlay = ContractStorageOverlay::new();
                }
            }
        }
        let root1 = overlay.root_from(&base);
        let root2 = overlay.root_from(&base);
        assert_eq!(root1, root2, "storage root not deterministic");
        let mut merged = base.clone();
        overlay.apply_to(&mut merged);
        assert_eq!(root1, contract_storage_root(&merged));
        iters += 1;
    }
    iters
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Success(i32),
    Failed,
}

fn gen_diff_wat(a: i32, b: i32, n: u16, kind: u8) -> String {
    let n = (n % 1001) as i32;
    match kind % 8 {
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

fn run_wasmi(wat: &str) -> Outcome {
    let engine = wasmi_engine();
    let Ok(module) = wasmi::Module::new(engine, wat.as_bytes()) else {
        return Outcome::Failed;
    };
    let mut store = wasmi::Store::new(engine, ());
    if store.set_fuel(DIFF_FUEL).is_err() {
        return Outcome::Failed;
    }
    let linker = <wasmi::Linker<()>>::new(engine);
    let Ok(instance) = linker.instantiate_and_start(&mut store, &module) else {
        return Outcome::Failed;
    };
    let Ok(func) = instance.get_typed_func::<(), i32>(&store, "main") else {
        return Outcome::Failed;
    };
    match func.call(&mut store, ()) {
        Ok(v) => Outcome::Success(v),
        Err(_) => Outcome::Failed,
    }
}

fn run_wasmtime(wat: &str) -> Outcome {
    let engine = wasmtime_engine();
    let module = match wasmtime::Module::new(engine, wat) {
        Ok(m) => m,
        Err(_) => return Outcome::Failed,
    };
    let mut store = wasmtime::Store::new(engine, ());
    if store.set_fuel(DIFF_FUEL).is_err() {
        return Outcome::Failed;
    }
    let linker = wasmtime::Linker::new(engine);
    let instance = match linker.instantiate(&mut store, &module) {
        Ok(i) => i,
        Err(_) => return Outcome::Failed,
    };
    let Ok(func) = instance.get_typed_func::<(), i32>(&mut store, "main") else {
        return Outcome::Failed;
    };
    match func.call(&mut store, ()) {
        Ok(v) => Outcome::Success(v),
        Err(_) => Outcome::Failed,
    }
}

fn soak_diff(rng: &mut Xorshift, deadline: Instant) -> u64 {
    let mut iters = 0u64;
    while Instant::now() < deadline {
        let a = rng.next() as i32;
        let b = rng.next() as i32;
        let n = rng.next() as u16;
        let kind = rng.next() as u8;
        let wat = gen_diff_wat(a, b, n, kind);
        assert_eq!(
            run_wasmi(&wat),
            run_wasmtime(&wat),
            "differential mismatch on module:\n{wat}"
        );
        iters += 1;
    }
    iters
}

fn main() {
    let seconds: u64 = std::env::var("SOAK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let phase = std::env::var("SOAK_PHASE").unwrap_or_else(|_| "all".to_string());

    let mut rng = Xorshift(0x5EED_5EED_5EED_5EED);
    // `all` splits the budget across the four phases; a named phase gets the
    // full `SOAK_SECONDS`.
    let per_phase = if phase == "all" { (seconds / 4).max(1) } else { seconds };

    if phase == "all" || phase == "host" {
        let deadline = Instant::now() + Duration::from_secs(per_phase);
        let iters = soak_host(&mut rng, deadline);
        println!("vm_fuzz_soak[host]: {iters} iterations in {per_phase}s, 0 panics");
    }
    if phase == "all" || phase == "gas" {
        let deadline = Instant::now() + Duration::from_secs(per_phase);
        let iters = soak_gas(&mut rng, deadline);
        println!("vm_fuzz_soak[gas]: {iters} iterations in {per_phase}s, 0 panics");
    }
    if phase == "all" || phase == "storage" {
        let deadline = Instant::now() + Duration::from_secs(per_phase);
        let iters = soak_storage(&mut rng, deadline);
        println!("vm_fuzz_soak[storage]: {iters} iterations in {per_phase}s, 0 panics");
    }
    if phase == "all" || phase == "diff" {
        let deadline = Instant::now() + Duration::from_secs(per_phase);
        let iters = soak_diff(&mut rng, deadline);
        println!("vm_fuzz_soak[diff]: {iters} iterations in {per_phase}s, 0 panics");
    }
}
