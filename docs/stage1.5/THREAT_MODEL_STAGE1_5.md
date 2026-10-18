# THREAT_MODEL_STAGE1_5.md — Stage 1.5 Smart Contract Threats (Summary)

**Document**: `docs/stage1.5/THREAT_MODEL_STAGE1_5.md`
**Status**: Complete ✅
**Date**: 2026-10-17
**Prompt**: S1.5-P18
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
- **#15** (block gas limit) — enforced (pre-execution `enforce_block_gas_limit` + post-execution `enforce_receipt_gas_limit`)
- **#18** (Receipt) — enforced (каждый tx → `Receipt`; derived data; receipts sub-trie)
- **#19** (state root) — extension (contract storage → `storage_root` → account leaf → global root)

## 2. Residual Risks Stage 2+ (§7.1b полной модели)

| Risk | Stage | Комментарий |
|------|-------|-------------|
| Block-STM (parallel execution) | Stage 3+ | Anti-goal Stage 1.5; serial — детерминизм |
| Account Abstraction | Stage 5 | Protocol-level AA нет; контракты мультиподписывают off-pattern |
| EIP-1559 dynamic base fee | Stage 5 | Сейчас фиксированный gas limit + fee=0 |
| State rent / global state cap | Stage 3+ | Per-contract 1MB не останавливает спам множеством контрактов |
| Receipts fold в header | Stage 2+ | Receipts — derived + sub-trie proofs; light-client receipts требуют fold |
| wasmi 0-days | continuous | Fuzz (S1.5-P16) + differential wasmi vs wasmtime; K-framework — Stage 6 |
| Контракт-level бизнес-логика | Stage 4+ | Sandbox не защищает от «легитимно злонамеренного» кода |

## 3. Monitoring (дополнение к THREAT_MODEL §8.1)

| Метрика | Порог | Действие |
|---------|-------|----------|
| `ReentrancyDetected` rejects | Spike per contract | Проверить атаку / regression guard |
| Storage cap rejects | Сustained rate | Storage exhaustion attempt |
| OutOfGas / Failed rate | Anomalous | Злонамеренный/битый контракт |
| Block gas limit rejects | Spike | Gas DoS / mispriced weights |
| wasmi trap (unexpected) | Any | **Critical**: sandbox escape / interpreter bug |

## 4. Fuzzing (S1.5-P16)

| Target | Покрытие |
|--------|----------|
| `fuzz/fuzz_targets/host_functions.rs` | 13 host functions на произвольных входах |
| `fuzz/fuzz_targets/gas_metering.rs` | gas weights / OutOfGas determinism |
| `fuzz/fuzz_targets/storage_trie.rs` | storage sub-trie / overlay flush |
| `fuzz/fuzz_targets/differential_wasmi_vs_wasmtime.rs` | детерминизм wasmi vs wasmtime |

---

**Related**: `docs/security/THREAT_MODEL.md` v3.1 · `docs/stage1.5/INVARIANTS_ENFORCED.md` · `analytics/prompt-stage1.5.md` S1.5-P18 · ADR-0012..0015 · `fuzz/README.md`
