# Changelog

## 1.1.8 — Stage 1.5 proper: contract storage в SMT + block gas limit, инварианты #15/#19 (S1.5-P15)

**Status**: In progress  
**Date**: 2026-10-16  
**Prompts**: S1.5-P15

### Contract storage в общем state trie + gas-limit enforcement

- **`ContractStorage` / `ContractStorageMap` / `ContractStorageOverlay` — `core/state/contract_storage.rs` (новый)** — пер-контрактные map (32-byte slot → value); overlay — write-set кадра с `from_writes` / `preimages_of` (пре-образы для undo) / `apply_to` / `root_from` (пост-flush sub-root без мутации base). `ContractStorageMap` = address → slot map, re-export из `strangecoin_core`.
- **`contract_storage_root(entries) — core/state/sparse_merkle.rs`** — sub-trie depth-256: key = slot key, leaf = `blake3(0x03 ‖ key ‖ value)` (доменный тег — три leaf-пространства account/receipt/storage не коллидируют); пустое хранилище → `[0;32]` (sentinel с P07, deploy-дефолт не меняется).
- **`ExecutionContext` — scoped storage (новые поля)** — `storage: ContractStorageMap` (снапшот persisted-хранилища всего цепочки; сидируется `apply_call` из `State.contract_storage` — nested call видит свои и чужие persisted-ключи), `storage_overlay: ContractStorageMap` (записи per contract), `storage_address: String` (какой контракт читает/пишет кадр; delegatecall сохраняет адрес родителя — callee-код на storage вызывающего). `storage_read` = overlay → base fall-through; `storage_write` — cap считает persisted + staged ≤ 1 MB (threat V-20, теперь против полного контракта, не только overlay).
- **`ExecutionResult.storage_overlay: ContractStorageMap`** — записи всех скоупов кадра возвращаются в core; precompile/transfer-пути — пустая map.
- **`State { balances, contract_storage, receipts, storage_undo }` — `core/state/inner.rs`** — `PartialEq` сравнивает balances **и** contract_storage (round-trip инварианта #4); `storage_undo: txid → Vec<(contract, key, Option<value>)>` — derived data tip-блока для reorg. `apply_call`: сидирует `ctx.storage` → `vm.execute` → при `Success` `flush_storage_writes` флашит каждый contract-скоуп (`ContractStorageOverlay::from_writes` → `apply_to`, пересчёт `storage_root` в account record — **инвариант #19 extension**), пре-образы → undo; failed frame отбрасывает записи. `unapply_call_storage` (новый): restore пре-образов + пересчёт storage_root (пустое → sentinel). `unapply_create` удаляет storage-запись. Undo накапливается через apply (chained unapply нескольких tip-блоков); `StateCache::commit` сбрасывает его каждый блок — в продакшене не растёт.
- **`root_after_with_vm(state, block, chain_id, allow_zero, vm)` — `core/state/mod.rs`** — mirror `root_after` через `apply_block_with_vm`: Create/Call блоки теперь валидируемы на уровне core (инвариант #19 покрывает contract storage); `root_after` (NoVm) — transfer-only, без изменений.
- **`enforce_receipt_gas_limit` — `src/blockchain/block_executor.rs`** — пост-execution **инвариант #15**: `sum(receipt.gas_used) ≤ BLOCK_GAS_LIMIT` после `apply_and_execute` (implies reservation-проверкой P09, но ловит ошибку metering'а); `BlockGasLimitExceeded`.
- **`StateCache` (узел)** — поле `contract_storage` прокидывается в `from_state`/`to_state`/`commit`/`replace` + `set_contract_storage`/`contract_storage()`; персист в LevelDB ключ `b"contract_storage"` (JSON-DTO `ContractStorageJson`: hex-ключи, sorted — детерминированные байты; отсутствующий ключ старых БД → пустая карта); `debug_db` + skip-list итератора учтывают новый ключ. Wire-формат (`BlockchainDeserialize`) не менялся — adopt/rebuild реконструирует storage replay'ем через VM.
- **Nested-call скоупы** — `nested_call`: Call/Static → `storage_address = to`; DelegateCall → storage-адрес родителя; child наследует оба map, при успехе parent = child (scoped-карты — записи callee-контракта летят в его скоуп, не в родительский). Cross-contract persisted-чтения работают (весь storage в ctx).
- **Тесты** — `core/tests/contract_storage_smt.rs` (8, новый): flush + storage_root/state-root (#19) + `root_after_with_vm` accept/reject, seeded-base persistence между блоками, unapply restore (chained, точный state root), failed call discard, EOA-скоуп игнорируется, детерминизм replay, PartialEq покрывает storage. `vm-wasm/tests/contract_storage.rs` (6, новый, wasmi e2e): write-flush + invariant #19, второй call читает persisted base, unapply roundtrip, **nested call A→B пишет в scope B** (внешний контракт не тронут), write > 1 MB cap → Failed без flush, детерминизм replay. `block_executor` unit (+2): receipt-gas limit boundary. `sparse_merkle` unit (+2): storage root sentinel/детерминизм/чувствительность; `contract_storage.rs` unit (+5). `cargo test --workspace` green, `clippy -D warnings` green.

### Not in this change

- Nested value-трансферы не коммитятся в `apply_call` (MVP P14, residual — S1.5-P17+).
- Конструктор Create не исполняется (MVP P14) — S1.5-P17.
- Клон всего `contract_storage` на каждый CallTx — O(размер хранилища); mainnet-путь — lazy state access через host functions (Stage 3+).
- Fold `receipts_root` в глобальный `state_root` — отдельное решение (receipts хранятся как данные sub-trie, в account-леафы не fold'ятся).

---

## 1.1.7 — Stage 1.5 proper: events/logs — Log + Receipt, инвариант #18 (S1.5-P14)

**Status**: In progress  
**Date**: 2026-10-16  
**Prompts**: S1.5-P14

### Receipts end-to-end: types, VM execution in `apply_block`, sub-trie

- **`TxStatus` 4 варианта — `core/vm/result.rs`** — `Success | Reverted | OutOfGas | Failed` (+ `canonical_byte()` для канонического encoding); `calls/mod.rs` маппит `OutOfGas | Failed → STATUS_FAILED`.
- **`Receipt` по спеке** — `{ txid, gas_used, logs, status, block_height, tx_index }` + `Receipt::from_execution(txid, h, i, &ExecutionResult)` и `Receipt::from_vm_error(...)` (исключительный отказ сжигает полный `gas_limit`, логов нет). Каноническое кодирование — `serialize::serialize_receipt`; порядоко-независимый commitment — `serialize::compute_receipts_root` (sorted blake3 leaves + `merkle_root`; пустой список → `[0;32]`).
- **`VmExecutor::register_code(address, wasm) -> Result<(), VmError>`** — валидация модуля при регистрации (`Module::new`); `WasmiExecutor` имплементирует (insert + validate), `MockVmExecutor` — no-op.
- **`State { balances, receipts }` + `apply_block_with_vm<V: VmExecutor>` — `core/state/inner.rs`**:
  - `Create` — deploy: `derive_contract_address(from, nonce_pre, chain_id)`, endowment `sender → contract`, install `Account::Contract { code_hash: blake3(init_code), storage_root: [0;32], nonce: 1 }`, `vm.register_code`, receipt `Success gas 0` (конструктор не исполняется — MVP).
  - `Call` — nonce++ всегда; callee-EOA → value-transfer + receipt `Success gas 0`; контракт → `ExecutionContext` (overlay ∅, input = `tx.data`) → `vm.execute`; value переводится **только при Success**; `Ok` → `from_execution`, `Err` → `from_vm_error` (OutOfBounds → `OutOfGas`).
  - `apply_block` (без VM) — transfer-only путь, Create/Call → `UnsupportedTxType` (light-node / feature-off).
  - `receipts` — производные данные tip-блока: ручной `PartialEq` сравнивает **только balances** (apply/unapply round-trip инварианта #4 сохраняется); `unapply_block(state, block, chain_id)` (**новый аргумент**) реверсирует Create (remove account + refund endowment) / Call (value — только если receipt Success; без receipts — по умолчанию reverse), затем `receipts.clear()`.
- **Receipts sub-trie — `core/state/sparse_merkle.rs`** — key = `txid`, leaf = `blake3(0x02 ‖ serialize_receipt)`; `receipts_subroot`, `prove_receipt`, `verify_receipt_proof` (reuse `hash_range`/`collect_siblings`). Корень хранится как данные (верифицируемый proof'ом); fold в глобальный `state_root` — S1.5-P15 (один rebuild генезиса вместо двух).
- **Узел** — `Blockchain.codes` (address → WASM, ре-выводится из Create-tx цепочки: `codes_from_chain`/`collect_block_codes` — invariant #1 стиль; восстанавливается при open/rebuild/adopt) и `Blockchain.receipts` (tip receipts; `BlockchainFacade::tip_receipts()`). `validate_and_apply` через feature-gated `apply_and_execute`: с `vm-wasm` — wasmi-executor из `view.codes` + `apply_block_with_vm`; без фичи — `apply_block`. `commit_block` коммитит receipts + коды; `rebuild_from_chain` возвращает `(StateCache, Vec<Receipt>)` и накапливает codes по ходу replay; `try_adopt_candidate` ре-устанавливает codes + receipts; cumulative gas_used ≤ 1M остаётся покрыт reservation-проверкой P09 (фактический gas — в receipts).
- **Иммутабельность (#18)** — нет API мутации receipts; замена только целиком при commit/adopt/rebuild; rebuild из цепочки выигрывает при расхождении.
- **Тесты** — `core/tests/receipts.rs` (12): from_execution/from_vm_error, canonical determinism/sensitivity, serde roundtrip, compute_receipts_root (empty/deterministic/order-independent), subroot prove/verify + tamper/wrong-proof. `core/tests/state_roundtrip.rs` (+2): Create+Call apply/unapply через mock (Success и OutOfGas), balances round-trip. `core/src/state/inner.rs` unit (+3): transfer receipts, unapply clear, PartialEq игнорирует receipts, без VM Create/Call отклоняются. `vm-wasm/tests/events_and_receipts.rs` (11): emit_log → Receipt log (address/topics/data), max_topics 4 → abort без логов, OutOfGas receipt сжигает budget, trap-after-log дискардит логи, детерминизм, **e2e `apply_block_with_vm`**: Create+Call(emit_log) в одном блоке → log-receipt + value-transfer + subroot proof; failed-call (infinite loop) → `OutOfGas`, value не двигается; unapply reverses; register_code отклоняет невалидный wasm. `tests/receipts.rs` (4, node): genesis/mined-transfer receipts в `tip_receipts`, rebuild ре-выводит идентично, receipts заменяются, не копятся. `cargo test --workspace` green, `clippy -D warnings` green.

### Not in this change

- Персист contract storage (`storage_root` updates, `unapply_call` для storage) — S1.5-P15.
- Fold `receipts_root` + `storage_root` в глобальный `state_root` / `root_after` extension / rebuild генезиса — S1.5-P15.
- Исполнение конструктора при Create — S1.5-P15/P17.
- Selector-based ABI dispatch (`ENTRY_POINT = "main"`) — S1.5-P17.
- Биллинг gas (списание у плательщика): gas = cap, fees как сейчас — Stage 5 fee market.

---

## 1.1.6 — Stage 1.5 proper: reentrancy guard + limits (S1.5-P13)

**Status**: In progress  
**Date**: 2026-10-16  
**Prompts**: S1.5-P13

### Reentrancy guard (threat V-18) + execution limits (V-19/V-20, ARCHITECT3 §3.9)

- **Minimal ABI stub — `crates/strangecoin-core/src/abi/mod.rs`** — `AbiEntry { name, selector, non_reentrant }` + `ContractAbi { format_version: "json-v1", entries }`; full JSON ABI encode/decode dispatch stays S1.5-P12. Registered via **`WasmiExecutor::insert_abi(address, abi)`**, shared into frames through `HostState.abis` (like `code`).
- **Reentrancy guard** — `nested_call` computes `is_reentrant = call_stack.contains(to)` (self-call and call-back-to-outer-frame both covered) and sets `ExecutionContext.reentrant` on the child; if the target's ABI entry matching the first 4 calldata bytes carries `non_reentrant`, the host function aborts with **`VmError::ReentrancyDetected`**. Reentrancy without the flag stays legal (test: storage-guarded self-call runs to completion). Security errors **bubble through parents** instead of degrading to a generic failed child (a mid-stack violation fails the whole transaction deterministically).
- **Typed host-error channel** — `VmHostError(VmError)` (wasmi `HostError`, `error.rs`): `call`/`storage_write` failures survive the engine round-trip via downcast in `From<wasmi::Error> for WasmiError` — exact variant, no string parsing. `From<WasmiError> for VmError` now passes `Core(inner)` through unwrapped.
- **`max_code_size` 24 KB** — `consensus::verify_transaction` Create branch rejects `init_code.len() > MAX_CODE_SIZE` with `InvalidContractDeploy` (boundary: exactly 24 KB accepted, 24 KB + 1 rejected, both signed-tx verified).
- **`max_storage_per_contract` 1 MB** — `storage_write` sums staged value bytes (replacing the same key is free of double-count) and aborts with **`VmError::MaxStoragePerContract`** when the next write crosses the cap (10 × 100 KB accepted, 11th rejected).
- **`max_call_depth` 1024** — enforced at the top of `nested_call` for **all three variants** (`call`/`delegatecall`/`staticcall` share it): `depth + 1 > 1024` → **`VmError::MaxCallDepth`** before the target is touched (boundary tests at 1023/1024).
- **`max_stack_size` + wasm recursion (wasmi config)** — `run_frame` sets `Config::set_max_stack_height(1_000_000)` and `set_max_recursion_depth(MAX_CALL_DEPTH)`; both exhaustion paths trap `StackOverflow` → **`VmError::OutOfMemory`** (runaway `(func $f (call $f))` recursion traps long before fuel runs out — asserted `OutOfMemory`, not `OutOfGas`).
- **`max_memory_size` 1 MB (wasmi store limiter)** — `HostState.limits = StoreLimitsBuilder::memory_size(16 × 64 KiB).trap_on_grow_failure(true)` installed via `Store::limiter`: a module declaring 17 pages fails instantiation, and `memory.grow` past 16 pages traps `GrowthOperationLimited` → **`VmError::OutOfMemory`**.
- **Core constants — `crates/strangecoin-core/src/vm/limits.rs`** (runtime-free, like `BLOCK_GAS_LIMIT`, so consensus checks need no `vm-wasm`): `MAX_CODE_SIZE`, `MAX_STORAGE_PER_CONTRACT`, `MAX_CALL_DEPTH`, `MAX_MEMORY_PAGES`/`MAX_MEMORY_BYTES`, `MAX_STACK_HEIGHT`, re-exported from `strangecoin_core::vm`.
- **Tests** — `vm-wasm/tests/reentrancy_and_limits.rs` (13): self-call rejected/allowed×2/no-ABI, cross-contract A→B→A bubbling, depth 1023 allowed / 1024 rejected, CreateTx 24 KB boundary ×2, storage 1 MB boundary ×2, memory declaration + `memory.grow`, runaway recursion. `cargo test --workspace` green (P10's 43 host-function tests unaffected), `clippy -D warnings` green.

### Not in this change

- Real ABI dispatch (selector → entry point, `input` accessors) — S1.5-P12.
- Receipts/logs persistence, SMT overlay flush — S1.5-P14/P15.
- Consensus-gas freeze for limits — SCIP-gated after mainnet freeze (ADR-0014 policy).

---

## 1.1.5 — Stage 1.5 proper: host functions — crypto + storage + calls + context + logs (S1.5-P10)

**Status**: In progress  
**Date**: 2026-10-16  
**Prompts**: S1.5-P10

### Host functions — the full S1.5-P10 set (12 new + `gas_left` from P09 = 13)

- **`crates/strangecoin-vm-wasm/src/host/`** — one module per host function, all registered under the `sc` namespace via `host::register(Linker)`, every one charges its ADR-0014 `HostCallWeight` **before** doing work and validates its memory-pointer inputs (threat V-21: a malicious module must never panic the host or read/write out of bounds):
  - **`crypto/`** — `secp256k1_verify(pk33, hash32, sig64)` → `1/0` (malformed material = `0`, bad memory = frame abort; gas 3000), `bls_verify` (**NotImplemented until Stage 7** — charges 12000 then fails the frame, no BLS12-381 dep), `blake3`/`keccak`/`sha256` via shared `hash_host` (`(in_ptr, in_len, out_ptr)` → `1`, or `0` when the out-buffer can't fit the digest so the contract can retry; ADR-0014 length-based formulas).
  - **`storage/`** — `storage_read(key_ptr, out_ptr, out_len)` → bytes written / `0` absent / `-1` buffer too small (gas 200); `storage_write(key_ptr, val_ptr, val_len)` → `1` (gas 5000, **refused in read-only frames**). Per-contract size cap lands in S1.5-P13; overlay wiring to the SMT in S1.5-P15.
  - **`calls/`** — `call`/`delegatecall`/`staticcall` share `nested_call`: child `ExecutionContext` derived with `depth + 1`, `read_only` propagated (staticcall forces it), value transfer for `call` only (insufficient balance → `STATUS_FAILED` + full child-budget burn), precompile fast-path reuses ADR-0015 `dispatch_precompile` (B's `call("0x02", …)` runs sha256), merge-on-success of child storage overlay/accounts/logs (revert keeps parent state untouched), gas = 700 base + child consumption deducted from the parent's single fuel counter, return data truncated into the caller's out-buffer (full copy in `last_return_data`). ABI status codes `0/1/2` (OK/reverted/failed); target length capped at 128 bytes (V-21).
  - **`context/`** — `block_context(out_ptr, out_len)` → `height ‖ timestamp ‖ gas_limit ‖ coinbase` LE (`-1` if the buffer is too small, gas 3 — consensus timestamp, never wall clock); `gas_left()` (P09) unchanged.
  - **`logs/`** — `emit_log(topics_ptr, topics_count, data_ptr, data_len)` → `1`; ≤ 4 × 32-byte topics (Ethereum-style), gas `375 + 375/topic`, **refused in read-only frames**; logs ride `ExecutionResult.logs` (receipts in S1.5-P14).
- **Memory-pointer ABI** — `host/abi.rs`: `read_bytes`/`read_array32`/`write_bytes` bounds-check every access against the current linear-memory size; a bad pointer aborts the frame with a host error (not an engine panic).
- **`ExecutionContext` gains `read_only: bool`** (core `vm/context.rs`) — set by `staticcall` for the child frame; `storage_write`/`emit_log` gate on it.
- **`run_frame` + `FrameOutcome`** (`vm-wasm/src/lib.rs`) — one call frame runs in a fresh engine/store and returns the result *plus* mutated working state (storage overlay, accounts, logs, return data) so nested calls merge child → parent on success; `WasmiExecutor.code` is an `Rc<HashMap>` shared into nested frames (zero copies per nested call).
- **Tests** — `vm-wasm/tests/host_functions.rs` (42): every host function gets valid + invalid + gas coverage through real wasmi execution of WAT fixtures — blake3/keccak/sha256 known vectors (`"abc"`) + out-of-bounds out-buffer → `0` + out-of-bounds in-buffer → frame abort + OOG below weight + gas-formula deltas via `gas_left`; secp256k1 valid/tampered/garbage-key/OOG; bls NotImplemented ×3 (message, below-weight OOG, determinism); storage write→read roundtrip, absent key → `0`, small buffer → `-1`, read-only refusal, OOG; block_context height/`-1`/gas; emit_log topic+data, >4 topics abort, OOG; nested call return-data copy, EOA call, precompile reuse, insufficient balance, 3-frame depth chain propagating the grandchild's return, gas base+child and determinism; delegatecall shared overlay / EOA / gas; staticcall write-blocked / return-data / gas; too-long target rejected. Unit tests for `transfer_value` in `host/calls/mod.rs` (3). `cargo test --workspace` green, `clippy -D warnings` green.

### Not in this change

- Reentrancy guard, `max_call_depth`, `max_code_size`, per-contract storage cap, wasmi stack/memory limits — S1.5-P13 (minimal ABI stub for the `non_reentrant` flag ships there).
- JSON ABI dispatch (real entry-point selection instead of `main`) — S1.5-P12.
- Sparse Merkle overlay flush + receipts/logs into `apply_block` — S1.5-P14/P15.
- Normative gas-cost freeze — SCIP-gated after mainnet freeze (ADR-0014/0015 policy).

---

## 1.1.4 — Stage 1.5 proper: precompiles 0x01..0x07 (S1.5-P11)

**Status**: In progress  
**Date**: 2026-10-16  
**Prompts**: S1.5-P11

### Precompiles (ADR-0015) — 7 built-in contracts at reserved addresses

- **`crates/strangecoin-vm-wasm/src/precompiles/`** — dispatcher `dispatch_precompile(callee, data) -> Result<PrecompileOutput, PrecompileError>` + one module per precompile: `0x01 ecrecover` (secp256k1 recovery, 97-byte `hash‖v‖r‖s` input, 33-byte compressed pubkey or empty on recovery failure, gas 3000), `0x02 sha256` (50+12/word32), `0x03 keccak` (Keccak-256 Ethereum padding 0x01 via `tiny-keccak`, 30+6/word32), `0x04 blake3` (50+6/chunk64, same primitive as chain serialization), `0x05 ed25519_verify` (`pubkey32‖sig64‖msg`, output 32-byte 1/0, gas 2000, RFC 8032 vectors), `0x06 bls_aggregate` (**NotImplemented until Stage 7**, reverts after charging 12000), `0x07 modexp` (Ethereum-style `base_len‖exp_len‖mod_len` 3×32-BE headers, square-and-multiply over u64 limbs — no bigint dep, gas 50+50/result-byte).
- **`WasmiExecutor::execute`** routes reserved callees to `execute_precompile` before WASM instantiation (no code lookup; precompile wins even if code were registered under `0x0N`). `ExecutionContext` gains **`input: Vec<u8>`** (calldata: `CallTx.data` top-level, nested-call `data` later) — the field WASM/ABI will read in S1.5-P12.
- **Failure semantics (ADR-0015)** — success: declared gas; `InvalidInput` (bad length/encoding/size overflow): revert + **full call-frame gas burn**; `NotImplemented`: revert + declared gas (12000). Declared gas > `gas_limit` → `OutOfGas`. Precompiles never panic; length headers validated (checked adds) before allocation (threat V-21).
- **Core** — `VmError::NotImplemented { feature }` (additive; used by `0x06` and future Stage 7 stubs). `strangecoin_core::address::is_precompile_address` (`0x01..0x07`, from S1.5-P08) is the single reserved-range gate.
- **New deps (vm-wasm)** — `secp256k1 0.29 +recovery` (same pin as core), `sha2 0.10`, `blake3 1`, `tiny-keccak 2 (keccak)`, `ed25519-dalek 2`.
- **Tests** — `vm-wasm/tests/precompiles.rs` (36): per-precompile valid/invalid/gas (ecrecover recover/tamper/bad-v/short, sha256+keccak+blake3 known vectors + gas word/chunk schedules + keccak≠NIST-SHA3, ed25519 RFC 8032 vector/tamper/short/bad-pubkey, bls NotImplemented×3, modexp 2^10 mod 1000 / 3^5 mod 13 / zero-exp / zero-mod / mod-one / padding / truncated header+body / huge length), dispatcher routing + unknown address + reserved-range cross-check with core, executor integration (sha256 CallTx-style without WASM, bls revert+12000, invalid-input full burn, OOG below declared, NoCode for non-precompile, precompile wins over registry). Drive-by: `gas_metering.rs` clippy `manual_range_contains`. `cargo test --workspace` green, `clippy -D warnings` green.

### Not in this change

- In-contract `call("0x0N", data)` host path — S1.5-P10 `call`/`staticcall`/`delegatecall` host functions will reuse `dispatch_precompile`.
- Remaining 12 host functions (crypto/storage/calls/context/logs) — S1.5-P10 (incomplete: only `gas_left` shipped with P09).
- `state::apply_block` Create/Call execution + receipt gas — S1.5-P15.
- Normative gas-cost freeze — SCIP-gated after mainnet freeze (ADR-0014/0015 policy).

---

## 1.1.3 — Stage 1.5 proper: gas metering + block gas limit (S1.5-P09)

**Status**: In progress  
**Date**: 2026-10-16  
**Prompts**: S1.5-P09

### Gas metering (ADR-0014) + invariant #15 first enforced

- **Instruction counting** — wasmi `=2.0.0` engine runs with `Config::consume_fuel(true)`; fuel is deducted continuously per operator (1 per substantive op, 0 for Nop/Drop/control markers — wasmi 2.0 default `OperatorCost`), `gas_used = gas_limit - store_fuel`. **Wasmi 2.0 removed the `ConsumptionStrategy::RecurringBlock` API** referenced by the pre-pin pseudocode/ADR sketch — strategy note updated in `vm-wasm/src/lib.rs` doc; semantics unchanged (continuous accounting, `BASE_INSTRUCTION_GAS = 1`).
- **Host-call weights** — `crates/strangecoin-vm-wasm/src/gas.rs`: `HostCallWeight` enum covers all 14 S1.5-P10 host functions + `MemoryGrow`, `gas()` applies ADR-0014/0015 formulas (`Secp256k1Verify=3000`, `BlsVerify=12000`, `Blake3=50+6/chunk64`, `Keccak=30+6/word32`, `Sha256=50+12/word32`, `StorageRead=200`, `StorageWrite=5000`, `Call/DelegateCall/StaticCall=700` sketch, `BlockContext=3`, `GasLeft=3`, `EmitLog=375+375/topic`, `MemoryGrow=3`); constants are SCIP-gated after mainnet freeze.
- **`FuelGasCounter`** — implements core `GasCounter` (`spend`/`gas_left`/`gas_used`) + inherent `spend_host_call(HostCallWeight)`; pure-logic counter, unit-tested without a runtime. **`charge_host_call(caller, weight)`** — in-engine counterpart deducting from the wasmi store's single fuel counter before host work (insufficient fuel → `TrapCode::OutOfFuel` → `VmError::OutOfGas`).
- **First host function: `sc::gas_left()`** — `crates/strangecoin-vm-wasm/src/host/` (namespace `sc`, charge 3 then return remaining fuel); registration path (`host::register` into `Linker`) established for S1.5-P10's remaining 12 functions.
- **`BLOCK_GAS_LIMIT = 1_000_000`** — consensus constant in **core** (`strangecoin_core::vm::gas`, runtime-free) so block validation enforces it without the `vm-wasm` feature.
- **Invariant #15 (first enforced)** — `block_executor::enforce_block_gas_limit`: `sum(CallTx.gas_limit) <= BLOCK_GAS_LIMIT` in `validate_and_apply` (full `gas_limit` is reserved up-front per ADR-0014; `Transfer`/`Create` reserve 0); new error `StrangecoinError::BlockGasLimitExceeded { total, limit }`.
- **Tests** — `vm-wasm/tests/gas_metering.rs` (9): infinite loop → `OutOfGas` (<50 ms CI-safe; local sub-ms), 10M-iteration compute under 1M gas → `OutOfGas`, `gas_left()` returns remaining budget + decreases across probes, host-call determinism, insufficient fuel for host call → `OutOfGas`, `FuelGasCounter` spend/underflow, ADR-0014 weight table; `block_executor` unit-тесты (5): empty/transfer-only/at-limit accepted, over-limit (multi-tx + single-tx) rejected. `cargo test --workspace` green, `clippy -D warnings` green.

### Not in this change

- Remaining 12 host functions (crypto/storage/calls/context/logs) — S1.5-P10 (consumes `HostCallWeight` + `charge_host_call`).
- `gas_left()` as an in-contract import is available now; full host ABI (memory pointer convention) lands with the P10 set.
- Receipt gas accounting in `state::apply_block` (Create/Call still `UnsupportedTxType`) and miner-side block assembly limit — S1.5-P15; until then the invariant guards validation/adoption only.
- Gas price / fee market — Stage 5.

---

## 1.1.2 — Stage 1.5 proper: Tx types transfer/create/call (S1.5-P08)

**Status**: In progress  
**Date**: 2026-10-15  
**Prompts**: S1.5-P08

### Transaction enum + canonical FORMAT_VERSION 5

- **Breaking (consensus):** `Transaction` — плоская структура → enum `Transfer(TransferTx) / Create(CreateTx) / Call(CallTx)`; канонический encoding **FORMAT_VERSION 4 → 5**: `version | tag(0=transfer,1=create,2=call) | variant payload`; **`fee` входит в подписываемые байты каждого варианта**. Десериализатор принимает только v5 (чистый break — mainnet не запущен, regtest/devnet цепочки одноразовые).
- **Regenerated (S1.5-P08):** `EXPECTED_GENESIS_HASH` → `0xfcd79a70…653101`, `EXPECTED_TESTNET_GENESIS_HASH` → `0x23dde7b1…b8cf89` (пересчёт через `examples/print_genesis_hashes.rs`); `genesis.json`/`genesis-testnet.json` поля `genesis_hash` обновлены; golden vectors `tests/serialize_golden.rs` перегенерированы (18 тестов, procedure BUG-S1-007).
- `TransferTx { from, to, amount, fee, nonce, chain_id, signature, is_coinbase }`; `CreateTx { from, init_code, init_args, endowment, fee, nonce, chain_id, signature }`; `CallTx { from, to, data, value, gas_limit, fee, nonce, chain_id, signature }`; accessors (`sender()/receiver()/value()/nonce()/chain_id()/fee()/is_coinbase()/signature_mut()`) + конструкторы `transfer/coinbase/create/call`.
- `verify_transaction` per-variant: подпись ECDSA-recoverable поверх канонических байтов варианта (payload в подписи — replay protection инварианта #10); shape-проверки: `Create` → пустой `init_code` = `InvalidContractDeploy`, `Call` → `gas_limit == 0` = `InvalidGasLimit`; coinbase-bypass только у `Transfer.is_coinbase`.
- `mempool::insert` валидирует все 3 варианта: nonce (core `validate_nonce`), баланс `value()+fee()` против состояния и пула, chain_id; RBF-логика без изменений.
- `state::apply_block`/`unapply_block`: **Transfer** — прежняя логика (deduct/credit/nonce); **Create/Call → `CoreError::UnsupportedTxType`** до S1.5-P15 (исполнение/contract storage — отдельный промпт).
- Serde-совместимость: `Transfer` сериализуется в legacy flat JSON `{sender, receiver, …}` (+ `fee`, default 0); `Create`/`Call` — internally tagged `{"type":"create"|"call",…}`; старые LevelDB-блобы и `GET_BLOCKCHAIN` снапшоты десериализуются как Transfer.
- `address::derive_contract_address(creator, nonce, network_id)` — bech32m-адрес контракта из `blake3("sc-contract-addr"‖creator‖nonce)` (32-byte payload не коллидирует с 33-byte EOA); `address::is_precompile_address` — guard диапазона `0x01..0x07` (ADR-0015, полный dispatch — S1.5-P11).
- Тесты: новый `crates/strangecoin-core/tests/tx_types.rs` (20 тестов: round-trip 3 вариантов, sign/verify, tamper-rejection, shape, serde-совместимость, apply-gate); mempool unit-тесты (8: transfer/create/call accepted, replay/cross-chain/gas_limit/init_code rejected, endowment balance accounting); `derive_contract_address` тесты (address.rs). Regression: Stage 0/1 тесты обновлены под конструкторы, `cargo test --workspace` зелёный.

### Not in this change

- Contract deploy/call **state transition** (установка ContractAccount, endowment transfer, execute) — S1.5-P15 (`state::apply_call`); код байтов не персистится до P15.
- Fee market / минимальная комиссия — Stage 5 (fee сейчас подписывается, но не валидируется на минимум; RBF использует прежний feerate-прокси).
- ABI-кодирование payload (`data`/`init_args` — «сырые» байты) — S1.5-P12.

---

## 1.1.1 — Stage 1.5 debt track (BUG-S0-011 + S1.5-P04 + S1.5-P01)

**Status**: In progress  
**Date**: 2026-10-08  
**Prompts**: S1.5-P02 (BUG-S0-011/014/016), S1.5-P04 (BUG-S0-018/019), S1.5-P01 (BUG-S0-015), BUG-S0-020, BUG-S1-001, BUG-S1-002, BUG-S1-003, BUG-S1-005, BUG-S1-006, BUG-S1-015 (tooling)

### BUG-S0-011: Sparse Merkle Tree replaces flat «Verkle»

- **Breaking:** `state_root` computation changed. Historical roots from the flat 256-slot structure are invalid. Reset LevelDB or resync testnet nodes. Wire `FORMAT_VERSION` unchanged (4).
- `crates/strangecoin-core/src/state/verkle.rs` **removed** → `sparse_merkle.rs`
- `SparseMerkleTrie`: binary tree, depth 256 over `blake3(address)` (256-bit key, MSB-first); leaf = `blake3(key‖balance‖nonce)`; internal = `blake3(left‖right)`; empty subtree hashes precomputed
- Closes **BUG-S0-011** (not a Verkle — now honestly an SMT), **BUG-S0-014** (no collisions for n ≤ 2^256; sweep 256→512 test), **BUG-S0-016** (`prove(address, account)` validates the claimed account against the trie; mismatch → `CoreError::ProofAccountMismatch`; pruned `(0,0)` proven by empty slot)
- Proof: 256 sibling hashes (8 KiB), one per level; `verify_proof` walks leaf→root
- **Breaking API (BUG-S0-016):** `SparseMerkleTrie::prove` → `Result<Vec<[u8; 32]>, CoreError>`; `build_witness` → `Result<StateWitness, CoreError>`
- ADR-0006 amended: honest SMT decision; true Verkle/KZG deferred to Stage 3+
- API surface unchanged for callers: `compute_state_root`, `root_after`, `build_witness`, `verify_block_stateless`
- Tests: unit + proptest (1000+ accounts unique root; 256→512 sweep; prove/verify round-trip; tamper rejection); full workspace green

### S1.5-P04: blockchain facade decomposition (BUG-S0-018/019)

- `src/blockchain/blockchain_facade.rs`: **1578 → 361 lines** (КГ ≤400 met; residual-trim: serde/wire moved out)
- Moved out of facade:
  - `block_executor.rs`: mining (`mine_block`, `mine_block_inner`), `commit_block`, genesis/grant construction
  - `state_cache.rs`: `open_blockchain` (DB open/load/migrate), LevelDB save, `add_transaction`, `validate_chain`, address migrations
  - `chain_selector.rs`: **real component** (BUG-S0-019) — `try_adopt_candidate`, `chain_has_tx`, `headers_from_height`, `blocks_by_hashes` + unit tests; **wire snapshot + serde** (`BlockchainDeserialize`, `Serialize`/`Deserialize` для `Blockchain`) — residual-trim 2026-10-08; pure fork-choice algorithm stays in core (strangler)
- Line-count evidence (`wc -l src/blockchain/*.rs`): facade 361, block_executor 571, state_cache 586, chain_selector 285, consensus_manager 132
- Public `BlockchainFacade` API unchanged (behavior preserved); all workspace tests green

### S1.5-P01: genesis key removed from code + SCIP-0001 (BUG-S0-015)

- **Closed:** `genesis_keypair()` and seed string `"strangecoin-genesis-seed-2026"` **deleted** from `src/consensus/mod.rs` — node code contains no genesis secret derivation path
- `genesis.json`: field `initial_holder` → `initial_holder_pubkey` (value unchanged; testnet `EXPECTED_GENESIS_HASH` stable)
- New **SCIP-0001** `docs/SCIP/scip-0001-genesis-key-replacement.md`: seed-derived key **BURNED**; offline-generated key mandatory before mainnet freeze/block 1; only pubkey may enter `genesis.json`
- Test `tests/genesis_key.rs`: genesis validates to `EXPECTED_GENESIS_HASH` without any key in code; receiver derives from pubkey only; seed-string regression guard
- THREAT_MODEL V-31: residual updated (code closed; ops obligation via SCIP-0001); Readme testnet-only warning
- **Not in this change (ops gate):** generating the replacement offline key and updating `EXPECTED_GENESIS_HASH` — required before any public/mainnet launch

### BUG-S0-020: P02 skeletons actualized — GUI moved out of lib.rs

- **gui:** `WalletApp` + `impl eframe::App` (~440 lines of egui UI) **moved** from `lib.rs` into feature-gated `src/gui/mod.rs` (490 lines); new `gui::run(app)` owns `spawn_blocking` + `eframe::run_native` (ADR-0007). `lib.rs` has zero eframe/WalletApp references; gui feature compiles (`cargo check/clippy --features gui` — first compile-check of this feature in repo CI history)
- **api:** stale `// TODO: P15+ наполнит` replaced with documented Stage 4 deferral (ROADMAP3 Dev-experience; ARCHITECT3 §3.10/§10.6 `crates/strangecoin-api`)
- **cli / governance:** not stubs — `--print-genesis-hash` works (BUG-S0-009); governance is a re-export of core SCIP (S1-P05); both got clarifying doc comments
- Closes **BUG-S0-020**; retro-stage0.md untouched per BUG-S0-006 precedent (decisions live in the bugfix catalog)

### Not in this change

- ~~BUG-S0-012 (`state_root == [0;32]` opt-out)~~ — **fixed 2026-10-11** (BUG-S1-002 / SCIP-0002)
- ~~BUG-S0-013 (`verify_block_stateless` post-root)~~ — **fixed 2026-10-11** (BUG-S1-003)
- ~~BUG-S0-015 (genesis key)~~ — **fixed 2026-10-08** (S1.5-P01); ops residual tracked in SCIP-0001
- ~~BUG-S0-020 (P02 skeletons)~~ — **fixed 2026-10-08**; Stage 4 will still move api/gui into dedicated crates (ARCHITECT3 §10.6)

### BUG-S0-023: fuzzing on CI — 10-minute cargo-fuzz job (S1.5-P07)

- CI job `fuzz-canonical-decode` in `.github/workflows/ci.yml`: ubuntu-latest, nightly, cargo-fuzz **0.13.2** (pinned), `cargo fuzz run canonical_decode -- -max_total_time=600 -rss_limit_mb=2560` on **every push/PR**; crash → job fail + `fuzz/artifacts/` upload (30-day retention)
- Local fallback soak **measured 600 s run (2026-10-09)**: 25,947,107 inputs / 0 panics — the S1-P19 checklist's «10-minute run without crashes» (evidence: `docs/stage1/STAGE1_SUMMARY.md` §3; `fuzz/README.md` results table)
- `fuzz/README.md`: CI section + results table; the previously claimed «10-min / 28,744,131 inputs» (no verification-log counterpart) superseded by the measured run
- `docs/security/THREAT_MODEL.md` §7/§8.4: cargo-fuzz obligation updated — CI job exists; residual = first CI run pending
- Hygiene: `fuzz/target/` gitignored; `fuzz/Cargo.lock` tracked (reproducible fuzz builds)
- Closes **BUG-S0-023**; residual: outcome of the first `fuzz-canonical-decode` run on GitHub Actions to be recorded in `fuzz/README.md`

### BUG-S1-001: first release pipeline run — tag `v0.0.1-rc1` (2026-10-10)

- Pre-flight fixes to `.github/workflows/release.yml` (commit `a830c51`) that would have failed the first run:
  - `aggregate-hashes`: removed `merge-multiple: true` (all 6 `hash.txt` flattened into one dir → 5 of 6 overwritten; glob matched nothing → empty SLSA subjects); added `wc -l == 6` guard
  - `base64-subjects` now base64-encoded per `slsa-github-generator@v2.1.0` contract (was raw text → provenance job would fail)
  - `RUSTFLAGS`: `$GITHUB_WORKSPACE` (not shell-expanded in `env:` — silent no-op) → `${{ github.workspace }}`
  - Retired `macos-13` runner → `macos-latest` + `x86_64-apple-darwin` target
  - Hygiene: `if-no-files-found: error`, OS-gated uploads, `hash.txt` excluded from signing/assets, published `SHA256SUMS.txt`
- Tag `v0.0.1-rc1` → run [38071514479](https://github.com/Reider85/strangecoin/actions/runs/38071514479) **green** (6 matrix builds + aggregate + sign + SLSA L3 provenance + release; 31 assets)
- Independent verification: local SHA256 match (windows/linux x86_64), `cosign verify-blob` = `Verified OK`, `slsa-verifier` = `PASSED` @ commit `a830c51`
- Evidence in `docs/security/REPRODUCIBLE_BUILDS.md` §Verified Release Runs (also fixed 3 stale `anomalyco/strangecoin` links → `Reider85/strangecoin`)
- Closes **BUG-S1-001** (BUG-S0-005 continuation): invariant #22 → ✅ (`INVARIANTS_ENFORCED.md`); `STAGE1_SUMMARY.md` §6.4 closed; THREAT_MODEL V-33 closed

### BUG-S1-002: mandatory state_root commitment (2026-10-11)

- **Breaking (consensus, pre-mainnet):** the unconditional zero-`state_root` opt-out is **removed**. Non-genesis blocks with `state_root == [0u8; 32]` are now **rejected** on mainnet/testnet; no config can re-enable them there. Activation per `docs/SCIP/scip-0002-state-root-enforcement.md` (Draft; `activation_height: 0`, `consensus_version` unchanged at 1 — a version bump would have broken legacy regtest chains harder than the state-root rule)
- `Config.allow_zero_state_root: Option<bool>` — network-aware default: unset → `true` on regtest (network_id 3), `false` on mainnet/testnet; `Config::validate` refuses `Some(true)` outside regtest; plumbed as `Blockchain.allow_zero_state_root` → `BlockView.allow_zero_state_root` (builder `.with_allow_zero_state_root`, secure-by-default `false`)
- Core `root_after(state, block, allow_zero_state_root)`: zero root accepted only for genesis (`index == 0`, invariant #19 exemption) or the explicit regtest opt-in; otherwise `Err(StateRootMismatch)`
- **Node-built blocks now commit to real state roots:** `mine_block_inner` computes the post-state root before sealing (unapplicable mempool tx aborts mining); `create_grant_block` likewise. Genesis keeps a zero root (exempt); `genesis.json` / `EXPECTED_GENESIS_HASH` untouched
- **No destructive DB migration:** rewriting stored `state_root` values would change block hashes and break the `previous_hash` chain — legacy regtest DBs stay valid via the regtest default of the flag (rationale in SCIP-0002 §Migration)
- Tests: core `tests/state_root.rs` (9: + zero-root reject, genesis exempt); `tests/block_executor.rs` (14: + strict reject / opt-in accept); e2e `tests/state_root.rs` (6: + strict adopt reject, mine/grant commit); `src/config.rs` unit tests (4: default resolution, override, mainnet/testnet reject, regtest accept)
- Hygiene (clippy-gate enablers, pre-existing lints in untouched files): `address.rs` redundant `matches!`, `merkle.rs` redundant closures/`vec!`, `consensus_proptest.rs` redundant import + OR-pattern range
- Closes **BUG-S1-002** (BUG-S0-012 continuation): invariant #19 → ✅ (`INVARIANTS_ENFORCED.md`, residual count 0); `STAGE1_SUMMARY.md` §6.5 п.5 closed, criteria #2/#12 updated; THREAT_MODEL V-35 zero-root vector closed

### BUG-S1-003: stateless verifier recomputes the post-state root (2026-10-11)

- `verify_block_stateless` no longer stops at pre-state proofs: it now **recomputes the post-state root from the parent root, the witness proofs and the applied block** (S1-P07 КГ, invariant #19) and compares it with the header commitment — `Err(PostStateRootMismatch { expected, got })` on mismatch. A light client can no longer be fed an arbitrary `state_root` (BUG-S0-013 exploit closed)
- New `SparseMerkleTrie::root_after_updates(pre_root, updates)` — mulproof update for the depth-256 SMT: verifies each pre-proof against the pre-root, seeds the node map from the proofs, overwrites the updated leaves with their post-images, recomputes the ancestor chains bottom-up. Untouched sibling subtrees keep their pre-image hashes anchored to `pre_root`, so an **untouched account cannot diverge** in the committed post-state (stronger than the catalog's literal Option A, which would only have verified post-proofs for touched accounts)
- Witness coverage guard: every address in `collect_touched_addresses(block)` must be present in `witness.proofs` (`Err(WitnessVerificationFailed)`) — `apply_block` on a partial state would otherwise default a missing account to `(0, 0)`
- Genesis exemption mirrors `root_after` / SCIP-0002: `index == 0 && state_root == [0;32]` skips the comparison; a non-genesis zero root can never match the recomputed value and is rejected
- **Witness format unchanged** (no new fields, no size doubling); full-node validation paths untouched; no new SCIP (stateless API only; witness is not yet on the wire — Stage 2+)
- New `CoreError::PostStateRootMismatch` + node-side mapping in `src/error.rs`
- Tests: core `sparse_merkle.rs` unit (8: single-key, overlapping key paths, pruned post, absent→present, invalid proof, wrong pre-account, empty updates, 64-account scatter) + proptest roundtrip; integration `tests/witness.rs` (6: forged root, untouched-account tamper in post-commitment, zero-root non-genesis, missing touched address, genesis exempt + forged genesis root, + proptest bit-flip)
- Closes **BUG-S1-003** (BUG-S0-013 continuation): stateless post-root gap → ✅; THREAT_MODEL V-36 mitigation now matches the code; `STAGE1_SUMMARY.md` §6.5 п.5 + criterion #1 caveat updated

### BUG-S1-005: per-message + per-type rate limiting (2026-10-11)

- **P13 enforcement gap closed:** the rate limiter ran only at TCP-accept time — a long-lived peer could stream unlimited messages through one connection. Every inbound message in the `handle_connection` request loop now goes through `RateLimiter::check_class` (total + per-class bucket); exceeding either bans the peer (5 min) and closes the connection
- New `MessageClass { GetHeaders, GetBlocks, ChainGossip, Other }` + `CLASS_LIMITS` (same 10 s window): GetHeaders=100, GetBlocks=100 (legit headers-first sync unaffected), ChainGossip=30, Other=30 (junk/legacy floods banned early without starving other classes). `PeerCounter` gained per-class counters, reset with the window; shared ban/window bookkeeping factored into `with_checked_counter`
- New `classify_request(&[u8])` (`src/lib.rs`): 0x01→GetHeaders, 0x02→GetBlocks, `UPDATE_BLOCKCHAIN:`→ChainGossip, else→Other. The per-message check runs **after** a successful read (classification needs the message bytes; idle timeouts no longer consume tokens) — the catalog suggested before-read; deviation recorded in `bugfixes-stage1.md`
- Old `check(addr)` (total-only) kept for the pre-HELLO accept gate, outbound sync rounds and existing callers (`network_id.rs`, `sync_engine` inbox-overflow bans)
- Tests: `rate_limiter.rs` unit (8: + per-class flood→ban, class isolation, window reset clears class counters, ban shared across `check`/`check_class`); integration `tests/rate_limit.rs::per_message_flood_bans_peer_on_single_connection` — one TCP connection, 200 junk messages <1 s → server closes (EOF/RST) + `rate_limiter.check(client_addr)` errs
- Docs: invariant #17 updated (`INVARIANTS_ENFORCED.md`); THREAT_MODEL V-01 Stage 1 amendment + V-17 partial + test-map rows; `bugfixes-stage1.md` status → fixed
- Residual / Stage 2-P03: buckets keyed by full `SocketAddr` (per-ephemeral-port, not per-IP); `CLASS_LIMITS` hard-coded; `RateLimiter` moves into `strangecoin-net` with the network-crate migration
- Closes **BUG-S1-005**: P13 «ban mid-flood» expectation met end-to-end; no wire-format or consensus change (no new SCIP)

### BUG-S1-006: block + per-transaction size checks in `validate_and_apply` (2026-10-11)

- **Invariant #7 / P12 enforcement gap closed:** `MAX_BLOCK_SIZE` (4 MiB) and `MAX_TX_SIZE` (256 KB) were checked only at the wire (`parse_blocks`) and whole-chain scan (`state_cache::validate_chain`) — a `ChainSnapshot` adopted via `adopt_candidate`/`commit_block` bypassed them at the validation layer
- `validate_and_apply` now rejects first-thing: serialized block (`serialize_block`, canonical wire metric) > `MAX_BLOCK_SIZE` → `SizeLimitExceeded("block")`; any signed tx (`serialize_transaction_signed`) > `MAX_TX_SIZE` → `SizeLimitExceeded("transaction")` (unconditional — the grant-block exemption covers signatures, not sizes). Both with `warn!` logs; constants reused from `network::protocol` (same pattern as `state_cache.rs`); `commit_block` inherits the check via `validate_and_apply`
- Tests (`tests/block_executor.rs`, 3 new): 5 MiB-coinbase block → `SizeLimitExceeded("block")`; signed 300 KB-receiver tx inside a sub-4 MiB block → `SizeLimitExceeded("transaction")`; positive — ~3.1 MiB block (coinbase + 13 signed transfers, each tx < 256 KB) applies with balances intact
- Docs: invariant #7 locations + tests updated; THREAT_MODEL V-13 mitigation; `bugfixes-stage1.md` status → fixed
- No new SCIP: completing the already-declared invariant #7 / P12 limits — wire constants unchanged, every wire-accepted block was already ≤ 4 MiB (no chain-split risk); only the internal validation path was closed
- Closes **BUG-S1-006** (High, audit/P12)

### BUG-S1-015: offline genesis keygen tooling — ops gate remains (2026-10-14)

- New `examples/generate_offline_genesis_key.rs`: generates a fresh secp256k1 key (`SecretKey::new(&mut OsRng)`); stdout = compressed pubkey only (33 bytes, `0x`-hex, one line — safe to redirect to `genesis_pubkey.txt`); private key hex → optional file argument via `create_new(true)` (never overwrites) or stderr with a warning banner when omitted; the secret never touches stdout
- `docs/SCIP/scip-0001-genesis-key-replacement.md` §4: Rust example is now the **primary** offline path; openssl path demoted to alternative; Current-state + Reference Implementation updated
- Docs: `STAGE1_SUMMARY.md §6.1` (tooling noted, status stays 🟡); THREAT_MODEL V-31 residual + test-map row; `bugfixes-stage1.md` status → partially fixed (tooling) / open (ops)
- **Not in this change (human ops, air-gapped machine):** running the example for the real mainnet key, vaulting the privkey, replacing `genesis.json:initial_holder_pubkey`, recomputing `EXPECTED_GENESIS_HASH`, tagging `v1.0.0-mainnet-freeze` — required before mainnet freeze/block 1 (SCIP-0001 §3)
- Partially closes **BUG-S1-015** (tooling side; ops keygen remains the mainnet blocker)

## 1.1.0 — Stage 1 (core extracted + Verkle + headers-first)

**Status**: Complete — verified by S1-P22; tag `v1.1.0-stage1`
**Date**: 2026-10-04
**DoD evidence**: `docs/stage1/STAGE1_SUMMARY.md`

### S1-P01: consensus — grant blocks behind regtest-only flag

- Added `Config.allow_grant_blocks` (default false; true only for regtest / legacy DB migration)
- `validate_chain`: grant exception (`block.index != 1`) and magic-string sender skips run **only** when the flag is true
- `create_grant_block` returns typed `GrantBlocksDisabled` + warn when disabled
- Magic-strings removed from the normal consensus path
- Tests: `tests/grant_flag.rs` (4)

### S1-P02: core — strangecoin-core crate + serialize (+ADR-0008)

- Cargo workspace `members = [".", "crates/strangecoin-core"]`, `resolver = "2"`
- Moved `serialize.rs` (canonical blake3 codec, txid) into `strangecoin-core`; golden vectors in `crates/strangecoin-core/tests/serialize_golden.rs`
- Stub `economics/fee_market.rs` (Stage 5)
- ADR-0008: RocksDB planning instead of redb/sled

### S1-P03: core — consensus rules + economics

- Moved consensus (chain_id, U256, retarget, MTP, CURRENT_CONSENSUS_VERSION) and `economics/emission.rs` into core
- Property tests → `crates/strangecoin-core/tests/consensus_proptest.rs`
- Core stays 0 I/O (no fs/net/tokio/leveldb)

### S1-P04: core — pure state apply/unapply

- `State`, `apply_block`, `unapply_block` in `crates/strangecoin-core/src/state.rs` (pure, no I/O)
- Property tests: round-trip `unapply(apply(s,b),b) == s` (invariant #4)
- Monolith apply paths delegate to core where applicable

### S1-P05: core — governance skeleton (SCIP + consensus_version + activation height)

- `crates/strangecoin-core/src/governance/scip.rs`: ScipDocument, ConsensusRules, activation-by-height
- `consensus_version: u32` in block header; `CURRENT_CONSENSUS_VERSION = 1`
- Canonical serialization bumped (format_version); golden vectors updated
- `docs/SCIP/README.md` + `docs/SCIP/scip-0000-process.md`
- Tests: `tests/consensus_version.rs`, core scip activation tests

### S1-P06: core — Verkle Trie + block.state_root (+ADR-0006)

- `crates/strangecoin-core/src/state/verkle.rs`: deterministic state commitment
- `block.state_root` in header; invariant #19 enforced in validation
- ADR-0006: Verkle Trie vs SMT
- Tests: core `tests/state_root.rs`, e2e `tests/state_root.rs`

### S1-P07: core — StateWitness + stateless verification

- `state/witness.rs`: `build_witness`, `verify_block_stateless`, AccountProof
- Tamper / wrong-parent-root / proptest coverage in `crates/strangecoin-core/tests/witness.rs`

### S1-P08: core — merkle tx_root in block header

- `merkle_root` / `compute_tx_root` in serialize; `tx_root` field in Block
- `validate_tx_root` in consensus; golden vectors updated
- Tests: `crates/strangecoin-core/tests/merkle.rs`

### S1-P09: node — EventBus (crossbeam) (+ADR-0009)

- `src/events.rs`: multi-subscriber bus (BlockApplied, BlockReorged, TxAccepted, TxRejected, StatePersisted, …)
- ADR-0009 written before code

### S1-P10: node — tokio runtime (+ADR-0007)

- tokio introduced for new subsystems; legacy mining/network threads remain until Stage 2
- ADR-0007; `tests/two_clients.rs::node_runs_on_tokio_and_shuts_down_cleanly`

### S1-P11: blockchain — chain_selector + deterministic tie-breaking

- `src/blockchain/chain_selector.rs` + core ChainSelector: **work → earliest timestamp → lowest hash**
- `docs/spec/fork_choice.md`
- Proptests: `tests/chain_selector_proptest.rs`

### S1-P12: blockchain — block_executor + state_cache

- `block_executor.rs`: single validate+apply path (no tip selection, no storage writes)
- `state_cache.rs`: balances/nonces reader-writer + `rebuild_from_chain` (invariant #1)
- Tests: `tests/block_executor.rs`, `tests/state_cache.rs`

### S1-P13: blockchain — facade + consensus_manager (5-component split)

- `blockchain_facade.rs`: sole public entry point for chain adoption/reads
- `consensus_manager.rs`: consensus phase/version by height
- Split complete: chain_selector, block_executor, state_cache, facade, consensus_manager

### S1-P14: network — network_id in genesis + HELLO

- `network_id` in genesis and HELLO handshake; foreign networks rejected/banned
- Constants: mainnet=1, testnet=2, regtest=3

### S1-P15: core — bech32 addresses with network HRP

- `encode_address` / `decode_address` (bech32m, 33-byte compressed pubkey)
- HRP: `sc1` / `tsc1` / `rsc1`; base64 addresses eliminated from core
- Legacy base64 migration path on DB open in facade
- Tests: address unit tests + `tests/two_clients.rs::bech32_address_transfer`

### S1-P16: network — headers-first sync

- Binary wire protocol: GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS (tags 0x01..0x04)
- Header codec, cumulative work, `plan_best_branch`, download-then-adopt
- Pre-P16 peers fall back to legacy GET_BLOCKCHAIN
- Tests: `tests/sync_headers.rs`

### S1-P17: mempool — replace-by-fee

- RBF_MIN_DELTA_BPS, find_replaceable, MAX_RBF_REPLACEMENTS; deterministic rules
- Tests: `tests/rbf.rs`

### S1-P18: node — SyncEngine inbox (+ADR-0010)

- `src/network/sync_engine.rs`: single consumer of incoming chain data
- Bounded lanes, dedupe, backpressure (drop/ban)
- Network no longer calls `adopt_candidate` / `apply_tx` / `save_state` directly
- Tests: `tests/sync_engine.rs`

### S1-P19: tests — Stage 1 matrix + first fuzz target

- Integration matrix: state_root, events (3 subscribers), network_id, RBF, facade/bech32 updates
- `fuzz/fuzz_targets/canonical_decode.rs` + `fuzz/README.md`
- cargo-fuzz not runnable on Windows dev host (recorded); fallback soak found + fixed OOB panic in `deserialize_block`
- Soak: 474,380 inputs / 0 panics (10 s smoke; the «10 min / 28,744,131» figure previously listed here had no verification-log counterpart — superseded by the measured 2026-10-09 600 s run, BUG-S0-023)

### S1-P20: docs — invariants re-audit

- `docs/stage1/INVARIANTS_ENFORCED.md`: all 22 invariants with Stage 1 enforcement locations
- Invariants #19 (state root) and #21 (consensus versioning) enforced for the first time
- Network modules cleaned to facade-only coupling

### S1-P21: security — threat model update

- `docs/security/THREAT_MODEL.md` → v3.0: Stage 1 vectors V-34..V-42 (headers-first, state_root, witness, tx_root, consensus_version, network_id, RBF, inbox, HRP)
- `docs/security/INCIDENT_RESPONSE.md` → v3.0: Stage 1 alert sources + SLA
- Residual risks documented (zero state_root opt-in, cargo-fuzz host, TLA+ coverage)

### S1-P22: DoD verification + tag

- `docs/stage1/STAGE1_SUMMARY.md`: 15/15 DoD criteria with evidence
- `crates/strangecoin-core/src/vm/traits.rs`: `VmExecutor` trait declaration (Stage 1.5; no runtime)
- Version sync 1.1.0 (Cargo.toml root + core, Changelog, AGENTS.md)
- Verification: `cargo test --workspace` green; `cargo clippy --all-targets -- -D warnings` green; fuzz smoke green
- Annotated tag `v1.1.0-stage1` (Gate Stage 1.5)

---

## 1.0.0 — Stage 0 (Sanitized Prototype)

### P25: Bug bounty + ADR-0003 (hybrid PoW→PoS) (2026-09-17)

- Created `docs/ADR/0003-hybrid-pow-pos.md` — hybrid PoW (Stage 0–6) → PoS (Stage 7+) migration decision
- Created `docs/security/BOUNTY.md` — bug bounty program: scope, 4 reward tiers ($500/$1k/$10k/$100k), 90-day disclosure, Immunefi setup (tier count corrected 2026-10-13, BUG-S1-013: delivered BOUNTY.md had 4 tiers, not the 3 originally planned in P25)
- Created `docs/security/SECURITY.md` — security contacts, PGP key placeholder, 48h SLA, safe harbor policy

### P24: Reproducible builds (2026-09-17)

- Added `[profile.release]` with LTO, single codegen unit, symbol stripping for deterministic builds
- Created `.github/workflows/release.yml` — release pipeline triggered on `v*` tags
  - Builds 6 targets: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64/aarch64
  - `RUSTFLAGS="--remap-path-prefix"` strips build paths for reproducibility
  - SHA256 checksums for each artifact
  - SLSA Level 3 provenance via `slsa-github-generator`
  - Keyless cosign signatures (Sigstore/OIDC)
- Created `docs/security/REPRODUCIBLE_BUILDS.md` — verification instructions

### P23: TLA+ consensus spec skeleton

- Created `docs/spec/consensus.tla` with safety properties (NoDoubleSpend, NoInflation, AllTxSigned, NonceMonotonic, ChainContinuity) and liveness
- Created `docs/spec/consensus.cfg` for TLC model checker
- Created `docs/spec/README.md` with verification instructions

### ~~P22: Threat Model (STRIDE)~~ — NOT COMPLETED IN STAGE 0

- **Entry corrected:** P22 was NOT completed during Stage 0. This changelog entry was added prematurely (commit "docs: add CHANGELOG.md" predates any THREAT_MODEL.md file).
- P22 is executed in **D02** (debt prompt, see `analytics/prompt-stage1.md`).
- Files `docs/security/THREAT_MODEL.md` and `docs/security/INCIDENT_RESPONSE.md` are created in D02 commit.

### D03: DoD-verification + version sync + tag (2026-09-28)

- Fixed `.github/workflows/release.yml`: added `aggregate-hashes` job for SLSA provenance, added 6th target (`aarch64-pc-windows-msvc`)
- Cleaned repository: removed `test.md`, `.idea/`, `.codebuddy/`, `.opencodeignore` from tracking; `Cargo.lock` now tracked
- Synchronized version to `1.0.0` across `Cargo.toml`, `Changelog.md`, `AGENTS.md`
- Created `docs/stage0/CRITICAL_ISSUES_CLOSED.md` — 13 issues from ARCHITECT2 §1.1 with honest statuses
- Created `docs/stage0/INVARIANTS_ENFORCED.md` — 22 invariants from ARCHITECT3 §5 with enforcement locations
- Created `docs/stage0/STAGE0_SUMMARY.md` — Stage 0 completion summary with Stage 1 obligations
- Tag `v1.0.0-stage0`
