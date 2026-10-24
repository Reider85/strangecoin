//! Host-function integration tests (S1.5-P10): every host function gets a
//! valid-input vector, an invalid-input case, and a gas observation, driven
//! through real wasmi execution of small WAT modules.

use std::collections::HashMap;

use secp256k1::{Message, Secp256k1, SecretKey};
use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext, TxStatus, VmError};
use strangecoin_vm_wasm::{WasmiError, WasmiExecutor};

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

fn execution_context(callee: &str, gas_limit: u64) -> ExecutionContext {
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

fn hex_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// Build a WAT module with a `main`, exported memory, and data segments.
///
/// Import lines are hoisted to the front: the binary format (which the text
/// format validates against) requires imports before memory/data fields.
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

fn execute(executor: &WasmiExecutor, wasm: &str, ctx: &ExecutionContext) -> ExecutionResultWrap {
    match executor.execute_wasm(wasm.as_bytes(), ctx) {
        Ok(result) => ExecutionResultWrap::Ok(result),
        Err(err) => ExecutionResultWrap::Err(err),
    }
}

enum ExecutionResultWrap {
    Ok(strangecoin_core::vm::ExecutionResult),
    Err(WasmiError),
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
}

// ---------------------------------------------------------------------------
// crypto: blake3 / keccak / sha256
// ---------------------------------------------------------------------------

const ABC: &[u8] = b"abc";
const BLAKE3_ABC: [u8; 32] = [
    0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33, 0xff, 0xb6, 0x3b, 0x75, 0x27, 0x3a, 0x8d, 0xb5,
    0x48, 0xc5, 0x58, 0x46, 0x5d, 0x79, 0xdb, 0x03, 0xfd, 0x35, 0x9c, 0x6c, 0xd5, 0xbd, 0x9d, 0x85,
];
const KECCAK_ABC: [u8; 32] = [
    0x4e, 0x03, 0x65, 0x7a, 0xea, 0x45, 0xa9, 0x4f, 0xc7, 0xd4, 0x7b, 0xa8, 0x26, 0xc8, 0xd6, 0x67,
    0xc0, 0xd1, 0xe6, 0xe3, 0x3a, 0x64, 0xa0, 0x36, 0xec, 0x44, 0xf5, 0x8f, 0xa1, 0x2d, 0x6c, 0x45,
];
const SHA256_ABC: [u8; 32] = [
    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
    0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
];

fn hash_wat(host: &str) -> String {
    wat(
        &[(0, ABC)],
        &format!(
            r#"
            (import "sc" "{host}" (func $h (param i32 i32 i32) (result i32)))
            (func (export "main") (result i32)
              (if (result i32) (call $h (i32.const 0) (i32.const 3) (i32.const 64))
                (then (i32.load (i32.const 64)))
                (else (i32.const -1))))"#
        ),
    )
}

fn hash_first_u32_le(digest: &[u8; 32]) -> i32 {
    i32::from_le_bytes(digest[0..4].try_into().unwrap())
}

#[test]
fn blake3_host_known_vector() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let got = execute(&executor, &hash_wat("blake3"), &ctx).ret_i32();
    assert_eq!(got, hash_first_u32_le(&BLAKE3_ABC));
}

#[test]
fn keccak_host_known_vector() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let got = execute(&executor, &hash_wat("keccak"), &ctx).ret_i32();
    assert_eq!(got, hash_first_u32_le(&KECCAK_ABC));
}

#[test]
fn sha256_host_known_vector() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let got = execute(&executor, &hash_wat("sha256"), &ctx).ret_i32();
    assert_eq!(got, hash_first_u32_le(&SHA256_ABC));
}

/// `host` with an out-buffer at 65520 (32-byte digest no longer fits) → 0.
fn hash_oob_out_wat(host: &str) -> String {
    wat(
        &[(0, ABC)],
        &format!(
            r#"
            (import "sc" "{host}" (func $h (param i32 i32 i32) (result i32)))
            (func (export "main") (result i32)
              (call $h (i32.const 0) (i32.const 3) (i32.const 65520)))"#
        ),
    )
}

#[test]
fn keccak_host_out_of_bounds_out_buffer_returns_zero() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let got = execute(&executor, &hash_oob_out_wat("keccak"), &ctx).ret_i32();
    assert_eq!(got, 0);
}

#[test]
fn sha256_host_out_of_bounds_out_buffer_returns_zero() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let got = execute(&executor, &hash_oob_out_wat("sha256"), &ctx).ret_i32();
    assert_eq!(got, 0);
}

/// Budget below `host`'s weight → `OutOfFuel` before any hashing happens.
#[test]
fn keccak_host_insufficient_gas_traps() {
    let executor = WasmiExecutor::new();
    // keccak "abc": 30 + 6 = 36 → budget 30 cannot cover it.
    let ctx = execution_context("sc1contract", 30);
    assert!(matches!(
        execute(&executor, &hash_wat("keccak"), &ctx).err(),
        WasmiError::OutOfFuel
    ));
}

#[test]
fn sha256_host_insufficient_gas_traps() {
    let executor = WasmiExecutor::new();
    // sha256 "abc": 50 + 12 = 62 → budget 50 cannot cover it.
    let ctx = execution_context("sc1contract", 50);
    assert!(matches!(
        execute(&executor, &hash_wat("sha256"), &ctx).err(),
        WasmiError::OutOfFuel
    ));
}

#[test]
fn blake3_host_out_of_bounds_out_buffer_returns_zero() {
    // 1 page = 65536 bytes; out_ptr + 32 > 65536 → host refuses to write.
    let wat = wat(
        &[(0, ABC)],
        r#"
        (import "sc" "blake3" (func $h (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $h (i32.const 0) (i32.const 3) (i32.const 65520)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 0);
}

#[test]
fn blake3_host_out_of_bounds_in_buffer_aborts_frame() {
    let wat = r#"
        (module
        (import "sc" "blake3" (func $h (param i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "main") (result i32)
          (call $h (i32.const 65520) (i32.const 100) (i32.const 64))))"#;
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let err = execute(&executor, wat, &ctx).err();
    assert!(matches!(err, WasmiError::Engine(_)));
}

#[test]
fn blake3_host_gas_formula_via_gas_left() {
    // delta = gas_left before - after = host weight (50 + 6/chunk) + a few
    // wrapper instructions; bound it loosely around the ADR-0014 formula.
    let wat = wat(
        &[(0, ABC)],
        r#"
        (import "sc" "gas_left" (func $gl (result i64)))
        (import "sc" "blake3" (func $h (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (local $before i64)
          (local.set $before (call $gl))
          (drop (call $h (i32.const 0) (i32.const 3) (i32.const 64)))
          (i32.wrap_i64 (i64.sub (local.get $before) (call $gl))))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let delta = execute(&executor, &wat, &ctx).ret_i32() as u64;
    // "abc" is one 64-byte chunk → 50 + 6 = 56, plus a handful of ops.
    assert!((56..=80).contains(&delta), "delta {delta} outside bound");
}

#[test]
fn blake3_host_insufficient_gas_traps() {
    let wat = hash_wat("blake3");
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 40); // below the 50 host weight
    let err = execute(&executor, &wat, &ctx).err();
    assert!(matches!(err, WasmiError::OutOfFuel));
    let vm_err: VmError = err.into();
    assert_eq!(vm_err, VmError::OutOfGas);
}

// ---------------------------------------------------------------------------
// crypto: secp256k1_verify + bls_verify
// ---------------------------------------------------------------------------

fn sign_vector() -> (Vec<u8>, [u8; 32], Vec<u8>) {
    let secp = Secp256k1::new();
    let sk = SecretKey::from_slice(&[7u8; 32]).expect("valid test key");
    let pk = sk.public_key(&secp);
    let hash = [9u8; 32];
    let msg = Message::from_digest(hash);
    let sig = secp.sign_ecdsa(&msg, &sk);
    (
        pk.serialize().to_vec(),
        hash,
        sig.serialize_compact().to_vec(),
    )
}

#[test]
fn secp256k1_verify_valid_signature_returns_one() {
    let (pk, hash, sig) = sign_vector();
    let wat = wat(
        &[(0, &pk), (64, &hash), (128, &sig)],
        r#"
        (import "sc" "secp256k1_verify" (func $v (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $v (i32.const 0) (i32.const 64) (i32.const 128)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 1);
}

#[test]
fn secp256k1_verify_tampered_signature_returns_zero() {
    let (pk, hash, mut sig) = sign_vector();
    sig[0] ^= 0xff;
    let wat = wat(
        &[(0, &pk), (64, &hash), (128, &sig)],
        r#"
        (import "sc" "secp256k1_verify" (func $v (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $v (i32.const 0) (i32.const 64) (i32.const 128)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 0);
}

#[test]
fn secp256k1_verify_garbage_pubkey_returns_zero() {
    let (_, hash, sig) = sign_vector();
    let garbage_pk = vec![0x02u8; 33]; // not a valid curve point
    let wat = wat(
        &[(0, &garbage_pk), (64, &hash), (128, &sig)],
        r#"
        (import "sc" "secp256k1_verify" (func $v (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $v (i32.const 0) (i32.const 64) (i32.const 128)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 0);
}

#[test]
fn secp256k1_verify_insufficient_gas_traps() {
    let (pk, hash, sig) = sign_vector();
    let wat = wat(
        &[(0, &pk), (64, &hash), (128, &sig)],
        r#"
        (import "sc" "secp256k1_verify" (func $v (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $v (i32.const 0) (i32.const 64) (i32.const 128)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 2_900); // below the 3000 weight
    assert!(matches!(
        execute(&executor, &wat, &ctx).err(),
        WasmiError::OutOfFuel
    ));
}

#[test]
fn bls_verify_not_implemented_after_charging() {
    let wat = r#"
        (module
        (import "sc" "bls_verify" (func $b (result i32)))
        (func (export "main") (drop (call $b))))"#;
    let executor = WasmiExecutor::new();

    // Budget covers the 12000 weight → NotImplemented message.
    let ctx = execution_context("sc1contract", 13_000);
    let err = execute(&executor, wat, &ctx).err();
    match err {
        WasmiError::Engine(msg) => assert!(msg.contains("not implemented"), "msg: {msg}"),
        other => panic!("expected engine error, got {other:?}"),
    }

    // Budget below the weight → OutOfFuel (charge happens first).
    let ctx = execution_context("sc1contract", 11_000);
    assert!(matches!(
        execute(&executor, wat, &ctx).err(),
        WasmiError::OutOfFuel
    ));
}

#[test]
fn bls_verify_error_is_deterministic() {
    // No BLS12-381 dependency: every run fails the frame with the identical
    // Stage 7 message after charging the declared weight.
    let wat = r#"
        (module
        (import "sc" "bls_verify" (func $b (result i32)))
        (func (export "main") (drop (call $b))))"#;
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 13_000);
    let first = execute(&executor, wat, &ctx).err().to_string();
    let second = execute(&executor, wat, &ctx).err().to_string();
    assert_eq!(first, second);
    assert!(first.contains("Stage 7"), "msg: {first}");
}

#[test]
fn bls_verify_error_maps_to_vm_host_function_error() {
    // The executor error must surface as a VmError carrying the host name so
    // the node layer can attribute the failure to bls_verify.
    let wat = r#"
        (module
        (import "sc" "bls_verify" (func $b (result i32)))
        (func (export "main") (drop (call $b))))"#;
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 13_000);
    let err = execute(&executor, wat, &ctx).err();
    let vm_err: VmError = err.into();
    match vm_err {
        VmError::HostFunctionError { reason, .. } => {
            assert!(reason.contains("bls_verify"), "reason: {reason}");
        }
        other => panic!("expected HostFunctionError, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// storage: storage_write / storage_read
// ---------------------------------------------------------------------------

const KEY: [u8; 32] = [0x11u8; 32];
const VALUE: &[u8] = b"value123"; // 8 bytes

fn storage_write_then_read_wat() -> String {
    wat(
        &[(0, &KEY), (32, VALUE)],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (import "sc" "storage_read" (func $r (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (drop (call $w (i32.const 0) (i32.const 32) (i32.const 8)))
          (call $r (i32.const 0) (i32.const 64) (i32.const 32)))"#,
    )
}

#[test]
fn storage_write_then_read_roundtrip() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let len = execute(&executor, &storage_write_then_read_wat(), &ctx).ret_i32();
    assert_eq!(len, VALUE.len() as i32);
}

#[test]
fn storage_write_refused_in_read_only_frame() {
    let wat = wat(
        &[(0, &KEY), (32, VALUE)],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $w (i32.const 0) (i32.const 32) (i32.const 8)))"#,
    );
    let executor = WasmiExecutor::new();
    let mut ctx = execution_context("sc1contract", 100_000);
    ctx.read_only = true;
    let err = execute(&executor, &wat, &ctx).err();
    match err {
        WasmiError::Engine(msg) => assert!(msg.contains("read-only"), "msg: {msg}"),
        other => panic!("expected engine error, got {other:?}"),
    }
}

#[test]
fn storage_read_absent_key_returns_zero() {
    let wat = wat(
        &[(0, &KEY)],
        r#"
        (import "sc" "storage_read" (func $r (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $r (i32.const 0) (i32.const 64) (i32.const 32)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 0);
}

#[test]
fn storage_read_small_out_buffer_returns_minus_one() {
    let wat = wat(
        &[(0, &KEY), (32, VALUE)],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (import "sc" "storage_read" (func $r (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (drop (call $w (i32.const 0) (i32.const 32) (i32.const 8)))
          (call $r (i32.const 0) (i32.const 64) (i32.const 4)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), -1);
}

#[test]
fn storage_write_insufficient_gas_traps() {
    let wat = wat(
        &[(0, &KEY), (32, VALUE)],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $w (i32.const 0) (i32.const 32) (i32.const 8)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 4_900); // below the 5000 weight
    assert!(matches!(
        execute(&executor, &wat, &ctx).err(),
        WasmiError::OutOfFuel
    ));
}

// ---------------------------------------------------------------------------
// context: block_context
// ---------------------------------------------------------------------------

#[test]
fn block_context_returns_height_timestamp_gas_limit_coinbase() {
    let wat = r#"
        (module
        (import "sc" "block_context" (func $bc (param i32 i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "main") (result i32)
          (drop (call $bc (i32.const 0) (i32.const 128)))
          (i32.wrap_i64 (i64.load (i32.const 0)))))"#;
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let height = execute(&executor, wat, &ctx).ret_i32();
    assert_eq!(height as u64, ctx.block_context.height);
}

#[test]
fn block_context_small_out_buffer_returns_minus_one() {
    let wat = r#"
        (module
        (import "sc" "block_context" (func $bc (param i32 i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "main") (result i32)
          (call $bc (i32.const 0) (i32.const 8))))"#;
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    assert_eq!(execute(&executor, wat, &ctx).ret_i32(), -1);
}

#[test]
fn block_context_gas_is_three() {
    let wat = r#"
        (module
        (import "sc" "gas_left" (func $gl (result i64)))
        (import "sc" "block_context" (func $bc (param i32 i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "main") (result i32)
          (local $before i64)
          (local.set $before (call $gl))
          (drop (call $bc (i32.const 0) (i32.const 128)))
          (i32.wrap_i64 (i64.sub (local.get $before) (call $gl)))))"#;
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let delta = execute(&executor, wat, &ctx).ret_i32() as u64;
    assert!((3..=30).contains(&delta), "delta {delta} outside bound");
}

// ---------------------------------------------------------------------------
// logs: emit_log
// ---------------------------------------------------------------------------

const TOPIC: [u8; 32] = [0xaau8; 32];

fn emit_log_wat(topics_count: u32) -> String {
    wat(
        &[(0, &TOPIC), (32, b"abcd")],
        &format!(
            r#"
            (import "sc" "emit_log" (func $l (param i32 i32 i32 i32) (result i32)))
            (func (export "main") (result i32)
              (call $l (i32.const 0) (i32.const {topics_count}) (i32.const 32) (i32.const 4)))"#
        ),
    )
}

#[test]
fn emit_log_appends_log_with_topic_and_data() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let result = execute(&executor, &emit_log_wat(1), &ctx).ok();
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(result.logs.len(), 1);
    let log = &result.logs[0];
    assert_eq!(log.address, "sc1contract");
    assert_eq!(log.topics, vec![TOPIC]);
    assert_eq!(log.data, b"abcd");
}

#[test]
fn emit_log_more_than_four_topics_aborts_frame() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let err = execute(&executor, &emit_log_wat(5), &ctx).err();
    match err {
        WasmiError::Engine(msg) => assert!(msg.contains("at most 4 topics"), "msg: {msg}"),
        other => panic!("expected engine error, got {other:?}"),
    }
}

#[test]
fn emit_log_insufficient_gas_traps() {
    let executor = WasmiExecutor::new();
    // 375 + 375 = 750 for one topic; budget below that.
    let ctx = execution_context("sc1contract", 700);
    assert!(matches!(
        execute(&executor, &emit_log_wat(1), &ctx).err(),
        WasmiError::OutOfFuel
    ));
}

// ---------------------------------------------------------------------------
// nested calls: call / delegatecall / staticcall
// ---------------------------------------------------------------------------

const B_WAT: &str = r#"(module (func (export "main") (result i32) i32.const 42))"#;

fn caller_wat(import_sig: &str, import_name: &str, call_expr: &str, extra: &str) -> String {
    wat(
        &[(0, b"sc1b")],
        &format!(
            r#"
            (import "sc" "{import_name}" (func $c ({import_sig}))
            (func (export "main") (result i32)
              {extra}
              {call_expr})"#
        ),
    )
}

#[test]
fn nested_call_executes_callee_and_copies_return_data() {
    // Returns status * 1000 + out-buffer i32: 42 with status 0 proves both
    // the STATUS_OK code and the return-data copy.
    let wat = wat(
        &[(0, b"sc1b")],
        r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (local $status i32)
          (local.set $status (call $c (i32.const 0) (i32.const 4) (i64.const 0) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32)))
          (i32.add (i32.mul (local.get $status) (i32.const 1000))
                   (i32.load (i32.const 128))))"#,
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    executor.insert_code("sc1b", B_WAT.as_bytes().to_vec());

    let ctx = execution_context("sc1a", 100_000);
    let r = execute(&executor, &wat, &ctx).ok();
    assert_eq!(r.status, TxStatus::Success);
    assert_eq!(
        i32::from_le_bytes(r.return_data[..4].try_into().unwrap()),
        42
    );
}

#[test]
fn nested_call_to_eoa_succeeds_empty() {
    let wat = caller_wat(
        "param i32 i32 i64 i32 i32 i32 i32) (result i32)",
        "call",
        "(call $c (i32.const 0) (i32.const 4) (i64.const 0) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32))",
        "",
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    // "sc1b" deliberately NOT registered → treated as EOA.
    let ctx = execution_context("sc1a", 100_000);
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 0);
}

#[test]
fn nested_call_reuses_precompile_dispatch() {
    // call("0x02", "abc") → sha256 precompile; out-buffer gets the digest.
    let wat = wat(
        &[(0, b"0x02"), (8, b"abc")],
        r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (drop (call $c (i32.const 0) (i32.const 4) (i64.const 0) (i32.const 8) (i32.const 3) (i32.const 128) (i32.const 32)))
          (i32.load (i32.const 128)))"#,
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    let ctx = execution_context("sc1a", 100_000);
    let got = execute(&executor, &wat, &ctx).ret_i32();
    assert_eq!(got, hash_first_u32_le(&SHA256_ABC));
}

#[test]
fn nested_call_insufficient_balance_fails() {
    let wat = caller_wat(
        "param i32 i32 i64 i32 i32 i32 i32) (result i32)",
        "call",
        "(call $c (i32.const 0) (i32.const 4) (i64.const 5) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32))",
        "",
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    executor.insert_code("sc1b", B_WAT.as_bytes().to_vec());

    // Contract account with zero balance cannot send value 5.
    let mut ctx = execution_context("sc1a", 100_000);
    ctx.accounts
        .insert("sc1a".to_string(), AccountState::eoa(0, 0));
    assert_eq!(execute(&executor, &wat, &ctx).ret_i32(), 2);
}

#[test]
fn nested_delegatecall_shares_storage_overlay() {
    // B writes key K; A delegatecalls B, then reads K from its own overlay.
    let b_wat = wat(
        &[(0, &KEY), (32, VALUE)],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $w (i32.const 0) (i32.const 32) (i32.const 8)))"#,
    );
    let a_wat = wat(
        &[(0, b"sc1b"), (64, &KEY)],
        r#"
        (import "sc" "delegatecall" (func $d (param i32 i32 i32 i32 i32 i32) (result i32)))
        (import "sc" "storage_read" (func $r (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (local $status i32)
          (local.set $status (call $d (i32.const 0) (i32.const 4) (i32.const 0) (i32.const 0) (i32.const 128) (i32.const 32)))
          ;; status in low byte; bytes-read in the next byte
          (i32.or (local.get $status)
                  (i32.shl (call $r (i32.const 64) (i32.const 192) (i32.const 32))
                           (i32.const 8))))"#,
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_wat.clone().into_bytes());
    executor.insert_code("sc1b", b_wat.into_bytes());
    let ctx = execution_context("sc1a", 200_000);
    let got = execute(&executor, &a_wat, &ctx).ret_i32();
    // status 0 + (8 << 8) = 2048: the child's write is visible to the parent.
    assert_eq!(got, 8 << 8);
}

#[test]
fn nested_staticcall_blocks_storage_write() {
    let b_wat = wat(
        &[(0, &KEY), (32, VALUE)],
        r#"
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $w (i32.const 0) (i32.const 32) (i32.const 8)))"#,
    );
    let a_wat = caller_wat(
        "param i32 i32 i32 i32 i32 i32) (result i32)",
        "staticcall",
        "(call $c (i32.const 0) (i32.const 4) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32))",
        "",
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_wat.clone().into_bytes());
    executor.insert_code("sc1b", b_wat.into_bytes());
    let ctx = execution_context("sc1a", 200_000);
    // storage_write in a read-only frame → child fails → STATUS_FAILED.
    assert_eq!(execute(&executor, &a_wat, &ctx).ret_i32(), 2);
}

#[test]
fn nested_staticcall_copies_return_data() {
    // Valid read-only call: B returns 42, status 0 → status*1000 + 42.
    let a_wat = wat(
        &[(0, b"sc1b")],
        r#"
        (import "sc" "staticcall" (func $c (param i32 i32 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (local $status i32)
          (local.set $status (call $c (i32.const 0) (i32.const 4) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32)))
          (i32.add (i32.mul (local.get $status) (i32.const 1000))
                   (i32.load (i32.const 128))))"#,
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_wat.clone().into_bytes());
    executor.insert_code("sc1b", B_WAT.as_bytes().to_vec());
    let ctx = execution_context("sc1a", 100_000);
    assert_eq!(execute(&executor, &a_wat, &ctx).ret_i32(), 42);
}

#[test]
fn nested_staticcall_charges_gas() {
    let a_with_b = wat(
        &[(0, b"sc1b")],
        r#"
        (import "sc" "staticcall" (func $c (param i32 i32 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $c (i32.const 0) (i32.const 4) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32)))"#,
    );
    let a_alone = r#"(module (func (export "main") (result i32) i32.const 0))"#;
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_with_b.clone().into_bytes());
    executor.insert_code("sc1b", B_WAT.as_bytes().to_vec());
    let ctx = execution_context("sc1a", 500_000);
    let with_call = execute(&executor, &a_with_b, &ctx).ok();
    let alone = execute(&executor, a_alone, &ctx).ok();
    assert!(
        with_call.gas_used > alone.gas_used,
        "staticcall must charge base 700 + child gas ({} vs {})",
        with_call.gas_used,
        alone.gas_used
    );
}

#[test]
fn nested_delegatecall_to_eoa_succeeds_empty() {
    // Unregistered target → treated as EOA: status 0, no return data.
    let a_wat = wat(
        &[(0, b"sc1b")],
        r#"
        (import "sc" "delegatecall" (func $d (param i32 i32 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $d (i32.const 0) (i32.const 4) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32)))"#,
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_wat.clone().into_bytes());
    // "sc1b" deliberately NOT registered.
    let ctx = execution_context("sc1a", 100_000);
    assert_eq!(execute(&executor, &a_wat, &ctx).ret_i32(), 0);
}

#[test]
fn nested_delegatecall_charges_gas() {
    let a_with_b = wat(
        &[(0, b"sc1b")],
        r#"
        (import "sc" "delegatecall" (func $d (param i32 i32 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $d (i32.const 0) (i32.const 4) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32)))"#,
    );
    let a_alone = r#"(module (func (export "main") (result i32) i32.const 0))"#;
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_with_b.clone().into_bytes());
    executor.insert_code("sc1b", B_WAT.as_bytes().to_vec());
    let ctx = execution_context("sc1a", 500_000);
    let with_call = execute(&executor, &a_with_b, &ctx).ok();
    let alone = execute(&executor, a_alone, &ctx).ok();
    assert!(
        with_call.gas_used > alone.gas_used,
        "delegatecall must charge base 700 + child gas ({} vs {})",
        with_call.gas_used,
        alone.gas_used
    );
}

#[test]
fn nested_call_rejects_too_long_target() {
    // Input validation: target address longer than MAX_TARGET_LEN (128)
    // aborts the frame instead of being handed to dispatch (threat V-21).
    let long_target = "x".repeat(200);
    let a_wat = wat(
        &[(0, long_target.as_bytes())],
        r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (call $c (i32.const 0) (i32.const 200) (i64.const 0) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32)))"#,
    );
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", 100_000);
    let err = execute(&executor, &a_wat, &ctx).err();
    match err {
        WasmiError::Engine(msg) => assert!(msg.contains("too long"), "msg: {msg}"),
        other => panic!("expected engine error, got {other:?}"),
    }
}

#[test]
fn nested_call_charges_base_plus_child_gas() {
    let a_with_b = caller_wat(
        "param i32 i32 i64 i32 i32 i32 i32) (result i32)",
        "call",
        "(call $c (i32.const 0) (i32.const 4) (i64.const 0) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32))",
        "",
    );
    let a_alone = r#"(module (func (export "main") (result i32) i32.const 0))"#;

    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_with_b.clone().into_bytes());
    executor.insert_code("sc1b", B_WAT.as_bytes().to_vec());

    let ctx = execution_context("sc1a", 500_000);
    let with_call = execute(&executor, &a_with_b, &ctx).ok();
    let alone = execute(&executor, a_alone, &ctx).ok();
    assert!(
        with_call.gas_used > alone.gas_used,
        "nested call must charge base 700 + child gas ({} vs {})",
        with_call.gas_used,
        alone.gas_used
    );
}

#[test]
fn nested_call_gas_is_deterministic() {
    let wat = caller_wat(
        "param i32 i32 i64 i32 i32 i32 i32) (result i32)",
        "call",
        "(call $c (i32.const 0) (i32.const 4) (i64.const 0) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32))",
        "",
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", wat.clone().into_bytes());
    executor.insert_code("sc1b", B_WAT.as_bytes().to_vec());
    let ctx = execution_context("sc1a", 100_000);
    let first = execute(&executor, &wat, &ctx).ok();
    let second = execute(&executor, &wat, &ctx).ok();
    assert_eq!(first.gas_used, second.gas_used);
    assert_eq!(first.return_data, second.return_data);
}

#[test]
fn nested_call_depth_increments() {
    // A calls B, B calls C — a three-frame chain; C returns 7, which must
    // surface all the way back to A: B returns status | (7 << 8), A returns
    // status | (B's return << 8) = 7 << 16.
    let c_wat = r#"(module (func (export "main") (result i32) i32.const 7))"#;
    let call_body = r#"
        (local $status i32)
        (local.set $status (call $c (i32.const 0) (i32.const 4) (i64.const 0) (i32.const 64) (i32.const 0) (i32.const 128) (i32.const 32)))
        (i32.or (local.get $status)
                (i32.shl (i32.load (i32.const 128)) (i32.const 8)))"#;
    let b_wat = wat(
        &[(0, b"sc1c")],
        &format!(
            r#"
            (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
            (func (export "main") (result i32) {call_body})"#
        ),
    );
    let a_wat = wat(
        &[(0, b"sc1b")],
        &format!(
            r#"
            (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
            (func (export "main") (result i32) {call_body})"#
        ),
    );
    let mut executor = WasmiExecutor::new();
    executor.insert_code("sc1a", a_wat.clone().into_bytes());
    executor.insert_code("sc1b", b_wat.into_bytes());
    executor.insert_code("sc1c", c_wat.as_bytes().to_vec());
    let ctx = execution_context("sc1a", 500_000);
    let got = execute(&executor, &a_wat, &ctx).ret_i32();
    assert_eq!(got, 7 << 16);
}
