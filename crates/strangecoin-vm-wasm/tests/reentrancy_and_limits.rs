//! S1.5-P13: reentrancy guard + execution limits — integration tests.
//!
//! Covers the checklist per limit: `nonReentrant` ABI flag →
//! `VmError::ReentrancyDetected`, `max_code_size` (24 KB) in
//! `verify_transaction`, `max_storage_per_contract` (1 MB) in
//! `storage_write`, `max_call_depth` (1024) in the nested-call host
//! functions, and the wasmi engine/store limits — value-stack/recursion
//! (stack) and linear memory (16 pages) — both surfacing as
//! `VmError::OutOfMemory`.

use std::collections::HashMap;

use secp256k1::{ecdsa::RecoverableSignature, Message, Secp256k1, SecretKey};
use strangecoin_core::abi::{AbiEntry, ContractAbi};
use strangecoin_core::consensus::{verify_transaction, CHAIN_ID_REGTEST};
use strangecoin_core::error::CoreError;
use strangecoin_core::serialize::serialize_transaction;
use strangecoin_core::types::{AccountState, BlockHeader, Transaction};
use strangecoin_core::vm::limits::{MAX_CALL_DEPTH, MAX_CODE_SIZE, MAX_STORAGE_PER_CONTRACT};
use strangecoin_core::vm::{BlockContext, ExecutionContext, TxStatus, VmError};
use strangecoin_vm_wasm::{WasmiError, WasmiExecutor};

// ---------------------------------------------------------------------------
// Helpers (mirroring host_functions.rs)
// ---------------------------------------------------------------------------

/// Fake 4-byte selector of the guarded `nonReentrant` entry.
const SEL: [u8; 4] = [0x7a, 0x3c, 0x11, 0x22];

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

fn execution_context_at(callee: &str, gas_limit: u64, depth: u32) -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert(callee.to_string(), AccountState::eoa(1_000, 0));
    ExecutionContext {
        block: block_header(),
        accounts,
        caller: "sc1caller".to_string(),
        callee: callee.to_string(),
        value: 0,
        gas_limit,
        gas_used: 0,
        depth,
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

fn execution_context(callee: &str, gas_limit: u64) -> ExecutionContext {
    execution_context_at(callee, gas_limit, 0)
}

fn hex_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// WAT module with an exported `main`, `pages` pages of memory, and data
/// segments. Import lines are hoisted to the front (module binary format
/// requires imports before memory/data fields).
fn wat_pages(pages: u32, segments: &[(u32, &[u8])], body: &str) -> String {
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
    out.push_str(&format!(
        "(memory (export \"memory\") {pages})"
    ));
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

fn wat(segments: &[(u32, &[u8])], body: &str) -> String {
    wat_pages(1, segments, body)
}

enum ExecutionResultWrap {
    Ok(strangecoin_core::vm::ExecutionResult),
    Err(WasmiError),
}

fn execute(executor: &WasmiExecutor, wasm: &str, ctx: &ExecutionContext) -> ExecutionResultWrap {
    match executor.execute_wasm(wasm.as_bytes(), ctx) {
        Ok(result) => ExecutionResultWrap::Ok(result),
        Err(err) => ExecutionResultWrap::Err(err),
    }
}

impl ExecutionResultWrap {
    fn ok(self) -> strangecoin_core::vm::ExecutionResult {
        match self {
            ExecutionResultWrap::Ok(r) => r,
            ExecutionResultWrap::Err(e) => panic!("expected success, got error: {e}"),
        }
    }
    fn err(self) -> WasmiError {
        match self {
            ExecutionResultWrap::Err(e) => e,
            ExecutionResultWrap::Ok(_) => panic!("expected error, got success"),
        }
    }
    fn ret_i32(self) -> i32 {
        let r = self.ok();
        assert_eq!(r.status, TxStatus::Success);
        i32::from_le_bytes(r.return_data[..4].try_into().unwrap())
    }
    fn vm_err(self) -> VmError {
        VmError::from(self.err())
    }
}

// ---------------------------------------------------------------------------
// Reentrancy guard (threat V-18)
// ---------------------------------------------------------------------------

/// Contract that guards itself with storage: first run writes the key, then
/// re-enters `target` with [`SEL`]; any later run sees the key and returns 0
/// without calling — so an *allowed* self-call terminates instead of
/// recursing forever. Dual-exported `main`/`guarded` so the nested call's
/// selector dispatch (S1.5-FBP03) resolves the same function body.
fn guarded_wat(target: &[u8]) -> String {
    wat(
        &[(64, target), (96, &SEL)],
        r#"
        (import "sc" "storage_read" (func $read (param i32 i32 i32) (result i32)))
        (import "sc" "storage_write" (func $write (param i32 i32 i32) (result i32)))
        (import "sc" "call" (func $call (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (export "guarded") (result i32)
          (if (i32.eqz (call $read (i32.const 0) (i32.const 128) (i32.const 32)))
            (then
              (drop (call $write (i32.const 0) (i32.const 128) (i32.const 4)))
              (return (call $call (i32.const 64) (i32.const 4) (i64.const 0)
                                  (i32.const 96) (i32.const 4)
                                  (i32.const 160) (i32.const 32)))))
          (i32.const 0))"#,
    )
}

fn guarded_abi(non_reentrant: bool) -> ContractAbi {
    ContractAbi::new(vec![AbiEntry::new("guarded", SEL, non_reentrant)])
}

#[test]
fn self_call_with_non_reentrant_abi_is_rejected() {
    // Contract A re-enters itself while its ABI entry carries the
    // `non_reentrant` flag → the call host function aborts with the exact
    // ReentrancyDetected variant (no string parsing).
    let wat = guarded_wat(b"sc1a");
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    executor.insert_abi("sc1a", guarded_abi(true));
    let ctx = execution_context("sc1a", 50_000);
    let vm = execute(&executor, &wat, &ctx).vm_err();
    assert_eq!(vm, VmError::ReentrancyDetected);
}

#[test]
fn self_call_without_registered_abi_is_allowed() {
    // Reentrancy itself is legal — only `non_reentrant` entries reject it.
    // The storage guard makes the second frame return without calling again.
    let wat = guarded_wat(b"sc1a");
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    let ctx = execution_context("sc1a", 50_000);
    let result = execute(&executor, &wat, &ctx).ok();
    assert_eq!(result.status, TxStatus::Success);
}

#[test]
fn self_call_with_non_reentrant_false_is_allowed() {
    // ABI present, flag off → the nested self-call runs (child frame returns
    // 0 via the storage guard) and the parent sees STATUS_OK.
    let wat = guarded_wat(b"sc1a");
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    executor.insert_abi("sc1a", guarded_abi(false));
    let ctx = execution_context("sc1a", 50_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 0);
}

#[test]
fn cross_contract_reentrancy_is_rejected() {
    // Classic pattern: A (guarded) → B → B calls A back onto the stack.
    // B's frame aborts with ReentrancyDetected and the security error
    // bubbles through A instead of degrading to a generic failed child.
    let a_wat = guarded_wat(b"sc1b");
    let b_wat = wat(
        &[(0, b"sc1a"), (8, &SEL)],
        r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $c (i32.const 0) (i32.const 4) (i64.const 0)
                   (i32.const 8) (i32.const 4)
                   (i32.const 64) (i32.const 8)))"#,
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_wat.clone().into_bytes());
    executor.insert_code("sc1b", b_wat.into_bytes());
    executor.insert_abi("sc1a", guarded_abi(true));
    let ctx = execution_context("sc1a", 50_000);
    let vm = execute(&executor, &a_wat, &ctx).vm_err();
    assert_eq!(vm, VmError::ReentrancyDetected);
}

// ---------------------------------------------------------------------------
// max_call_depth (1024)
// ---------------------------------------------------------------------------

fn eoa_call_wat() -> String {
    wat(
        &[(0, b"sc1b")],
        r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $c (i32.const 0) (i32.const 4) (i64.const 0)
                   (i32.const 64) (i32.const 0)
                   (i32.const 128) (i32.const 32)))"#,
    )
}

#[test]
fn call_depth_at_limit_is_allowed() {
    // depth 1023 → child would be 1024 ≤ MAX_CALL_DEPTH → allowed
    // (`sc1b` is a plain EOA: status 0, empty return).
    let executor = WasmiExecutor::new();
    let ctx = execution_context_at("sc1a", 50_000, MAX_CALL_DEPTH - 1);
    assert_eq!(execute(&executor, &eoa_call_wat(), &ctx).ret_i32(), 0);
}

#[test]
fn call_depth_beyond_limit_is_rejected() {
    // depth 1024 → child would be 1025 → the call host function refuses
    // before touching the target.
    let executor = WasmiExecutor::new();
    let ctx = execution_context_at("sc1a", 50_000, MAX_CALL_DEPTH);
    let vm = execute(&executor, &eoa_call_wat(), &ctx).vm_err();
    assert_eq!(vm, VmError::MaxCallDepth);
}

// ---------------------------------------------------------------------------
// max_code_size (24 KB) — consensus-level CreateTx validation
// ---------------------------------------------------------------------------

fn keypair() -> (String, SecretKey) {
    let secp = Secp256k1::new();
    let mut sk_bytes = [7u8; 32];
    sk_bytes[0] = 42;
    let sk = SecretKey::from_slice(&sk_bytes).expect("valid secret key");
    let pk = secp256k1::PublicKey::from_secret_key(&secp, &sk);
    let addr = strangecoin_core::address::encode_address(&pk, CHAIN_ID_REGTEST)
        .expect("encode address");
    (addr, sk)
}

fn sign(tx: &mut Transaction, sk: &SecretKey) {
    let secp = Secp256k1::new();
    let msg_bytes = serialize_transaction(tx);
    let msg_hash = blake3::hash(&msg_bytes);
    let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
    let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, sk);
    let (rec_id, sig_bytes) = sig.serialize_compact();
    let mut sig_vec = Vec::with_capacity(65);
    sig_vec.extend_from_slice(&sig_bytes);
    sig_vec.push(rec_id.to_i32() as u8);
    *tx.signature_mut() = sig_vec;
}

fn signed_create(init_code: Vec<u8>) -> Transaction {
    let (addr, sk) = keypair();
    let mut tx = Transaction::create(&addr, init_code, Vec::new(), 0, 1, CHAIN_ID_REGTEST);
    sign(&mut tx, &sk);
    tx
}

#[test]
fn create_code_size_at_limit_accepted() {
    let tx = signed_create(vec![0x00; MAX_CODE_SIZE]);
    verify_transaction(&tx).expect("init_code of exactly 24 KB is accepted");
}

#[test]
fn create_code_size_over_limit_rejected() {
    let tx = signed_create(vec![0x00; MAX_CODE_SIZE + 1]);
    let err = verify_transaction(&tx).expect_err("24 KB + 1 init_code must be rejected");
    match err {
        CoreError::InvalidContractDeploy { reason } => {
            assert!(reason.contains("max code size"), "reason: {reason}");
        }
        other => panic!("expected InvalidContractDeploy, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// max_storage_per_contract (1 MB) — storage_write host function
// ---------------------------------------------------------------------------

const STORAGE_VALUE_LEN: usize = 100_000;

/// Writes `count` copies of a fixed [`STORAGE_VALUE_LEN`] buffer under
/// distinct keys (key[0..4] = i as LE bytes; the rest is zero-initialized
/// linear memory). Returns `0` only if every write succeeded.
fn storage_loop_wat(count: u32) -> String {
    let val = vec![0xaau8; STORAGE_VALUE_LEN];
    wat_pages(
        2,
        &[(100, &val)],
        &format!(
            r#"
            (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
            (func (export "main") (result i32)
              (local $i i32)
              (local.set $i (i32.const 0))
              (block $done
                (loop $loop
                  (br_if $done (i32.ge_u (local.get $i) (i32.const {count})))
                  (i32.store (i32.const 0) (local.get $i))
                  (drop (call $w (i32.const 0) (i32.const 100) (i32.const {STORAGE_VALUE_LEN})))
                  (local.set $i (i32.add (local.get $i) (i32.const 1)))
                  (br $loop)))
              (i32.const 0))"#
        ),
    )
}

#[test]
fn storage_cap_at_limit_accepted() {
    // Largest whole number of 100 KB writes that fits in 1 MB.
    let count = (MAX_STORAGE_PER_CONTRACT / STORAGE_VALUE_LEN) as u32;
    assert_eq!(count, 10);
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 200_000);
    let result = execute(&executor, &storage_loop_wat(count), &ctx).ok();
    assert_eq!(result.status, TxStatus::Success);
}

#[test]
fn storage_cap_exceeded_rejected() {
    // One more 100 KB write crosses 1 MB → the host function aborts the
    // frame with the exact MaxStoragePerContract variant.
    let count = (MAX_STORAGE_PER_CONTRACT / STORAGE_VALUE_LEN) as u32 + 1;
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 200_000);
    let vm = execute(&executor, &storage_loop_wat(count), &ctx).vm_err();
    assert_eq!(vm, VmError::MaxStoragePerContract);
}

// ---------------------------------------------------------------------------
// Memory + stack limits (wasmi config / store limiter)
// ---------------------------------------------------------------------------

#[test]
fn memory_declaration_over_limit_rejected() {
    // 17 pages > MAX_MEMORY_PAGES (16): the store's resource limiter denies
    // the initial allocation and instantiation fails.
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let wasm = r#"(module (memory 17) (func (export "main")))"#;
    match execute(&executor, wasm, &ctx).err() {
        WasmiError::Instantiate(msg) => assert!(
            msg.contains("limiter") || msg.contains("denied") || msg.contains("memory"),
            "msg: {msg}"
        ),
        other => panic!("expected instantiation failure, got {other:?}"),
    }
}

#[test]
fn memory_grow_beyond_limit_traps_out_of_memory() {
    // 1 + 16 pages = 17 > 16: `memory.grow` denial traps with
    // GrowthOperationLimited (trap_on_grow_failure) → OutOfMemory.
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let wasm = r#"(module (memory 1)
        (func (export "main") (drop (memory.grow (i32.const 16)))))"#;
    let vm = execute(&executor, wasm, &ctx).vm_err();
    assert_eq!(vm, VmError::OutOfMemory);
}

#[test]
fn runaway_recursion_hits_stack_limit() {
    // Unbounded wasm-internal recursion exhausts the engine's frame stack
    // (set to MAX_CALL_DEPTH) long before the fuel budget runs out →
    // StackOverflow is reported as OutOfMemory, not OutOfGas.
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 500_000);
    let wasm = r#"(module
        (func $f (call $f))
        (func (export "main") (call $f)))"#;
    let vm = execute(&executor, wasm, &ctx).vm_err();
    assert_eq!(vm, VmError::OutOfMemory);
}
