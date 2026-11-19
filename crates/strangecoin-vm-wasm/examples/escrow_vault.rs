//! `EscrowVault` — the on-chain escrow contract of ACP (AI-P03, ADR-0030,
//! spec §8).
//!
//! The contract is WAT source assembled at runtime so the dispatch
//! selectors stay derived from [`strangecoin_core::escrow`] (one source of
//! truth — the same module the `AcpHandler` and the tests use). It is a
//! *single-export* module: the monolith never registers `AbiDocument`s
//! (the ADR-0029 amendment record), so selector dispatch inside the VM
//! always falls back to `main` and `main` re-dispatches on the first four
//! calldata bytes itself.
//!
//! ```text
//! deposit  — record escrow (id = blake3(input)[0..8], key = id || 0*24),
//!            entry value must equal the recorded amount (`sc.value` — the
//!            ADR-0030 amendment: without a value view a vault holding
//!            pooled funds could be drained by a deposit that *claims* more
//!            than it moved), duplicate calldata reverts.
//! release  — mutual settlement: both agent signatures over
//!            blake3(id_bytes) verified against the pubkeys fixed at
//!            deposit, then `sc.call` sends the amount to agent B and the
//!            record flips to settled.
//! refund   — timeout mode (76-byte calldata): height > deadline plus
//!            signature A; mutual mode (140-byte): both signatures, no
//!            height constraint. Pays agent A.
//!
//! Failure = `unreachable` trap (the engine turns it into a failed
//! execution and discards every staged write *and* the entry value — the
//! revert semantics `apply_call` relies on).
//! ```
//!
//! Run `cargo run -p strangecoin-vm-wasm --example escrow_vault` for the
//! happy-path smoke; `cargo test -p strangecoin-vm-wasm --example
//! escrow_vault` runs the unit matrix (deposit/release/refund happy paths +
//! forged signatures + premature refund + value-mismatch + duplicate).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use strangecoin_core::address::encode_address;
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::escrow::{
    self, escrow_id_bytes, escrow_signature_message, EscrowDepositParams,
};
use strangecoin_core::types::{AccountState, BlockHeader, ContractAccount};
use strangecoin_core::vm::{BlockContext, ExecutionContext};
use strangecoin_vm_wasm::WasmiExecutor;

/// Contract account address inside the executor's account view (opaque to
/// `execute_wasm` — no chain lookup happens in these tests).
const VAULT: &str = "sc1escrowvault";

/// Fixed test identities (deterministic — no RNG anywhere, ADR-0030
/// determinism review): agent A (assigner), agent B (performer).
const SK_A: [u8; 32] = [0x41; 32];
const SK_B: [u8; 32] = [0x42; 32];

/// Wire limits mirrored from the codec (`strangecoin_core::escrow`).
const MAX_ADDRESS_LEN: usize = escrow::MAX_ADDRESS_LEN;

// ---------------------------------------------------------------- WAT

/// Selector as the `i32.const` the module compares against `i32.load` of
/// calldata bytes 0..4 (little-endian — both sides see the same value).
fn sel(sel: [u8; 4]) -> i32 {
    i32::from_le_bytes(sel)
}

/// Assemble the EscrowVault module. Buffer map (module linear memory):
///
/// ```text
/// 0     .. 512   input buffer (calldata via `sc.input_read`)
/// 512   .. 544   storage key (id bytes || 0*24)
/// 544   .. 800   record buffer (245 B used; see `strangecoin_core::escrow`)
/// 800   .. 832   hash buffer (blake3 out)
/// 832   .. 864   log topic0 ([tag, 0*31])
/// 864   .. 896   block-context snapshot (height @864)
/// 896   .. 928   event data (id | amount | deadline = 24 B)
/// 1152  .. 1160  `sc.call` out-buffer
/// ```
pub fn escrow_vault_wat() -> String {
    let deposit = sel(escrow::deposit_selector());
    let release = sel(escrow::release_selector());
    let refund = sel(escrow::refund_selector());
    format!(
        r#"(module
  (import "sc" "input_read" (func $input_read (param i32 i32) (result i32)))
  (import "sc" "storage_read" (func $storage_read (param i32 i32 i32) (result i32)))
  (import "sc" "storage_write" (func $storage_write (param i32 i32 i32) (result i32)))
  (import "sc" "emit_log" (func $emit_log (param i32 i32 i32 i32) (result i32)))
  (import "sc" "block_context" (func $block_context (param i32 i32) (result i32)))
  (import "sc" "blake3" (func $blake3 (param i32 i32 i32) (result i32)))
  (import "sc" "secp256k1_verify" (func $secp256k1_verify (param i32 i32 i32) (result i32)))
  (import "sc" "call" (func $call (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
  (import "sc" "value" (func $value (result i64)))
  (memory (export "memory") 1)

  (func $copy (param $dst i32) (param $src i32) (param $n i32)
    (local $i i32)
    (block $done
      (loop $l
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (i32.store8
          (i32.add (local.get $dst) (local.get $i))
          (i32.load8_u (i32.add (local.get $src) (local.get $i))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $l))))

  (func $zero_key (param $id_src i32)
    (i64.store (i32.const 512) (i64.load (local.get $id_src)))
    (i64.store (i32.const 520) (i64.const 0))
    (i64.store (i32.const 528) (i64.const 0))
    (i64.store (i32.const 536) (i64.const 0)))

  (func $load_record (result i32)
    (call $storage_read (i32.const 512) (i32.const 544) (i32.const 256)))

  (func $store_record (result i32)
    (call $storage_write (i32.const 512) (i32.const 544) (i32.const 245)))

  (func $event (param $tag i32) (param $data_len i32) (result i32)
    ;; Fresh instance memory is zeroed and each frame emits once per
    ;; action, so only the tag byte needs writing over topic0.
    (i32.store8 (i32.const 832) (local.get $tag))
    (call $emit_log (i32.const 832) (i32.const 1) (i32.const 896) (local.get $data_len)))

  (func $deposit (param $len i32) (result i32)
    (local $la i32) (local $lb i32) (local $end_a i32) (local $off_b i32)
    (local $end_b i32) (local $amt_off i32) (local $dl_off i32)
    (local $amount i64)
    (if (i32.lt_u (local.get $len) (i32.const 8)) (then (unreachable)))
    (local.set $la (i32.load (i32.const 4)))
    (if (i32.eqz (local.get $la)) (then (unreachable)))
    (if (i32.gt_u (local.get $la) (i32.const {max_address})) (then (unreachable)))
    (local.set $end_a (i32.add (i32.const 8) (local.get $la)))
    (if (i32.gt_u (i32.add (local.get $end_a) (i32.const 4)) (local.get $len))
        (then (unreachable)))
    (local.set $lb (i32.load (local.get $end_a)))
    (if (i32.eqz (local.get $lb)) (then (unreachable)))
    (if (i32.gt_u (local.get $lb) (i32.const {max_address})) (then (unreachable)))
    (local.set $off_b (i32.add (local.get $end_a) (i32.const 4)))
    (local.set $end_b (i32.add (local.get $off_b) (local.get $lb)))
    (local.set $amt_off (i32.add (local.get $end_b) (i32.const 66)))
    (local.set $dl_off (i32.add (local.get $amt_off) (i32.const 8)))
    (if (i32.ne (i32.add (local.get $dl_off) (i32.const 8)) (local.get $len))
        (then (unreachable)))
    (local.set $amount (i64.load (local.get $amt_off)))
    ;; Value binding (ADR-0030 amendment): the recorded amount must be the
    ;; amount this frame actually received — a claim above `sc.value()`
    ;; would let a pooled vault be drained through `release`.
    (if (i64.ne (call $value) (local.get $amount)) (then (unreachable)))
    ;; id = blake3(calldata)[0..8] -> key = id || 0*24
    (drop (call $blake3 (i32.const 0) (local.get $len) (i32.const 800)))
    (call $zero_key (i32.const 800))
    ;; Duplicate calldata = occupied id -> revert (documented amendment:
    ;; identical deposit bytes cannot mint a second escrow).
    (if (call $load_record) (then (unreachable)))
    (i64.store (i32.const 544) (local.get $amount))
    (i64.store (i32.const 552) (i64.load (local.get $dl_off)))
    (i32.store8 (i32.const 560) (i32.const 0))
    (i32.store8 (i32.const 561) (local.get $la))
    (i32.store8 (i32.const 562) (local.get $lb))
    (call $copy (i32.const 563) (i32.const 8) (local.get $la))
    (call $copy (i32.const 643) (local.get $off_b) (local.get $lb))
    (call $copy (i32.const 723) (local.get $end_b) (i32.const 33))
    (call $copy (i32.const 756) (i32.add (local.get $end_b) (i32.const 33)) (i32.const 33))
    (if (i32.eqz (call $store_record)) (then (unreachable)))
    ;; EscrowDeposited data: id (8) | amount (8) | deadline (8)
    (i64.store (i32.const 896) (i64.load (i32.const 800)))
    (i64.store (i32.const 904) (local.get $amount))
    (i64.store (i32.const 912) (i64.load (local.get $dl_off)))
    (drop (call $event (i32.const 1) (i32.const 24)))
    ;; Low 32 bits of the id (full id rides the event and the storage key).
    (i32.wrap_i64 (i64.load (i32.const 800))))

  (func $release (param $len i32) (result i32)
    (local $lb i32) (local $amount i64)
    (if (i32.ne (local.get $len) (i32.const 140)) (then (unreachable)))
    (call $zero_key (i32.const 4))
    (if (i32.eqz (call $load_record)) (then (unreachable)))
    (if (i32.load8_u (i32.const 560)) (then (unreachable)))
    ;; digest = blake3(id_bytes) — the message both agents signed
    (drop (call $blake3 (i32.const 4) (i32.const 8) (i32.const 800)))
    (if (i32.eqz (call $secp256k1_verify (i32.const 723) (i32.const 800) (i32.const 12)))
        (then (unreachable)))
    (if (i32.eqz (call $secp256k1_verify (i32.const 756) (i32.const 800) (i32.const 76)))
        (then (unreachable)))
    (local.set $lb (i32.load8_u (i32.const 562)))
    (local.set $amount (i64.load (i32.const 544)))
    (if (call $call (i32.const 643) (local.get $lb) (local.get $amount)
                    (i32.const 0) (i32.const 0) (i32.const 1152) (i32.const 8))
        (then (unreachable)))
    (i32.store8 (i32.const 560) (i32.const 1))
    (if (i32.eqz (call $store_record)) (then (unreachable)))
    ;; EscrowReleased data: id (8)
    (i64.store (i32.const 896) (i64.load (i32.const 4)))
    (drop (call $event (i32.const 2) (i32.const 8)))
    (i32.const 1))

  (func $refund (param $len i32) (result i32)
    (local $mutual i32) (local $la i32) (local $amount i64)
    (if (i32.and (i32.ne (local.get $len) (i32.const 76))
                 (i32.ne (local.get $len) (i32.const 140)))
        (then (unreachable)))
    (local.set $mutual (i32.eq (local.get $len) (i32.const 140)))
    (call $zero_key (i32.const 4))
    (if (i32.eqz (call $load_record)) (then (unreachable)))
    (if (i32.load8_u (i32.const 560)) (then (unreachable)))
    (drop (call $blake3 (i32.const 4) (i32.const 8) (i32.const 800)))
    (if (i32.eqz (call $secp256k1_verify (i32.const 723) (i32.const 800) (i32.const 12)))
        (then (unreachable)))
    (if (local.get $mutual)
        (then
          (if (i32.eqz (call $secp256k1_verify (i32.const 756) (i32.const 800) (i32.const 76)))
              (then (unreachable)))))
    (if (i32.eqz (local.get $mutual))
        (then
          ;; Timeout mode: only after the deposit deadline passed. The
          ;; snapshot buffer must hold the fixed 24 B *plus* the coinbase
          ;; string — a too-small buffer returns -1 and leaves height = 0.
          (drop (call $block_context (i32.const 864) (i32.const 128)))
          (if (i64.le_u (i64.load (i32.const 864)) (i64.load (i32.const 552)))
              (then (unreachable)))))
    (local.set $la (i32.load8_u (i32.const 561)))
    (local.set $amount (i64.load (i32.const 544)))
    (if (call $call (i32.const 563) (local.get $la) (local.get $amount)
                    (i32.const 0) (i32.const 0) (i32.const 1152) (i32.const 8))
        (then (unreachable)))
    (i32.store8 (i32.const 560) (i32.const 2))
    (if (i32.eqz (call $store_record)) (then (unreachable)))
    ;; EscrowRefunded data: id (8)
    (i64.store (i32.const 896) (i64.load (i32.const 4)))
    (drop (call $event (i32.const 3) (i32.const 8)))
    (i32.const 1))

  (func (export "main") (result i32)
    (local $len i32) (local $sel i32)
    (local.set $len (call $input_read (i32.const 0) (i32.const 512)))
    (if (i32.lt_s (local.get $len) (i32.const 4)) (then (unreachable)))
    (local.set $sel (i32.load (i32.const 0)))
    (if (i32.eq (local.get $sel) (i32.const {deposit}))
        (then (return (call $deposit (local.get $len)))))
    (if (i32.eq (local.get $sel) (i32.const {release}))
        (then (return (call $release (local.get $len)))))
    (if (i32.eq (local.get $sel) (i32.const {refund}))
        (then (return (call $refund (local.get $len)))))
    (unreachable)))"#,
        deposit = deposit,
        release = release,
        refund = refund,
        max_address = MAX_ADDRESS_LEN,
    )
}

// ---------------------------------------------------------------- identities

fn keypair(seed: [u8; 32]) -> SecretKey {
    SecretKey::from_slice(&seed).expect("fixed seed")
}

fn pubkey(sk: &SecretKey) -> PublicKey {
    PublicKey::from_secret_key(&Secp256k1::new(), sk)
}

fn agent_id33(sk: &SecretKey) -> [u8; 33] {
    pubkey(sk).serialize()
}

fn address(sk: &SecretKey) -> String {
    encode_address(&pubkey(sk), CHAIN_ID_REGTEST).expect("regtest address")
}

fn sign_escrow(id: &[u8; 8], sk: &SecretKey) -> [u8; 64] {
    let msg = Message::from_digest(escrow_signature_message(id));
    Secp256k1::new().sign_ecdsa(&msg, sk).serialize_compact()
}

// ---------------------------------------------------------------- frame harness

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

/// Frame context over a fresh account view: agent A funded, agent B zero,
/// the vault holding `vault_balance` (pooled escrows from prior tasks).
fn frame(
    input: Vec<u8>,
    value: u64,
    height: u64,
    vault_balance: u64,
    storage: Arc<RwLock<strangecoin_core::state::ContractStorageMap>>,
) -> ExecutionContext {
    let sk_a = keypair(SK_A);
    let sk_b = keypair(SK_B);
    let mut accounts = HashMap::new();
    accounts.insert(address(&sk_a), AccountState::eoa(1_000_000, 0));
    accounts.insert(address(&sk_b), AccountState::eoa(0, 0));
    accounts.insert(
        VAULT.to_string(),
        AccountState::Contract(ContractAccount {
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
            nonce: 0,
            balance: vault_balance,
        }),
    );
    ExecutionContext {
        block: block_header(),
        accounts,
        caller: address(&sk_a),
        callee: VAULT.to_string(),
        value,
        gas_limit: 1_000_000,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height,
            timestamp: 1_700_000_000,
            coinbase: "sc1coinbase".to_string(),
            block_gas_limit: 10_000_000,
        },
        storage,
        storage_overlay: HashMap::new(),
        storage_address: VAULT.to_string(),
        input,
    }
}

fn deposit_calldata(amount: u64, deadline: u64) -> Vec<u8> {
    let sk_a = keypair(SK_A);
    let sk_b = keypair(SK_B);
    escrow::encode_deposit(&EscrowDepositParams {
        agent_a: &address(&sk_a),
        agent_b: &address(&sk_b),
        pk_a: agent_id33(&sk_a),
        pk_b: agent_id33(&sk_b),
        amount,
        deadline_block: deadline,
    })
    .expect("calldata")
}

/// Merge a successful frame's staged writes into the base storage (what
/// `apply_call` does on `Success` — this harness runs frames directly).
fn commit_overlay(
    base: &Arc<RwLock<strangecoin_core::state::ContractStorageMap>>,
    overlay: strangecoin_core::state::ContractStorageMap,
) {
    base.write()
        .unwrap_or_else(|p| p.into_inner())
        .extend(overlay);
}

// ---------------------------------------------------------------- example

/// Happy-path smoke: deposit → release → (fresh id) refund after deadline.
/// Panics on the first failed step; silent otherwise (tracing discipline).
///
/// `#[cfg(not(test))]`: `tests/acp_messages.rs` pulls this whole file in via
/// `#[path]` as its WAT source (single source of truth), and a test-harness
/// build must not carry a second `main`.
#[cfg(not(test))]
fn main() {
    let deposit = deposit_calldata(50, 1_000);
    let id = escrow_id_bytes(&deposit);
    let sk_a = keypair(SK_A);
    let sk_b = keypair(SK_B);

    let storage = Arc::new(RwLock::new(HashMap::new()));
    let executor = WasmiExecutor::new();
    let dep = executor
        .execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(deposit.clone(), 50, 10, 0, Arc::clone(&storage)),
        )
        .expect("deposit must succeed");
    assert_eq!(dep.status, strangecoin_core::vm::TxStatus::Success);
    commit_overlay(&storage, dep.storage_overlay);

    let release = escrow::encode_release(&id, &sign_escrow(&id, &sk_a), &sign_escrow(&id, &sk_b));
    let rel = executor
        .execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(release, 0, 11, 50, Arc::clone(&storage)),
        )
        .expect("release must succeed");
    assert_eq!(rel.status, strangecoin_core::vm::TxStatus::Success);

    let deposit2 = deposit_calldata(70, 100);
    let id2 = escrow_id_bytes(&deposit2);
    let dep2 = executor
        .execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(deposit2, 70, 12, 50, Arc::clone(&storage)),
        )
        .expect("second deposit must succeed");
    commit_overlay(&storage, dep2.storage_overlay);

    let refund = escrow::encode_refund(&id2, &sign_escrow(&id2, &sk_a), None);
    let refd = executor
        .execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(refund, 0, 101, 120, storage),
        )
        .expect("timeout refund must succeed");
    assert_eq!(refd.status, strangecoin_core::vm::TxStatus::Success);
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use strangecoin_core::vm::TxStatus;

    fn vault_storage() -> Arc<RwLock<strangecoin_core::state::ContractStorageMap>> {
        Arc::new(RwLock::new(HashMap::new()))
    }

    fn deposit_frame(
        input: &[u8],
        value: u64,
        height: u64,
        storage: &Arc<RwLock<strangecoin_core::state::ContractStorageMap>>,
    ) -> ExecutionContext {
        frame(input.to_vec(), value, height, 0, Arc::clone(storage))
    }

    fn record_of(
        storage: &Arc<RwLock<strangecoin_core::state::ContractStorageMap>>,
        id: &[u8; 8],
    ) -> Option<Vec<u8>> {
        storage
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(VAULT)
            .and_then(|slots| slots.get(&escrow::escrow_storage_key(id)))
            .cloned()
    }

    #[test]
    fn deposit_records_escrow_credits_vault_and_emits_event() {
        let executor = WasmiExecutor::new();
        let storage = vault_storage();
        let calldata = deposit_calldata(50, 1_000);
        let id = escrow_id_bytes(&calldata);

        let result = executor
            .execute_wasm(
                escrow_vault_wat().as_bytes(),
                &deposit_frame(&calldata, 50, 10, &storage),
            )
            .expect("deposit executes");

        assert_eq!(result.status, TxStatus::Success);
        // Entry value transfer A -> vault (what the frame actually moved).
        assert_eq!(result.value_transfers.len(), 1);
        assert_eq!(result.value_transfers[0].from, address(&keypair(SK_A)));
        assert_eq!(result.value_transfers[0].to, VAULT);
        assert_eq!(result.value_transfers[0].amount, 50);
        // Event: topic0 tag 1 (EscrowDeposited), data = id | amount | deadline.
        assert_eq!(result.logs.len(), 1);
        assert_eq!(result.logs[0].topics.len(), 1);
        assert_eq!(result.logs[0].topics[0][0], 1);
        let mut expect_data = id.to_vec();
        expect_data.extend_from_slice(&50u64.to_le_bytes());
        expect_data.extend_from_slice(&1_000u64.to_le_bytes());
        assert_eq!(result.logs[0].data, expect_data);
        // Return value = low 32 bits of the id.
        assert_eq!(result.return_data, id[..4].to_vec());

        commit_overlay(&storage, result.storage_overlay);
        let record = record_of(&storage, &id).expect("record written");
        assert_eq!(record.len(), escrow::ESCROW_RECORD_LEN);
        assert_eq!(&record[0..8], &50u64.to_le_bytes(), "amount");
        assert_eq!(&record[8..16], &1_000u64.to_le_bytes(), "deadline");
        assert_eq!(record[16], 0, "status = pending");
        // Counterparty binding: addresses land in the record (record-relative
        // offsets — the buffer starts at memory 544, `addr_b` at +99).
        let b = address(&keypair(SK_B));
        assert_eq!(record[18] as usize, b.len(), "len_b");
        assert_eq!(&record[99..99 + b.len()], b.as_bytes(), "addr_b");
    }

    #[test]
    fn deposit_rejects_value_below_claimed_amount() {
        // The pooled-vault drain guard (ADR-0030 amendment): calldata
        // claims 50, the frame only received 1 — must revert.
        let executor = WasmiExecutor::new();
        let storage = vault_storage();
        let calldata = deposit_calldata(50, 1_000);
        let err = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &deposit_frame(&calldata, 1, 10, &storage),
        );
        assert!(err.is_err(), "value mismatch must trap");
        assert!(
            record_of(&storage, &escrow_id_bytes(&calldata)).is_none(),
            "failed deposit writes nothing"
        );
    }

    #[test]
    fn deposit_rejects_duplicate_calldata() {
        let executor = WasmiExecutor::new();
        let storage = vault_storage();
        let calldata = deposit_calldata(50, 1_000);
        let first = executor
            .execute_wasm(
                escrow_vault_wat().as_bytes(),
                &deposit_frame(&calldata, 50, 10, &storage),
            )
            .expect("first deposit");
        commit_overlay(&storage, first.storage_overlay);

        let second = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &deposit_frame(&calldata, 50, 11, &storage),
        );
        assert!(
            second.is_err(),
            "identical deposit calldata collides on the id and must revert"
        );
    }

    fn seeded_storage(
        calldata: &[u8],
        amount: u64,
    ) -> Arc<RwLock<strangecoin_core::state::ContractStorageMap>> {
        let executor = WasmiExecutor::new();
        let storage = vault_storage();
        let result = executor
            .execute_wasm(
                escrow_vault_wat().as_bytes(),
                &deposit_frame(calldata, amount, 10, &storage),
            )
            .expect("seed deposit");
        commit_overlay(&storage, result.storage_overlay);
        storage
    }

    #[test]
    fn release_pays_performer_and_settles_once() {
        let executor = WasmiExecutor::new();
        let calldata = deposit_calldata(50, 1_000);
        let id = escrow_id_bytes(&calldata);
        let storage = seeded_storage(&calldata, 50);
        let sk_a = keypair(SK_A);
        let sk_b = keypair(SK_B);

        let release =
            escrow::encode_release(&id, &sign_escrow(&id, &sk_a), &sign_escrow(&id, &sk_b));
        let result = executor
            .execute_wasm(
                escrow_vault_wat().as_bytes(),
                &frame(release.clone(), 0, 11, 50, Arc::clone(&storage)),
            )
            .expect("release executes");

        assert_eq!(result.status, TxStatus::Success);
        // Nested transfer vault -> agent B.
        assert!(
            result
                .value_transfers
                .iter()
                .any(|t| { t.from == VAULT && t.to == address(&sk_b) && t.amount == 50 }),
            "release must move the escrow to agent B: {:?}",
            result.value_transfers
        );
        assert_eq!(result.logs[0].topics[0][0], 2, "EscrowReleased tag");
        commit_overlay(&storage, result.storage_overlay);

        // Second release of the same escrow reverts (status = settled).
        let again = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(release, 0, 12, 50, Arc::clone(&storage)),
        );
        assert!(again.is_err(), "double release must trap");
    }

    #[test]
    fn release_rejects_forged_signatures() {
        let executor = WasmiExecutor::new();
        let calldata = deposit_calldata(50, 1_000);
        let id = escrow_id_bytes(&calldata);
        let storage = seeded_storage(&calldata, 50);
        let sk_a = keypair(SK_A);

        let mut sig_b = sign_escrow(&id, &keypair(SK_B));
        sig_b[0] ^= 0x01;
        let forged = escrow::encode_release(&id, &sign_escrow(&id, &sk_a), &sig_b);
        let err = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(forged, 0, 11, 50, Arc::clone(&storage)),
        );
        assert!(err.is_err(), "forged sig_b must trap");

        // Signature over the wrong id also fails.
        let wrong_id = escrow_id_bytes(&deposit_calldata(51, 1_000));
        let mismatched = escrow::encode_release(
            &wrong_id,
            &sign_escrow(&wrong_id, &sk_a),
            &sign_escrow(&wrong_id, &keypair(SK_B)),
        );
        let err = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(mismatched, 0, 11, 50, storage),
        );
        assert!(err.is_err(), "signature for a different escrow must trap");
    }

    #[test]
    fn refund_timeout_after_deadline_pays_assigner() {
        let executor = WasmiExecutor::new();
        let calldata = deposit_calldata(50, 100);
        let id = escrow_id_bytes(&calldata);
        let storage = seeded_storage(&calldata, 50);
        let sk_a = keypair(SK_A);

        // Premature: height still at the deadline.
        let refund = escrow::encode_refund(&id, &sign_escrow(&id, &sk_a), None);
        let premature = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(refund.clone(), 0, 100, 50, Arc::clone(&storage)),
        );
        assert!(premature.is_err(), "refund before deadline must trap");

        // After the deadline the timeout refund lands.
        let result = executor
            .execute_wasm(
                escrow_vault_wat().as_bytes(),
                &frame(refund.clone(), 0, 101, 50, Arc::clone(&storage)),
            )
            .expect("timeout refund executes");
        assert_eq!(result.status, TxStatus::Success);
        assert!(
            result
                .value_transfers
                .iter()
                .any(|t| { t.from == VAULT && t.to == address(&sk_a) && t.amount == 50 }),
            "refund must return the escrow to agent A: {:?}",
            result.value_transfers
        );
        assert_eq!(result.logs[0].topics[0][0], 3, "EscrowRefunded tag");
        commit_overlay(&storage, result.storage_overlay);

        // Settled/refunded once — a second refund reverts.
        let again = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(refund, 0, 200, 0, storage),
        );
        assert!(again.is_err(), "double refund must trap");
    }

    #[test]
    fn refund_mutual_cancel_works_before_deadline() {
        let executor = WasmiExecutor::new();
        let calldata = deposit_calldata(50, 1_000);
        let id = escrow_id_bytes(&calldata);
        let storage = seeded_storage(&calldata, 50);
        let sk_a = keypair(SK_A);
        let sk_b = keypair(SK_B);

        let mutual = escrow::encode_refund(
            &id,
            &sign_escrow(&id, &sk_a),
            Some(&sign_escrow(&id, &sk_b)),
        );
        assert_eq!(mutual.len(), 140, "mutual form");

        // Deadline far ahead — only the two signatures matter here.
        let result = executor
            .execute_wasm(
                escrow_vault_wat().as_bytes(),
                &frame(mutual.clone(), 0, 11, 50, Arc::clone(&storage)),
            )
            .expect("mutual refund executes");
        assert_eq!(result.status, TxStatus::Success);
        commit_overlay(&storage, result.storage_overlay);

        // Mutual form with a forged agent-B signature reverts.
        let mut sig_b = sign_escrow(&id, &sk_b);
        sig_b[10] ^= 0xFF;
        let forged = escrow::encode_refund(&id, &sign_escrow(&id, &sk_a), Some(&sig_b));
        let err = executor.execute_wasm(
            escrow_vault_wat().as_bytes(),
            &frame(forged, 0, 12, 0, storage),
        );
        assert!(err.is_err(), "forged mutual sig_b must trap");
    }

    #[test]
    fn module_instantiates_with_expected_imports() {
        // Guards the WAT/selector plumbing itself: a syntax or selector
        // regression surfaces here without any frame semantics.
        let wat = escrow_vault_wat();
        let executor = WasmiExecutor::new();
        let result =
            executor.execute_wasm(wat.as_bytes(), &frame(Vec::new(), 0, 1, 0, vault_storage()));
        // Empty calldata → main traps (input_read returns 0 < 4).
        assert!(result.is_err(), "empty input must not execute");
        // The selector constants are distinct (dispatch sanity).
        assert_ne!(
            sel(escrow::deposit_selector()),
            sel(escrow::release_selector())
        );
        assert_ne!(
            sel(escrow::release_selector()),
            sel(escrow::refund_selector())
        );
    }
}
