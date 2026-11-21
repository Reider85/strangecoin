//! `BatchExecutor` — the batched-call contract of `agent_signAndSubmitBatch`
//! (AI-P04, ADR-0028 #5).
//!
//! The contract is WAT source assembled at runtime so the dispatch selector
//! stays derived from [`strangecoin_core::batch`] (one source of truth — the
//! same module the agent JSON-RPC layer and the codec tests use). It is a
//! *single-export* module: the monolith never registers `AbiDocument`s (the
//! ADR-0029 amendment record), so selector dispatch inside the VM always
//! falls back to `main` and `main` re-dispatches on the first four calldata
//! bytes itself.
//!
//! ```text
//! execute — two passes over the same input buffer:
//!   pass 1 validates bounds (count, addr_len, data_len, exact consumption),
//!          accumulates sum(values) with an overflow guard, then binds
//!          `sc.value() == sum` (the ADR-0030 value-binding pattern — a
//!          batch can only forward what its outer tx attached, so pooled
//!          leftovers can never be drained);
//!   pass 2 performs `sc.call` per entry; any non-OK status traps.
//!
//! Failure = `unreachable` trap (the engine turns it into a failed
//! execution: full-gas burn, no storage flush, no value transfers commit —
//! `apply_call` only commits on `TxStatus::Success`, so the whole batch is
//! atomic: all entries land or none do).
//!
//! Authorization is the *outer transaction* signature (the API layer signs
//! one `Call` tx from the session subkey) — no in-contract signature check;
//! see `docs/spec/agent_integration.md` §5.2 implementation notes.
//! ```
//!
//! Run `cargo run -p strangecoin-vm-wasm --example batch_executor` for the
//! happy-path smoke; `cargo test -p strangecoin-vm-wasm --example
//! batch_executor` runs the unit matrix (3-transfer happy path, value
//! mismatch, truncation, trailing bytes, zero count, atomic revert when one
//! entry fails).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use strangecoin_core::address::encode_address;
use strangecoin_core::batch::{self, BatchEntry};
use strangecoin_core::consensus::CHAIN_ID_REGTEST;
use strangecoin_core::types::{AccountState, BlockHeader, ContractAccount};
use strangecoin_core::vm::{BlockContext, ExecutionContext};
use strangecoin_vm_wasm::WasmiExecutor;

/// Contract account address inside the executor's account view (opaque to
/// `execute_wasm` — no chain lookup happens in these tests).
const EXECUTOR: &str = "sc1batchexecutor";

/// A second contract used by the atomicity test: every call into it traps.
const TRAP_TARGET: &str = "sc1traptarget";

/// Fixed recipient identities (deterministic — no RNG, ADR-0028
/// determinism review). 32-byte seeds → valid secp256k1 keys.
const SK_R1: [u8; 32] = [0x61; 32];
const SK_R2: [u8; 32] = [0x62; 32];
const SK_R3: [u8; 32] = [0x63; 32];

// ---------------------------------------------------------------- WAT

/// Selector as the `i32.const` the module compares against `i32.load` of
/// calldata bytes 0..4 (little-endian — both sides see the same value).
fn sel(sel: [u8; 4]) -> i32 {
    i32::from_le_bytes(sel)
}

/// Assemble the BatchExecutor module. Buffer map (module linear memory):
///
/// ```text
/// 0       .. 16384  input buffer (calldata via `sc.input_read`)
/// 16384   .. 16400  `sc.call` out-buffer (16 B)
/// ```
///
/// The executor stages top-level calldata by *appending* it past the
/// module's initial 64 KiB (`stage_calldata`), so the fixed buffers at the
/// head are never overwritten by staging.
pub fn batch_executor_wat() -> String {
    let execute = sel(batch::execute_selector());
    let max_addr = batch::MAX_BATCH_ADDR_LEN;
    let max_data = batch::MAX_BATCH_DATA_LEN;
    let max_count = batch::MAX_BATCH_ENTRIES;
    format!(
        r#"(module
  (import "sc" "input_read" (func $input_read (param i32 i32) (result i32)))
  (import "sc" "call" (func $call (param i32 i32 i64 i32 i32 i32 i32) (result i32)))
  (import "sc" "value" (func $value (result i64)))
  (memory (export "memory") 1)

  (func $execute (param $len i32) (result i32)
    (local $count i32) (local $off i32) (local $i i32)
    (local $alen i32) (local $dlen i32) (local $val i64)
    (local $sum i64) (local $status i32)
    (local $aptr i32) (local $dptr i32)
    ;; Header: signed compare so `input_read`'s -1 (buffer too small) traps.
    (if (i32.lt_s (local.get $len) (i32.const 8)) (then (unreachable)))
    (local.set $count (i32.load (i32.const 4)))
    (if (i32.eqz (local.get $count)) (then (unreachable)))
    (if (i32.gt_u (local.get $count) (i32.const {max_count})) (then (unreachable)))
    ;; ---- pass 1: validate bounds + sum(values) ----
    (local.set $off (i32.const 8))
    (local.set $i (i32.const 0))
    (block $done1
      (loop $l1
        (br_if $done1 (i32.ge_u (local.get $i) (local.get $count)))
        (if (i32.gt_u (i32.add (local.get $off) (i32.const 4)) (local.get $len))
            (then (unreachable)))
        (local.set $alen (i32.load (local.get $off)))
        (local.set $off (i32.add (local.get $off) (i32.const 4)))
        (if (i32.eqz (local.get $alen)) (then (unreachable)))
        (if (i32.gt_u (local.get $alen) (i32.const {max_addr})) (then (unreachable)))
        ;; addr + value(8) + data_len(4) must fit before the data payload.
        (if (i32.gt_u (i32.add (local.get $off) (i32.add (local.get $alen) (i32.const 12)))
                      (local.get $len))
            (then (unreachable)))
        (local.set $off (i32.add (local.get $off) (local.get $alen)))
        (local.set $val (i64.load (local.get $off)))
        (local.set $off (i32.add (local.get $off) (i32.const 8)))
        (local.set $dlen (i32.load (local.get $off)))
        (local.set $off (i32.add (local.get $off) (i32.const 4)))
        (if (i32.gt_u (local.get $dlen) (i32.const {max_data})) (then (unreachable)))
        (if (i32.gt_u (i32.add (local.get $off) (local.get $dlen)) (local.get $len))
            (then (unreachable)))
        (local.set $off (i32.add (local.get $off) (local.get $dlen)))
        ;; u64 payload with the high bit set reads as a negative i64 — reject.
        (if (i64.lt_s (local.get $val) (i64.const 0)) (then (unreachable)))
        (local.set $sum (i64.add (local.get $sum) (local.get $val)))
        ;; Wrapped addition goes negative while both addends are >= 0.
        (if (i64.lt_s (local.get $sum) (local.get $val)) (then (unreachable)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $l1)))
    ;; Exact consumption: no trailing bytes after the last entry.
    (if (i32.ne (local.get $off) (local.get $len)) (then (unreachable)))
    ;; Value binding (ADR-0030 amendment pattern): forward exactly what the
    ;; outer tx attached — pooled leftovers in the contract stay put.
    (if (i64.ne (local.get $sum) (call $value)) (then (unreachable)))
    ;; ---- pass 2: execute (buffer bounds already proven by pass 1) ----
    (local.set $off (i32.const 8))
    (local.set $i (i32.const 0))
    (block $done2
      (loop $l2
        (br_if $done2 (i32.ge_u (local.get $i) (local.get $count)))
        (local.set $alen (i32.load (local.get $off)))
        (local.set $off (i32.add (local.get $off) (i32.const 4)))
        (local.set $aptr (local.get $off))
        (local.set $off (i32.add (local.get $off) (local.get $alen)))
        (local.set $val (i64.load (local.get $off)))
        (local.set $off (i32.add (local.get $off) (i32.const 8)))
        (local.set $dlen (i32.load (local.get $off)))
        (local.set $off (i32.add (local.get $off) (i32.const 4)))
        (local.set $dptr (local.get $off))
        (local.set $off (i32.add (local.get $off) (local.get $dlen)))
        ;; STATUS_OK == 0; REVERTED/FAILED (1/2) trap the whole batch.
        (local.set $status
          (call $call (local.get $aptr) (local.get $alen) (local.get $val)
                      (local.get $dptr) (local.get $dlen)
                      (i32.const 16384) (i32.const 16)))
        (if (local.get $status) (then (unreachable)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $l2)))
    ;; Return value = entry count (the API layer derives per-entry txids
    ;; from the outer txid instead — see `batch::derive_batch_txid`).
    (local.get $count))

  (func (export "main") (result i32)
    (local $len i32) (local $sel i32)
    (local.set $len (call $input_read (i32.const 0) (i32.const 16384)))
    (if (i32.lt_s (local.get $len) (i32.const 4)) (then (unreachable)))
    (local.set $sel (i32.load (i32.const 0)))
    (if (i32.eq (local.get $sel) (i32.const {execute}))
        (then (return (call $execute (local.get $len)))))
    (unreachable)))"#,
        execute = execute,
        max_addr = max_addr,
        max_data = max_data,
        max_count = max_count,
    )
}

// ---------------------------------------------------------------- helpers

fn keypair(seed: [u8; 32]) -> secp256k1::SecretKey {
    secp256k1::SecretKey::from_slice(&seed).expect("fixed seed")
}

fn address(seed: [u8; 32]) -> String {
    let pk = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &keypair(seed));
    encode_address(&pk, CHAIN_ID_REGTEST).expect("regtest address")
}

/// A minimal module whose `main` always traps (the atomicity test's
/// failing entry target).
fn trap_wat() -> &'static str {
    r#"(module
  (memory (export "memory") 1)
  (func (export "main") (unreachable)))"#
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

/// Frame context over a fresh account view: the executor contract funded
/// with `contract_balance`, three recipient EOAs at zero, and (when
/// `with_trap_target`) the trapping contract.
fn frame(
    input: Vec<u8>,
    value: u64,
    contract_balance: u64,
    with_trap_target: bool,
) -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert(
        EXECUTOR.to_string(),
        AccountState::Contract(ContractAccount {
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
            nonce: 0,
            balance: contract_balance,
        }),
    );
    for seed in [SK_R1, SK_R2, SK_R3] {
        // The caller (SK_R1) must cover the entry value: `run_frame`
        // settles caller -> executor before the code runs, and an
        // under-funded caller fails the frame before any batch logic.
        accounts.insert(address(seed), AccountState::eoa(1_000_000, 0));
    }
    if with_trap_target {
        accounts.insert(
            TRAP_TARGET.to_string(),
            AccountState::Contract(ContractAccount {
                code_hash: [0u8; 32],
                storage_root: [0u8; 32],
                nonce: 0,
                balance: 0,
            }),
        );
    }
    ExecutionContext {
        block: block_header(),
        accounts,
        caller: address(SK_R1),
        callee: EXECUTOR.to_string(),
        value,
        gas_limit: 10_000_000,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height: 10,
            timestamp: 1_700_000_000,
            coinbase: "sc1coinbase".to_string(),
            block_gas_limit: 10_000_000,
        },
        storage: Arc::new(RwLock::new(HashMap::new())),
        storage_overlay: HashMap::new(),
        storage_address: EXECUTOR.to_string(),
        input,
    }
}

fn sample_entries() -> Vec<BatchEntry> {
    vec![
        BatchEntry {
            to: address(SK_R1),
            value: 40,
            data: Vec::new(),
        },
        BatchEntry {
            to: address(SK_R2),
            value: 60,
            data: Vec::new(),
        },
        BatchEntry {
            to: address(SK_R3),
            value: 50,
            data: Vec::new(),
        },
    ]
}

/// Executor with the trap target registered (mirrors how the chain seeds
/// `WasmiExecutor` from the facade's code registry).
fn executor_with_trap_target() -> WasmiExecutor {
    let mut executor = WasmiExecutor::new();
    executor.insert_code(TRAP_TARGET, trap_wat().as_bytes().to_vec());
    executor
}

// ---------------------------------------------------------------- example

/// Happy-path smoke: one 3-entry batch lands atomically; a value-mismatched
/// batch traps. Panics on the first failed step; silent otherwise (tracing
/// discipline).
///
/// `#[cfg(not(test))]`: tests pull this whole file in via `#[path]`-style
/// single-source inclusion patterns (escrow precedent) and a test-harness
/// build must not carry a second `main`.
#[cfg(not(test))]
fn main() {
    let entries = sample_entries();
    let total: u64 = entries.iter().map(|e| e.value).sum();
    let calldata = batch::encode_execute(&entries).expect("calldata");

    let executor = WasmiExecutor::new();
    let ok = executor
        .execute_wasm(
            batch_executor_wat().as_bytes(),
            &frame(calldata.clone(), total, 0, false),
        )
        .expect("batch executes");
    assert_eq!(ok.status, strangecoin_core::vm::TxStatus::Success);
    assert_eq!(ok.value_transfers.len(), 3, "one move per entry");
    assert_eq!(
        ok.return_data,
        (entries.len() as u32).to_le_bytes().to_vec()
    );

    let err = executor.execute_wasm(
        batch_executor_wat().as_bytes(),
        &frame(calldata, total + 1, 0, false),
    );
    assert!(err.is_err(), "value mismatch must trap");
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use strangecoin_core::vm::TxStatus;

    #[test]
    fn batch_executes_three_transfers_and_returns_count() {
        let entries = sample_entries();
        let total: u64 = entries.iter().map(|e| e.value).sum();
        let calldata = batch::encode_execute(&entries).expect("calldata");
        let executor = WasmiExecutor::new();

        let result = executor
            .execute_wasm(
                batch_executor_wat().as_bytes(),
                &frame(calldata, total, 0, false),
            )
            .expect("batch executes");

        assert_eq!(result.status, TxStatus::Success);
        // Entry transfer (caller -> executor) + one nested move per entry.
        assert!(
            result
                .value_transfers
                .iter()
                .any(|t| t.to == EXECUTOR && t.amount == total),
            "entry value must credit the executor: {:?}",
            result.value_transfers
        );
        for (entry, seed) in entries.iter().zip([SK_R1, SK_R2, SK_R3]) {
            assert!(
                result.value_transfers.iter().any(|t| {
                    t.from == EXECUTOR && t.to == address(seed) && t.amount == entry.value
                }),
                "missing move to {}: {:?}",
                address(seed),
                result.value_transfers
            );
        }
        assert_eq!(
            result.return_data,
            (entries.len() as u32).to_le_bytes().to_vec()
        );
    }

    #[test]
    fn batch_value_binding_rejects_underfunded_frame() {
        // Calldata sums to 150 but the frame only attached 149 — the
        // pooled-drain guard must trap before any entry executes.
        let entries = sample_entries();
        let calldata = batch::encode_execute(&entries).expect("calldata");
        let executor = WasmiExecutor::new();

        let err = executor.execute_wasm(
            batch_executor_wat().as_bytes(),
            &frame(calldata, 149, 0, false),
        );
        assert!(err.is_err(), "value mismatch must trap");
    }

    #[test]
    fn batch_value_binding_cannot_drain_pooled_leftovers() {
        // The contract holds 500 SC of leftovers; the batch attaches only
        // its own 150. Binding passes (sum == attached value) and exactly
        // 150 leaves — the leftovers are untouched by construction, but a
        // *binding bypass* would let sum < attached drain them, so pin the
        // exact outcome: 3 moves totaling 150, no more.
        let entries = sample_entries();
        let total: u64 = entries.iter().map(|e| e.value).sum();
        let calldata = batch::encode_execute(&entries).expect("calldata");
        let executor = WasmiExecutor::new();

        let result = executor
            .execute_wasm(
                batch_executor_wat().as_bytes(),
                &frame(calldata, total, 500, false),
            )
            .expect("batch executes");

        let forwarded: u64 = result
            .value_transfers
            .iter()
            .filter(|t| t.from == EXECUTOR)
            .map(|t| t.amount)
            .sum();
        assert_eq!(forwarded, total, "exactly the attached value leaves");
    }

    #[test]
    fn batch_rejects_truncated_calldata() {
        let entries = sample_entries();
        let calldata = batch::encode_execute(&entries).expect("calldata");
        let executor = WasmiExecutor::new();

        // Cut inside the first entry's address length prefix.
        let err = executor.execute_wasm(
            batch_executor_wat().as_bytes(),
            &frame(calldata[..10].to_vec(), 150, 0, false),
        );
        assert!(err.is_err(), "truncated calldata must trap");
    }

    #[test]
    fn batch_rejects_trailing_bytes() {
        let entries = sample_entries();
        let mut calldata = batch::encode_execute(&entries).expect("calldata");
        calldata.push(0);
        let total: u64 = entries.iter().map(|e| e.value).sum();
        let executor = WasmiExecutor::new();

        let err = executor.execute_wasm(
            batch_executor_wat().as_bytes(),
            &frame(calldata, total, 0, false),
        );
        assert!(err.is_err(), "trailing bytes must trap");
    }

    #[test]
    fn batch_rejects_zero_count() {
        let mut calldata = batch::execute_selector().to_vec();
        calldata.extend_from_slice(&0u32.to_le_bytes());
        let executor = WasmiExecutor::new();

        let err = executor.execute_wasm(
            batch_executor_wat().as_bytes(),
            &frame(calldata, 0, 0, false),
        );
        assert!(err.is_err(), "zero count must trap");
    }

    #[test]
    fn batch_is_atomic_when_one_entry_fails() {
        // Entry 2 targets a contract whose `main` traps: the whole batch
        // must revert — entry 1's already-executed move must not survive
        // (engine turns the trap into `TxStatus::Failed` / Err, and
        // `apply_call` commits nothing on failure).
        let entries = vec![
            BatchEntry {
                to: address(SK_R1),
                value: 40,
                data: Vec::new(),
            },
            BatchEntry {
                to: TRAP_TARGET.to_string(),
                value: 60,
                data: Vec::new(),
            },
        ];
        let total: u64 = entries.iter().map(|e| e.value).sum();
        let calldata = batch::encode_execute(&entries).expect("calldata");
        let executor = executor_with_trap_target();

        let err = executor.execute_wasm(
            batch_executor_wat().as_bytes(),
            &frame(calldata, total, 0, true),
        );
        match err {
            Err(_) => {} // top-level trap path
            Ok(result) => panic!(
                "batch must not report success when an entry fails: {:?}",
                result.status
            ),
        }
    }

    #[test]
    fn batch_rejects_unknown_selector() {
        let mut calldata = vec![0u8; 4];
        calldata[0] = 0xff;
        calldata.extend_from_slice(&1u32.to_le_bytes());
        let executor = WasmiExecutor::new();

        let err = executor.execute_wasm(
            batch_executor_wat().as_bytes(),
            &frame(calldata, 0, 0, false),
        );
        assert!(err.is_err(), "unknown selector must trap");
    }

    #[test]
    fn wat_selector_matches_core_codec() {
        // The WAT compares `i32.load(0)` against `execute_selector()` —
        // pin that the codec's selector round-trips into the module text
        // (a selector bump without a WAT bump would trap every batch).
        let wat = batch_executor_wat();
        let expected = i32::from_le_bytes(batch::execute_selector());
        assert!(
            wat.contains(&format!("i32.const {expected}")),
            "WAT must dispatch on the codec selector ({expected})"
        );
    }

    #[test]
    fn batch_handles_max_count_entries() {
        // 64 entries at the count cap: all valid, all execute.
        let entries: Vec<BatchEntry> = (0..batch::MAX_BATCH_ENTRIES)
            .map(|i| BatchEntry {
                to: address(SK_R1),
                value: 1,
                data: if i % 2 == 0 {
                    vec![i as u8]
                } else {
                    Vec::new()
                },
            })
            .collect();
        let calldata = batch::encode_execute(&entries).expect("calldata");
        let executor = WasmiExecutor::new();

        let result = executor
            .execute_wasm(
                batch_executor_wat().as_bytes(),
                &frame(calldata, batch::MAX_BATCH_ENTRIES as u64, 0, false),
            )
            .expect("max-count batch executes");
        assert_eq!(result.status, TxStatus::Success);
        assert_eq!(
            result.return_data,
            (batch::MAX_BATCH_ENTRIES as u32).to_le_bytes().to_vec()
        );
    }
}
