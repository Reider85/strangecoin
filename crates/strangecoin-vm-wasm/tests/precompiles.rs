//! S1.5-P11: precompile unit tests (ADR-0015) — valid input, invalid input,
//! gas accounting, `bls_aggregate` NotImplemented, dispatcher routing, and
//! `WasmiExecutor::execute` integration (no WASM bytecode involved).

use std::collections::HashMap;

use secp256k1::{Message, Secp256k1};
use strangecoin_core::address::is_precompile_address;
use strangecoin_core::types::{AccountState, BlockHeader};
use strangecoin_core::vm::{BlockContext, ExecutionContext, TxStatus, VmError, VmExecutor};
use strangecoin_vm_wasm::precompiles::{
    blake3, bls_aggregate, dispatch_precompile, ecrecover, ed25519_verify, keccak, modexp,
    sha256, PrecompileError,
};
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

fn execution_context(callee: &str, input: Vec<u8>, gas_limit: u64) -> ExecutionContext {
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
        storage: Default::default(),
        storage_overlay: HashMap::new(),
        storage_address: callee.to_string(),
        input,
    }
}

// ---------------------------------------------------------------------------
// 0x01 ecrecover
// ---------------------------------------------------------------------------

fn ecrecover_input_ok() -> (Vec<u8>, secp256k1::PublicKey) {
    let secp = Secp256k1::new();
    let sk = secp256k1::SecretKey::from_slice(&[1u8; 32]).expect("static secret key");
    let pubkey = secp256k1::PublicKey::from_secret_key(&secp, &sk);
    let digest = [42u8; 32];
    let msg = Message::from_digest_slice(&digest).expect("digest");
    let (rec_id, compact) = secp.sign_ecdsa_recoverable(&msg, &sk).serialize_compact();

    let mut input = Vec::with_capacity(ecrecover::INPUT_LEN);
    input.extend_from_slice(&digest);
    input.push(27 + u8::try_from(rec_id.to_i32()).expect("rec_id"));
    input.extend_from_slice(&compact);
    (input, pubkey)
}

#[test]
fn ecrecover_recovers_compressed_pubkey() {
    let (input, pubkey) = ecrecover_input_ok();
    let out = ecrecover::run(&input).expect("valid ecrecover");
    assert_eq!(out.output, pubkey.serialize().to_vec());
    assert_eq!(out.output.len(), 33);
    assert_eq!(out.gas_used, 3000);
}

#[test]
fn ecrecover_short_input_is_invalid() {
    let err = ecrecover::run(&[0u8; 96]).expect_err("96 < 97");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
}

#[test]
fn ecrecover_bad_v_is_invalid() {
    let (mut input, _) = ecrecover_input_ok();
    input[32] = 26;
    let err = ecrecover::run(&input).expect_err("v must be 27/28");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
}

#[test]
fn ecrecover_tampered_signature_returns_empty() {
    let (mut input, _) = ecrecover_input_ok();
    input[50] ^= 0xff;
    let out = ecrecover::run(&input).expect("well-formed input must not error");
    assert!(out.output.is_empty());
    assert_eq!(out.gas_used, 3000);
}

// ---------------------------------------------------------------------------
// 0x02 sha256
// ---------------------------------------------------------------------------

#[test]
fn sha256_known_vectors_and_gas() {
    let empty = sha256::run(b"").expect("empty input");
    assert_eq!(
        hex::encode(&empty.output),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(empty.gas_used, 50);

    let abc = sha256::run(b"abc").expect("abc");
    assert_eq!(
        hex::encode(&abc.output),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(abc.gas_used, 62);

    let word_boundary = sha256::run(&[0u8; 33]).expect("33 bytes");
    assert_eq!(word_boundary.gas_used, 74);
}

#[test]
fn sha256_output_is_always_32_bytes() {
    for len in [0usize, 1, 31, 32, 64, 100] {
        let out = sha256::run(&vec![0u8; len]).expect("any length");
        assert_eq!(out.output.len(), 32, "len={len}");
    }
}

// ---------------------------------------------------------------------------
// 0x03 keccak (Ethereum variant)
// ---------------------------------------------------------------------------

#[test]
fn keccak_empty_is_ethereum_vector() {
    let out = keccak::run(b"").expect("empty input");
    assert_eq!(
        hex::encode(&out.output),
        "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
    );
    assert_eq!(out.gas_used, 30);
}

#[test]
fn keccak_gas_word_schedule() {
    assert_eq!(keccak::run(&[0u8; 32]).expect("32").gas_used, 36);
    assert_eq!(keccak::run(&[0u8; 33]).expect("33").gas_used, 42);
    assert_eq!(keccak::run(&[0u8; 64]).expect("64").gas_used, 42);
}

#[test]
fn keccak_differs_from_sha3_nist_padding() {
    // Keccak-256 (0x01 padding) must not equal NIST SHA3-256 (0x06).
    // SHA3-256("") = a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a
    let out = keccak::run(b"").expect("empty");
    assert_ne!(
        hex::encode(&out.output),
        "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
    );
}

// ---------------------------------------------------------------------------
// 0x04 blake3
// ---------------------------------------------------------------------------

#[test]
fn blake3_known_vector_and_gas() {
    let empty = blake3::run(b"").expect("empty input");
    assert_eq!(
        hex::encode(&empty.output),
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );
    assert_eq!(empty.gas_used, 50);

    let chunk_boundary = blake3::run(&[0u8; 64]).expect("64 bytes");
    assert_eq!(chunk_boundary.gas_used, 56);

    let past_boundary = blake3::run(&[0u8; 65]).expect("65 bytes");
    assert_eq!(past_boundary.gas_used, 62);
}

#[test]
fn blake3_matches_core_chain_hash() {
    // Same primitive as canonical serialization (ADR-0015).
    let data = b"strangecoin";
    let out = blake3::run(data).expect("run");
    assert_eq!(
        out.output.to_vec(),
        ::blake3::hash(data).as_bytes().to_vec()
    );
}

#[test]
fn blake3_output_is_32_bytes() {
    for len in [0usize, 1, 100, 1000] {
        let out = blake3::run(&vec![7u8; len]).expect("any length");
        assert_eq!(out.output.len(), 32, "len={len}");
    }
}

// ---------------------------------------------------------------------------
// 0x05 ed25519_verify (RFC 8032 vector 1: empty message)
// ---------------------------------------------------------------------------

const ED25519_PK_HEX: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const ED25519_SIG_HEX: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";

fn ed25519_input(msg: &[u8]) -> Vec<u8> {
    let mut input = hex::decode(ED25519_PK_HEX).expect("pk hex");
    input.extend_from_slice(&hex::decode(ED25519_SIG_HEX).expect("sig hex"));
    input.extend_from_slice(msg);
    input
}

#[test]
fn ed25519_valid_signature_returns_one() {
    let out = ed25519_verify::run(&ed25519_input(b"")).expect("valid");
    assert_eq!(out.output, {
        let mut one = vec![0u8; 32];
        one[31] = 1;
        one
    });
    assert_eq!(out.gas_used, 2000);
}

#[test]
fn ed25519_tampered_signature_returns_zero() {
    let mut input = ed25519_input(b"");
    input[40] ^= 0x01;
    let out = ed25519_verify::run(&input).expect("well-formed");
    assert_eq!(out.output, vec![0u8; 32]);
    assert_eq!(out.gas_used, 2000);
}

#[test]
fn ed25519_wrong_message_returns_zero() {
    let out = ed25519_verify::run(&ed25519_input(b"x")).expect("well-formed");
    assert_eq!(out.output, vec![0u8; 32]);
}

#[test]
fn ed25519_short_input_is_invalid() {
    let err = ed25519_verify::run(&[0u8; 95]).expect_err("95 < 96");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
}

#[test]
fn ed25519_bad_pubkey_returns_zero() {
    let mut input = vec![0xffu8; 32]; // not a valid compressed point
    input.extend_from_slice(&hex::decode(ED25519_SIG_HEX).expect("sig hex"));
    let out = ed25519_verify::run(&input).expect("well-formed length");
    assert_eq!(out.output, vec![0u8; 32]);
}

// ---------------------------------------------------------------------------
// 0x06 bls_aggregate — NotImplemented until Stage 7
// ---------------------------------------------------------------------------

#[test]
fn bls_aggregate_always_not_implemented_with_declared_gas() {
    for data in [vec![], vec![0u8; 96], vec![1u8; 1024]] {
        let err = bls_aggregate::run(&data).expect_err("NotImplemented");
        match err {
            PrecompileError::NotImplemented { feature, gas_used } => {
                assert_eq!(feature, "bls_aggregate");
                assert_eq!(gas_used, 12000);
            }
            other => panic!("expected NotImplemented, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// 0x07 modexp
// ---------------------------------------------------------------------------

fn modexp_input(base: &[u8], exp: &[u8], modulus: &[u8]) -> Vec<u8> {
    let mut input = Vec::new();
    input.extend_from_slice(&len32(base.len()));
    input.extend_from_slice(&len32(exp.len()));
    input.extend_from_slice(&len32(modulus.len()));
    input.extend_from_slice(base);
    input.extend_from_slice(exp);
    input.extend_from_slice(modulus);
    input
}

fn len32(len: usize) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&(len as u64).to_be_bytes());
    out
}

#[test]
fn modexp_two_pow_ten_mod_1000() {
    // 2^10 mod 1000 = 24 = 0x0018
    let input = modexp_input(&[0x02], &[0x0a], &[0x03, 0xe8]);
    let out = modexp::run(&input).expect("valid");
    assert_eq!(out.output, vec![0x00, 0x18]);
    assert_eq!(out.gas_used, 50 + 50 * 2);
}

#[test]
fn modexp_three_pow_five_mod_13() {
    // 3^5 = 243 = 18*13 + 9
    let input = modexp_input(&[0x03], &[0x05], &[0x0d]);
    let out = modexp::run(&input).expect("valid");
    assert_eq!(out.output, vec![0x09]);
}

#[test]
fn modexp_zero_exponent_is_one() {
    let input = modexp_input(&[0x07], &[0x00], &[0x11]); // 7^0 mod 17 = 1
    let out = modexp::run(&input).expect("valid");
    assert_eq!(out.output, vec![0x01]);
}

#[test]
fn modexp_zero_modulus_is_empty() {
    let input = modexp_input(&[0x02], &[0x03], &[]);
    let out = modexp::run(&input).expect("valid");
    assert!(out.output.is_empty());
    assert_eq!(out.gas_used, 50);
}

#[test]
fn modexp_modulus_one_is_zero() {
    let input = modexp_input(&[0x05], &[0x05], &[0x01]);
    let out = modexp::run(&input).expect("valid");
    assert_eq!(out.output, vec![0x00]);
}

#[test]
fn modexp_output_padded_to_mod_len() {
    // 2^8 mod 251 = 5, one-byte modulus → one-byte output
    let input = modexp_input(&[0x02], &[0x08], &[0xfb]);
    let out = modexp::run(&input).expect("valid");
    assert_eq!(out.output, vec![0x05]);

    // 2^3 mod 0x0100 = 8, two-byte modulus → two-byte left-padded output
    let input = modexp_input(&[0x02], &[0x03], &[0x01, 0x00]);
    let out = modexp::run(&input).expect("valid");
    assert_eq!(out.output, vec![0x00, 0x08]);
}

#[test]
fn modexp_truncated_header_is_invalid() {
    let err = modexp::run(&[0u8; 95]).expect_err("95 < 96");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
}

#[test]
fn modexp_truncated_body_is_invalid() {
    let mut input = modexp_input(&[0x02], &[0x02], &[0x03]);
    input.pop();
    let err = modexp::run(&input).expect_err("body shorter than lengths");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
}

#[test]
fn modexp_huge_length_header_is_invalid() {
    let mut input = vec![0xffu8; 32]; // base_len = 2^64-1 → overflow
    input.extend_from_slice(&len32(1));
    input.extend_from_slice(&len32(1));
    let err = modexp::run(&input).expect_err("length overflow");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

#[test]
fn dispatcher_routes_every_reserved_address() {
    // 0x01, structurally valid (v=27) but unrecoverable r/s → empty output.
    let mut ecrecover_input = vec![0u8; 97];
    ecrecover_input[32] = 27;
    let out = dispatch_precompile("0x01", &ecrecover_input).expect("ecrecover");
    assert!(out.output.is_empty());

    let sha = dispatch_precompile("0x02", b"abc").expect("sha256");
    assert_eq!(sha.output.len(), 32);

    let kec = dispatch_precompile("0x03", b"").expect("keccak");
    assert_eq!(kec.output.len(), 32);

    let bl = dispatch_precompile("0x04", b"").expect("blake3");
    assert_eq!(bl.output.len(), 32);

    let ed = dispatch_precompile("0x05", &ed25519_input(b"")).expect("ed25519");
    assert_eq!(ed.output.len(), 32);

    let err = dispatch_precompile("0x06", b"").expect_err("bls");
    assert!(matches!(err, PrecompileError::NotImplemented { .. }));

    let mx = dispatch_precompile("0x07", &modexp_input(&[2], &[3], &[7])).expect("modexp");
    assert_eq!(mx.output, vec![1]); // 2^3 mod 7 = 1
}

#[test]
fn dispatcher_unknown_address_is_invalid() {
    let err = dispatch_precompile("0x08", b"").expect_err("not a precompile");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
    let err = dispatch_precompile("sc1someone", b"").expect_err("bech32");
    assert!(matches!(err, PrecompileError::InvalidInput(_)));
}

#[test]
fn core_reserved_range_matches_dispatcher() {
    for addr in ["0x01", "0x02", "0x03", "0x04", "0x05", "0x06", "0x07"] {
        assert!(is_precompile_address(addr));
        // Dispatch never panics on the reserved range (empty input).
        let _ = dispatch_precompile(addr, b"");
    }
    assert!(!is_precompile_address("0x00"));
    assert!(!is_precompile_address("0x08"));
}

// ---------------------------------------------------------------------------
// WasmiExecutor integration
// ---------------------------------------------------------------------------

#[test]
fn execute_sha256_precompile_without_wasm() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("0x02", b"abc".to_vec(), 100_000);
    let result = executor.execute(ctx).expect("precompile call");
    assert_eq!(result.status, TxStatus::Success);
    assert_eq!(
        hex::encode(&result.return_data),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(result.gas_used, 62);
}

#[test]
fn execute_bls_precompile_reverts_with_declared_gas() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("0x06", Vec::new(), 100_000);
    let result = executor.execute(ctx).expect("precompile call");
    assert_eq!(result.status, TxStatus::Reverted);
    assert_eq!(result.gas_used, 12_000);
    assert!(result.return_data.is_empty());
}

#[test]
fn execute_ecrecover_invalid_input_burns_full_frame() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("0x01", vec![0u8; 10], 4242);
    let result = executor.execute(ctx).expect("precompile call");
    assert_eq!(result.status, TxStatus::Reverted);
    assert_eq!(result.gas_used, 4242);
}

#[test]
fn execute_precompile_out_of_gas_when_budget_below_declared() {
    let executor = WasmiExecutor::new();
    // Structurally valid input: declared ecrecover gas 3000 > budget 100.
    let (input, _) = ecrecover_input_ok();
    let ctx = execution_context("0x01", input, 100);
    let err = executor.execute(ctx).expect_err("3000 > 100");
    let vm_err: VmError = err.into();
    assert_eq!(vm_err, VmError::OutOfGas);
}

#[test]
fn execute_non_precompile_still_requires_code() {
    let executor = WasmiExecutor::new();
    let ctx = execution_context("sc1contract", Vec::new(), 100_000);
    let err = executor.execute(ctx).expect_err("no code registered");
    assert!(matches!(err, WasmiError::NoCode { .. }));
}

#[test]
fn execute_precompile_does_not_consume_wasmi_registry() {
    // A precompile address must win even if someone registered "code" under it.
    let mut executor = WasmiExecutor::new();
    executor.insert_code("0x02", b"not wasm".to_vec());
    let ctx = execution_context("0x02", b"abc".to_vec(), 100_000);
    let result = executor.execute(ctx).expect("precompile wins");
    assert_eq!(result.gas_used, 62);
}
