# THREAT_MODEL_STAGE1_5.md — Stage 1.5 Smart Contract Threats (Summary)

**Document**: `docs/stage1.5/THREAT_MODEL_STAGE1_5.md`
**Status**: Complete ✅
**Date**: 2026-10-17 (S1.5-P18); re-audit 2026-10-20 (S1.5-FBP08)
**Prompt**: S1.5-P18 + S1.5-FBP08 (`analytics/bugfixes-stage1.5.md`)
**Full model**: `docs/security/THREAT_MODEL.md` v3.1 (§3.7 — V-18..V-21; §6.2b/§6.3/§6.4 — coverage; §7.1b — residual)

Краткое резюме threat-векторов, впервые актуализированных появлением WASM смарт-контрактов (Stage 1.5). Нумерация векторов — общий threat model (ARCHITECT3 §6); инварианты — `docs/stage1.5/INVARIANTS_ENFORCED.md`.

## 1. Закрытые векторы (V-18..V-21 → Mitigated)

| # | Вектор | STRIDE | Митигация (Stage 1.5) | Ключевой код | Ключевой тест |
|---|--------|--------|----------------------|--------------|---------------|
| V-18 | Reentrancy | Tampering | `call_stack` + ABI-флаг `non_reentrant` → `VmError::ReentrancyDetected`; depth cap `MAX_CALL_DEPTH`=1024 | `vm-wasm/src/host/calls/mod.rs:108-140`, `core/abi/mod.rs` | `tests/vm_reentrancy_attack.rs::reentrancy_attack_end_to_end_is_rejected` |
| V-19 | Integer overflow | Tampering | Rust checked/saturating arith; `modexp` precompile (checked lengths); `MAX_CODE_SIZE`=24KB в `verify_transaction` | `core/consensus.rs:354`, `vm-wasm/src/precompiles/modexp.rs` | `tests/precompiles.rs::modexp_*`, `create_code_size_over_limit_rejected` |
| V-20 | Storage exhaustion | DoS | `MAX_STORAGE_PER_CONTRACT`=1MB в `storage_write`; block gas limit `BLOCK_GAS_LIMIT`=1M (pre+post exec) | `core/vm/limits.rs`, `vm-wasm/src/host/storage/storage_write.rs`, `src/blockchain/block_executor.rs::enforce_*` | `storage_cap_exceeded_rejected`, `tests/vm_gas_exhaustion.rs` |
| V-21 | Code injection (malicious WASM) | Tampering / EoP | wasmi sandbox (0 I/O); host whitelist (13 funcs); gas metering (ADR-0014); `MAX_MEMORY_PAGES`=16, `MAX_STACK_HEIGHT`=1M | `vm-wasm/src/lib.rs`, `core/vm/limits.rs` | `memory_grow_beyond_limit_traps_out_of_memory`, `register_code_rejects_invalid_wasm`, fuzz differential |

Связанные инварианты:
- **#15** (block gas limit) — enforced (pre-execution `enforce_block_gas_limit` + post-execution `enforce_receipt_gas_limit`; FBP04: `Create.gas_limit` резервируется тоже)
- **#18** (Receipt) — enforced (каждый tx → `Receipt`; derived data; receipts sub-trie; FBP07: `receipts_root` в BlockHeader — light-client commitments)
- **#19** (state root) — extension (contract storage → `storage_root` → account leaf → global root; FBP07: `receipts_root` validation в `validate_and_apply`)

## 2. S1.5-FBP01..FBP07 residuals (re-audit S1.5-FBP08, 2026-10-20)

Регрессия после S1.5-FBP03..FBP07 (selector dispatch, constructor execution,
nested value settlement, lazy contract storage, `receipts_root` в header).
Порядок проверок в коде **verified** (см. таблицу):

| # | Вектор | Новый attack surface | Митигация (verified) | Ключевой код | Ключевой тест |
|---|--------|----------------------|----------------------|--------------|---------------|
| V-18 | Reentrancy через nested value transfer (FBP05) | `call(to, value)` — получатель-контракт может вызвать вызывающего обратно (в т.ч. через value-обработчик) до завершения внешнего frame | depth-cap + `non_reentrant` ABI-флаг проверяются в `nested_call` **до** value transfer (`mod.rs:118-133` гейт → `:191-206` `transfer_value`); `ReentrancyDetected` — до движения средств; delegatecall: value остаётся у parent (storage_address наследуется); staticcall: value>0 → fail | `vm-wasm/src/host/calls/mod.rs:118-206` | `tests/vm_reentrancy_attack.rs` (nested-вызов с селектором резолвится строго), `tests/vm_transfer_to_contract.rs` (chain A→B→C settle/unapply; failed chain) |
| V-19 | Integer overflow в constructor init_code (FBP04) | `CreateTx.init_code` — arbitrary WASM; конструктор исполняется при deploy (`execute_constructor`) | wasmi sandbox (fuel, stack/recursion/memory caps — не ослаблены для конструкторов); `MAX_CODE_SIZE`=24KB в `verify_transaction`; gas metering: `Create.gas_limit` резервируется pre-execution (`enforce_block_gas_limit`) и выгорает в OutOfGas → failed-receipt rollback (endowment не переведён, storage откачен); Rust checked/saturating arith в host/precompiles | `core/state/inner.rs::apply_create` (`execute_constructor`), `src/blockchain/block_executor.rs:278-285` | `tests/vm_deploy_call.rs` (constructor OutOfGas → rejected + rollback; legacy без export → ok), `tests/vm_gas_exhaustion.rs` |
| V-21 | Selector-based dispatch (FBP03) | calldata[0..4] → `entry_by_selector` → именованный WASM export; семантика miss | miss (calldata<4 / нет зарегистрированного ABI / нет записи для селектора) → fallback legacy `main` (safe pre-FBP03 semantics); ABI-hit без export → строгий `MissingExport` (контракт-баг, не silent fallback); неподдерживаемая сигнатура экспортa → `TypeMismatch`; sandbox/limits **не ослаблены** — dispatch только выбирает export внутри уже sandboxed инстанса; calldata ограничен размером tx | `vm-wasm/src/lib.rs:275-278, 367-373, 406-414` | vm-wasm `tests/basic_execution.rs` (16/16: двух-функциональный dispatch, legacy fallback, `sc::input_read`), `tests/reentrancy_and_limits.rs` (13/13) |

Дополнительно: FBP06 (lazy contract storage через `Arc<RwLock>`) не ослабил
V-20 — `MAX_STORAGE_PER_CONTRACT`=1MB cap сохранён (`storage_cap_exceeded_rejected`);
FBP07 (consensus-breaking `receipts_root`) закрыт ADR-0016 + TLA+
`ReceiptsRootMatch` (инварианты консенсуса, не sandbox — см. `INVARIANTS_ENFORCED.md` #18/#19).

## 3. Residual Risks Stage 2+ (§7.1b полной модели)

Полный перечень — `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` (S1.5-FBP08).
Ключевые строки:

| Risk | Stage | Комментарий |
|------|-------|-------------|
| Block-STM (parallel execution) | Stage 3+ | Anti-goal Stage 1.5; serial — детерминизм |
| Account Abstraction | Stage 5 | Protocol-level AA нет; контракты мультиподписывают off-pattern |
| EIP-1559 dynamic base fee | Stage 5 | Сейчас фиксированный gas limit + fee=0; constructor fee burning — тоже Stage 5 |
| State rent / global state cap | Stage 3+ | Per-contract 1MB не останавливает спам множеством контрактов; contract storage prune — Stage 3+ (SCIP) |
| ~~Receipts fold в header~~ | — | **closed S1.5-FBP07** (`BlockHeader.receipts_root`, ADR-0016); receipts prune — Stage 3+ (SCIP) |
| wasmi 0-days | continuous | Fuzz (S1.5-P16) + differential wasmi vs wasmtime; K-framework — Stage 6 |
| Контракт-level бизнес-логика | Stage 4+ | Sandbox не защищает от «легитимно злонамеренного» кода |

## 4. Monitoring (дополнение к THREAT_MODEL §8.1)

| Метрика | Порог | Действие |
|---------|-------|----------|
| `ReentrancyDetected` rejects | Spike per contract | Проверить атаку / regression guard |
| Storage cap rejects | Сustained rate | Storage exhaustion attempt |
| OutOfGas / Failed rate | Anomalous | Злонамеренный/битый контракт |
| Block gas limit rejects | Spike | Gas DoS / mispriced weights |
| wasmi trap (unexpected) | Any | **Critical**: sandbox escape / interpreter bug |

## 5. Fuzzing (S1.5-P16)

| Target | Покрытие |
|--------|----------|
| `fuzz/fuzz_targets/host_functions.rs` | 13 host functions на произвольных входах |
| `fuzz/fuzz_targets/gas_metering.rs` | gas weights / OutOfGas determinism |
| `fuzz/fuzz_targets/storage_trie.rs` | storage sub-trie / overlay flush |
| `fuzz/fuzz_targets/differential_wasmi_vs_wasmtime.rs` | детерминизм wasmi vs wasmtime |

---

**Related**: `docs/security/THREAT_MODEL.md` v3.1 · `docs/stage1.5/INVARIANTS_ENFORCED.md` · `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` · `analytics/prompt-stage1.5.md` S1.5-P18 · `analytics/bugfixes-stage1.5.md` S1.5-FBP08 · ADR-0012..0016 · `fuzz/README.md`
