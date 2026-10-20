# bugfixes-prompts2.md — Промпты для закрытия прорех Stage 1.5

**Версия:** 1.2
**Дата:** 2026-10-20
**Источник:** аудит `analytics/prompt-stage1.5.md` (S1.5-P01..P19), `Changelog.md` (секции 1.1.1–1.1.8), `docs/stage1.5/INVARIANTS_ENFORCED.md`, `docs/stage1.5/THREAT_MODEL_STAGE1_5.md`, `crates/strangecoin-core/src/abi/mod.rs`, `crates/strangecoin-vm-wasm/src/lib.rs`, `crates/strangecoin-core/src/state/inner.rs`, `git tag -l`, `Cargo.toml`.
**Предусловие:** 17 из 19 промптов `prompt-stage1.5.md` выполнены (S1.5-P01..P11, P13..P18). Не выполнены: **S1.5-P12** (полный JSON ABI) и **S1.5-P19** (DoD-верификация + тег `v1.5.0-stage1.5`).
**Цель:** превратить остаточные расхождения Stage 1.5 в измеримый, трассируемый список промптов со сквозной нумерацией. Каждый промпт — самостоятельное задание для ИИ-агента, завершающееся конкретными артефактами и чек-листом КГ.

---

## Статус каталога (v1.2, обновлено 2026-10-20)

Каталог писался на коммите `274b6b9` (S1.5-P18), когда S1.5-P12/P19 ещё не
были выполнены. С тех пор оба gap-closure промпта закрыты **в рамках
`prompt-stage1.5.md`** (не FBP-коммитами). Актуальный статус:

| FBP | Статус | Evidence |
|-----|--------|----------|
| S1.5-FBP01 | **fixed** (2026-10-19) | S1.5-P12 gap-closure `b925f71` (2026-10-18) + residuals `10e9acc` (FixedArray len>0, array-of-fixedarray roundtrip); verification 2026-10-19: `cargo check/test/clippy -p strangecoin-core` green, `abi_roundtrip` 15/15, abi unit 7/7 |
| S1.5-FBP02 | **fixed** (2026-10-19) | S1.5-P12 `b925f71` (selector.rs, golden vectors literal hex) + residual `eee7b7b` (`AbiEntry::selector_from_name`) |
| S1.5-FBP03 | **fixed** (2026-10-19) | selector dispatch в `vm-wasm/src/lib.rs` (ENTRY_POINT удалён, invoke-export try-порядок, строгий MissingExport), `sc::input_read` host function, ADR-0013/0014 amended; `basic_execution` 16/16, workspace green, clippy green (`f4dadda`) |
| S1.5-FBP04 | **fixed** (2026-10-19) | конструктор исполняется: `execute_constructor` trait + `FrameEntry::Constructor` (skip до инстанцирования), `apply_create` rework (Success flush + endowment; failure = failed receipt + rollback), `unapply_create` receipt-gated; `CreateTx.gas_limit`+`constructor_params`, FORMAT_VERSION 6, genesis hashes + golden vectors перегенерированы (BUG-S1-007); ADR-0013 amended; Changelog 1.5.2; тесты vm-wasm contract_storage 10/10, vm_deploy_call 2/2, serialize_golden 19/19 (`f217314`) |
| S1.5-FBP05 | **fixed** (2026-10-19) | nested value-transfer селтится: on-entry crediting в `run_frame` + `nested_call` (sender = `storage_address`), `ExecutionResult/Receipt.value_transfers` audit trail, `State.value_transfers_undo` (accumulating), `commit/reverse_value_transfers` (sequential, self-call mint-bug fixed); ADR-0013 amended; Changelog 1.5.3; `vm_transfer_to_contract` 3/3 (+ chain A→B→C, failed chain), `contract_storage` 11/11, workspace + clippy green |
| S1.5-FBP06 | **fixed** (2026-10-19) | `SharedContractStorage = Arc<RwLock<ContractStorageMap>>` в `State`/`ExecutionContext` (Arc вместо Rc — Send/Sync сохранён), per-call `Arc::clone` O(1) вместо deep-clone; overlay unchanged; flush/undo in place через `write()`; ручной глубокий `State::clone` + deep-copy на границе `StateCache` (инвариант #4); ADR-0013 amended; Changelog 1.5.4; КГ-тест pointer-stability (100 CallTx × 10K keys); workspace + clippy green (`06c3444`) |
| S1.5-FBP07 | **fixed** (2026-10-20) | `BlockHeader.receipts_root` + FORMAT_VERSION 7 (HEADER_WIRE_LEN 221), validate в `validate_and_apply` (zero tolerance — genesis only), set в mine/grant; ADR-0016; genesis hashes + golden vectors перегенерированы (BUG-S1-007); TLA+ `ReceiptsRootMatch` TLC PASS 35,207 states 10/10; Changelog 1.5.5; `tests/receipts.rs` light-client proof + `tests/state_root.rs` tamper + `tests/block_executor.rs` zero/mismatch; workspace + clippy green |
| S1.5-FBP08 | **fixed** (2026-10-20) | `docs/stage1.5/INVARIANTS_ENFORCED.md` re-audit 22/22 enforced (extensions #4/#5/#8/#11/#15/#18/#19, #20 degenerate), `THREAT_MODEL_STAGE1_5.md` §2 residuals V-18/V-19/V-21 (порядок гейтов до value transfer verified `mod.rs:118-193`; dispatch miss→main + strict MissingExport `lib.rs:275-414`; constructor gas `Create.gas_limit` `block_executor.rs:278-285`), новый `RESIDUAL_RISKS_STAGE2_PLUS.md` (constructor gas burning Stage 5, receipts/contract prune Stage 3+ SCIP); workspace + clippy + soak (402,672 inputs, 0 panics) green; fuzz 5×600s — CI-side, локально не верифицировано + причина (Windows) (`768fc99`) |
| S1.5-FBP09 | **fixed** (2026-10-18; refresh 2026-10-20) | S1.5-P19 `1f133c7` — `docs/stage1.5/STAGE1_5_SUMMARY.md` существует, 14 DoD ✅; drift-sync `100a388` (2026-10-20): DoD #2 14 host funcs, #4 wire v6/v7, #6 финальный ABI (FBP01/02), #7 strict MissingExport, #10 Arc storage (FBP06), #12 22/22 (FBP08), §2 FBP01..08, §4 residuals #12/#13 → done (FBP05/FBP07), §4.1 anti-goals 16/16 подтверждены нереализованными; workspace + clippy green |
| S1.5-FBP10 | **fixed** (2026-10-20) | residual закрыт `3826a44` (2026-10-20): `strangecoin-core` 1.1.0→1.5.0, `strangecoin-vm-wasm` 0.1.0→0.2.0, `Cargo.lock` + `fuzz/Cargo.lock` синхронизированы; root 1.5.0/AGENTS.md/Changelog 1.5.0 — ранее `1f133c7`; verification 2026-10-20: `cargo check/test/clippy --workspace` green (core v1.5.0, vm-wasm v0.2.0) |
| S1.5-FBP11 | **fixed** (2026-10-18; residual closed 2026-10-20) | annotated tag `v1.5.0-stage1.5` создан и pushed на origin (`1f133c7`); residual закрыт 2026-10-20: release-run verified через GitHub REST API (run 39110855701, success 13/13 jobs, 26 assets, SHA256SUMS сверен) — Actions run URL + SHA256 в `REPRODUCIBLE_BUILDS.md` §Verified Release Runs, Release section в `STAGE1_5_SUMMARY.md` §6, Changelog 1.5.6; cosign/slsa local blob verify — не верифицировано + причина (tools не установлены на Windows-хосте, verify commands зафиксированы) |

Оставшийся proper work: **нет** — все 11 промптов закрыты
(FBP08 — 2026-10-20 `768fc99`; FBP10 — 2026-10-20 `3826a44`; FBP11
residual — 2026-10-20). Тег-гейт Stage 2/3/4 открыт (tag pushed +
release pipeline verified, run 39110855701).

Замечания по хосту (Windows): gcc для `secp256k1-sys` — `C:\msys64\mingw64\bin`
(добавлять в PATH для cargo-команд); `gh` CLI не установлен.

---

## 0. Общий контекст (вставлять в начало каждого промпта)

```
Проект: Strangecoin, Rust, edition 2021, версия 1.1.0 (последний коммит HEAD
274b6b9 — S1.5-P18, regression 22 invariants).
Структура (после Stage 1.5 proper):
  Монолит: src/main.rs + src/{lib,consensus,economics,mempool,network,
    blockchain,storage,config,cli,wallet,address,error,events,api,gui}.rs +
    подпапки blockchain/{mod,block_executor,chain_selector,state_cache,
    blockchain_facade,consensus_manager}.rs, network/{mod,protocol,rate_limiter,
    sync,sync_engine}.rs, mempool/mod.rs, consensus/mod.rs, economics/mod.rs,
    governance/mod.rs, api/mod.rs, gui/mod.rs, storage/mod.rs, cli/mod.rs.
  Core-крейт: crates/strangecoin-core/src/{lib,types,serialize,consensus,
    chain_selector,address,error,abi/mod}.rs + state/{mod,inner,sparse_merkle,
    witness,contract_storage}.rs + vm/{mod,traits,context,result,error,gas,
    limits}.rs + economics/{mod,emission,fee_market}.rs + governance/scip.rs.
  VM-крейт: crates/strangecoin-vm-wasm/src/{lib,error,gas}.rs + host/{mod,abi,
    state}.rs + host/crypto/{mod,secp256k1_verify,bls_verify,blake3,keccak,
    sha256}.rs + host/storage/{mod,storage_read,storage_write}.rs + host/calls/
    {mod,call,delegatecall,staticcall}.rs + host/context/{mod,block_context,
    gas_left}.rs + host/logs/{mod,emit_log}.rs + precompiles/{mod,ecrecover,
    sha256,keccak,blake3,ed25519_verify,bls_aggregate,modexp}.rs.
  Workspace: Cargo.toml с [workspace] members = [".", "crates/strangecoin-core",
    "crates/strangecoin-vm-wasm"].
  Тесты: tests/ (34 файла, включая vm_deploy_call, vm_transfer_to_contract,
    vm_reentrancy_attack, vm_gas_exhaustion, vm_precompiles_integration,
    block_executor, receipts, state_root, sync_engine, rate_limit, two_clients);
    crates/strangecoin-core/tests/ (12 файлов: account_model, contract_storage_smt,
    receipts, tx_types, vm_trait_mock, state_roundtrip, state_root, witness,
    serialize_golden, merkle, u256, consensus_proptest);
    crates/strangecoin-vm-wasm/tests/ (8 файлов: basic_execution, contract_storage,
    differential, events_and_receipts, gas_metering, host_functions, precompiles,
    reentrancy_and_limits).
  Fuzz: fuzz/fuzz_targets/{canonical_decode, host_functions, gas_metering,
    storage_trie, differential_wasmi_vs_wasmtime}.rs — 5 таргетов, 600s каждый
    в CI (matrix: ubuntu-latest, cargo-fuzz 0.13.2).
Теги: v0.0.1-rc1, v1.0.0-stage0, v1.1.0-stage1. Тега v1.5.0-stage1.5 — нет.
Версия Cargo.toml: strangecoin=1.1.0, strangecoin-core=1.1.0,
  strangecoin-vm-wasm=0.1.0. Ожидается после S1.5-P19: strangecoin=1.5.0,
  strangecoin-core=1.5.0, strangecoin-vm-wasm=0.2.0.
Стейдж-документация: docs/stage1.5/{INVARIANTS_ENFORCED.md, THREAT_MODEL_
  STAGE1_5.md} — оба созданы в S1.5-P18. STAGE1_5_SUMMARY.md — отсутствует
  (S1.5-P19 не выполнен).
Bug-трекинг: analytics/bugfixes-stage0.md (35 багов, все закрыты),
  analytics/bugfixes-stage1.md (16 багов BUG-S1-001..BUG-S1-016: 14 кодом,
  2 ops-gate).
Известные прорехи (по Changelog секций 1.1.5–1.1.8):
  - S1.5-P12 (полный JSON ABI encode/decode + method_selector + wit-v2
    migration path) — не выполнен. Только stub abi/mod.rs (56 строк) с
    AbiEntry{name, selector, non_reentrant} + ContractAbi{format_version,
    entries}. AbiType/encode_args/decode_args/method_selector отсутствуют.
  - S1.5-P19 (DoD-верификация + тег v1.5.0-stage1.5) — не выполнен.
    docs/stage1.5/STAGE1_5_SUMMARY.md отсутствует. Cargo.toml версия = 1.1.0
    (ожидается 1.5.0). Git tag v1.5.0-stage1.5 не поставлен.
  - Конструктор Create не исполняется — only code_hash + register_code,
    init_args игнорируются (state/inner.rs:322-323). MVP-обязательство
    S1.5-P17+, не закрыто.
  - Nested value-трансферы не коммитятся в apply_call — value переводится
    только при Success верхнего frame, но не для nested call→call. MVP-
    обязательство S1.5-P17+, не закрыто.
  - contract_storage клонируется целиком на каждый CallTx — O(размер
    хранилища) на каждый call. Mainnet-путь — lazy state access через host
    functions (Stage 3+), не закрыто.
  - receipts_root не сложен в глобальный state_root — receipts хранятся
    как derived data sub-trie с proofs, но light-client receipts требуют
    fold в header (Stage 2+), не закрыто.
  - Constructor fee burning не реализован — gas = cap, fees = 0. Stage 5
    fee market.
  - Constructor execution residue: при Create конструктор не исполняется,
    что противоречит спеке ARCHITECT3 §3.9 (CreateTx должен init контракт
    через init_code + init_args).
Принципы:
  - 1 промпт = 1 атомарный коммит, ID промпта в сообщении, формат:
    «[S1.5-FBPXX] <area>: <short description>».
  - ADR пишется ДО того, как решение затронет код (если промпт затрагивает
    архитектуру — ADR обязателен).
  - Любое изменение консенсусных правил — через SCIP + activation height.
    На Stage 1.5 mainnet не запущен, цепочка regtest/devnet — прямые правки
    допустимы до тега v1.5.0-stage1.5, но каждая правка фиксируется в
    Changelog.
  - Валидация на входе (размеры, подписи, время, генезис, replay, rate,
    gas) не ослабляется НИ одним промптом (DoD: «All Stage 0/1/1.5
    invariants still enforced — no regressions»).
  - Determinism: host functions не должны утекать недетерминизм (время,
    random, I/O); каждая host function ревьюится на детерминизм (threat
    V-21).
  - Strangler pattern: core остаётся чистым (0 I/O); VM runtime живёт в
    отдельном крейте и импортируется через feature flag.
  - После каждого промпта: cargo check и cargo test --workspace обязаны
    проходить. clippy -D warnings — green.
  - Логирование — только через tracing (structured fields); println!
    запрещён.
  - Секреты — только в keystore/env; в config не писать.
  - Запрещено обновлять Changelog/README/AGENTS.md о работе, для которой
    ещё нет коммита с файлами (урок retro §8.1).
Сокращения: КГ — критерии готовности.
```

---

## 1. Карта зависимостей промптов

> **Обновлено 2026-10-19:** ABI-трек (FBP01→FBP02) и Verification-трек
> (FBP09→FBP10→FBP11) закрыты S1.5-P12/P19 (см. «Статус каталога» вверху;
> FBP10/FBP11 — частично, residuals зафиксированы). Тег-гейт Stage 2/3/4
> уже открыт. Остатся proper work FBP03→FBP04→FBP05→FBP06→FBP07 и
> regression FBP08. Карта ниже приведена в исходном виде (историческая).

```
PROPER (старт только после подтверждения коммита 274b6b9 — S1.5-P18):

S1.5-FBP01 (Полный JSON ABI: AbiType + encode/decode) ──▶ S1.5-FBP02 (method_selector
    blake3-based 4-byte + WIT placeholder wit-v2)
                                                              │
                                                              ▼
                                                        S1.5-FBP03 (Selector-based
                                                        dispatch в WasmiExecutor
                                                        вместо hardcoded ENTRY_POINT="main")
                                                              │
                                                              ▼
S1.5-FBP03 ──▶ S1.5-FBP04 (Конструктор Create: исполнение init_code с init_args)
                        │
                        ▼
                S1.5-FBP05 (Nested value-transfer: коммит value в apply_call
                            для chain call→call)
                        │
                        ▼
                S1.5-FBP06 (Lazy contract storage access: убрать клон
                            contract_storage на каждый CallTx; mainnet-путь)
                        │
                        ▼
                S1.5-FBP07 (Receipts fold в глобальный state_root: light-client
                            receipts требуют fold в header)
                        │
                        ▼
S1.5-FBP01..S1.5-FBP07 ──▶ S1.5-FBP08 (Regression: 22 invariants + new threats
                            для ABI/конструктора/nested value)
                        │
                        ▼
                S1.5-FBP09 (STAGE1_5_SUMMARY.md: 14 DoD критериев с ✅ + evidence)
                        │
                        ▼
                S1.5-FBP10 (Версия Cargo.toml → 1.5.0 + AGENTS.md update + Changelog
                            секция 1.5.0)
                        │
                        ▼
                S1.5-FBP11 (Тег v1.5.0-stage1.5 — строго после прохождения всех КГ
                            предыдущих промптов)
```

Параллельные треки:
- **ABI-трек:** S1.5-FBP01 → S1.5-FBP02 → S1.5-FBP03 — последовательный, ABI нужен
  до selector-based dispatch.
- **State-трек:** S1.5-FBP04 (конструктор) → S1.5-FBP05 (nested value) → S1.5-FBP06
  (lazy storage) → S1.5-FBP07 (receipts fold) — последовательный, каждое требует
  предыдущего в apply_call.
- **Verification-трек:** S1.5-FBP08 (regression) → S1.5-FBP09 (summary) → S1.5-FBP10
  (versions) → S1.5-FBP11 (tag) — строго после всех proper промптов.

Жёсткие гейты:
1. **ABI gate:** S1.5-FBP03 (selector dispatch) не начинается без S1.5-FBP02
   (method_selector).
2. **Constructor gate:** S1.5-FBP04 (constructor execution) требует S1.5-FBP03
   (selector dispatch — конструктор — это тоже функция с selector).
3. **State gate:** S1.5-FBP05..FBP07 требуют S1.5-FBP04 (конструктор может
   писать в storage).
4. **Tag gate:** S1.5-FBP11 (тег v1.5.0-stage1.5) — последний, строго после всех
   КГ предыдущих промптов. Без него Stage 2/3/4 gate не пройден.

---

## 2. Список промптов

### S1.5-FBP01. Полный JSON ABI: AbiType + encode_args/decode_args

**Статус:** fixed (2026-10-19)

**Решение (2026-10-19):** промпт закрыт коммитом S1.5-P12 gap-closure
`b925f71` (2026-10-18) — `core/abi/{mod,encode,decode,selector}.rs` +
`tests/abi_roundtrip.rs`: `AbiDocument`/`AbiType` (9 variants,
`validate_bits` 8..=256), head/tail codec (32-byte words, 4-byte BE
offsets/lengths, canonical-offset enforcement), `AbiError` (богаче спеки:
`UnexpectedEnd`/`TrailingData`/`NonCanonicalOffset` вместо `InvalidLength`),
`AbiValue`. Residual-расхождения с КГ закрыты `10e9acc` (2026-10-19):
`AbiType::parse` отклоняет `T[0]` (FixedArray len > 0, task 2 спеки),
round-trip `Array(FixedArray(Uint8,2))` + negative `T[0]` в
`abi_roundtrip.rs` (15/15 green, abi unit 7/7, clippy `-D warnings` green).
Отклонения от первоначальной спеки (осознанные, задокументированы в
`abi/mod.rs`): decl-error назван `AbiErrorDecl` (имя `AbiError` занято
codec-ошибкой), `AbiEventParam.indexed` per-param вместо `indexed: Vec<bool>`
в `AbiEvent`, serde `Deserialize` вместо `AbiDocument::parse(raw: &str)`
(parsing остаётся tooling-side), `NonCanonicalOffset` строже спеки
(только канонический layout декодируется).

**Цель:** закрыть первую половину S1.5-P12 — реализовать `AbiType` enum (≥9
variants), `AbiParam`, `AbiDocument`, `AbiValue`, и канонический
`encode_args`/`decode_args` (head/tail split, как Ethereum ABI). Stub
`crates/strangecoin-core/src/abi/mod.rs` (56 строк, только `AbiEntry` +
`ContractAbi`) раскрыть до полного API.

**Контекст:** Текущий stub достаточен для reentrancy guard (S1.5-P13) —
`AbiEntry { name, selector, non_reentrant }` + `ContractAbi { format_version,
entries }`. Но нет типизированного интерфейса: контракты не могут
декларировать `function transfer(address to, uint256 amount)`, SDK не может
кодировать вызовы, explorer не может показать decoded calldata. ADR-0013
(S1.5-P02) зафиксировал JSON ABI (Ethereum-style) для MVP с `format_version:
"json-v1"` обязательным полем. Этот промпт — encode/decode primitives;
selector и dispatch — S1.5-FBP02/FBP03.

**Задачи:**

1. `crates/strangecoin-core/src/abi/mod.rs` — раскрыть с сохранением
   существующего `AbiEntry`/`ContractAbi`:
   - `AbiDocument { format_version: String, name: String, functions:
     Vec<AbiFunction>, events: Vec<AbiEvent>, errors: Vec<AbiError> }`
   - `AbiFunction { name: String, inputs: Vec<AbiParam>, outputs:
     Vec<AbiParam>, state_mutability: StateMutability }`
   - `AbiParam { name: String, type: AbiType }`
   - `AbiEvent { name, inputs: Vec<AbiParam>, indexed: Vec<bool> }`
   - `AbiError { name, inputs: Vec<AbiParam> }`
   - `StateMutability { Pure, View, NonPayable, Payable }`
2. `AbiType` enum — ≥9 variants: `Uint(bits: u16)`, `Int(bits: u16)`, `Bool`,
   `Address`, `Bytes`, `String`, `Array(Box<AbiType>)`, `FixedArray(Box<AbiType>,
   usize)`, `Tuple(Vec<AbiType>)`. Валидация: `bits % 8 == 0 && bits <= 256` для
   Uint/Int; `len > 0` для FixedArray.
3. `crates/strangecoin-core/src/abi/encode.rs` — `encode_args(params:
   &[AbiParam], values: &[AbiValue]) -> Result<Vec<u8>, AbiError>`:
   - Static types: fixed 32-byte encoding (Uint → big-endian left-padded, Bool →
     32 bytes 0/1, Address → right-padded 32 bytes)
   - Dynamic types: length-prefixed (4-byte length + data)
   - Head/tail split: static types в head, dynamic types — offset в head +
     data в tail (как Ethereum ABI)
4. `crates/strangecoin-core/src/abi/decode.rs` — `decode_args(params:
   &[AbiParam], data: &[u8]) -> Result<Vec<AbiValue>, AbiError>` — обратное
   к encode, с валидацией (offset в bounds, length в bounds, тип matches).
5. `AbiValue` enum — зеркальный к `AbiType` для runtime values.
6. `AbiError` enum — `InvalidType { reason: String }`, `InvalidLength {
   expected: usize, actual: usize }`, `InvalidOffset`, `UnsupportedFormatVersion
   { version: String }`, `NotImplemented { feature: String }`.
7. Тесты: `crates/strangecoin-core/tests/abi_roundtrip.rs` — round-trip для
   каждого AbiType; negative tests (short data, bad offset, bad length, bits
   not multiple of 8); head/tail split корректен для mixed static/dynamic.

**Артефакты:**
- `crates/strangecoin-core/src/abi/mod.rs` (раскрытый)
- `crates/strangecoin-core/src/abi/encode.rs`
- `crates/strangecoin-core/src/abi/decode.rs`
- `crates/strangecoin-core/src/abi/error.rs` (AbiError)
- `crates/strangecoin-core/tests/abi_roundtrip.rs`

**КГ (чек-лист):**
- [ ] `AbiDocument` с обязательным `format_version: "json-v1"`
- [ ] `AbiType` enum с ≥9 variants + валидация (bits % 8 == 0 && bits <= 256)
- [ ] `AbiParam`, `AbiFunction`, `AbiEvent`, `AbiError`, `StateMutability`
- [ ] `encode_args`/`decode_args` round-trip для каждого типа (включая nested
      Tuple, Array of FixedArray, mixed static/dynamic)
- [ ] Head/tail split корректен (offset = position of tail section)
- [ ] Negative tests: short data, bad offset, bad length — все возвращают
      `AbiError`
- [ ] Существующий `AbiEntry`/`ContractAbi` сохранён (no regression для
      reentrancy guard из S1.5-P13)
- [ ] `cargo check -p strangecoin-core` green
- [ ] `cargo test -p strangecoin-core --test abi_roundtrip` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S1.5-FBP01] core/abi: AbiType + encode_args/decode_args (head/tail split)`

---

### S1.5-FBP02. method_selector blake3-based 4-byte + WIT placeholder wit-v2

**Статус:** fixed (2026-10-19)

**Решение (2026-10-19):** закрыт S1.5-P12 gap-closure `b925f71`:
`core/abi/selector.rs` — `method_selector` (blake3, первые 4 байта),
`method_signature`, golden vectors literal hex в `abi_roundtrip.rs`
(`transfer(address,uint256)` → `[0xce,0xae,0xeb,0xc1]` и др., pattern
BUG-S1-007); `AbiType::canonical_name` для всех variants; format-version
gating: `"json-v1"` ok, `"wit-v2"` → `NotImplemented`, иначе →
`UnknownFormatVersion`. Residual-КГ `AbiEntry::selector_from_name` закрыт
`eee7b7b` (2026-10-19, golden vector `totalSupply()`). Отклонение:
`AbiDocument::parse(raw: &str)` не реализован как метод — parsing через
serde `Deserialize` + `check_format_version()` (ADR-0013: calldata-байты в
core, JSON parsing tooling-side).

**Цель:** закрыть вторую половину S1.5-P12 — реализовать `method_selector(name,
params) -> [u8; 4]` (первые 4 байта `blake3("function_name(types)")`, не keccak
как в Ethereum), и `format_version` migration path (`"json-v1"` accepted,
`"wit-v2"` reserved → `NotImplemented`).

**Контекст:** ADR-0013 (S1.5-P02) зафиксировал blake3-based selector (не
keccak) — StrangeCoin использует blake3 как canonical hash для всего остального.
Stub `abi/mod.rs` уже имеет `AbiEntry.selector: [u8; 4]`, но без функции для его
вычисления — caller обязан сам вычислять и передавать. WIT migration на Stage 4+
через `format_version: "wit-v2"` — ADR-0013 обязывает placeholder (вызов с
`wit-v2` → `AbiError::NotImplemented { feature: "wit-v2" }`).

**Задачи:**

1. `crates/strangecoin-core/src/abi/selector.rs`:
   ```rust
   pub fn method_selector(name: &str, params: &[AbiParam]) -> [u8; 4] {
       let signature = format!("{}({})", name, params.iter()
           .map(|p| p.type.canonical_name())
           .collect::<Vec<_>>().join(","));
       let hash = blake3::hash(signature.as_bytes());
       let mut out = [0u8; 4];
       out.copy_from_slice(&hash.as_bytes()[..4]);
       out
   }
   ```
2. `AbiType::canonical_name(&self) -> String` — `Uint(256)` → `"uint256"`,
   `Int(128)` → `"int128"`, `Bool` → `"bool"`, `Address` → `"address"`, `Bytes`
   → `"bytes"`, `String` → `"string"`, `Array(T)` → `"T[]"`, `FixedArray(T, n)`
   → `"T[n]"`, `Tuple(ts)` → `"(t1,t2,...)"`.
3. `format_version` validation в `AbiDocument::parse(raw: &str) ->
   Result<AbiDocument, AbiError>`:
   - `"json-v1"` → ok
   - `"wit-v2"` → `AbiError::NotImplemented { feature: "wit-v2 ABI migration
     (Stage 4+)" }`
   - любое другое → `AbiError::UnsupportedFormatVersion`
4. `AbiEntry::selector_from_name(name: &str) -> [u8; 4]` — convenience для
   S1.5-P13 reentrancy guard (существующий stub selector field теперь
   вычисляется через `method_selector(name, &[])` — для функций без params
   selector всё равно валиден).
5. Golden vector: `method_selector("transfer", &[AbiParam{type:
   AbiType::Address}, AbiParam{type: AbiType::Uint(256)}])` — зафиксировать
   ожидаемые 4 байта в `tests/abi_roundtrip.rs` (literal hex, не computed) —
   pattern BUG-S1-007 (literal golden vectors).
6. Тесты: `tests/abi_roundtrip.rs` — `method_selector` stable (golden vector),
   canonical_name для каждого AbiType, `wit-v2` → NotImplemented, unknown
   format → UnsupportedFormatVersion.

**Артефакты:**
- `crates/strangecoin-core/src/abi/selector.rs`
- `crates/strangecoin-core/src/abi/mod.rs` (добавить `canonical_name` к AbiType,
  `parse` к AbiDocument)
- `crates/strangecoin-core/tests/abi_roundtrip.rs` (extend: golden vector +
  format_version tests)

**КГ (чек-лист):**
- [ ] `method_selector` — blake3-based, первые 4 байта, stable golden vector
- [ ] `AbiType::canonical_name` для всех 9+ variants
- [ ] `AbiDocument::parse` принимает `"json-v1"`, отклоняет `"wit-v2"` с
      `NotImplemented`, отклоняет unknown с `UnsupportedFormatVersion`
- [ ] Golden vector — literal hex (не computed at runtime)
- [ ] `AbiEntry::selector_from_name` для обратной совместимости с S1.5-P13
- [ ] `cargo test -p strangecoin-core --test abi_roundtrip` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S1.5-FBP02] core/abi: method_selector (blake3 4-byte) + wit-v2 placeholder`

---

### S1.5-FBP03. Selector-based dispatch в WasmiExecutor (убрать hardcoded ENTRY_POINT="main")

**Статус:** fixed (2026-10-19)

**Решение (2026-10-19):** `ENTRY_POINT` удалён; `run_frame` резолвит
dispatch-таргет через `resolve_dispatch_entry` (calldata ≥ 4 →
`entry_by_selector` в зарегистрированном ABI) и вызывает экспорт через
`invoke_export` с try-порядком сигнатур `(i32,i32)->i32`,
`(i32,i32)->()`, `()->i32`, `()->()` (для `()->`-стиля calldata доступен
через новый host function `sc::input_read`, gas 3 —
`HostCallWeight::InputRead`). Строгое MissingExport для ABI-hit без
экспорта (контракт-баг, не fallback); TypeMismatch для неподдерживаемой
сигнатуры. `stage_calldata` копирует `ctx.input` в хвост linear memory
(рост при необходимости, unmetered — calldata ограничен размером tx).
Reentrancy WAT поправлены на dual-export `main`/`guarded`
(`reentrancy_and_limits.rs`, `tests/vm_reentrancy_attack.rs`) — nested
вызов с селектором резолвится строго. ADR-0013 amended (calling
convention), ADR-0014 amended (`HOST_CALL_INPUT_READ = 3`), Changelog
1.5.1. Верификация: `basic_execution` 16/16 (8 новых), `reentrancy_and_limits`
13/13, `cargo test --workspace` green, clippy `-D warnings` green.

**Цель:** заменить hardcoded `const ENTRY_POINT: &str = "main"` в
`crates/strangecoin-vm-wasm/src/lib.rs:27` на selector-based dispatch: calldata
первые 4 байта → lookup в `ContractAbi.entries` по `selector` → вызов
соответствующей экспортируемой WASM-функции.

**Контекст:** Текущая реализация требует, чтобы каждый контракт экспортировал
`main()` — это ограничение несовместимо с реальными контрактами (ERC20-like с
`transfer`/`balanceOf`/`approve` не может иметь одну точку входа). ADR-0013
(S1.5-P02) заявляет JSON ABI dispatch как MVP. S1.5-FBP01/FBP02 дали primitives;
этот промпт их интегрирует в wasmi executor.

**Задачи:**

1. `crates/strangecoin-vm-wasm/src/lib.rs`:
   - Убрать `const ENTRY_POINT: &str = "main"`.
   - В `run_frame` (или новой функции `dispatch_call`): если `ctx.input.len()
     >= 4` → извлечь selector = `ctx.input[0..4]`, lookup в
     `abis.get(&ctx.callee).entry_by_selector(selector)`; если найдено — call
     WASM-функцию с именем `entry.name` (expected signature: `(ptr: i32, len:
     i32) -> i32` для calldata); если `ctx.input.len() < 4` или selector не
     найден — fallback на `main()` (backward compat для legacy контрактов).
   - Передача calldata: contract читает через host function `input_read(ptr,
     len)` (новая, реализуется в этом промпте) или через memory.copy в
     pre-allocated buffer (Stage 4 SDK решит). MVP: host function
     `sc::input_read(out_ptr, out_len) -> i32` (returns bytes written; same
     pattern как `storage_read`).
2. `crates/strangecoin-vm-wasm/src/host/context/input_read.rs` (новый):
   - input: `(out_ptr: i32, out_len: i32)`, returns: `i32` (bytes written)
   - gas: 3 (как `gas_left`, `block_context`)
   - читает `ctx.input` (calldata из CallTx.data или nested call data)
3. `crates/strangecoin-vm-wasm/src/host/context/mod.rs` — register `input_read`
   в `sc` namespace.
4. `HostCallWeight::InputRead = 3` в `crates/strangecoin-vm-wasm/src/gas.rs`
   (extend ADR-0014 constants).
5. ADR-0013 amend: зафиксировать calling convention — selector-based dispatch
   с `main()` fallback, host function `sc::input_read` для calldata.
6. Тесты в `crates/strangecoin-vm-wasm/tests/basic_execution.rs` (extend):
   - Contract с двумя функциями `foo` и `bar` — calldata с selector для `foo`
     → вызывает `foo`; selector для `bar` → вызывает `bar`.
   - Contract только с `main` — calldata без recognized selector → fallback на
     `main()` (legacy compat).
   - `input_read` host function — calldata доступен контракту.

**Артефакты:**
- `crates/strangecoin-vm-wasm/src/lib.rs` (selector-based dispatch)
- `crates/strangecoin-vm-wasm/src/host/context/input_read.rs` (новый)
- `crates/strangecoin-vm-wasm/src/host/context/mod.rs` (register)
- `crates/strangecoin-vm-wasm/src/gas.rs` (HostCallWeight::InputRead)
- `docs/ADR/0013-json-abi-for-mvp-vs-wit.md` (amend: calling convention)
- `crates/strangecoin-vm-wasm/tests/basic_execution.rs` (extend)

**КГ (чек-лист):**
- [ ] `ENTRY_POINT: &str = "main"` удалён из `lib.rs`
- [ ] Selector-based dispatch: calldata[0..4] → `entry_by_selector` → call WASM
      export с именем `entry.name`
- [ ] Fallback на `main()` для legacy контрактов без ABI / с calldata < 4 bytes
- [ ] `sc::input_read(out_ptr, out_len) -> i32` host function реализована
- [ ] `HostCallWeight::InputRead = 3` добавлен в enum + ADR-0014 amend
- [ ] ADR-0013 amended с calling convention (selector-based + fallback)
- [ ] Тесты: 2 функции в одном контракте, selector dispatch работает
- [ ] Regression: существующие тесты (basic_execution, gas_metering,
      events_and_receipts) — green
- [ ] `cargo test --workspace` green, `cargo clippy -- -D warnings` green
- [ ] Коммит `[S1.5-FBP03] vm-wasm: selector-based dispatch + sc::input_read + main() fallback`

---

### S1.5-FBP04. Конструктор Create: исполнение init_code с init_args

**Статус:** fixed (2026-10-19)

**Решение (2026-10-19):** закрыт коммитом `f217314` (см. «Статус каталога»
вверху): `VmExecutor::execute_constructor` + `FrameEntry::Constructor`
(skip без экспорта `"constructor"` резолвится по export-секции модуля до
инстанцирования); `apply_create` rework (Success — flush storage +
endowment + metered receipt; ошибка — failed-receipt rollback, nonce
потребляется); `unapply_create` receipt-gated refund; `CreateTx.gas_limit`
+ `constructor_params` на wire (FORMAT_VERSION 6, genesis hashes + golden
vectors перегенерированы); ADR-0013 amended; Changelog 1.5.2. Отклонения
от спеки: бюджет конструктора — новое поле `CreateTx.gas_limit` (спека
предполагала `create.gas_limit` уже существующим); падение конструктора —
failed-receipt семантика (спека противоречила сама себе: «entire CreateTx
rejected» + «Receipt: OutOfGas/Failed»); `constructor_params` на wire, но
MVP диспатчит по имени (как и требовала спека).

**Цель:** закрыть MVP-прореху из Changelog 1.1.7: «Конструктор Create не
исполняется (MVP P14)». Реализовать исполнение конструктора при `CreateTx`:
deploy contract code → execute constructor export с `init_args` → storage writes
persisted, endowment transferred, `nonce = 1` после конструктора.

**Контекст:** Текущий `apply_create` в `crates/strangecoin-core/src/state/inner.rs:
322-323` явно комментирует: «The constructor export is not executed (MVP —
S1.5-P17 exercises the deployed entry point directly); `init_args` are stored but
not used.» Это блокирует любой реальный контракт (ERC20 constructor, который
mint-ит initial supply в deployer address, не работает). S1.5-FBP03 дал
selector-based dispatch — конструктор это тоже функция с selector
`method_selector("constructor", &init_params)`.

**Задачи:**

1. `crates/strangecoin-core/src/state/inner.rs::apply_create` — расширить
   (продолжать register_code):
   - После `vm.register_code(address, init_code)` — конструировать
     `ExecutionContext` с `callee = address`, `caller = create.from`, `value =
     create.endowment`, `input = init_args` (encoded ABI bytes, calldata для
     конструктора), `gas_limit = create.gas_limit`, `depth = 0`, `reentrant =
     false`, `storage_address = address`.
   - Selector для конструктора: `method_selector("constructor",
     &constructor_params)` — но MVP не требует ABI-driven selector; вместо этого
     WASM-функция с именем `"constructor"` (если экспортирована). Если
     `"constructor"` не экспортирована — skip constructor execution (legacy
     compat для контрактов без state init).
   - Execute constructor через `vm.execute(ctx)` → `ExecutionResult`.
   - При `Success`: применить `storage_overlay` к `State.contract_storage` (как
     в `apply_call`), пересчитать `storage_root`, перевести `endowment` sender →
     contract, nonce++ (уже делается).
   - При ошибке (`OutOfGas`, `Reverted`, `Failed`): entire `CreateTx` rejected,
     endowment не переводится, account не создаётся. Receipt: `OutOfGas` (с
     `gas_used = gas_limit`) или `Failed` (с `gas_used` = consumed).
2. `crates/strangecoin-core/src/state/inner.rs::unapply_create` — расширить:
   reverse storage writes (если конструктор выполнился), reverse endowment,
   remove account. Использовать `storage_undo` (как в `apply_call`).
3. `crates/strangecoin-vm-wasm/src/lib.rs::run_frame` — fallback на
   `"constructor"` экспорт при `callee == ctx.callee && depth == 0 && tx_type ==
   Create` (новый параметр в ExecutionContext или отдельный path). MVP: без
   отдельного path — selector-based dispatch из S1.5-FBP03 применяется одинаково
   к Create и Call.
4. `crates/strangecoin-core/src/types.rs` — добавить `CreateTx.constructor_params:
   Vec<AbiParam>` (optional, default empty) — для future ABI-driven selector;
   MVP не использует (constructor вызывается по имени `"constructor"`).
5. ADR-0013 amend: зафиксировать constructor calling convention — WASM export
   `"constructor"`, calldata = `init_args`, gas_limit из CreateTx.
6. Тесты:
   - `tests/vm_deploy_call.rs` (extend): deploy с constructor, который пишет в
     storage (initial supply) → `storage_root` != zero-sentinel → after deploy,
     call `totalSupply()` → возвращает initial supply.
   - Constructor с `OutOfGas` → `CreateTx` rejected, account не создан,
     endowment не переведён.
   - Constructor без `"constructor"` export → ok (legacy compat), account
     создан, endowment переведён.
   - `unapply_create` — reverse constructor storage writes + endowment.

**Артефакты:**
- `crates/strangecoin-core/src/state/inner.rs` (apply_create + unapply_create
  extended)
- `crates/strangecoin-core/src/types.rs` (CreateTx.constructor_params optional)
- `docs/ADR/0013-json-abi-for-mvp-vs-wit.md` (amend: constructor convention)
- `tests/vm_deploy_call.rs` (extend: constructor execution tests)
- `crates/strangecoin-vm-wasm/tests/contract_storage.rs` (extend: constructor
  writes persisted)

**КГ (чек-лист):**
- [ ] `apply_create` исполняет конструктор (WASM export `"constructor"`) с
      `init_args` как calldata
- [ ] При `Success` — storage writes persisted, `storage_root` пересчитан,
      endowment переведён, nonce++
- [ ] При ошибке — `CreateTx` rejected целиком, account не создан
- [ ] Constructor без `"constructor"` export → ok (legacy compat)
- [ ] `unapply_create` корректно reverse-ит storage writes и endowment
- [ ] `tests/vm_deploy_call.rs` — constructor с initial supply, OutOfGas,
      legacy compat — все green
- [ ] Regression: существующие тесты (vm_deploy_call без constructor, gas_metering)
      — green
- [ ] `cargo test --workspace` green, `cargo clippy -- -D warnings` green
- [ ] Коммит `[S1.5-FBP04] core/state: constructor execution in apply_create + unapply_create`

---

### S1.5-FBP05. Nested value-transfer: коммит value в apply_call для chain call→call

**Статус:** fixed (2026-10-19)

**Решение (2026-10-19):** закрыт коммитом S1.5-FBP05 (Changelog 1.5.3,
ADR-0013 amended — «value settlement & receipt audit trail»). Суть бага:
`nested_call` уже двигал value в копии accounts фрейма, но `ExecutionResult`
не несёт accounts/transfers — `apply_call` коммитил только top-level
`CallTx.value`, вложенные трансферы терялись. Реализация: on-entry
crediting top-level entry transfer в `run_frame` (контракт видит входящее
value до запуска кода — withdraw pattern); `nested_call` sender =
`ctx.storage_address` (delegatecall-фреймы тратят баланс делегирующего
контракта), guard value>0 для non-Call kinds → STATUS_FAILED; audit trail
`ExecutionResult.value_transfers` → `Receipt.value_transfers` (canonical
encoding, serde default; FORMAT_VERSION не бампается — receipts derived
data, version-byte за FBP07) + `State.value_transfers_undo` (accumulating,
нужен для chained rollbacks — расширка Receipt дополнена undo-map'ом по
тем же причинам, по которым накапливается `storage_undo`); `commit_value_transfers`/`reverse_value_transfers`
(sequential checked — заодно fixed latent self-call mint-bug fast path);
`apply_call`/`apply_create`/`unapply_*` переключены на helper. Отклонения:
trail в Receipt + undo-map (не только receipts, как в тексте промпта —
chained rollbacks иначе теряли данные); top-level transfer селтится в
`run_frame`, не в `nested_call`. Тесты: `vm_transfer_to_contract` переписан
+2 (chain A→B→C settles/unapplies; failed chain — ничего не коммитится) +
исправлен memory-layout баг WAT-фикстуры (STATUS_KEY перезаписывала хвост
bech32-адреса); `vm-wasm contract_storage` +1; core unit +4; workspace +
clippy green.

**Цель:** закрыть MVP-прореху из Changelog 1.1.7: «Nested value-трансферы не
коммитятся в `apply_call` (MVP P14, residual — S1.5-P17+).» Сейчас value
переводится только при Success верхнего frame; для nested call→call value
chain не работает. Нужно: при nested `call(to, value, data)`, где `to` — другой
контракт, value должно переводиться sender→callee в момент call, и revers-иться
при failed child frame.

**Контекст:** `nested_call` в
`crates/strangecoin-vm-wasm/src/host/calls/mod.rs:76` создаёт child
`ExecutionContext` с `value` = переданное, но `apply_call` в
`crates/strangecoin-core/src/state/inner.rs` коммитит value только на верхнем
уровне. Это означает: contract A вызывает contract B с value=100 — B видит
`ctx.value = 100` (через host function `block_context` или явный read), но
баланс B не увеличивается до тех пор, пока A не вернётся успешно. Это ломает
pattern «withdraw pattern» — B вызывает C, передавая value, которое B ещё не
получил от A.

**Задачи:**

1. `crates/strangecoin-vm-wasm/src/host/calls/call.rs` — value transfer
   происходит **в** `nested_call` (не в `apply_call` на верхнем уровне):
   - Deduct `value` из `caller_account.balance` (caller = `ctx.storage_address`
     или `ctx.caller` для top-level).
   - Credit `value` в `callee_account.balance`.
   - Если `caller_account.balance < value` → `STATUS_FAILED` + child-budget burn
     (как сейчас).
   - При revert child frame — reverse: deduct from callee, credit back to
     caller.
2. `crates/strangecoin-core/src/state/inner.rs::apply_call` — убрать верхний
   value-transfer (теперь он в nested_call); apply_call только вызывает
   `vm.execute(ctx)` и persist-ит storage_overlay при Success.
3. `ExecutionResult.value_transfers: Vec<(from, to, amount)>` — новый field в
   `ExecutionResult` для audit trail (для receipts и undo). При revert —
   `value_transfers` не пуст, но revers-ится.
4. `unapply_call` — reverse value_transfers в обратном порядке.
5. Тесты:
   - `tests/vm_transfer_to_contract.rs` (extend): A → B → C chain, each with
     value=50 — after success, balances: A=-100, B=0 (50 in, 50 out), C=+100.
   - A → B → C chain, C fails — B's transfer to C reverses, B's transfer from
     A reverses (если B сам revert-ится из-за C), только A остается без изменений.
   - `apply_block_with_vm` — все value transfers in receipts (audit trail).

**Артефакты:**
- `crates/strangecoin-vm-wasm/src/host/calls/call.rs` (value transfer in
  nested_call)
- `crates/strangecoin-vm-wasm/src/host/calls/delegatecall.rs` (delegatecall:
  value не передаётся — caller's storage_address = parent's storage_address,
  но value остаётся у parent)
- `crates/strangecoin-vm-wasm/src/host/calls/staticcall.rs` (staticcall: value
  запрещён — `STATUS_FAILED` если value > 0)
- `crates/strangecoin-core/src/vm/result.rs` (ExecutionResult.value_transfers)
- `crates/strangecoin-core/src/state/inner.rs` (apply_call — убрать верхний
  value transfer; unapply_call — reverse value_transfers)
- `tests/vm_transfer_to_contract.rs` (extend: chain A→B→C)
- `crates/strangecoin-vm-wasm/tests/contract_storage.rs` (extend: nested value
  transfer)

**КГ (чек-лист):**
- [ ] `nested_call` для `call` выполняет value transfer sender→callee
- [ ] При revert child frame — value transfer reverse-ится
- [ ] `delegatecall` не передаёт value (storage_address наследуется)
- [ ] `staticcall` с value > 0 → `STATUS_FAILED`
- [ ] `ExecutionResult.value_transfers` — audit trail
- [ ] `unapply_call` reverse-ит value_transfers в обратном порядке
- [ ] Тесты: A→B→C chain с value — green
- [ ] Regression: existing `vm_transfer_to_contract.rs` (top-level value) —
      green
- [ ] `cargo test --workspace` green, `cargo clippy -- -D warnings` green
- [ ] Коммит `[S1.5-FBP05] vm-wasm: nested value-transfer in call (commit on call, reverse on revert)`

---

### S1.5-FBP06. Lazy contract storage access (убрать клон contract_storage на каждый CallTx)

**Цель:** закрыть MVP-прореху из Changelog 1.1.8: «Клон всего
`contract_storage` на каждый CallTx — O(размер хранилища); mainnet-путь — lazy
state access через host functions (Stage 3+).» Сейчас `apply_call` клонирует
`State.contract_storage` целиком для передачи в `ExecutionContext.storage`,
что для контракта с 100K keys (~5MB) делает каждый call неприемлемо дорогим.
Stage 1.5 mainnet gate: O(1) setup per call, O(touched_keys) work.

**Контекст:** Текущая реализация оправдана для regtest/devnet (контракты
маленькие), но блокирует production. ADR-0013 (S1.5-P02) явно откладывает
lazy state access до Stage 3+ (RocksDB + iterator-based reads). Однако на
Stage 1.5 можно сделать intermediate решение: `Rc<RefCell<ContractStorageMap>>`
или `Arc<RwLock<...>>` с copy-on-write overlay — клонирование только при
записи, reads через shared reference.

**Задачи:**

1. `crates/strangecoin-core/src/state/contract_storage.rs` — `ContractStorageMap`
   → обернуть в `Rc<RefCell<...>>` (или оставить `HashMap` и добавить
   `ContractStorageView` с shared reference + overlay).
2. `ExecutionContext.storage: Rc<RefCell<ContractStorageMap>>` (shared) —
   nested calls видят ту же map (zero-copy).
3. `ExecutionContext.storage_overlay: ContractStorageMap` — per-frame overlay
   (как сейчас), merge в parent при Success.
4. `storage_read` host function — read from overlay → fall through to shared
   base (zero-copy).
5. `storage_write` host function — write to overlay (zero-copy), проверка
   `max_storage_per_contract` 1MB (sum of staged values, как сейчас).
6. `apply_call` — НЕ клонировать `State.contract_storage`; передавать `Rc::clone`
   в `ExecutionContext.storage`.
7. `flush_storage_writes` — merge overlay в shared base (mutate in place).
8. `unapply_call` — reverse overlay (как сейчас, через `storage_undo`).
9. Тесты: contract с 10K keys → 100 sequential calls — O(1) setup, no
   per-call clone (assert через timing или allocation counter, MVP: assert
   `State.contract_storage` pointer stable across calls).

**Артефакты:**
- `crates/strangecoin-core/src/state/contract_storage.rs` (Rc/RefCell или
  ContractStorageView)
- `crates/strangecoin-core/src/vm/context.rs` (ExecutionContext.storage: Rc)
- `crates/strangecoin-vm-wasm/src/host/storage/storage_read.rs` (zero-copy read)
- `crates/strangecoin-vm-wasm/src/host/storage/storage_write.rs` (zero-copy
  write to overlay)
- `crates/strangecoin-core/src/state/inner.rs` (apply_call — без clone)
- `crates/strangecoin-core/tests/contract_storage_smt.rs` (extend: lazy access
  test)

**КГ (чек-лист):**
- [ ] `ExecutionContext.storage` — shared (Rc/RefCell), не cloned per call
- [ ] `storage_read` — zero-copy (read from overlay → fall through to shared
      base)
- [ ] `storage_write` — zero-copy to overlay
- [ ] `flush_storage_writes` — merge overlay в shared base (in place)
- [ ] Тест: contract с 10K keys → 100 calls — O(1) setup (assert pointer
      stable or allocation count)
- [ ] `max_storage_per_contract` 1MB cap сохранён (regression)
- [ ] Regression: existing `contract_storage_smt.rs`, `vm-wasm/tests/contract_storage.rs`
      — green
- [ ] `cargo test --workspace` green, `cargo clippy -- -D warnings` green
- [ ] Коммит `[S1.5-FBP06] core/state: lazy contract storage access (Rc shared + overlay)`

---

### S1.5-FBP07. Receipts fold в глобальный state_root (light-client receipts требуют fold в header)

**Статус:** fixed (2026-10-20)

**Решение (2026-10-20):** вариант (b) из спеки — `BlockHeader.receipts_root:
[u8; 32]` (ADR-0016), commitment = `receipts_subroot` (txid-keyed SMT,
proof-friendly; alias `state::receipts_root`), **не** fold в глобальный
`state_root` (mixing concerns + слом ADR-0006 key space) и **не**
`compute_receipts_root` (flat merkle, proof-unfriendly). FORMAT_VERSION
6→7 (HEADER_WIRE_LEN 189→221; locks receipts-domain version byte,
отложенный FBP05) → genesis hashes (mainnet `0x0d400a…83cc`, testnet
`0x710d83…b270`) + golden vectors перегенерированы (BUG-S1-007).
Validation: `validate_and_apply` — после state_root check; zero tolerance
только `index == 0` (genesis хранит `[0;32]` — build-time constant,
`load_genesis` не исполняет блок; `allow_zero_state_root` ослабляет только
state). Set: `mine_block_inner`/`create_grant_block` — реальный subroot из
post.receipts до seal/PoW. Тесты: light-client proof против header root
(`tests/receipts.rs`), tamper/zero reject (`tests/state_root.rs`,
`tests/block_executor.rs`), craft-хелперы (`craft_child`/`commit_block`/
`with_receipts_commitment`/`time.rs`) ставят реальный root; ~60 literals
дополнены полем (fuzz — source-level, CI-side). TLA+: `ReceiptsRootMatch`
(abstract fold `ReceiptsRootOf`, empty→0), TLC PASS 35,207 states,
10/10 invariants (README обновлён). Changelog 1.5.5. Отклонения от спеки:
fold-вариант (a) отклонён (спека сама рекомендовала (b)); TLA+ инвариант —
abstract fold, не SMT (структура — Rust-тесты, честно задокументировано в
README residual). Verification: workspace + clippy green.

**Цель:** закрыть MVP-прореху из Changelog 1.1.7: «Fold `receipts_root` + ... в
глобальный `state_root` / `root_after` extension / rebuild генезиса — S1.5-P15.»
Сейчас receipts хранятся как derived data sub-trie (`receipts_subroot`,
`prove_receipt`, `verify_receipt_proof` в
`crates/strangecoin-core/src/state/sparse_merkle.rs:410+`), но не fold-ятся в
`block.state_root`. Light-client receipts требуют fold в header — иначе
light-client не может верифицировать receipts без full state.

**Контекст:** ADR-0006 (Stage 1, amended) постановил SMT depth 256 для account
state, без отдельного receipts sub-trie в header. Однако Stage 1.5 добавил
receipts sub-trie — теперь нужно решить: (a) fold `receipts_subroot` в
`block.state_root` (mixing account state + receipts в одном root — Ethereum
style separate root в header лучше), или (b) добавить `block.receipts_root` в
`BlockHeader` (Ethereum-style). Решение (b) — cleanest, требует ADR + consensus
change (но Stage 1.5 mainnet ещё не запущен, прямая правка допустима).

**Задачи:**

1. ADR-0016 (новый): `Receipts Root in Block Header`:
   - Decision: добавить `block_header.receipts_root: [u8; 32]` (32-byte SMT
     root, Ethereum-style separate root)
   - Context: light-client receipts верификация без full state
   - Consequences: consensus-breaking (но mainnet не запущен, прямой коммит до
     тега v1.5.0-stage1.5)
   - Alternatives: fold в global state_root (mixing concerns), receipts как
     account data (per-tx accounts — overkill)
2. `crates/strangecoin-core/src/types.rs::BlockHeader` — добавить поле
   `receipts_root: [u8; 32]`.
3. `crates/strangecoin-core/src/serialize.rs::serialize_block_header` —
   canonical encoding для нового поля (в конце, для backward compat с
   Stage 1 headers — но chain_id уже differentiates, так что breaking change
   ok).
4. `crates/strangecoin-core/src/state/mod.rs::root_after` — НЕ менять (account
   state root остаётся отдельным); добавить `receipts_root(receipts: &[Receipt])
   -> [u8; 32]` (alias для `receipts_subroot`).
5. `src/blockchain/block_executor.rs::validate_and_apply` — после
   `apply_and_execute`, вычислить `receipts_root` из receipts в блоке,
   сравнить с `block.receipts_root` → reject при mismatch.
6. `genesis.json` / `genesis-testnet.json` — rebuild (receipts_root для empty
   genesis = `[0;32]`); `EXPECTED_GENESIS_HASH` / `EXPECTED_TESTNET_GENESIS_HASH`
   — пересчитать через `examples/print_genesis_hashes.rs` (procedure BUG-S1-007).
7. `tests/serialize_golden.rs` — перегенерировать golden vectors (literal hex,
   pattern BUG-S1-007).
8. TLA+ spec `docs/spec/consensus.tla` — добавить инвариант
   `receipts_root_match` (block.receipts_root == compute_receipts_root(block.receipts));
   TLC run — PASS.
9. Тесты:
   - `tests/state_root.rs` (extend): block с receipts — `block.receipts_root`
     matches computed.
   - Tamper receipts_root → reject.
   - Light-client receipts proof: `prove_receipt(txid)` + `block.receipts_root`
     → `verify_receipt_proof` works without full state.

**Артефакты:**
- `docs/ADR/0016-receipts-root-in-block-header.md` (новый ADR)
- `crates/strangecoin-core/src/types.rs` (BlockHeader.receipts_root)
- `crates/strangecoin-core/src/serialize.rs` (canonical encoding)
- `crates/strangecoin-core/src/state/mod.rs` (receipts_root function)
- `src/blockchain/block_executor.rs` (validate receipts_root)
- `genesis.json` / `genesis-testnet.json` (rebuild)
- `tests/serialize_golden.rs` (regenerate golden vectors)
- `docs/spec/consensus.tla` (new invariant)
- `tests/state_root.rs` (extend)
- `tests/receipts.rs` (extend: light-client proof)

**КГ (чек-лист):**
- [ ] ADR-0016 создан (Decision: separate `receipts_root` field in BlockHeader)
- [ ] `BlockHeader.receipts_root: [u8; 32]` — новое поле
- [ ] Canonical serialization обновлена
- [ ] `validate_and_apply` — проверяет `block.receipts_root` matches computed
- [ ] `genesis.json` / `genesis-testnet.json` — rebuild, `EXPECTED_GENESIS_HASH`
      пересчитан
- [ ] Golden vectors перегенерированы (literal hex)
- [ ] TLA+ spec — new invariant `receipts_root_match`, TLC PASS
- [ ] Light-client receipts proof работает без full state
- [ ] Regression: existing `state_root.rs`, `block_executor.rs` tests — green
- [ ] `cargo test --workspace` green, `cargo clippy -- -D warnings` green
- [ ] Коммит `[S1.5-FBP07] core: receipts_root in BlockHeader + ADR-0016 + TLA+ invariant`

---

### S1.5-FBP08. Regression: 22 инвариантов + new threats для ABI/конструктора/nested value

**Статус:** fixed (2026-10-20)

**Решение (2026-10-20):** закрыт коммитом `768fc99` (Changelog-секции не
требуется — docs-only, прецедент P18-аудита). `INVARIANTS_ENFORCED.md` —
re-audit 22/22 enforced, no regressions: extensions #4 (constructor +
nested value undo chain: `value_transfers_undo`, receipt-gated
`unapply_create`, Arc deep-copy StateCache), #5 (header canonical v7),
#8 (genesis rebuild), #11 (constructor nonce++), #15 (`Create.gas_limit`
в `enforce_block_gas_limit`), #18 (`receipts_root` в header + light-client
proofs + value_transfers trail), #19 (`receipts_root` validation +
TLA+ `ReceiptsRootMatch`); #20 — enforced degenerate (fee=0 → сплит
тривиально; gas через #15; fee market Stage 5). `THREAT_MODEL_STAGE1_5.md`
§2 — residuals V-18 (гейты до value transfer, `mod.rs:118-193`),
V-19 (constructor init_code: sandbox + MAX_CODE_SIZE + gas metering),
V-21 (dispatch miss→main, strict MissingExport, `lib.rs:275-414`).
Новый `RESIDUAL_RISKS_STAGE2_PLUS.md` (constructor gas burning Stage 5;
receipts/contract prune Stage 3+ SCIP; cross-refs). Верификация:
workspace suite green, clippy `-D warnings` green, soak 402,672 inputs
0 panics; fuzz 5×600s — CI-side, локально не верифицировано + причина
(хост Windows, cargo-fuzz недоступен — прецедент FBP07/FBP09).
Отклонения от спеки: status summary — 22/22 enforced (per решение
владельца; #20 — degenerate-форма, не настоящий fee market).

**Цель:** обновить `docs/stage1.5/INVARIANTS_ENFORCED.md` и
`docs/stage1.5/THREAT_MODEL_STAGE1_5.md` после S1.5-FBP01..FBP07. Проверить, что
22 инварианта по-прежнему enforced (no regressions), и зафиксировать новые
mitigation для ABI dispatch (threat V-21 extension), constructor execution
(threat V-19 — init_code может содержать overflow), nested value transfer
(threat V-18 — reentrancy через value).

**Контекст:** S1.5-P18 зафиксировал 21/0/1 (enforced/residual/deferred-5).
S1.5-FBP01..FBP07 не должны ослабить ни один инвариант, но добавляют новые
attack surfaces. Особенно: constructor execution (init_code — arbitrary WASM,
threat V-21), nested value transfer (reentrancy через value — нужно убедиться,
что `non_reentrant` flag покрывает value path), receipts fold (consensus-
breaking change — TLA+ invariant).

**Задачи:**

1. `docs/stage1.5/INVARIANTS_ENFORCED.md` — обновить колонку "Stage 1.5
   enforced location" для всех 22 инвариантов после S1.5-FBP01..FBP07:
   - #4 (apply/unapply round-trip) — extension: constructor execution + nested
     value transfers в undo chain
   - #15 (block gas limit) — extension: constructor gas теперь учитывается
   - #18 (Receipts) — extension: `receipts_root` в BlockHeader, light-client
     proofs
   - #19 (state root match) — extension: `receipts_root` проверяется в
     `validate_and_apply`
2. `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` — добавить раздел "S1.5-FBP01..FBP07
   residuals":
   - V-19 (integer overflow) — `init_code` может содержать arbitrary WASM с
     overflow; mitigation: Rust checked arithmetic + `modexp` precompile (как
     сейчас) + gas metering на constructor
   - V-18 (reentrancy) — nested value transfer может создать reentrancy path
     через value; mitigation: `non_reentrant` flag проверяется в `nested_call`
     ДО value transfer (убедиться, что порядок сохранён после S1.5-FBP05)
   - V-21 (code injection) — selector-based dispatch не ослабляет sandbox, но
     добавляет attack surface (malicious selector → unexpected function);
     mitigation: `entry_by_selector` lookup отклоняет unknown selectors с
     fallback на `main()` (или явный reject — MVP choice)
3. Residual risks для Stage 2+:
   - Constructor gas burning не реализован (Stage 5 fee market)
   - Receipts prune (старые receipts можно удалить из sub-trie) — Stage 3+
   - Contract storage prune (удаление контракта с storage) — Stage 3+ через
     SCIP
4. Regression matrix: запустить полный `cargo test --workspace` + fuzz 5
   targets × 600s — все green, no panics, no differential failures.

**Артефакты:**
- `docs/stage1.5/INVARIANTS_ENFORCED.md` (updated для S1.5-FBP01..FBP07)
- `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` (updated)
- `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` (новый документ)

**КГ (чек-лист):**
- [ ] `INVARIANTS_ENFORCED.md` — 22/22 enforced (no regressions), updated
      locations для #4, #15, #18, #19
- [ ] `THREAT_MODEL_STAGE1_5.md` — S1.5-FBP01..FBP07 residuals зафиксированы
      (V-18, V-19, V-21 mitigation обновлены)
- [ ] `RESIDUAL_RISKS_STAGE2_PLUS.md` — создан (constructor gas, receipts prune,
      contract prune)
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Fuzz 5 targets × 600s — все green, 0 panics, 0 differential failures
- [ ] Коммит `[S1.5-FBP08] docs: invariants regression + threat model update после FBP01..FBP07`

---

### S1.5-FBP09. STAGE1_5_SUMMARY.md: 14 DoD критериев с ✅ + evidence

**Статус:** fixed (2026-10-18; refresh 2026-10-20 `100a388`) — выполнен как часть S1.5-P19 (`1f133c7`),
без FBP-нумерации: `docs/stage1.5/STAGE1_5_SUMMARY.md` существует, 14 DoD
критериев с evidence (см. DoD #6: P12 gap-closure зафиксирован отдельной
строкой). Пункт «Fuzz 5 targets × 600s» — CI-side, локально не
верифицировано (хост Windows, cargo-fuzz недоступен). Refresh `100a388`
(2026-10-20): drift-sync относительно S1.5-FBP01..FBP08 — DoD #2/#4/#6/#7/#10/#12
актуализированы, §2 дополнен строками FBP01..FBP08, §4 residuals #12/#13 помечены
done (FBP05/FBP07), §4.1 anti-goals 16/16 подтверждены нереализованными; workspace +
clippy green.

**Цель:** создать `docs/stage1.5/STAGE1_5_SUMMARY.md` — аналог
`docs/stage1/STAGE1_SUMMARY.md`, но для Stage 1.5. 14 DoD критериев из ROADMAP3
Этап 1.5, каждый с ✅ статусом и evidence (ссылки на коммиты, тесты, файлы).

**Контекст:** S1.5-P19 (DoD-верификация + тег) — это formal gate Stage 2/3/4.
Без `STAGE1_5_SUMMARY.md` Stage 1.5 не закрыт, даже если все proper промпты
выполнены. Этот промпт — первый шаг к закрытию: документ-сводка. Тег
`v1.5.0-stage1.5` — следующий промпт (S1.5-FBP11).

**Задачи:**

1. Создать `docs/stage1.5/STAGE1_5_SUMMARY.md` со структурой:
   - Header: статус, дата, prompt references (S1.5-P01..P19 + S1.5-FBP01..FBP08)
   - 14 DoD критериев из ROADMAP3 Этап 1.5 (таблица 1:1 с `prompt-stage1.5.md`
     §4)
   - Для каждого критерия: ✅ статус + evidence (commit hash, test file, file
     path)
   - Residual obligations для Stage 2+ (anti-goals Stage 1.5 — Block-STM, AA,
     EIP-1559, WIT, K-framework, ZK-VM, BLS12-381, Noise/Erlay, RocksDB)
   - 2 ops-gate из bugfixes-stage1.md (BUG-S1-013 PGP-key, BUG-S1-015 offline
     genesis key) — НЕ блокируют Stage 1.5, остаются human ops gates для mainnet
     freeze
2. Сверить с фактическим кодом: для каждого DoD критерия — `git log` grep,
   `ls` файлов, `cargo test` run.
3. Обязательства для Stage 2+ (зафиксировать):
   - Stage 2: Noise Protocol, Erlay, compact blocks, network crate migration
   - Stage 3: RocksDB, Block-STM parallel execution, state rent опционально
   - Stage 4: SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet, WIT ABI
     migration
   - Stage 5: Account Abstraction, EIP-1559 dynamic base fee, fee market,
     EncryptedMempool
   - Stage 6: K-framework formal verification, ZK-VM integration (research)
   - Stage 7: PoS migration, Casper FFG, BLS consensus signatures,
     bls_aggregate precompile impl
4. Anti-goals Stage 1.5 (явно отложено — `prompt-stage1.5.md` §7): перенести в
   summary с подтверждением, что не были реализованы.

**Артефакты:**
- `docs/stage1.5/STAGE1_5_SUMMARY.md` (новый документ)

**КГ (чек-лист):**
- [ ] `docs/stage1.5/STAGE1_5_SUMMARY.md` создан
- [ ] 14 DoD критериев — таблица 1:1 с `prompt-stage1.5.md` §4
- [ ] Каждый критерий — ✅ + evidence (commit hash, test file, file path)
- [ ] Residual obligations для Stage 2+ — перечислены
- [ ] 2 ops-gate (BUG-S1-013, BUG-S1-015) — зафиксированы как human ops gates
- [ ] Anti-goals Stage 1.5 — перенесены с подтверждением нереализации
- [ ] Коммит `[S1.5-FBP09] docs: STAGE1_5_SUMMARY.md — 14 DoD criteria с evidence`

---

### S1.5-FBP10. Версия Cargo.toml → 1.5.0 + AGENTS.md update + Changelog секция 1.5.0

**Статус:** fixed (2026-10-20) — большая часть выполнена S1.5-P19
(`1f133c7`): root `Cargo.toml` version=1.5.0 ✅, `AGENTS.md` ✅,
`Changelog.md` секция 1.5.0 ✅. Residual (версии крейтов) закрыт коммитом
`3826a44` (2026-10-20): `crates/strangecoin-core/Cargo.toml` 1.1.0→1.5.0,
`crates/strangecoin-vm-wasm/Cargo.toml` 0.1.0→0.2.0, `Cargo.lock` +
`fuzz/Cargo.lock` синхронизированы; `cargo check/test/clippy --workspace`
green с новыми версиями.

**Цель:** поднять версию `Cargo.toml` с `1.1.0` до `1.5.0` (strangecoin +
strangecoin-core); `strangecoin-vm-wasm` с `0.1.0` до `0.2.0` (semantic
versioning — minor bump для new functionality после S1.5-FBP01..FBP07).
Обновить `AGENTS.md` (убрать «Stage 1.5 in progress», поставить «Stage 1.5
complete»). Добавить секцию в `Changelog.md` `1.5.0 — WASM smart contracts
(basic)`.

**Контекст:** `prompt-stage1.5.md` §0 зафиксировал: «Version: 1.1.0 (Stage 1
complete; S1.5-P19 поднимет до 1.5.0)». После S1.5-FBP01..FBP09 все артефакты
созданы, версия должна быть поднята. Это второй шаг к закрытию Stage 1.5
(тег — S1.5-FBP11).

**Задачи:**

1. `Cargo.toml` — `version = "1.1.0"` → `version = "1.5.0"`.
2. `crates/strangecoin-core/Cargo.toml` — `version = "1.1.0"` → `version =
   "1.5.0"`.
3. `crates/strangecoin-vm-wasm/Cargo.toml` — `version = "0.1.0"` → `version =
   "0.2.0"` (minor bump для S1.5-FBP01..FBP07 new functionality).
4. `AGENTS.md` — обновить:
   - Убрать формулировку «Stage 1 complete»
   - Добавить «Stage 1.5 complete (tag `v1.5.0-stage1.5` — pending
     S1.5-FBP11)»
   - Обновить раздел "Key docs": добавить `docs/stage1.5/STAGE1_5_SUMMARY.md`,
     `docs/stage1.5/INVARIANTS_ENFORCED.md`, `docs/stage1.5/THREAT_MODEL_
     STAGE1_5.md`
   - Обновить "Current Architecture": добавить `crates/strangecoin-vm-wasm/` с
     host functions, precompiles, ABI, gas metering
   - Обновить "Key Constraints": убрать «Stage 1.5 — WASM until 1.5» (теперь
     Stage 1.5 done)
5. `Changelog.md` — добавить секцию `## 1.5.0 — Stage 1.5 complete: WASM smart
   contracts (basic)`:
   - **Status**: Complete (pending tag S1.5-FBP11)
   - **Date**: 2026-10-18
   - **Prompts**: S1.5-P01..P19 (S1.5-P12 закрыт через S1.5-FBP01..FBP02,
     S1.5-P19 закрыт через S1.5-FBP09..FBP11)
   - Список изменений: ADR-0011..0016, VmExecutor trait, wasmi integration,
     account model, tx types, gas metering, host functions (13), precompiles (7),
     JSON ABI (full), selector-based dispatch, constructor execution, nested
     value transfer, lazy storage, receipts root in header, reentrancy guard +
     limits, events/logs, fuzzing (5 targets), integration tests, invariants
     22/22, threat model V-18..V-21 mitigated
   - Not in this change: 2 ops-gate (BUG-S1-013, BUG-S1-015) — mainnet freeze
     gates
6. Запустить `cargo check --workspace` + `cargo test --workspace` + `cargo
   clippy --all-targets -- -D warnings` — все green с новыми версиями.

**Артефакты:**
- `Cargo.toml` (version 1.5.0)
- `crates/strangecoin-core/Cargo.toml` (version 1.5.0)
- `crates/strangecoin-vm-wasm/Cargo.toml` (version 0.2.0)
- `AGENTS.md` (updated)
- `Changelog.md` (секция 1.5.0)

**КГ (чек-лист):**
- [ ] `Cargo.toml` workspace version = 1.5.0
- [ ] `strangecoin-core` version = 1.5.0
- [ ] `strangecoin-vm-wasm` version = 0.2.0
- [ ] `AGENTS.md` — Stage 1.5 marked complete, key docs updated, architecture
      updated
- [ ] `Changelog.md` — секция `1.5.0 — Stage 1.5 complete: WASM smart contracts
      (basic)` с полным списком изменений
- [ ] `cargo check --workspace` green с новыми версиями
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S1.5-FBP10] release: bump version 1.5.0 + AGENTS.md + Changelog секция 1.5.0`

---

### S1.5-FBP11. Тег v1.5.0-stage1.5 (strictly после всех КГ предыдущих промптов)

**Статус:** fixed (2026-10-20) — annotated tag `v1.5.0-stage1.5` создан и
pushed на origin (2026-10-18, target `1f133c7`, tagger Reider85, verified
через `git ls-remote --tags origin`); release-run verification (residual)
закрыт 2026-10-20: GitHub REST API (без `gh` CLI) подтвердил run
[39110855701](https://github.com/Reider85/strangecoin/actions/runs/39110855701)
— success, 13/13 jobs (6 matrix + aggregate + sign + SLSA ×4 + Create
Release); release с 26 assets опубликован, `SHA256SUMS.txt` сверен.
Actions run URL + SHA256 зафиксированы в `docs/security/REPRODUCIBLE_BUILDS.md`
§Verified Release Runs (procedure BUG-S1-001); Release section +
verify-команды — в `docs/stage1.5/STAGE1_5_SUMMARY.md` §6; Changelog 1.5.6.
Independent cosign/SLSA blob verification — не верифицировано локально +
причина (`cosign`/`slsa-verifier`/`gh` не установлены на Windows-хосте);
verify commands зафиксированы для независимого повтора.

**Цель:** поставить аннотированный git tag `v1.5.0-stage1.5` — **Gate Stage
2/3/4**: после этого Stage 2 (network crate migration) и Stage 3 (RocksDB) могут
стартовать; Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*)
требует Stage 1.5 завершённым.

**Контекст:** `prompt-stage1.5.md` §6 «Порядок выполнения»: «Гейт №3 — тег
`v1.5.0-stage1.5` (S1.5-P19); без него Stage 2/3/4 не начинаются (мягкая
зависимость для Stage 2, жёсткая для Stage 4)». S1.5-FBP01..FBP10 закрыли все
предыдущие пункты КГ; этот промпт — формальное закрытие Stage 1.5.

**Задачи:**

1. Финальная верификация (doD):
   - `git log --oneline | grep "S1.5-FBP0"` — все 11 коммитов на месте
   - `cargo test --workspace` — все green
   - `cargo clippy --all-targets -- -D warnings` — green
   - Fuzz 5 targets × 600s — 0 panics (последний CI run)
   - `git tag -l | grep "v1.5.0-stage1.5"` — нет (до этого промпта)
   - `docs/stage1.5/STAGE1_5_SUMMARY.md` — существует
   - `Cargo.toml` version = 1.5.0
   - `AGENTS.md` — Stage 1.5 marked complete
   - `Changelog.md` — секция 1.5.0 существует
2. Поставить аннотированный тег:
   ```bash
   git tag -a v1.5.0-stage1.5 -m "Stage 1.5 complete: WASM smart contracts (basic)

   Prompts: S1.5-P01..P19 (S1.5-P12 via FBP01..FBP02, S1.5-P19 via FBP09..FBP11)
   Bugfixes: S1.5-FBP01..FBP11 (this file)

   14 DoD criteria — all ✅ (docs/stage1.5/STAGE1_5_SUMMARY.md)
   22 invariants — all enforced (docs/stage1.5/INVARIANTS_ENFORCED.md)
   Threats V-18..V-21 — mitigated (docs/stage1.5/THREAT_MODEL_STAGE1_5.md)

   ADRs: 0011 (async migration, Stage 1), 0012 (wasmi), 0013 (JSON ABI),
   0014 (gas metering), 0015 (precompiles), 0016 (receipts root in header)

   Fuzz: 5 targets × 600s, 0 panics
   TLA+: 35,207 states PASS + receipts_root_match invariant

   Ops-gate (NOT blocking Stage 1.5, blocks mainnet):
   - BUG-S1-013: PGP-key offline keygen
   - BUG-S1-015: offline genesis key air-gapped keygen (SCIP-0001)

   Gate for Stage 2 (network), Stage 3 (RocksDB), Stage 4 (dev-experience: SDK,
   devnet, indexer, JSON-RPC eth_*, faucet).
   "
   ```
3. Push тег на origin: `git push origin v1.5.0-stage1.5`.
4. После push — `release.yml` CI job должен сработать (triggers on `push: tags:
   ['v*']`), собрать 6 matrix артефактов + SHA256 + cosign signature + SLSA L3
   provenance. Записать Actions run URL + artifact SHA256s в
   `docs/security/REPRODUCIBLE_BUILDS.md` (процедура BUG-S1-001).
5. Обновить `docs/stage1.5/STAGE1_5_SUMMARY.md` — добавить раздел "Release"
   с Actions run URL + verify commands (`cosign verify-blob`, `slsa-verifier`).
6. Зафиксировать в Changelog: «Tag v1.5.0-stage1.5 pushed; release pipeline
   run <URL>; artifacts verified».

**Артефакты:**
- Git tag `v1.5.0-stage1.5` (annotated)
- `docs/security/REPRODUCIBLE_BUILDS.md` (Actions run URL + SHA256s + verify
  commands)
- `docs/stage1.5/STAGE1_5_SUMMARY.md` (Release section)
- `Changelog.md` (tag pushed + release verified)

**КГ (чек-лист):**
- [ ] `git log --oneline | grep "S1.5-FBP0"` — все 11 коммитов на месте
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Fuzz 5 targets × 600s — 0 panics (last CI run)
- [ ] `Cargo.toml` version = 1.5.0
- [ ] `docs/stage1.5/STAGE1_5_SUMMARY.md` существует
- [ ] Annotated tag `v1.5.0-stage1.5` создан (с полным сообщением)
- [ ] Tag pushed to origin
- [ ] `release.yml` CI job сработал (triggers on `push: tags: ['v*']`)
- [ ] 6 matrix artifacts собраны + SHA256 + cosign signature + SLSA L3
- [ ] Actions run URL + SHA256s записаны в `REPRODUCIBLE_BUILDS.md`
- [ ] `cosign verify-blob` + `slsa-verifier` outputs зафиксированы
- [ ] `STAGE1_5_SUMMARY.md` — Release section обновлён
- [ ] `Changelog.md` — tag pushed + release verified
- [ ] Коммит `[S1.5-FBP11] release: tag v1.5.0-stage1.5 + release pipeline verified`

---

## 3. Сводная таблица промптов

| ID | Заголовок | Ключевые артефакты | Зависимости |
|----|-----------|--------------------|-------------|
| S1.5-FBP01 | Полный JSON ABI: AbiType + encode/decode | core/abi/{mod,encode,decode,error}.rs, tests/abi_roundtrip.rs | (Gate: S1.5-P18 done) |
| S1.5-FBP02 | method_selector blake3 4-byte + wit-v2 placeholder | core/abi/selector.rs, tests/abi_roundtrip.rs (extend) | S1.5-FBP01 |
| S1.5-FBP03 | Selector-based dispatch (убрать ENTRY_POINT="main") | vm-wasm/src/lib.rs, host/context/input_read.rs, gas.rs, ADR-0013 amend | S1.5-FBP02 |
| S1.5-FBP04 | Конструктор Create: исполнение init_code + init_args | core/state/inner.rs, types.rs, ADR-0013 amend, tests/vm_deploy_call.rs (extend) | S1.5-FBP03 |
| S1.5-FBP05 | Nested value-transfer (commit on call, reverse on revert) | vm-wasm/src/host/calls/{call,delegatecall,staticcall}.rs, core/vm/result.rs, core/state/inner.rs | S1.5-FBP04 |
| S1.5-FBP06 | Lazy contract storage access (Rc shared + overlay) | core/state/contract_storage.rs, core/vm/context.rs, vm-wasm/src/host/storage/*.rs | S1.5-FBP05 |
| S1.5-FBP07 | Receipts fold в BlockHeader (ADR-0016 + TLA+ invariant) | docs/ADR/0016-receipts-root-in-block-header.md, types.rs, serialize.rs, genesis.json, TLA+ spec, tests/state_root.rs (extend) | S1.5-FBP06 |
| S1.5-FBP08 | Regression 22 invariants + new threats | docs/stage1.5/{INVARIANTS_ENFORCED,THREAT_MODEL_STAGE1_5,RESIDUAL_RISKS_STAGE2_PLUS}.md | S1.5-FBP01..FBP07 |
| S1.5-FBP09 | STAGE1_5_SUMMARY.md (14 DoD criteria + evidence) | docs/stage1.5/STAGE1_5_SUMMARY.md | S1.5-FBP08 |
| S1.5-FBP10 | Версия Cargo.toml → 1.5.0 + AGENTS.md + Changelog | Cargo.toml (3 файла), AGENTS.md, Changelog.md | S1.5-FBP09 |
| S1.5-FBP11 | Тег v1.5.0-stage1.5 + release pipeline verified | Git tag, REPRODUCIBLE_BUILDS.md, STAGE1_5_SUMMARY.md (Release section), Changelog.md | S1.5-FBP10 |

Параллельные треки:
- (a) **ABI-трек** S1.5-FBP01 → FBP02 → FBP03 — последовательный, ABI primitives
  до dispatch.
- (b) **State-трек** S1.5-FBP04 → FBP05 → FBP06 → FBP07 — последовательный, каждое
  требует предыдущего в apply_call.
- (c) **Verification-трек** S1.5-FBP08 → FBP09 → FBP10 → FBP11 — строго после
  всех proper промптов; FBP11 — последний, gate для Stage 2/3/4.

---

## 4. Покрытие DoD Этапа 1.5 (ROADMAP3) после S1.5-FBP01..FBP11

| # | Критерий DoD Этапа 1.5 (ROADMAP3) | Промпт | Evidence (после закрытия) |
|---|-----------------------------------|--------|--------------------------|
| 1 | `crates/strangecoin-vm-wasm/` создан, wasmi интегрирован | S1.5-P06 | Cargo.toml workspace member, wasmi=2.0.0 pinned |
| 2 | Все host functions реализованы и протестированы | S1.5-P10 + S1.5-FBP03 | tests/host_functions.rs (13 funcs × ≥3 tests) + tests/basic_execution.rs (input_read) |
| 3 | Gas metering работает (test: `gas_left()` корректно уменьшается) | S1.5-P09 | tests/gas_metering.rs |
| 4 | Tx types `transfer/create/call` работают | S1.5-P08 + S1.5-FBP04 | tests/tx_types.rs + tests/vm_deploy_call.rs (constructor) |
| 5 | Precompiles реализованы и протестированы | S1.5-P11 | tests/precompiles.rs (7 precompiles) |
| 6 | JSON ABI работает (test: round-trip encode/decode) | S1.5-FBP01 + S1.5-FBP02 | tests/abi_roundtrip.rs (полный round-trip + selector + wit-v2) |
| 7 | Reentrancy guard работает | S1.5-P13 | tests/reentrancy_and_limits.rs |
| 8 | Limits enforced | S1.5-P13 | tests/reentrancy_and_limits.rs |
| 9 | Events/logs работают | S1.5-P14 | tests/events_and_receipts.rs |
| 10 | Contract storage в SMT (test: state.root_after == block.state_root) | S1.5-P15 + S1.5-FBP06 + S1.5-FBP07 | tests/contract_storage_smt.rs + tests/state_root.rs (receipts_root) |
| 11 | Differential testing (wasmi vs wasmtime) | S1.5-P16 | fuzz/fuzz_targets/differential_wasmi_vs_wasmtime.rs |
| 12 | All Stage 0+1 invariants still enforced | S1.5-P18 + S1.5-FBP08 | docs/stage1.5/INVARIANTS_ENFORCED.md (22/22/22) |
| 13 | ADR-0011..0014 написаны | S1.5-P01..P04 + S1.5-FBP07 (ADR-0016) | docs/ADR/0011..0016-*.md |
| 14 | `Changelog.md` обновлён: `1.5.0 — WASM smart contracts (basic)` | S1.5-FBP10 + S1.5-FBP11 | Changelog.md, Cargo.toml 1.5.0, tag v1.5.0-stage1.5 |

Плюс: тег `v1.5.0-stage1.5` (S1.5-FBP11) — Gate для Stage 2/3/4.

---

## 5. Покрытие инвариантов ARCHITECT3 §5 после S1.5-FBP01..FBP11

| # | Инвариант | Статус после FBP01..FBP11 | Где enforce / тест |
|---|-----------|---------------------------|--------------------|
| 1 | Валидность — из цепочки | Без изменений (extension: contract_storage + receipts в chain) | state_cache::rebuild_from_chain, contract_storage_smt.rs |
| 2 | Каждая tx подписана | Расширено (constructor execution) | consensus::verify_transaction per-variant |
| 3 | `hash <= target` | Без изменений | regression |
| 4 | `apply_block`/`unapply_block` — обратные чистые | Расширено (constructor + nested value undo) | state::apply_call + unapply_call + unapply_create (extended) |
| 5 | Хэш/подпись — на канонических байтах | Расширено (receipts_root в header) | serialize_block_header (extended) |
| 6 | Блок не содержит непроверимых tx/наград | Без изменений | regression |
| 7 | Размеры сообщений/блоков ограничены | Без изменений | regression (max_code_size preserved) |
| 8 | Генезис детерминирован | Расширено (receipts_root в genesis — rebuild) | EXPECTED_GENESIS_HASH recalculated |
| 9 | Секреты не пишутся на диск | Без изменений | regression |
| 10 | Replay protection (chain_id) | Без изменений | regression |
| 11 | Nonce инкрементируется | Расширено (constructor nonce++) | state::apply_create (extended) |
| 12 | Transaction hash = commitment | Без изменений | regression |
| 13 | Mempool basic rules | Без изменений | regression |
| 14 | Graceful shutdown | Без изменений | regression |
| 15 | Block gas limit | Расширено (constructor gas учитывается) | block_executor::enforce_* (extended) |
| 16 | P2P framing | Без изменений | regression |
| 17 | Rate limiting | Без изменений | regression |
| 18 | Event log / Receipt | Расширено (receipts_root в header, light-client proofs) | emit_log + Receipt + receipts_root (extended) |
| 19 | State root match | Расширено (receipts_root match — TLA+ invariant) | root_after + receipts_root (extended) |
| 20 | Fee invariant | Deferred (Stage 5) | — (fixed fee proxy) |
| 21 | Consensus versioning | Без изменений | regression |
| 22 | Reproducible builds | Расширено (v1.5.0-stage1.5 release run verified) | release.yml run for v1.5.0-stage1.5 |

**Итог после S1.5-FBP01..FBP11:** 22/22 invariants enforced (расширение coverage
для #4, #5, #8, #11, #15, #18, #19, #22; 1 deferred до Stage 5: #20). Regression:
S1.5-FBP08.

---

## 6. Примечания по использованию

1. **Порядок выполнения:** строго по карте зависимостей (§1). Гейт №1 —
   подтверждение коммита `274b6b9` (S1.5-P18 done). Гейт №2 — S1.5-FBP01..FBP07
   (proper промпты). Гейт №3 — S1.5-FBP08 (regression). Гейт №4 — S1.5-FBP11
   (тег v1.5.0-stage1.5); без него Stage 2/3/4 не начинаются.

2. **Размер промпта:** каждый промпт — ≤200 строк markdown, реализация помещается
   в контекст 32k-token агента. Если промпт кажется большим — делить на a/b
   (например, S1.5-FBP01a: AbiType + encode; S1.5-FBP01b: decode + tests).

3. **Контекст:** вставлять блок "Общий контекст" (§0) в начало каждого промпта
   при отправке агенту.

4. **Артефакты и измеримость:** прогресс = число созданных файлов / общее число
   ожидаемых. КГ обязательны: нельзя закрывать промпт без прохождения всех
   пунктов; если КГ не проходит — итерировать тот же промпт, не переходить к
   следующему.

5. **Процессные страховки из retro §8.1** (обязательны к применению):
   - 1 промпт = 1 атомарный коммит с ID промпта в сообщении;
   - Changelog/README/AGENTS.md обновляются только коммитом, в котором артефакты
     уже существуют;
   - "done" объявления — только через DoD-промпт S1.5-FBP11 (тег), никогда —
     через ощущение завершённости;
   - факты, которые агент не может проверить (CI-run, fuzz-статистика,
     differential testing), фиксируются как "не верифицировано + причина", а не
     молча опускаются.

6. **Связь с чек-листом ARCHITECT3 §17:** после S1.5-FBP11 прогнать чек-лист §17
   и убедиться, что все применимые к Stage 1.5 пункты отмечены (включая "VM
   trait в core, runtime отдельно (wasmi)", "selector-based dispatch", "constructor
   execution", "receipts root in header").

7. **Детерминизм — критический инвариант:** каждая host function (включая
   `input_read` из S1.5-FBP03) обязана быть детерминированной (no time, no
   random, no network I/O). Ревью каждой host function на детерминизм —
   обязательный пункт КГ (threat V-21).

8. **Wasmi version pinning:** wasmi = "=2.0.0" (не "^", не patch) — gas metering
   API может меняться между версиями; pin в Cargo.toml (уже сделано в S1.5-P06);
   обновление только через SCIP после mainnet freeze.

9. **Consensus-breaking changes в S1.5-FBP07** (receipts_root в BlockHeader) —
   mainnet не запущен, прямая правка допустима до тега v1.5.0-stage1.5; после
   тега — только через SCIP + activation height. ADR-0016 обязателен.

10. **2 ops-gate из bugfixes-stage1.md** (BUG-S1-013 PGP-key, BUG-S1-015 offline
    genesis key) — НЕ блокируют Stage 1.5 (regtest/devnet scope); становятся
    обязательными gates только для mainnet freeze. Зафиксировать в
    STAGE1_5_SUMMARY.md (S1.5-FBP09) как residual ops gates.

11. **SCIP-0001 (genesis key replacement)** — отдельный трек, не входит в
    bugfixes-prompts2.md. После S1.5-FBP11 SCIP-0001 остаётся единственным
    блокером mainnet; его выполнение — вне Stage 1.5 scope, ops + code track.

12. **Anti-goals Stage 1.5 (явно отложено):** Block-STM (Stage 3+), Account
    Abstraction (Stage 5), EIP-1559 (Stage 5), WIT/Wasm Component Model для ABI
    (Stage 4+), K-framework формальная верификация WASM subset (Stage 6), ZK-VM
    integration (Stage 6+ research), State rent (Stage 3+ опционально через
    activation height), BLS12-381 dependency + bls_aggregate precompile impl
    (Stage 7), Noise Protocol Framework, Erlay, compact blocks, gossip-
    оптимизации (Stage 2), RocksDB миграция (Stage 3), EncryptedMempool
    (threshold encryption) реализация (Stage 5), SDK, devnet, indexer, explorer,
    JSON-RPC eth_*, faucet (Stage 4), PoS миграция, Casper FFG, BLS consensus
    signatures (Stage 7), External audit, formal verification, reproducible
    builds на Stage 1.5 scope (Stage 6), Перенос legacy-threads майнинга в
    async (Stage 2), TLA+ формальная верификация VM (Stage 6).

---

## 7. Что НЕ входит в bugfixes-prompts2.md (явно отложено)

Следующие задачи появляются на более поздних стадиях; если промпт случайно
затрагивает их — остановить и переориентировать на Stage 1.5 bugfix scope:

- **Block-STM (parallel execution)** — Stage 3+ (S1.5 — serial execution
  только).
- **Account Abstraction (social recovery, multisig, session keys, paymaster)** —
  Stage 5.
- **EIP-1559 dynamic base fee** — Stage 5 (сейчас фиксированный gas limit).
- **Multidimensional fees (EIP-7706 analog)** — Stage 6+ research.
- **WIT/Wasm Component Model для ABI** — Stage 4+ (сейчас только `format_version:
  "wit-v2"` placeholder, S1.5-FBP02).
- **K-framework формальная верификация WASM subset** — Stage 6.
- **ZK-VM integration (RISC Zero / SP1)** — Stage 6+ research.
- **State rent** — Stage 3+ опционально через activation height.
- **BLS12-381 dependency + `bls_aggregate` precompile impl** — Stage 7 (PoS-фаза;
  сейчас precompile возвращает `NotImplemented`, S1.5-P11).
- **Noise Protocol Framework, Erlay, compact blocks, gossip-оптимизации** —
  Stage 2.
- **RocksDB миграция** — Stage 3 (сейчас LevelDB).
- **EncryptedMempool (threshold encryption) реализация** — Stage 5.
- **SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet** — Stage 4 (требует
  VM завершённым, S1.5-FBP11).
- **PoS миграция, Casper FFG, BLS consensus signatures** — Stage 7.
- **External audit, formal verification, reproducible builds на Stage 1.5 scope** —
  Stage 6 (security track; сейчас только fuzzing harness, S1.5-P16).
- **Перенос legacy-threads майнинга в async** — Stage 2.
- **TLA+ формальная верификация VM** — Stage 6 (сейчас только consensus.tla +
  receipts_root_match invariant из S1.5-FBP07).
- **SCIP-0001 (genesis key replacement)** — отдельный трек, ops + code, не входит
  в bugfixes-prompts2.md scope.
- **BUG-S1-013 (PGP-key offline keygen)** — ops gate, не код; mainnet freeze
  gate.
- **BUG-S1-015 (offline genesis key air-gapped keygen)** — ops gate, не код;
  mainnet freeze gate. Tooling закрыт (`examples/generate_offline_genesis_key.rs`),
  ops остаётся.
- **Constructor fee burning** — Stage 5 fee market (сейчас gas = cap, fees = 0).
- **Receipts prune (старые receipts можно удалить из sub-trie)** — Stage 3+
  через SCIP.
- **Contract storage prune (удаление контракта с storage)** — Stage 3+ через
  SCIP.
