//! S1.5-P17: every precompile callable from a contract through the block
//! pipeline (ADR-0015) — nested `sc.call` routing, output surfacing via
//! event logs, and revert semantics for the NotImplemented entry.

#![cfg(feature = "vm-wasm")]

mod common;

use common::vm_helpers::{child_block, commit_block, derive, init_chain, keypair, sign, wat};
use secp256k1::{Message, Secp256k1};
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::types::Transaction;
use strangecoin_core::vm::TxStatus;
use strangecoin_vm_wasm::precompiles::ecrecover;

const TOPIC: [u8; 32] = [0x55u8; 32];
const STATUS_KEY: [u8; 32] = [0x75u8; 32];

/// Contract that nested-calls the precompile at `addr` with `input`, emits
/// the child's return data as a single-topic log and persists the
/// nested-call status byte (0 = OK, 1 = reverted, 2 = failed) under
/// [`STATUS_KEY`].
fn precompile_call_wat(addr: &str, input: &[u8], out_len: usize) -> String {
    let addr_len = addr.len();
    let input_len = input.len();
    wat(
        &[
            (0, addr.as_bytes()),
            (32, &STATUS_KEY),
            (64, &TOPIC),
            (128, input),
        ],
        &format!(
            r#"
        (import "sc" "call" (func $c (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
        (import "sc" "emit_log" (func $l (param i32 i32 i32 i32) (result i32)))
        (import "sc" "storage_write" (func $w (param i32 i32 i32) (result i32)))
        (func (export "main") (result i32)
          (local $status i32)
          (local.set $status
            (call $c (i32.const 0) (i32.const {addr_len}) (i64.const 0)
                     (i32.const 128) (i32.const {input_len})
                     (i32.const 1024) (i32.const {out_len})))
          (drop (call $l (i32.const 64) (i32.const 1) (i32.const 1024) (i32.const {out_len})))
          (i32.store (i32.const 96) (local.get $status))
          (drop (call $w (i32.const 32) (i32.const 96) (i32.const 1)))
          (local.get $status))"#
        ),
    )
}

/// Deploy the precompile-caller contract and call it; returns the precompile
/// output (log data) and the persisted nested-call status byte.
fn call_precompile(precompile: &str, input: &[u8], out_len: usize) -> (Vec<u8>, u8) {
    let (alice, alice_sk) = keypair();
    let (mut chain, mut state) = init_chain(&alice, 10_000);
    let mut codes = std::collections::HashMap::new();

    let wasm = precompile_call_wat(precompile, input, out_len);
    let create = sign(
        Transaction::create(
            &alice,
            wasm.clone().into_bytes(),
            vec![],
            0,
            0,
            CHAIN_ID_REGTEST,
        ),
        &alice_sk,
    );
    let create_block = child_block(chain.last().unwrap(), vec![create]);
    state = commit_block(&mut chain, &state, create_block, &codes);
    let contract = derive(&alice, 0);
    codes.insert(contract.clone(), wasm.into_bytes());

    let call = sign(
        Transaction::call(&alice, &contract, vec![], 0, 200_000, 1, CHAIN_ID_REGTEST),
        &alice_sk,
    );
    let call_block = child_block(chain.last().unwrap(), vec![call]);
    state = commit_block(&mut chain, &state, call_block, &codes);

    let receipt = &state.receipts[1];
    assert_eq!(receipt.status, TxStatus::Success, "outer frame must succeed");
    assert_eq!(receipt.logs.len(), 1);
    assert_eq!(receipt.logs[0].topics, vec![TOPIC]);
    let status = state
        .contract_storage
        .get(&contract)
        .and_then(|storage| storage.get(&STATUS_KEY))
        .expect("status slot must be flushed")
        .first()
        .copied()
        .expect("status byte");
    (receipt.logs[0].data.clone(), status)
}

fn ecrecover_input_ok() -> (Vec<u8>, Vec<u8>) {
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
    (input, pubkey.serialize().to_vec())
}

fn ed25519_valid_input() -> Vec<u8> {
    let pk = hex::decode(
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
    )
    .expect("rfc8032 pk");
    let sig = hex::decode(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    )
    .expect("rfc8032 sig");
    let mut input = pk;
    input.extend_from_slice(&sig);
    input // empty message
}

fn modexp_input(base: &[u8], exp: &[u8], modulus: &[u8]) -> Vec<u8> {
    fn len32(len: usize) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[24..].copy_from_slice(&(len as u64).to_be_bytes());
        out
    }
    let mut input = Vec::new();
    input.extend_from_slice(&len32(base.len()));
    input.extend_from_slice(&len32(exp.len()));
    input.extend_from_slice(&len32(modulus.len()));
    input.extend_from_slice(base);
    input.extend_from_slice(exp);
    input.extend_from_slice(modulus);
    input
}

#[test]
fn precompile_0x01_ecrecover_from_contract() {
    let (input, expected_pubkey) = ecrecover_input_ok();
    let (output, status) = call_precompile("0x01", &input, 33);
    assert_eq!(status, 0, "valid ecrecover must return STATUS_OK");
    assert_eq!(output, expected_pubkey);
}

#[test]
fn precompile_0x02_sha256_from_contract() {
    let (output, status) = call_precompile("0x02", b"", 32);
    assert_eq!(status, 0);
    assert_eq!(
        hex::encode(&output),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn precompile_0x03_keccak_from_contract() {
    let (output, status) = call_precompile("0x03", b"", 32);
    assert_eq!(status, 0);
    assert_eq!(
        hex::encode(&output),
        "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
    );
}

#[test]
fn precompile_0x04_blake3_from_contract() {
    let (output, status) = call_precompile("0x04", b"", 32);
    assert_eq!(status, 0);
    assert_eq!(
        hex::encode(&output),
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );
}

#[test]
fn precompile_0x05_ed25519_verify_from_contract() {
    let input = ed25519_valid_input();
    let (output, status) = call_precompile("0x05", &input, 32);
    assert_eq!(status, 0);
    let mut one = vec![0u8; 32];
    one[31] = 1;
    assert_eq!(output, one, "valid RFC 8032 signature must yield 1");
}

#[test]
fn precompile_0x06_bls_aggregate_reverts_as_not_implemented() {
    let (output, status) = call_precompile("0x06", &[0u8; 96], 0);
    assert_eq!(status, 1, "NotImplemented must surface as STATUS_REVERTED");
    assert!(output.is_empty());
}

#[test]
fn precompile_0x07_modexp_from_contract() {
    // 2^10 mod 1000 = 24 = 0x0018
    let input = modexp_input(&[0x02], &[0x0a], &[0x03, 0xe8]);
    let (output, status) = call_precompile("0x07", &input, 2);
    assert_eq!(status, 0);
    assert_eq!(output, vec![0x00, 0x18]);
}
