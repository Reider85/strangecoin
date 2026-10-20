# Stage 1.5 DoD Verification (S1.5-P19)

**Document**: `docs/stage1.5/STAGE1_5_SUMMARY.md`
**Status**: Complete ✅
**Date**: 2026-10-18
**Refreshed**: 2026-10-20 (S1.5-FBP09 drift sync + S1.5-FBP11 release verification)
**Prompt**: S1.5-P19 — Definition of Done verification for Stage 1.5 (WASM smart contracts)
**Prompt refs**: S1.5-P01..P19 + S1.5-FBP01..FBP11 (`analytics/bugfixes-stage1.5.md`)
**Gate**: Tag `v1.5.0-stage1.5` (Gate Stage 2/3/4; Stage 4 требует Stage 1.5 завершённым)
**Predecessor**: Tag `v1.1.0-stage1` (S1-P22)

---

## 1. DoD Criteria — 14 items (ROADMAP3 Этап 1.5 §Definition of Done)

| # | Criterion | Evidence | Status | Residual |
|---|-----------|----------|--------|----------|
| 1 | `crates/strangecoin-vm-wasm/` создан, wasmi интегрирован | Workspace member (`Cargo.toml` members); `vm-wasm/Cargo.toml` — `wasmi = "=2.0.0"` (exact pin, ADR-0012); `WasmiExecutor` в `vm-wasm/src/lib.rs`; `main.rs` wiring через feature `vm-wasm` (default-on) | ✅ | нет |
| 2 | Все host functions реализованы и протестированы | 14 functions: crypto 5 (`secp256k1_verify`, `bls_verify`, `blake3`, `keccak`, `sha256`) + storage 2 (`storage_read`, `storage_write`) + calls 3 (`call`, `delegatecall`, `staticcall`) + context 3 (`gas_left`, `block_context`, `input_read` — S1.5-FBP03 `f4dadda`) + logs 1 (`emit_log`) — `vm-wasm/src/host/**`; тесты `vm-wasm/tests/host_functions.rs` (43) | ✅ | нет |
| 3 | Gas metering работает (`gas_left()` уменьшается) | ADR-0014: instruction counting + host-call weights; `vm-wasm/src/gas.rs`; host `gas_left`; `vm-wasm/tests/gas_metering.rs` (9); e2e `tests/vm_gas_exhaustion.rs` (3: OutOfGas burn, block-limit accept/reject) | ✅ | нет |
| 4 | Tx types `transfer/create/call` работают | `core/types.rs` + per-variant serialization (S1.5-P08); `CreateTx.gas_limit` + `constructor_params` на wire (S1.5-FBP04, FORMAT_VERSION 6), `receipts_root` в header (S1.5-FBP07, FORMAT_VERSION 7, HEADER_WIRE_LEN 221); `core/tests/tx_types.rs` (20: roundtrip ×3, apply/unapply, mempool insert, serde); e2e deploy/call в `tests/vm_deploy_call.rs` | ✅ | нет |
| 5 | Precompiles реализованы и протестированы | 7 шт (ADR-0015): `0x01` ecrecover, `0x02` sha256, `0x03` keccak, `0x04` blake3, `0x05` ed25519_verify, `0x06` bls_aggregate (**NotImplemented** до Stage 7), `0x07` modexp; `vm-wasm/tests/precompiles.rs` (36); e2e `tests/vm_precompiles_integration.rs` (7) | ✅ | `bls_aggregate` — NotImplemented by design (Stage 7, §4) |
| 6 | JSON ABI работает (round-trip encode/decode) | Gap-closed S1.5-P12 `66c5659` + **residuals S1.5-FBP01/FBP02 (2026-10-19)**: `core/abi/{mod,encode,decode,selector}.rs` — `AbiDocument`/`AbiType` (9 variants), head/tail codec (32-byte words, 4-byte BE offsets/lengths, canonical-offset enforcement), `method_selector` = blake3-4B (golden vectors literal hex), `AbiEntry::selector_from_name` (`eee7b7b`), FixedArray len>0 validation (`10e9acc`), финальный gap-closure `b925f71`; `format_version: "wit-v2"` → `NotImplemented`; `core/tests/abi_roundtrip.rs` (15: per-type roundtrip incl. array-of-fixedarray, golden selectors, JSON roundtrip, negative paths) + unit tests | ✅ (gap closed pre-tag + FBP01/02 residuals) | нет |
| 7 | Reentrancy guard работает | `call_stack` + ABI-флаг `non_reentrant` → `VmError::ReentrancyDetected` (`vm-wasm/src/host/calls/mod.rs`); S1.5-FBP03 (`f4dadda`): selector-based dispatch, строгое MissingExport для ABI-hit без экспорта, порядок валидации guard → value transfer (`mod.rs:118-193`, threat V-18); `vm-wasm/tests/reentrancy_and_limits.rs` (13); e2e `tests/vm_reentrancy_attack.rs` (2: attack rejected, storage-guarded self-call allowed) | ✅ | нет |
| 8 | Limits enforced (превышение → OutOfGas/OutOfMemory) | `core/vm/limits.rs`: `MAX_CODE_SIZE`=24KB (verify_transaction), `MAX_STORAGE_PER_CONTRACT`=1MB, `MAX_CALL_DEPTH`=1024, `MAX_MEMORY_PAGES`=16 (1 MB), `MAX_STACK_HEIGHT`=1M (wasmi config); `tests/reentrancy_and_limits.rs` (boundary ×2 per limit) | ✅ | нет |
| 9 | Events/logs работают (emitted log in receipt) | `Log`/`Receipt`/`TxStatus` (`core/vm/result.rs`); receipts sub-trie (`sparse_merkle.rs`); `vm-wasm/tests/events_and_receipts.rs` (11); node `tests/receipts.rs` (4); инвариант #18 — `docs/stage1.5/INVARIANTS_ENFORCED.md` №18 ✅ | ✅ | нет |
| 10 | Contract storage в общем state trie (`state_root` покрывает storage) | `ContractStorage`/`Overlay` (`core/state/contract_storage.rs`); `contract_storage_root` sub-trie (leaf `blake3(0x03‖key‖value)`); `ContractAccount.storage_root` входит в account leaf → глобальный `state_root` (#19 extension); S1.5-FBP06 (`06c3444`): `SharedContractStorage = Arc<RwLock<...>>` — O(1) setup per call, per-frame overlay, pointer-stability тест (100 CallTx × 10K keys); `root_after_with_vm`; `core/tests/contract_storage_smt.rs` (8); `vm-wasm/tests/contract_storage.rs` (11) | ✅ | **Доставлено как SMT, не Verkle**: true Verkle/KZG — Stage 3+ (ADR-0006 amended, Stage 1 residual сохранён) |
| 11 | Differential testing проходит (wasmi vs wasmtime) | `vm-wasm/tests/differential.rs` (13: identical results on bounded modules, wasmi =2.0.0 vs wasmtime =49.0.3 dev-dep); fuzz-target `fuzz/fuzz_targets/differential_wasmi_vs_wasmtime.rs` (S1.5-P16) | 🟡 | cargo-fuzz **не запускался локально** (Windows host, нет cargo-fuzz/dlltool — как Stage 1 §6.2); CI jobs добавлены (`.github/workflows/ci.yml`), первый полный прогон 4 новых targets — верифицировать в CI |
| 12 | All Stage 0+1 invariants still enforced (no regressions) | `docs/stage1.5/INVARIANTS_ENFORCED.md` (S1.5-P18, re-audit S1.5-FBP08 `768fc99`): **22/22 enforced** (extensions #4/#5/#8/#11/#15/#18/#19; #20 degenerate fee=0 по дизайну); #15/#18 впервые enforced; #19 extension; полный workspace suite green (§3) | ✅ | #20 fee-split — Stage 5 (по дизайну, degenerate) |
| 13 | ADR-0012..0015 написаны | `docs/ADR/0012-wasm-wasmi-vs-risc-v.md`, `0013-json-abi-for-mvp-vs-wit.md`, `0014-gas-metering-strategy.md`, `0015-precompiles-list.md` (S1.5-P01..P04) | ✅ | **Renumber**: спека промпта называла их 0011..0014; 0011 занят async-migration, Stage 1.5 ADR сдвинуты на 0012..0015 (ROADMAP3 поправлен в `fe1afed`) |
| 14 | `Changelog.md` обновлён: `1.5.0 — WASM smart contracts (basic)` | Секция `## 1.5.0 — WASM smart contracts (basic)` (этот коммит); промежуточные секции 1.1.1–1.1.8 сохранены как история по-промптовых поставок | ✅ | нет |

Плюс обязательство для Stage 2+: тег `v1.5.0-stage1.5` — **Gate Stage 2/3/4** (мягкая зависимость для Stage 2, жёсткая для Stage 4). Тег поставлен и release pipeline verified — §6 (S1.5-FBP11, 2026-10-20); gate открыт.

---

## 2. What was delivered (S1.5-P01 → S1.5-P18, + S1.5-P12 gap-closure)

| Prompt | Commit | Date | Summary |
|--------|--------|------|---------|
| S1.5-P01 | `3be3973` | 2026-10-15 | ADR-0012: WASM (wasmi) vs RISC-V — Decision: wasmi `=2.0.0` |
| S1.5-P02 | `dcf48c2` | 2026-10-15 | ADR-0013: JSON ABI for MVP (WIT migration placeholder) |
| S1.5-P03 | `d855895` | 2026-10-15 | ADR-0014: gas metering (instruction counting + host-call weights) |
| S1.5-P04 | `e9e6049` + `fe1afed` | 2026-10-15 | ADR-0015: precompiles list; ROADMAP3 ADR renumber (Stage 1.5 = 0012–0015) |
| S1.5-P05 | `f332db6` | 2026-10-15 | core/vm: `VmExecutor` trait + `ExecutionContext`/`ExecutionResult`/`VmError` |
| S1.5-P06 | `7e4a46e` | 2026-10-15 | vm-wasm crate + `WasmiExecutor` + wasmi integration |
| S1.5-P07 | `84d786c` | 2026-10-15 | Account model: EOA + Contract (`code_hash`, `storage_root`, nonce) |
| S1.5-P08 | `03d47ef` | 2026-10-15 | Tx types transfer/create/call + per-variant serialization |
| S1.5-P09 | `e36d401` | 2026-10-16 | Gas metering + `gas_left()` + block gas limit (pre-execution reservations) |
| S1.5-P11 | `ee85331` | 2026-10-16 | Precompiles 0x01..0x07 (bls_aggregate → NotImplemented) |
| S1.5-P10 | `e7ac03b` | 2026-10-16 | Host functions: crypto + storage + calls + context + logs (13) |
| S1.5-P13 | `0be19c8` | 2026-10-16 | Reentrancy guard + limits (code/storage/depth/stack/memory) |
| S1.5-P14 | `9007e91` | 2026-10-16 | Events/logs: Log + Receipt, receipts sub-trie, инвариант #18 |
| S1.5-P15 | `9a5cdfd` | 2026-10-16 | Contract storage в SMT + post-execution block gas, инварианты #15/#19 |
| S1.5-P16 | `3f9c8b5` | 2026-10-17 | Fuzz: 4 targets (host_functions, gas, storage_trie, differential wasmi vs wasmtime) + CI |
| S1.5-P17 | `6136ce4` | 2026-10-18 | Интеграционные тесты: vm_{deploy_call, transfer_to_contract, reentrancy_attack, gas_exhaustion, precompiles_integration} + `tests/common/vm_helpers.rs` |
| S1.5-P18 | `274b6b9` | 2026-10-18 | Регрессия 22 инвариантов (`docs/stage1.5/INVARIANTS_ENFORCED.md`) + threat model (`THREAT_MODEL_STAGE1_5.md`, THREAT_MODEL v3.1) |
| S1.5-P12 | `66c5659` | 2026-10-18 | **Gap-closure (выполнен после P18)**: JSON ABI encode/decode + method_selector + WIT placeholder + `abi_roundtrip.rs` |

Пост-релизные bugfix-промпты (`analytics/bugfixes-stage1.5.md`, 2026-10-19/20):

| FBP | Commit | Date | Summary |
|-----|--------|------|---------|
| S1.5-FBP01 | `b925f71` + `10e9acc` + `eee7b7b` | 2026-10-18/19 | Полный JSON ABI residual: FixedArray len>0, array-of-fixedarray roundtrip, `AbiEntry::selector_from_name` |
| S1.5-FBP02 | `b925f71` + `eee7b7b` | 2026-10-18/19 | method_selector golden vectors literal hex (закрыт в рамках P12 gap-closure) |
| S1.5-FBP03 | `f4dadda` | 2026-10-19 | Selector-based dispatch в WasmiExecutor (ENTRY_POINT удалён, strict MissingExport), `sc::input_read`, ADR-0013/0014 amended |
| S1.5-FBP04 | `f217314` | 2026-10-19 | Constructor execution: `execute_constructor` trait, `CreateTx.gas_limit` + `constructor_params`, FORMAT_VERSION 6, failed-receipt rollback; ADR-0013 amended |
| S1.5-FBP05 | `4dcebad` | 2026-10-19 | Nested value-transfer: on-entry crediting, `value_transfers` audit trail, undo-map; chain A→B→C settles/unapplies; ADR-0013 amended |
| S1.5-FBP06 | `06c3444` | 2026-10-19 | Lazy contract storage: `Arc<RwLock>` shared base + per-frame overlay, O(1) setup per call, pointer-stability КГ-тест |
| S1.5-FBP07 | `c8b93a8` | 2026-10-20 | `BlockHeader.receipts_root` + FORMAT_VERSION 7, validate в `validate_and_apply`, TLA+ `ReceiptsRootMatch` TLC PASS; ADR-0016 |
| S1.5-FBP08 | `768fc99` | 2026-10-20 | Regression: INVARIANTS_ENFORCED re-audit 22/22, THREAT_MODEL residuals V-18/V-19/V-21, `RESIDUAL_RISKS_STAGE2_PLUS.md` |

Замечания:
- **Порядок P10/P11**: фактический git-порядок обратен нумерации (P11 закоммичен раньше P10) — оба зелёные, зависимостей нет.
- **P12 gap**: промпт-спека S1.5-P12 не была выполнена инлайн (в коде оставалась только ABI-заглушка P13). Gap обнаружен при сверке КГ S1.5-P19 и закрыт до тега (`66c5659`); критерий DoD #6 подтверждён тестами.
- S1.5-P16..P18 не имели собственных промежуточных секций Changelog — агрегированы секцией 1.5.0.

---

## 3. Verification runs (2026-10-18)

| Run | Command | Result |
|-----|---------|--------|
| Full workspace suite | `cargo test --workspace` | **green** — все бинарники: core lib 96, abi_roundtrip 14, tx_types 20, account_model 15, contract_storage_smt 8, receipts 12, vm-wasm (host 43, precompiles 36, differential 13, events 11, gas 9, reentrancy+limits 13, contract_storage 6, basic 8), node integration (vm_deploy_call, vm_transfer_to_contract, vm_reentrancy_attack, vm_gas_exhaustion, vm_precompiles_integration, two_clients, network/sync, reorg, rbf, …) |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | **green** (0 warnings) |
| Fuzz (coverage-guided) | cargo-fuzz 4 Stage 1.5 targets | **не верифицировано локально** (Windows host, gnu toolchain, нет cargo-fuzz/dlltool) — CI jobs в `.github/workflows/ci.yml` (S1.5-P16); локальный smoke — `cargo run --example canonical_decode_soak`; первый CI-прогон 4 новых targets — pending |
| Differential (non-fuzz) | `vm-wasm/tests/differential.rs` (wasmtime oracle) | **green** (13) |
| Re-verification (S1.5-FBP08, 2026-10-20) | `cargo test --workspace` + `cargo clippy --all-targets -- -D warnings` + soak example | **green** — после FBP01..FBP07; canonical_decode_soak 402,672 inputs, 0 panics; fuzz CI-side (не верифицировано локально, Windows) |
| Refresh verification (S1.5-FBP09, 2026-10-20) | `cargo test --workspace` + `cargo clippy --all-targets -- -D warnings` | **green** (drift-sync этого документа; код не менялся — regression только документальная сверка) |

Тестовое окружение: Windows, `stable-x86_64-pc-windows-gnu` + `rust-lld` linker (`.cargo/config.toml`), Rust 1.98.1.

---

## 4. Residual obligations for Stage 2+ (зафиксировано S1.5-P19)

| # | Obligation | Stage | Source |
|---|-----------|-------|--------|
| 1 | Block-STM (parallel execution) | Stage 3+ | anti-goal Stage 1.5 (ARCHITECT3 §15) |
| 2 | EIP-1559 dynamic base fee | Stage 5 | ARCHITECT3 §15; сейчас fixed limit + fee=0 stub |
| 3 | Account Abstraction (social recovery, multisig, session keys, paymaster) | Stage 5 | ARCHITECT3 §15 |
| 4 | WIT ABI migration (`format_version: "wit-v2"`, SCIP + activation height) | Stage 4+ | ADR-0013; `"wit-v2"` reserved, `NotImplemented` |
| 5 | BLS12-381 dependency + `bls_aggregate` precompile impl (`0x06`) | Stage 7 | ADR-0015; сейчас NotImplemented |
| 6 | K-framework formal verification (WASM subset) | Stage 6 | ARCHITECT3 §3.9 |
| 7 | ZK-VM integration (RISC Zero / SP1) | Stage 6+ (research) | ARCHITECT3 §15 |
| 8 | **Ops-gate** BUG-S1-013: PGP-key offline keygen + keyservers + Immunefi | mainnet freeze (human ops) | `bugfixes-stage1.md`; doc-side closed, ops остаётся |
| 9 | **Ops-gate** BUG-S1-015: offline genesis key air-gapped keygen → vault → genesis rebuild | mainnet freeze (human ops) | tooling closed (`examples/generate_offline_genesis_key.rs`), ops остаётся |
| 10 | CI-верификация: первый прогон 4 новых fuzz targets + `fuzz-canonical-decode` на GitHub Actions | CI | Stage 1 §6.2 precedent; jobs существуют |
| 11 | ~~Конструктор Create-транзакции не исполняется~~ — **закрыт post-release S1.5-FBP04** (2026-10-19): экспорт `"constructor"` исполняется с `init_args` как calldata; `CreateTx.gas_limit` + `constructor_params` на wire (FORMAT_VERSION 6); failed-receipt rollback. Residual: constructor fee burning (Stage 5), ABI-driven selector (Stage 4 SDK) | ~~Stage 4+ (SDK)~~ done (S1.5-FBP04) | Changelog 1.5.2; ADR-0013 amended |
| 12 | ~~Nested value-трансферы в `apply_call` не коммитятся (P14 MVP residual)~~ — **закрыт post-release S1.5-FBP05** (2026-10-19): on-entry crediting в `run_frame`/`nested_call` (sender = `storage_address`), `value_transfers` audit trail в Receipt, undo-map для chained rollbacks, chain A→B→C settles/unapplies. Residual: нет | ~~Stage 4+ (SDK)~~ done (S1.5-FBP05) | Changelog 1.5.3; ADR-0013 amended |
| 13 | ~~Receipts fold в header / light-client receipts~~ — **закрыт post-release S1.5-FBP07** (2026-10-20): `BlockHeader.receipts_root` + FORMAT_VERSION 7 (HEADER_WIRE_LEN 221), zero-tolerance validate, TLA+ `ReceiptsRootMatch` TLC PASS. Residual: receipts prune (только SCIP) — `RESIDUAL_RISKS_STAGE2_PLUS.md` §2 | ~~Stage 2+~~ done (S1.5-FBP07) | Changelog 1.5.5; ADR-0016 |
| 14 | True Verkle/KZG state trie | Stage 3+ | ADR-0006 amended (BUG-S0-011); Stage 1.5 — SMT extension |
| 15 | TLA+ свойства VM/consensus | Stage 6 | Stage 1 residual §6.3 (bounded model PASS) |

Ops-gates (№8, №9) **не блокируют Stage 1.5** (regtest/devnet scope); становятся обязательными только для mainnet freeze.

Смежные residuals (constructor gas burning Stage 5, receipts/contract storage prune Stage 3+ через SCIP) — `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` (S1.5-FBP08).

### 4.1 Anti-goals Stage 1.5 (prompt-stage1.5.md §7 — подтверждено нереализованными при refresh S1.5-FBP09, 2026-10-20)

| Anti-goal | Stage | Статус на refresh |
|-----------|-------|-------------------|
| Noise Protocol, Erlay, compact blocks, gossip-оптимизации | Stage 2 | не реализовано (network crate — Stage 2) |
| RocksDB миграция | Stage 3 | не реализовано (LevelDB остаётся) |
| Block-STM (parallel execution) | Stage 3+ | не реализовано (sequential apply) |
| State rent (опционально) | Stage 3+ | не реализовано (только per-contract cap 1MB) |
| SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet | Stage 4 | не реализовано |
| WIT/Wasm Component Model ABI | Stage 4+ | placeholder `"wit-v2"` → `NotImplemented` |
| Account Abstraction (social recovery, multisig, session keys, paymaster) | Stage 5 | не реализовано |
| EIP-1559 dynamic base fee | Stage 5 | не реализовано (fixed limit + fee=0 stub) |
| EncryptedMempool (threshold encryption) | Stage 5 | не реализовано |
| Multidimensional fees (EIP-7706 analog) | Stage 6+ (research) | не реализовано |
| K-framework формальная верификация WASM subset | Stage 6 | не реализовано |
| ZK-VM integration (RISC Zero / SP1) | Stage 6+ (research) | не реализовано |
| External audit / formal verification (security track) | Stage 6 | не реализовано (только fuzzing harness) |
| TLA+ формальная верификация VM (свойства) | Stage 6 | не реализовано (только consensus.tla + ReceiptsRootMatch) |
| BLS12-381 dependency + `bls_aggregate` impl | Stage 7 | NotImplemented precompile `0x06` |
| PoS миграция, Casper FFG, BLS consensus signatures | Stage 7 | не реализовано (PoW остаётся) |

---

## 5. Process notes

1. **ARCHITECT3 §17 checklist** — прогнан в рамках self-review этого промпта: применимые пункты отмечены («VM trait в core, runtime отдельно (wasmi)» — `core/vm/traits.rs` + `crates/strangecoin-vm-wasm`, 0 I/O в core сохранён).
2. **Детерминизм host functions** (V-21): все 14 host functions (13 P10 + `input_read` FBP03) — без time/random/network I/O; ревью закреплено в THREAT_MODEL_STAGE1_5 §1 (V-21 Mitigated) + fuzz-таргет `host_functions`.
3. **Wasmi pinning**: `wasmi = "=2.0.0"` (не `^`); обновление — только через SCIP после mainnet freeze (ADR-0012).
4. **Gas constants**: hardcoded по ADR-0014/0015; до mainnet freeze правки — прямым коммитом с обновлением ADR; после — SCIP + activation height.
5. **«Done» только через этот DoD-промпт** (retro §8.1): тег `v1.5.0-stage1.5` поставлен после прохождения КГ S1.5-P19; Stage 2/3/4 gate открыт.
6. **ECONOMICS.md §2.6** (Stage 1.5+): завершение Stage 1.5 открывает канал «разработка контрактов под ключ» / SDK-интеграции — не код-задача, зафиксировано как экономическое основание.

---

## 6. Release (S1.5-FBP11, verified 2026-10-20)

- **Tag**: annotated `v1.5.0-stage1.5` → commit `1f133c7e1910ac870050d6fc0da0b37d3b5338df` (tagger Reider85; pushed to origin, verified via `git ls-remote --tags origin`).
- **Release**: https://github.com/Reider85/strangecoin/releases/tag/v1.5.0-stage1.5 — published 2026-10-18, 26 assets (6 binaries, 6 `.sha256`, 6 `.cosign`, 6 `.cosign.pem`, `SHA256SUMS.txt`, `multiple.intoto.jsonl` SLSA L3).
- **Workflow run**: https://github.com/Reider85/strangecoin/actions/runs/39110855701 — **success, 13/13 jobs** (6 matrix builds + `aggregate-hashes` + `sign` + `SLSA Provenance` ×4 + `Create Release`). Verified via GitHub REST API 2026-10-20 (`gh` CLI not installed on host; REST used instead).
- **SHA256**: 6 published checksums зафиксированы в `docs/security/REPRODUCIBLE_BUILDS.md` §Verified Release Runs (v1.5.0-stage1.5).
- **Independent cosign/SLSA blob verification**: не верифицировано локально + причина — `cosign`/`slsa-verifier`/`gh` не установлены на Windows-хосте; pipeline-level jobs (`sign`, `SLSA Provenance`) green в том же ране. Verify commands (procedure BUG-S1-001):
  ```bash
  cosign verify-blob --signature strangecoin-windows-x86_64.exe.cosign \
    --certificate strangecoin-windows-x86_64.exe.cosign.pem \
    --certificate-identity "https://github.com/Reider85/strangecoin/.github/workflows/release.yml@refs/tags/v1.5.0-stage1.5" \
    --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
    strangecoin-windows-x86_64.exe

  slsa-verifier verify-artifact strangecoin-windows-x86_64.exe \
    --provenance-path multiple.intoto.jsonl \
    --source-uri github.com/Reider85/strangecoin \
    --source-tag v1.5.0-stage1.5
  ```
