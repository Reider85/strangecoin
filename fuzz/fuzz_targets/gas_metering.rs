//! S1.5-P16: gas-metering fuzzing. Random "WASM" (valid-ish modules, byte
//! mutations of known-valid templates, and pure garbage) under a random gas
//! budget must always terminate in `Ok` or a typed error — never a panic,
//! never an unbounded loop (threat V-21: fuel is the only halt mechanism).
//! Successful runs must respect `gas_used <= gas_limit`, and the whole
//! outcome must be deterministic.

#![no_main]

use std::collections::HashMap;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext, TxStatus};
use strangecoin_vm_wasm::WasmiExecutor;

const MAX_DATA: usize = 4096;

#[derive(Arbitrary, Debug)]
struct GasInput {
    /// Selects the module family: 0 = raw bytes, 1 = mutated valid WAT,
    /// 2 = bounded-loop WAT, 3 = infinite-loop WAT.
    kind: u8,
    gas_limit: u64,
    payload: Vec<u8>,
}

fn hex_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
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
        receipts_root: [0u8; 32],
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
        storage: Default::default(),
        storage_overlay: HashMap::new(),
        storage_address: "sc1contract".to_string(),
        input: Vec::new(),
    }
}

/// Small valid module family for mutation: a data-holding module whose
/// `main` does a little arithmetic and calls `sc.gas_left` once — deep
/// enough to exercise fuel accounting through host calls.
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

fuzz_target!(|input: GasInput| {
    let gas_limit = input.gas_limit % 10_000_000;
    let wat: String = match input.kind % 4 {
        0 => {
            // Raw bytes: mostly decode failures — the decoder must reject,
            // never panic (canonical-decode coverage for the VM boundary).
            let raw: Vec<u8> = input.payload.iter().copied().take(MAX_DATA).collect();
            return fuzz_raw_wasm(&raw, gas_limit);
        }
        1 => {
            // Mutate a valid WAT: flip bytes in the template body.
            let mut bytes = seed_wat(&input.payload).into_bytes();
            let flips = 1 + (input.payload.len() % 4) as usize;
            for i in 0..flips {
                let idx = match bytes.len() {
                    0 => break,
                    n => (input.payload.get(i).copied().unwrap_or(0) as usize) % n,
                };
                bytes[idx] ^= 0x20;
            }
            String::from_utf8_lossy(&bytes).into_owned()
        }
        2 => {
            let iterations = u32::from_le_bytes(
                input
                    .payload
                    .get(..4)
                    .and_then(|s| s.try_into().ok())
                    .unwrap_or([1, 0, 0, 0]),
            ) % 100_000;
            bounded_loop_wat(iterations)
        }
        _ => INFINITE_LOOP_WAT.to_string(),
    };

    let executor = WasmiExecutor::new();
    let first = executor.execute_wasm(wat.as_bytes(), &execution_context(gas_limit));
    let second = executor.execute_wasm(wat.as_bytes(), &execution_context(gas_limit));

    // Termination: either outcome is fine, but it must be one of them.
    // (A panic inside execute_wasm aborts the fuzz process = crash.)
    match (&first, &second) {
        (Ok(a), Ok(b)) => {
            assert!(a.gas_used <= gas_limit, "gas_used {} exceeds limit {gas_limit}", a.gas_used);
            assert_eq!(a.status, TxStatus::Success);
            assert_eq!(a, b, "gas-metered run not deterministic");
        }
        (Err(a), Err(b)) => {
            assert_eq!(a.to_string(), b.to_string(), "vm error not deterministic");
        }
        _ => panic!("gas-metered run diverged between runs"),
    }
});

/// Kind 0 path: raw bytes executed directly (no WAT wrapping).
fn fuzz_raw_wasm(raw: &[u8], gas_limit: u64) {
    let executor = WasmiExecutor::new();
    let first = executor.execute_wasm(raw, &execution_context(gas_limit));
    let second = executor.execute_wasm(raw, &execution_context(gas_limit));
    match (&first, &second) {
        (Ok(a), Ok(b)) => {
            assert!(a.gas_used <= gas_limit, "gas_used {} exceeds limit {gas_limit}", a.gas_used);
            assert_eq!(a, b, "raw-wasm run not deterministic");
        }
        (Err(a), Err(b)) => {
            assert_eq!(a.to_string(), b.to_string(), "raw-wasm error not deterministic");
        }
        _ => panic!("raw-wasm run diverged between runs"),
    }
}
