//! S1.5-P16: host-surface fuzzing (threat V-21). Random host-function call
//! with random input (in-bounds, out-of-bounds, negative pointers) must not
//! panic the host and must be deterministic: same input, same output.
//!
//! Two surfaces per iteration:
//! 1. A generated WAT module calls one `sc.*` import with fuzzer-chosen
//!    pointer/length arguments over a fuzzer-chosen data segment, executed
//!    twice through `WasmiExecutor::execute_wasm`.
//! 2. `dispatch_precompile` (the pure precompile router of S1.5-P11) is fed
//!    the same payload twice.

#![no_main]

use std::collections::HashMap;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext};
use strangecoin_vm_wasm::{dispatch_precompile, WasmiExecutor};

/// Cap on the data segment: keeps the generated WAT small (hex-escaped
/// 4 chars/byte) and memory accesses within the single 64KiB page.
const MAX_DATA: usize = 4096;
const GAS_LIMIT: u64 = 1_000_000;

#[derive(Arbitrary, Debug)]
struct HostCallInput {
    fn_id: u8,
    ptrs: [i32; 4],
    data: Vec<u8>,
}

/// Host functions of S1.5-P10 with their WAT signatures (under `sc`).
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

fn hex_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// WAT module: import `sc.<name>`, one page of memory holding `data` at
/// offset 0, and a `main` that calls the import with `ptrs` (typed per the
/// import's signature) and drops the result.
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

fn execution_context(callee: &str) -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert(callee.to_string(), AccountState::eoa(1_000_000, 0));
    ExecutionContext {
        block: block_header(),
        accounts,
        caller: "sc1caller".to_string(),
        callee: callee.to_string(),
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
        storage_address: callee.to_string(),
        input: Vec::new(),
    }
}

fuzz_target!(|input: HostCallInput| {
    let data: Vec<u8> = input.data.iter().copied().take(MAX_DATA).collect();

    // 1. Host-function call through real wasmi execution, twice.
    let (name, params) = HOST_FNS[input.fn_id as usize % HOST_FNS.len()];
    let wat = module_wat(name, params, &input.ptrs, &data);
    let executor = WasmiExecutor::new();
    let first = executor.execute_wasm(wat.as_bytes(), &execution_context("sc1contract"));
    let second = executor.execute_wasm(wat.as_bytes(), &execution_context("sc1contract"));
    match (&first, &second) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "host call not deterministic: {name}"),
        (Err(a), Err(b)) => assert_eq!(
            a.to_string(),
            b.to_string(),
            "host error not deterministic: {name}"
        ),
        _ => panic!("host call diverged between runs: {name} ({first:?} vs {second:?})"),
    }

    // 2. Precompile router on the same payload, twice. Unknown addresses
    // (`0x08`..) must reject as InvalidInput without panicking.
    let callee = format!("0x{:02x}", 1 + (input.fn_id % 8));
    let p1 = dispatch_precompile(&callee, &data);
    let p2 = dispatch_precompile(&callee, &data);
    match (&p1, &p2) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "precompile not deterministic: {callee}"),
        (Err(a), Err(b)) => assert_eq!(a, b, "precompile error not deterministic: {callee}"),
        _ => panic!("precompile diverged between runs: {callee}"),
    }
});
