# prompt-stage1.5.md — Промпты для реализации Stage 1.5 (WASM смарт-контракты)

**Версия:** 1.1
**Дата:** 2026-10-15
**Источник:** `ROADMAP3.md` (Этап 1.5 — WASM смарт-контракты), `ARCHITECT3.md` (§3.9, §4.5, §5, §6, §10.3, §15), `ECONOMICS.md` (§2.6, §2.8), `prompt-stage0.md` (P01–P26), `prompt-stage1.md` (D01–D03, S1-P01..S1-P22), `bugfixes-stage0.md` (35 багов — все закрыты), `bugfixes-stage1.md` (16 багов BUG-S1-001..BUG-S1-016 — 14 закрыты кодом, 2 ops-gate).
**Предусловие:** Stage 1 завершён и помечен тегом `v1.1.0-stage1` (S1-P22). Сверка `bugfixes-stage0.md` и `bugfixes-stage1.md` по коду (`/tmp/strangecoin`, HEAD `6ba26ea`):

- **`bugfixes-stage0.md`** (35 багов BUG-S0-001..BUG-S0-035) — **все закрыты**: 29 `fixed`, 3 `wontfix` (BUG-S0-006, 010, 028 — задокументированы), 3 перенесены в `bugfixes-stage1.md` как BUG-S1-001/002/003 (теперь `fixed`).
- **`bugfixes-stage1.md`** (16 багов BUG-S1-001..BUG-S1-016):
  - **14 закрыты кодом** (verified): BUG-S1-001 (release.yml outputs.hashes + tag v0.0.1-rc1), 002 (state_root opt-out removed), 003 (post-root в witness), 004 (chain_id from Config + per-network genesis), 005 (per-message rate-limit), 006 (size checks в validate_and_apply), 007 (golden vectors literal hex), 008 (`sha2` removed from core/Cargo.toml — verified), 009 (`tests/wallet.rs`), 010 (`tests/chain_id.rs`), 011 (`tests/retarget.rs`), 012 (`source-gates` CI job), 014 (`scripts/audit/check_stage0_arith.py` + 3 Stage 0 headers rewritten), 016 (`u256_mul_sat` + LE-limb converters + `crates/strangecoin-core/tests/u256.rs`).
  - **2 ops-gate** (human ops, не код): BUG-S1-013 (PGP-key offline keygen + keyservers + Immunefi — doc-side closed 2026-10-13, ops остаётся), BUG-S1-015 (offline genesis key air-gapped keygen → vault → genesis.json/EXPECTED_GENESIS_HASH rebuild — tooling closed 2026-10-14 `examples/generate_offline_genesis_key.rs`, ops остаётся). Оба — **не блокируют Stage 1.5** (regtest/devnet scope; mainnet freeze gate).
- **Workspace:** `Cargo.toml` members = `[".", "crates/strangecoin-core"]` — `strangecoin-vm-wasm` не создан (Stage 1.5 не начат).
- **Version:** `1.1.0` (Stage 1 complete; S1.5-P19 поднимет до `1.5.0`).

**Цель:** реализовать WASM (wasmi) VM в отдельном крейте `strangecoin-vm-wasm`: host functions, gas metering, account model, precompiles, events/logs, ABI; интегрировать contract storage в Sparse Merkle Tree (общий state trie); закрыть все deferred-инварианты Stage 1 (#15 block gas limit, #18 receipts); поставить тег `v1.5.0-stage1.5`. **19 промптов со сквозной нумерацией S1.5-P01..S1.5-P19** — только новый Stage 1.5 proper work (retrospective debt-track удалён; всё закрытое кодом зафиксировано в `bugfixes-stage1.md`). Каждый промпт — самостоятельное задание для ИИ-агента, завершающееся конкретными артефактами и чек-листом КГ.

---

## 0. Общий контекст (вставлять в начало каждого промпта)

```
Проект: Strangecoin, Rust, edition 2021, версия 1.1.0 (тег v1.1.0-stage1).
Структура (после Stage 1):
  Монолит: src/main.rs (~3452 строки) + src/{serialize,consensus/economics,mempool,
    network/{mod,protocol,rate_limiter,sync,sync_engine},storage,config,cli,wallet,
    address,error,events}.rs + src/blockchain/{mod,block_executor,chain_selector,
    state_cache,blockchain_facade,consensus_manager}.rs.
  Крейт: crates/strangecoin-core/ (serialize, consensus, state/{mod,sparse_merkle,
    witness,inner}, economics/{emission,fee_market}, governance/scip, address, types,
    error, vm/{mod,traits}).
  Workspace: Cargo.toml с [workspace] members = [".", "crates/strangecoin-core"].
  Тесты: tests/ (27 файлов, включая chain_id, retarget, wallet, genesis_key, state_root,
    block_executor, sync_engine, rate_limit, address_migration, network_id);
    crates/strangecoin-core/tests/ (6 файлов: u256, serialize_golden, state_root,
    state_roundtrip, witness, consensus_proptest).
Криптоядро (сделано в Stage 0/1):
  - secp256k1 recoverable ECDSA; verify_transaction принуждает sender == address(pubkey).
  - Каноническая сериализация blake3 (core/src/serialize.rs), txid = commitment
    от подписанных канонических байтов; golden-векторы с literal hex.
  - Детерминированный генезис: genesis.json + EXPECTED_GENESIS_HASH + panic при mismatch;
    per-network генезис (mainnet=1, testnet=2, regtest=3); chain_id из Config.
  - Sparse Merkle Tree depth 256, state.root_after(block) == block.state_root enforced.
Консенсус (сделано в Stage 0/1): PoW U256 с LE-limb конвертерами + u256_mul_sat,
  retarget каждые 2016 блоков (clamp ×4/÷4 + u64 short-circuit), median-time-past
  (окно 11) + запрет будущего (+2h), tail emission (economics/emission.rs), реконструкция
  балансов из цепочки в validate_chain, consensus_version + activation height.
Защита (сделано в Stage 0/1): MAX_MESSAGE/BLOCK/TX_SIZE + safe framing, per-message +
  per-type rate-limit (100/30 msg per 10s per peer), single-block + per-tx size checks
  в validate_and_apply, mempool с валидацией на insert (MAX_PENDING_TXS=10000),
  RwLock<BlockchainInner>, graceful shutdown (AtomicBool + Drop + ctrlc).
Безопасность: THREAT_MODEL.md (V-1..V-42, 42 вектора), INCIDENT_RESPONSE.md,
  SECURITY.md, REPRODUCIBLE_BUILDS.md (tag v0.0.1-rc1, run 38071514479 green),
  BOUNTY.md, PGP_SETUP.md (placeholder-файл pgp_key.asc отсутствует — ops gate).
CI: source-gates job (check_stage0_arith.py + allow-comments check),
  fuzz-canonical-decode (cargo-fuzz 600s, 25.9M inputs / 0 panics),
  matrix Linux/macOS/Windows + clippy -D warnings.
Spec: docs/spec/consensus.tla (bounded-модель), TLC 2.19 PASS 35,207 состояний,
  9 инвариантов + Liveness; docs/spec/README.md.
SCIP: scip-0000-process.md, scip-0001-genesis-key-replacement.md (Draft,
  activation_height = TBD — mainnet freeze gate), scip-0002-state-root-enforcement.md
  (Draft, mainnet/testnet reject zero state_root, regtest opt-in).
Известные ops-gate (не код, не блокируют Stage 1.5):
  - BUG-S1-013: PGP-key offline keygen + keyservers + Immunefi (doc-side closed,
    ops остаётся; чек-лист PGP_SETUP.md §7)
  - BUG-S1-015: offline genesis key air-gapped keygen → vault → genesis rebuild
    (tooling closed: examples/generate_offline_genesis_key.rs; human ops остаётся)
Целевая архитектура Stage 1.5 (ARCHITECT3.md §3.9, §10.3, ROADMAP3 Этап 1.5):
  - crates/strangecoin-vm-wasm/ — отдельный крейт, wasmi runtime, host functions, gas,
    precompiles; core зависит от VmExecutor trait, не от рантайма.
  - VmExecutor trait в crates/strangecoin-core/src/vm/traits.rs (сейчас — stub на 16
    строк; раскрыть до полного API).
  - WASM runtime: wasmi (no_std, детерминированный, готовые фаззеры).
  - Account model: EOA + контрактные (`code_hash`, `storage_root`, `nonce`, `balance`).
  - Tx types: transfer / create / call; payload вызова входит в подписываемые данные.
  - Gas metering: instruction counting + host-call weights; block gas limit 1M gas
    (фиксированный, до EIP-1559 на Stage 5).
  - Host functions: secp256k1_verify, bls_verify, blake3, keccak, sha256, storage_read,
    storage_write, call, delegatecall, staticcall, block_context, gas_left, emit_log.
  - Precompiles: ecrecover, sha256, keccak, blake3, ed25519_verify, bls_aggregate, modexp.
  - ABI: простой JSON ABI (как в Ethereum) для MVP; WIT migration placeholder через
    format_version (WIT migration на Stage 4+).
  - Reentrancy guard: reentrant bool в ExecutionContext; nonReentrant modifier pattern.
  - Limits: max_code_size (24KB), max_storage_per_contract (1MB), max_call_depth (1024),
    max_stack_size, max_memory_size.
  - Events/logs: Log { address, topics, data } в receipt; Receipt { gas_used, logs,
    status }; receipts неизменны (инвариант #18).
  - Contract storage в общем state trie (Sparse Merkle Tree); state.apply_call обновляет
    storage trie; state.root_after(block) == block.state_root (инвариант #19 — extension
    до contract storage).
  - Block gas limit enforced (инвариант #15 — впервые enforce).
  - Fuzzing harness: cargo-fuzz targets для host functions; differential testing wasmi
    vs wasmtime.
  - ADR-0011..0014 написаны ДО кода.
Принципы:
  - Strangler pattern: core остаётся чистым (0 I/O); VM runtime живёт в отдельном крейте
    и импортируется через feature flag.
  - ADR пишется ДО того, как решение затронет код (ADR-0011..0014 — обязательный гейт).
  - Любое изменение консенсусных правил — через SCIP + activation height. На Stage 1.5
    mainnet не запущен, цепочка regtest/devnet — прямые правки допустимы до тега
    v1.5.0-stage1.5, но каждая правка фиксируется в Changelog.
  - Валидация на входе (размеры, подписи, время, генезис, replay, rate, gas) не
    ослабляется НИ одним промптом (DoD: «All Stage 0+1 invariants still enforced —
    no regressions»).
  - Determinism: host functions не должны утекать недетерминизм (время, random, I/O);
    каждая host function ревьюится на детерминизм (threat V-21).
Anti-goal на Stage 1.5: Block-STM (Stage 3+), Account Abstraction (Stage 5), EIP-1559
  (Stage 5), Noise Protocol/Erlay (Stage 2), RocksDB (Stage 3), ZK-VM (Stage 6+),
  WIT/Wasm Component Model для ABI (Stage 4+ — сейчас только format_version placeholder),
  state rent (Stage 3+ через activation height), K-framework формальная верификация WASM
  subset (Stage 6).
Стиль кода:
  - Не добавлять комментарии без необходимости.
  - После каждого промпта: cargo check и cargo test --workspace обязаны проходить.
  - 1 промпт = 1 атомарный коммит, ID промпта в сообщении, формат:
    «[S1.5-P05] vm: VmExecutor trait + ExecutionContext/ExecutionResult in core».
  - Логирование — только через tracing (structured fields); println! запрещён.
  - Секреты — только в keystore/env; в config не писать.
  - Запрещено обновлять Changelog/README/AGENTS.md о работе, для которой ещё нет коммита
    с файлами (урок retro §8.1).
Сокращения: КГ — критерии готовности.
```

---

## 1. Карта зависимостей промптов

```
STAGE 1.5 PROPER (старт только после подтверждения тега v1.1.0-stage1 и закрытия
всех P0/P1 багов из bugfixes-stage0.md / bugfixes-stage1.md — см. предусловие в
шапке документа; 14 код-багов закрыты, 2 ops-gate не блокируют):

ADRs (ДО кода — обязательный гейт):
S1.5-P01 (ADR-0011: WASM vs RISC-V) ──▶ S1.5-P02 (ADR-0012: JSON ABI vs WIT)
                                              │
                                              ├──▶ S1.5-P03 (ADR-0013: Gas metering strategy)
                                              │
                                              └──▶ S1.5-P04 (ADR-0014: Precompiles list)
                                                            │
                                                            ▼
TRAIT + WORKSPACE:
S1.5-P01..S1.5-P04 ──▶ S1.5-P05 (VmExecutor trait + types в core)
                              │
                              ▼
                        S1.5-P06 (crates/strangecoin-vm-wasm/ + wasmi integration)
                              │
                              ▼
                        S1.5-P07 (Account model: EOA + контрактные) ──▶ S1.5-P08 (Tx types: transfer/create/call)
                                                                          │
                                                                          ▼
                        S1.5-P09 (Gas metering + gas_left()) ──┐
                                                                   │
S1.5-P06 ──▶ S1.5-P10 (Host functions: crypto + storage + calls + context + logs) ──▶ S1.5-P11 (Precompiles)
                                                                                              │
                                                                                              ▼
S1.5-P10 + S1.5-P03 ──▶ S1.5-P12 (JSON ABI encode/decode + WIT placeholder)
                                                              │
                                                              ▼
S1.5-P07 + S1.5-P10 ──▶ S1.5-P13 (Reentrancy guard + Limits)
                                              │
                                              ▼
S1.5-P10 + S1.5-P06 ──▶ S1.5-P14 (Events/logs: Log + Receipt)
                                              │
                                              ▼
S1.5-P07 + S1.5-P14 + S1.5-P09 ──▶ S1.5-P15 (Contract storage в SMT + block gas limit + инвариант #15/#19)
                                              │
                                              ▼
S1.5-P10 + S1.5-P11 ──▶ S1.5-P16 (Fuzzing harness + differential testing wasmi vs wasmtime)
                                              │
                                              ▼
S1.5-P08..S1.5-P15 ──▶ S1.5-P17 (Интеграционные тесты Stage 1.5: end-to-end deploy + call)
                              │
                              ▼
                  S1.5-P18 (Регрессия 22 инвариантов + threat model актуализация)
                              │
                              ▼
                  S1.5-P19 (DoD-верификация Stage 1.5 + тег v1.5.0-stage1.5)
```

Параллельные треки (независимые линии):
- **ADRs-трек:** S1.5-P01 → S1.5-P02 → (∥) S1.5-P03 + S1.5-P04 — все 4 ADR можно писать
  параллельно после S1.5-P01 (решение "wasmi vs RISC-V" определяет scope остальных).
- **Runtime-трек:** S1.5-P05 → S1.5-P06 — trait и workspace связаны, но можно параллелить
  с ADRs (после S1.5-P01).
- **Host-functions-трек:** S1.5-P10 (host funcs) + S1.5-P11 (precompiles) — после S1.5-P06;
  параллелен с S1.5-P07/S1.5-P08 (account model + tx types) — обе не зависят друг от друга.
- **Storage/State-трек:** S1.5-P15 (contract storage в SMT) требует S1.5-P07 (account model)
  и S1.5-P14 (receipts — для storage_write events).

Жёсткие гейты:
1. **Долговой гейт = Gate Stage 1.5:** ни один промпт S1.5-P01+ не начинается до подтверждения
   тега `v1.1.0-stage1` и закрытия всех P0/P1 багов из `bugfixes-stage0.md` /
   `bugfixes-stage1.md` (см. предусловие в шапке).
2. **ADR до кода:** S1.5-P05 (VmExecutor trait) не начинается без ADR-0011..0014 (S1.5-P01..04);
   S1.5-P09 (gas metering) не начинается без ADR-0013.
3. **Runtime до host functions:** S1.5-P10 не начинается без S1.5-P06 (wasmi integration).
4. **Host functions до precompiles:** S1.5-P11 не начинается без S1.5-P10 (host funcs — crypto
   primitives переиспользуются в precompiles).
5. **Account model до storage integration:** S1.5-P15 требует S1.5-P07 (contract account type
   с storage_root) и S1.5-P14 (receipts для storage events).
6. **S1.5-P19 = Gate Stage 2/3:** Stage 2 (network crate migration) и Stage 3 (RocksDB) не
   начинаются до тега `v1.5.0-stage1.5` (мягкая зависимость: Stage 2 может идти параллельно
   с Stage 1.5 согласно ROADMAP3, но формальное завершение Stage 1.5 — обязательный гейт для
   Stage 4 (dev-experience), который требует VM).

---

## 2. Список промптов

### 2.1. ADRs — решения до кода (S1.5-P01–S1.5-P04)

Эти 4 ADR — обязательный гейт для всех последующих промптов. Принцип ARCHITECT3 §16:
"решения должны быть зафиксированы в `docs/ADR/` до того, как затронут консенсус". На
Stage 1.5 mainnet не запущен, но gas metering, precompiles list, ABI format — все
затрагивают будущий консенсус (по activation height после mainnet freeze). Без ADR
код пишется "на ощупь" — позже оказывается, что выбор был suboptimal, и миграция
требует SCIP. Каждый ADR — короткий документ (1-2 экрана) с разделами Context /
Decision / Consequences / Alternatives considered.

---

### S1.5-P01. ADR-0011: WASM (wasmi) vs RISC-V (ckb-vm)

**Цель:** зафиксировать выбор VM runtime — WASM (wasmi) vs RISC-V (ckb-vm). Это
решение определяет всю архитектуру Stage 1.5: host functions API, ABI, gas metering,
tooling. Без ADR — последующие промпты S1.5-P05..S1.5-P16 не имеют основания.

**Контекст:** ARCHITECT3 §3.9 заявляет WASM (wasmi), но без обоснования. Альтернативы:
(1) **WASM (wasmi)** — no_std, детерминированный, готовые фаззеры, LLVM backend,
tooling-зрелость (wasmtime, wasmer, wabt), широкая экосистема компиляторов
(rustc, clang, go, assemblyscript); (2) **RISC-V (ckb-vm)** — используется в Nervos,
более простой ISA, лучшая auditability, но меньше tooling; (3) **Своя VM
(anti-goal, ARCHITECT3 §15)** — слишком дорого, без фаззеров, без LLVM backend;
(4) **EVM** — совместимость с Ethereum, но ограниченная (нет generic types, нет
float, ограниченная криптография); (5) **Move VM** — ресурсо-ориентированная,
но без production-grade runtime вне diem-экосистемы.

**Задачи:**

1. Создать `docs/ADR/0011-wasm-wasmi-vs-risc-v.md` со структурой ADR-0001 template:
   - Status: Accepted
   - Context: почему нужен выбор, какие требования (детерминизм, gas metering, no_std,
     fuzzing, tooling)
   - Decision: WASM (wasmi)
   - Consequences:
     - Positive: готовые фаззеры, LLVM backend, широкая экосистема, no_std support
     - Negative: wasmi медленнее wasmtime (interpreter vs JIT) — но для Stage 1.5
       достаточно; memory model WASM может конфликтовать с Sparse Merkle Tree
     - Neutral: gas metering через instruction counting + host-call weights (см. ADR-0013)
   - Alternatives considered: RISC-V (ckb-vm), EVM, Move VM, своя VM — каждая с
     обоснованием отклонения
2. Зафиксировать wasmi version в `Cargo.toml` (не patch, чтобы избежать breaking
   changes в gas metering).
3. Ссылка на ADR-0011 в `crates/strangecoin-vm-wasm/Cargo.toml` comments.

**Артефакты:**
- `docs/ADR/0011-wasm-wasmi-vs-risc-v.md`

**КГ (чек-лист):**
- [ ] ADR-0011 создан по template ADR-0001
- [ ] Раздел "Alternatives considered" содержит ≥3 альтернативы с обоснованием
- [ ] wasmi version зафиксирован в `Cargo.toml` (не patch, не "^")
- [ ] Ссылка на ADR-0011 в `crates/strangecoin-vm-wasm/Cargo.toml` (после S1.5-P06)
- [ ] Коммит `[S1.5-P01] adr-0011: WASM (wasmi) vs RISC-V (ckb-vm) — Decision: wasmi`

---

### S1.5-P02. ADR-0012: JSON ABI for MVP vs WIT

**Цель:** зафиксировать выбор ABI формата — JSON ABI (как в Ethereum) для MVP vs WIT
(Wasm Component Model). Решение определяет, как контракты описывают свой интерфейс
(functions, types, events) — это необходимо для SDK, explorer, devnet (Stage 4).

**Контекст:** ARCHITECT3 §3.9 заявляет "простой JSON ABI для MVP, WIT migration на
Stage 4+ когда tooling дозреет". Но без ADR нет обоснования. Альтернативы: (1) **JSON
ABI (Ethereum-style)** — зрелый, документированный, совместим с eth_* (Stage 4),
но менее type-safe; (2) **WIT (Wasm Component Model)** — W3C standard 2023+, более
type-safe, но tooling молодой; (3) **Cairo ABI / Move ABI** — resource-oriented,
но требует resource-typed VM (wasmi не такой); (4) **Custom binary ABI** —
максимальная compactness, но без tooling. Решение: JSON ABI для MVP с `format_version`
полем для forward-compatibility; WIT migration placeholder через `format_version: "json-v1"
→ "wit-v2"` activation height.

**Задачи:**

1. Создать `docs/ADR/0012-json-abi-for-mvp-vs-wit.md`:
   - Status: Accepted
   - Context: требования к ABI (type-safe enough, tooling-ready, forward-compatible)
   - Decision: JSON ABI (Ethereum-style) для MVP
   - Consequences:
     - Positive: совместимость с eth_* (Stage 4), зрелый tooling, documentation
     - Negative: less type-safe than WIT; `format_version` migration path обязателен
     - Neutral: WIT migration на Stage 4+ через activation height
   - Alternatives: WIT, Cairo ABI, Move ABI, Custom binary ABI
2. Определить JSON ABI schema (sketch, не полная спецификация — это в S1.5-P12):
   - `name`, `type` (function/event/error), `inputs`, `outputs`, `state_mutability`
3. Зафиксировать `format_version: "json-v1"` как обязательное поле.

**Артефакты:**
- `docs/ADR/0012-json-abi-for-mvp-vs-wit.md`

**КГ (чек-лист):**
- [ ] ADR-0012 создан по template
- [ ] Раздел "Alternatives considered" содержит ≥3 альтернативы
- [ ] JSON ABI schema sketch включён (полнота — в S1.5-P12)
- [ ] `format_version` поле зафиксировано как обязательное
- [ ] Коммит `[S1.5-P02] adr-0012: JSON ABI for MVP (WIT migration placeholder)`

---

### S1.5-P03. ADR-0013: Gas metering strategy

**Цель:** зафиксировать стратегию gas metering — instruction counting + host-call
weights (ARCHITECT3 §3.9). Решение определяет, как контракты платят за исполнение;
без ADR последующие промпты S1.5-P09 (gas metering impl) и S1.5-P10 (host functions)
не имеют основания.

**Контекст:** ARCHITECT3 §3.9 заявляет "instruction counting + host-call weights (не
таблица опкодов вручную)". Альтернативы: (1) **Instruction counting (wasmi)** — wasmi
engine считает executed instructions, host-call weights — в host function impl;
(2) **Opcode gas table (Ethereum-style)** — hand-written таблица opcodes → gas cost;
(3) **Memory-only metering** — gas = memory_used * factor; (4) **Time-based metering**
(не детерминированный, anti-goal); (5) **Hybrid (instruction + memory + host-call)**.
Решение: instruction counting (base) + host-call weights (separately metered) +
memory metering (через host-call weight для `memory.grow`). Anti-goal ARCHITECT3 §15:
"своя VM инструкция" — поэтому hand-written opcode table запрещён; только wasmi
instruction counting.

**Задачи:**

1. Создать `docs/ADR/0013-gas-metering-strategy.md`:
   - Status: Accepted
   - Context: требования (детерминизм, anti-DoS, simple auditability)
   - Decision: instruction counting (wasmi `fuel` feature) + host-call weights
     (per-host-function constant) + memory metering (via `memory.grow` weight)
   - Consequences:
     - Positive: нет hand-written opcode table, wasmi maintained, fuzzing-ready
     - Negative: wasmi fuel feature — API может меняться (pin version!); instruction
       counting не учитывает memory access patterns
     - Neutral: host-call weights — hardcoded constants, обновляются через SCIP
   - Alternatives: opcode gas table, memory-only, time-based, hybrid
2. Определить gas constants (sketch):
   - `BASE_INSTRUCTION_GAS = 1` (per wasmi instruction)
   - `HOST_CALL_SECP256K1_VERIFY = 3000` (estimate from libsecp256k1 benchmarks)
   - `HOST_CALL_BLS_VERIFY = 12000` (BLS12-381 pairings, future)
   - `HOST_CALL_BLAKE3 = 50` (per 64-byte chunk)
   - `HOST_CALL_STORAGE_READ = 200` (SMT proof verification)
   - `HOST_CALL_STORAGE_WRITE = 5000` (SMT update)
   - `HOST_CALL_MEMORY_GROW = 3` (per page)
3. Block gas limit: 1M gas (фиксированный, до EIP-1559 на Stage 5).

**Артефакты:**
- `docs/ADR/0013-gas-metering-strategy.md`

**КГ (чек-лист):**
- [ ] ADR-0013 создан по template
- [ ] Раздел "Alternatives considered" содержит ≥3 альтернативы
- [ ] Gas constants sketch включён (≥6 constants с обоснованием)
- [ ] Block gas limit 1M зафиксирован (anti-DoS)
- [ ] Ссылка на SCIP для future gas constant updates
- [ ] Коммит `[S1.5-P03] adr-0013: gas metering (instruction counting + host-call weights)`

---

### S1.5-P04. ADR-0014: Precompiles list

**Цель:** зафиксировать список precompiles — встроенных контрактов, доступных через
`call(precompile_address, data)`. Решение определяет, какие криптографические
примитивы доступны контрактам без написания WASM; без ADR последующий промпт
S1.5-P11 (precompiles impl) не имеет основания.

**Контекст:** ARCHITECT3 §3.9 заявляет список: `ecrecover, sha256, keccak, blake3,
ed25519_verify, bls_aggregate, modexp`. Альтернативы: (1) **Полный Ethereum precompiles
list** (ecrecover, sha256, ripemd160, identity, modexp, altbn128_pairing, blake2f);
(2) **Минимум Strangecoin** (ecrecover, sha256, keccak, blake3, ed25519_verify,
bls_aggregate, modexp); (3) **Расширенный** (+ RSA verify, + VDF verify, + ZK
verify — слишком рано); (4) **Только SHA + ECrecover** (недостаточно для BLS PoS).
Решение: минимум Strangecoin (7 precompiles) — закрывает replay (ecrecover),
hashing (sha256, keccak, blake3), legacy (ed25519_verify), future PoS (bls_aggregate),
math (modexp). Addresses: `0x01..0x07` (как Ethereum, но другой список).

**Задачи:**

1. Создать `docs/ADR/0014-precompiles-list.md`:
   - Status: Accepted
   - Context: какие криптографические операции нужны контрактам без WASM impl
   - Decision: 7 precompiles (ecrecover, sha256, keccak, blake3, ed25519_verify,
     bls_aggregate, modexp)
   - Consequences:
     - Positive: покрывает replay (ecrecover), hashing (3 варианта), legacy
       (ed25519), PoS (bls_aggregate), math (modexp)
     - Negative: 7 precompiles — больше surface area, audit burden; bls_aggregate
       — без BLS12-381 dependency (отложено до Stage 7), поэтому precompile возвращает
       `NotImplemented` до PoS
     - Neutral: addresses `0x01..0x07` (порядок = Ethereum compatibility, но другой
       список — нельзя смешивать)
   - Alternatives: полный Ethereum list, минимум 2, расширенный
2. Определить addresses и gas cost (sketch):
   - `0x01 ecrecover` — 3000 gas
   - `0x02 sha256` — 50 + 12 per 32-byte word
   - `0x03 keccak` — 30 + 6 per 32-byte word (keccak, не sha3 NIST)
   - `0x04 blake3` — 50 + 6 per 64-byte chunk
   - `0x05 ed25519_verify` — 2000 (lib code available)
   - `0x06 bls_aggregate` — 12000 (returns NotImplemented до Stage 7)
   - `0x07 modexp` — 50 + 50 per byte of result
3. Определить ABI для каждого precompile (input/output format).

**Артефакты:**
- `docs/ADR/0014-precompiles-list.md`

**КГ (чек-лист):**
- [ ] ADR-0014 создан по template
- [ ] 7 precompiles перечислены с addresses и gas cost
- [ ] bls_aggregate помечен как NotImplemented до Stage 7
- [ ] ABI для каждого precompile (input/output format)
- [ ] Коммит `[S1.5-P04] adr-0014: precompiles list (7 precompiles)`

---

### 2.2. VM trait + workspace (S1.5-P05–S1.5-P06)

После ADR-0011..0014 — старт кода. Сначала trait в `core` (чистый, без зависимостей
от wasmi), потом — отдельный крейт `strangecoin-vm-wasm` с wasmi integration. Это
strangler pattern: `core` остаётся чистым, runtime — отдельно.

---

### S1.5-P05. VmExecutor trait + ExecutionContext/ExecutionResult/VmError в core

**Цель:** раскрыть stub `crates/strangecoin-core/src/vm/traits.rs` (16 строк) до
полного API: `VmExecutor` trait + `ExecutionContext` + `ExecutionResult` + `VmError` +
`Opcode` enum + `GasCounter` trait. `core` зависит от trait, не от рантайма —
соответствует принципу "Чистое ядро" и ARCHITECT3 §3.9.

**Контекст:** Текущий `vm/traits.rs`:
```rust
pub trait VmExecutor {
    type Error;
    fn execute(&self, block: &Block, accounts: &std::collections::HashMap<String, AccountState>) -> Result<(), Self::Error>;
}
```
Это слишком примитивно для Stage 1.5: нет `ExecutionContext` (gas, block_context,
caller, value, depth), нет `ExecutionResult` (gas_used, logs, status, return_data),
нет error variants (OutOfGas, OutOfMemory, Reverted, HostFunctionError), нет
`Opcode`/`GasCounter` (testability). Решение: раскрыть trait до production API,
определяя только interface; implementation — в `crates/strangecoin-vm-wasm/` (S1.5-P06).

**Задачи:**

1. `crates/strangecoin-core/src/vm/traits.rs` — раскрыть:
   ```rust
   pub trait VmExecutor {
       type Error: Into<VmError>;
       fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error>;
       fn gas_cost(&self, opcode: Opcode) -> u64;
   }
   ```
2. `crates/strangecoin-core/src/vm/mod.rs` — добавить:
   - `ExecutionContext { block, accounts, caller, callee, value, gas_limit, gas_used,
     depth, reentrant, block_context, storage_overlay }`
   - `ExecutionResult { gas_used, logs, status, return_data }`
   - `VmError { OutOfGas, OutOfMemory, Reverted, HostFunctionError, ReentrancyDetected,
     MaxCallDepth, MaxCodeSize, MaxStoragePerContract, GasMeteringBug }`
   - `Opcode` enum (placeholder — real opcodes из wasmi, не our own)
   - `GasCounter` trait (`spend(&mut self, gas: u64) -> Result<(), VmError>`)
3. `Log { address, topics: Vec<[u8; 32]>, data: Vec<u8> }` (см. S1.5-P14).
4. `Receipt { gas_used, logs: Vec<Log>, status: TxStatus }` (см. S1.5-P14).
5. Тесты: trait mockable — `MockVmExecutor` для core тестов без wasmi.

**Артефакты:**
- `crates/strangecoin-core/src/vm/traits.rs` (раскрытый trait + types)
- `crates/strangecoin-core/src/vm/mod.rs` (re-exports)
- `crates/strangecoin-core/src/vm/context.rs` (ExecutionContext)
- `crates/strangecoin-core/src/vm/result.rs` (ExecutionResult + Receipt + Log)
- `crates/strangecoin-core/src/vm/error.rs` (VmError)
- `crates/strangecoin-core/src/vm/gas.rs` (GasCounter trait)
- `crates/strangecoin-core/tests/vm_trait_mock.rs` (MockVmExecutor test)

**КГ (чек-лист):**
- [ ] `VmExecutor` trait имеет `execute(ExecutionContext) -> Result<ExecutionResult, _>`
- [ ] `ExecutionContext` содержит ≥9 полей (gas, depth, reentrant, etc.)
- [ ] `VmError` имеет ≥8 variants
- [ ] `Log` и `Receipt` определены (используются в S1.5-P14)
- [ ] `MockVmExecutor` green в core тестах (без wasmi)
- [ ] `cargo check -p strangecoin-core` green
- [ ] Коммит `[S1.5-P05] core/vm: VmExecutor trait + ExecutionContext/ExecutionResult/VmError`

---

### S1.5-P06. crates/strangecoin-vm-wasm/ + wasmi integration

**Цель:** создать отдельный крейт `crates/strangecoin-vm-wasm/` с wasmi runtime
integration. Реализовать `impl VmExecutor for WasmiExecutor` — минимальный
end-to-end: load WASM module, instantiate, call `main()`, return gas_used. Без host
functions (это S1.5-P10) — только basic WASM execution.

**Контекст:** ARCHITECT3 §10.3: "Создаётся `crates/strangecoin-vm-wasm/`; реализуется
`VmExecutor` trait в `core`; `wasmi` runtime, host functions, gas metering; монолит
`main.rs` обновляется, чтобы использовать `vm` как зависимость." Wasmi имеет `fuel`
feature для gas metering (см. ADR-0013) — version pinned в Cargo.toml. Этот промпт
только structural; host functions — S1.5-P10.

**Задачи:**

1. Создать `crates/strangecoin-vm-wasm/Cargo.toml`:
   ```toml
   [package]
   name = "strangecoin-vm-wasm"
   version = "0.1.0"
   edition = "2021"
   
   [dependencies]
   wasmi = "=0.31.0"  # pin, не "^", см. ADR-0011
   strangecoin-core = { path = "../strangecoin-core" }
   ```
2. `crates/strangecoin-vm-wasm/src/lib.rs` — `WasmiExecutor` struct, `impl VmExecutor
   for WasmiExecutor`:
   - `execute(ctx: ExecutionContext) -> Result<ExecutionResult, WasmiError>`
   - Load WASM module from `ctx.accounts[&callee].code`
   - Instantiate with `wasmi::Engine::default()` + `fuel` config
   - Call `main()` (или contract entry point — ABI define в S1.5-P12)
   - Return `ExecutionResult { gas_used: fuel_consumed, logs: vec![], status: Success,
     return_data }`
3. `WasmiError` — `From<wasmi::Error> + Into<VmError>`.
4. Workspace `Cargo.toml` — добавить `crates/strangecoin-vm-wasm` в members.
5. `src/main.rs` — добавить feature flag `vm-wasm = ["dep:strangecoin-vm-wasm"]`;
   default-on для regtest/devnet, default-off для light node.

**Артефакты:**
- `crates/strangecoin-vm-wasm/Cargo.toml`
- `crates/strangecoin-vm-wasm/src/lib.rs` (WasmiExecutor + impl VmExecutor)
- `crates/strangecoin-vm-wasm/src/error.rs` (WasmiError)
- `Cargo.toml` (workspace members updated)
- `src/main.rs` (feature flag `vm-wasm`)

**КГ (чек-лист):**
- [ ] `crates/strangecoin-vm-wasm/` создан, в workspace
- [ ] `WasmiExecutor` имплементирует `VmExecutor` trait из core
- [ ] Wasmi version pinned (`wasmi = "=0.31.0"`, не `^`)
- [ ] `cargo build -p strangecoin-vm-wasm` green
- [ ] Feature flag `vm-wasm` в `src/main.rs` (default-on regtest)
- [ ] Тест: load simple WASM (return 42), call, assert gas_used > 0
- [ ] Коммит `[S1.5-P06] vm-wasm: crate + WasmiExecutor + wasmi integration (basic)`

---

### 2.3. Account model + Tx types (S1.5-P07–S1.5-P08)

Contract storage требует различать EOA (внешние accounts, без code) и контрактные
accounts (с code, storage_root, nonce, balance). Tx types должны поддерживать deploy
(create) и call — иначе контрактов нет. Эти два промпта — фундамент для gas, host
functions, state integration.

---

### S1.5-P07. Account model: EOA + контрактные аккаунты

**Цель:** ввести тип `Account` с различением EOA и контрактных accounts. EOA: только
`balance` и `nonce` (secp256k1-keypair контролирует). Контрактный account: `code_hash`,
`storage_root`, `nonce`, `balance`. Это precondition для S1.5-P08 (tx types) и
S1.5-P15 (contract storage в SMT).

**Контекст:** Текущий `AccountState` в `crates/strangecoin-core/src/types.rs` —
вероятно только `{ balance, nonce }` (как в Stage 0). Для Stage 1.5 нужно
различать: `Account::Eoa(EoaAccount { balance, nonce })` vs `Account::Contract(
ContractAccount { code_hash, storage_root, nonce, balance })`. ARCHITECT3 §3.9:
"Account model: EOA + контрактные (`code_hash`, `storage_root`, `nonce`, `balance`)".
Решение: enum с двумя variants; storage_root — SMT root for contract storage subtree
(не global SMT root — это block state_root).

**Задачи:**

1. `crates/strangecoin-core/src/types.rs` — `Account` enum:
   ```rust
   pub enum Account {
       Eoa(EoaAccount),
       Contract(ContractAccount),
   }
   pub struct EoaAccount { pub balance: u64, pub nonce: u64 }
   pub struct ContractAccount {
       pub code_hash: [u8; 32],
       pub storage_root: [u8; 32],  // SMT root of contract storage
       pub nonce: u64,
       pub balance: u64,
   }
   ```
2. Backward compat: старый `AccountState` — type alias для `Account` или миграция.
3. `state::apply_block` — обновить для обработки `Account::Contract` (code_hash,
   storage_root updates).
4. `state::root_after(block)` — учитывать contract storage_root в global SMT root.
5. Тесты: EOA account create, contract account create (deploy), contract storage
   update (storage_root changes), SMT root changes.

**Артефакты:**
- `crates/strangecoin-core/src/types.rs` (Account enum + EoaAccount + ContractAccount)
- `crates/strangecoin-core/src/state/mod.rs` (apply_block updated)
- `crates/strangecoin-core/src/state/inner.rs` (contract storage handling)
- `crates/strangecoin-core/tests/account_model.rs`

**КГ (чек-лист):**
- [ ] `Account` enum с двумя variants
- [ ] `ContractAccount` имеет `code_hash`, `storage_root`, `nonce`, `balance`
- [ ] `state::apply_block` обрабатывает contract accounts
- [ ] `state::root_after` учитывает contract storage_root
- [ ] Тесты: EOA + Contract create + storage update — green
- [ ] Regression: Stage 0/1 tests still green
- [ ] Коммит `[S1.5-P07] core: Account model (EOA + Contract) with code_hash + storage_root`

---

### S1.5-P08. Tx types: transfer / create / call

**Цель:** ввести 3 типа транзакций: `transfer` (EOA → EOA, без контракта), `create`
(deploy contract), `call` (EOA → contract method). Payload вызова входит в подписываемые
данные (replay protection, инвариант №10 extension). Это precondition для S1.5-P10
(host functions `call`/`delegatecall`/`staticcall`).

**Контекст:** Текущий `Transaction` type (Stage 0): `{ id, sender, receiver, amount }`.
Для Stage 1.5 нужно различать: `Transaction::Transfer(TransferTx)`, `Transaction::Create(
CreateTx)`, `Transaction::Call(CallTx)`. ABI: payload `call` — encoded method
selector + args (см. ADR-0012, S1.5-P12). `create` — WASM bytecode + constructor args.
ARCHITECT3 §3.9: "Tx types: `transfer` / `create` / `call`; payload вызова входит в
подписываемые данные".

**Задачи:**

1. `crates/strangecoin-core/src/types.rs` — `Transaction` enum:
   ```rust
   pub enum Transaction {
       Transfer(TransferTx),
       Create(CreateTx),
       Call(CallTx),
   }
   pub struct TransferTx { pub nonce, pub from, pub to, pub amount, pub fee, pub chain_id, pub signature }
   pub struct CreateTx { pub nonce, pub from, pub init_code: Vec<u8>, pub init_args: Vec<u8>,
     pub endowment: u64, pub fee, pub chain_id, pub signature }
   pub struct CallTx { pub nonce, pub from, pub to, pub data: Vec<u8>, pub value: u64,
     pub gas_limit: u64, pub fee, pub chain_id, pub signature }
   ```
2. Каноническая сериализация: `serialize_transaction_signed` для каждого варианта
   (txid = commitment over подписанных байтов).
3. `verify_transaction` — проверить подпись для каждого варианта.
4. `mempool::insert` — валидация для каждого варианта (nonce, balance, chain_id,
   signature; + для `CallTx` — gas_limit).
5. Тесты: transfer ok, create ok, call ok; для каждого — replay (same nonce twice)
   rejected, cross-chain rejected.

**Артефакты:**
- `crates/strangecoin-core/src/types.rs` (Transaction enum)
- `crates/strangecoin-core/src/serialize.rs` (per-variant serialization)
- `crates/strangecoin-core/src/consensus.rs` (verify_transaction per-variant)
- `src/mempool/mod.rs` (per-variant validation)
- `crates/strangecoin-core/tests/tx_types.rs`

**КГ (чек-лист):**
- [ ] `Transaction` enum с 3 variants
- [ ] Каждый variant имеет `chain_id`, `nonce`, `signature` (replay protection)
- [ ] `serialize_transaction_signed` работает для всех 3 variants
- [ ] `verify_transaction` проверяет подпись для всех 3 variants
- [ ] `mempool::insert` валидирует все 3 variants
- [ ] Regression: Stage 0/1 transfer tests still green (compat)
- [ ] Коммит `[S1.5-P08] core: Tx types (transfer / create / call) + per-variant serialization`

---

### 2.4. Gas metering (S1.5-P09)

Gas metering — критический anti-DoS механизм. Без него malicious contract может
вызвать infinite loop (threat V-21). ARCHITECT3 §3.9: "Gas metering: instruction
counting + host-call weights; Block gas limit: 1M gas".

---

### S1.5-P09. Gas metering: instruction counting + host-call weights + gas_left()

**Цель:** реализовать gas metering согласно ADR-0013. Wasmi `fuel` feature для
instruction counting; host-call weights — в каждой host function impl (S1.5-P10);
`gas_left()` host function; `OutOfGas` error variant (S1.5-P05). Block gas limit 1M
enforced в `block_executor::validate_and_apply` (инвариант №15).

**Контекст:** ADR-0013 (S1.5-P03) зафиксировал стратегию. Wasmi engine с `fuel`
configuration: `Engine::default()` + `gas_limit` per call. Host-call weights —
hardcoded constants (см. ADR-0013 sketch). `gas_left()` — host function, возвращает
remaining gas в execution context. Block gas limit: сумма `gas_used` всех tx в блоке
≤ 1M, иначе block rejected.

**Задачи:**

1. `crates/strangecoin-vm-wasm/src/lib.rs` — wasmi engine с `fuel` config:
   ```rust
   let engine = wasmi::Engine::config()
       .with_consumption_strategy(wasmi::ConsumptionStrategy::RecurringBlock)
       .with_fuel_limit(ctx.gas_limit)
       .build();
   ```
2. `crates/strangecoin-vm-wasm/src/gas.rs` — `GasCounter` impl:
   - `spend(&mut self, gas: u64) -> Result<(), VmError>` — decrement, return
     `OutOfGas` if underflow
   - `spend_host_call(&mut self, weight: HostCallWeight) -> Result<(), VmError>` —
     use ADR-0013 constants
   - `gas_left(&self) -> u64`
3. `HostCallWeight` enum — все host functions из S1.5-P10, hardcoded gas costs.
4. `gas_left()` host function — проксирует в `GasCounter::gas_left()`.
5. `block_executor::validate_and_apply` — проверка `sum(tx.gas_used) <= 1_000_000`
   (инвариант №15); если больше — `BlockGasLimitExceeded`.
6. Тесты: contract с infinite loop → `OutOfGas` за < 1ms; contract с heavy
   computation (10M instructions) → `OutOfGas` after 1M; `gas_left()` корректно
   уменьшается; block с > 1M total gas → reject.

**Артефакты:**
- `crates/strangecoin-vm-wasm/src/lib.rs` (wasmi fuel config)
- `crates/strangecoin-vm-wasm/src/gas.rs` (GasCounter impl + HostCallWeight enum)
- `crates/strangecoin-vm-wasm/src/host/gas_left.rs` (host function)
- `src/blockchain/block_executor.rs` (block gas limit check, инвариант №15)
- `crates/strangecoin-vm-wasm/tests/gas_metering.rs` (4+ tests)

**КГ (чек-лист):**
- [ ] Wasmi engine с `fuel` config (RecurringBlock strategy)
- [ ] `GasCounter` impl с `spend`, `spend_host_call`, `gas_left`
- [ ] `HostCallWeight` enum с ≥6 weights из ADR-0013
- [ ] `gas_left()` host function работает
- [ ] Block gas limit 1M enforced в `validate_and_apply` (инвариант №15)
- [ ] Тест: infinite loop → `OutOfGas` за < 1ms
- [ ] Тест: block с > 1M total gas → reject
- [ ] Коммит `[S1.5-P09] vm-wasm: gas metering (instruction counting + host-call weights) + block gas limit`

---

### 2.5. Host functions + precompiles (S1.5-P10–S1.5-P11)

Host functions — единственный канал взаимодействия контракта с внешним миром
(crypto, storage, calls, context, logs). ARCHITECT3 §6 threat V-21: "Code injection —
malicious WASM; mitigation: wasmi sandbox (no I/O by default), host functions
whitelist, gas metering prevents infinite loops". Каждая host function — детерминированная
(no time, no random, no network I/O). Precompiles — separate abstraction, доступные
через `call(precompile_address, data)` без WASM bytecode.

---

### S1.5-P10. Host functions: crypto + storage + calls + context + logs

**Цель:** реализовать полный набор host functions из ARCHITECT3 §3.9: `secp256k1_verify`,
`bls_verify` (NotImplemented до Stage 7), `blake3`, `keccak`, `sha256`, `storage_read`,
`storage_write`, `call`, `delegatecall`, `staticcall`, `block_context`, `gas_left`,
`emit_log`. Каждая — детерминированная, с gas cost из ADR-0013 (через `HostCallWeight`).

**Контекст:** Wasmi sandbox не имеет I/O по умолчанию — host functions — единственный
канал. Список whitelist зафиксирован в ADR-0014 (precompiles) и ADR-0013 (gas cost).
Threat V-21: malicious WASM может попробовать вызвать host function с bad args —
каждая host function обязана валидировать input. Решение: каждый host function —
отдельный модуль с input validation, gas metering (через `GasCounter::spend_host_call`),
deterministic-only behavior.

**Задачи:**

1. `crates/strangecoin-vm-wasm/src/host/` — директория с одним файлом на host function:
   - `crypto/secp256k1_verify.rs` — input: `[pubkey (33 bytes), msg_hash (32), signature (64)]`,
     returns: `bool`; gas: 3000
   - `crypto/bls_verify.rs` — returns `NotImplemented` до Stage 7 (no BLS12-381 dep)
   - `crypto/blake3.rs` — input: `data: Vec<u8>`, returns: `[u8; 32]`; gas: 50 + 6 per 64-byte chunk
   - `crypto/keccak.rs` — input: `data: Vec<u8>`, returns: `[u8; 32]`; gas: 30 + 6 per 32-byte word
   - `crypto/sha256.rs` — input: `data: Vec<u8>`, returns: `[u8; 32]`; gas: 50 + 12 per 32-byte word
   - `storage/storage_read.rs` — input: `key: [u8; 32]`, returns: `Vec<u8>` (from contract
     storage overlay); gas: 200
   - `storage/storage_write.rs` — input: `key: [u8; 32], value: Vec<u8>`, no return; gas: 5000
   - `calls/call.rs` — input: `to: address, value: u64, data: Vec<u8>`, returns: `Result`;
     gas: per-call weight + child gas_limit deducted; depth+1
   - `calls/delegatecall.rs` — same as call but keep caller context
   - `calls/staticcall.rs` — same as call but no state changes
   - `context/block_context.rs` — returns: `(height, timestamp, coinbase, gas_limit)`; gas: 3
   - `context/gas_left.rs` — returns: `u64` (remaining gas); gas: 3
   - `logs/emit_log.rs` — input: `topics: Vec<[u8; 32]>, data: Vec<u8>`, no return; gas: 375 + 375 per topic
2. Каждая host function: input validation (sizes, types); gas deduction via
   `GasCounter::spend_host_call`; deterministic (no time/random/I/O).
3. Wasmi `imports` — register all host functions in wasmi `Linker`.
4. Тесты: каждая host function — unit test (valid + invalid input + gas deduction).

**Артефакты:**
- `crates/strangecoin-vm-wasm/src/host/mod.rs` (Linker registration)
- `crates/strangecoin-vm-wasm/src/host/crypto/{secp256k1_verify,bls_verify,blake3,keccak,sha256}.rs`
- `crates/strangecoin-vm-wasm/src/host/storage/{storage_read,storage_write}.rs`
- `crates/strangecoin-vm-wasm/src/host/calls/{call,delegatecall,staticcall}.rs`
- `crates/strangecoin-vm-wasm/src/host/context/{block_context,gas_left}.rs`
- `crates/strangecoin-vm-wasm/src/host/logs/emit_log.rs`
- `crates/strangecoin-vm-wasm/tests/host_functions.rs`

**КГ (чек-лист):**
- [ ] Все 13 host functions реализованы (12 + `bls_verify` NotImplemented)
- [ ] Каждая host function: input validation + gas deduction + determinism
- [ ] Wasmi `Linker` регистрирует все host functions
- [ ] `bls_verify` returns `NotImplemented` (no BLS12-381 dep)
- [ ] `call`/`delegatecall`/`staticcall` увеличивают depth (см. S1.5-P13 max_call_depth)
- [ ] Unit tests: ≥3 теста на host function (valid + invalid + gas)
- [ ] Коммит `[S1.5-P10] vm-wasm: host functions (crypto + storage + calls + context + logs)`

---

### S1.5-P11. Precompiles implementation

**Цель:** реализовать 7 precompiles из ADR-0014: `ecrecover`, `sha256`, `keccak`,
`blake3`, `ed25519_verify`, `bls_aggregate` (NotImplemented), `modexp`. Precompiles
— встроенные контракты по addresses `0x01..0x07`, доступны через `call(precompile_address,
data)` из контракта или напрямую через `CallTx`.

**Контекст:** ADR-0014 (S1.5-P04) зафиксировал список и gas costs. Precompiles — это
частный случай `call`, где `callee` — зарезервированный address `0x01..0x07`. Реализация:
в `WasmiExecutor::execute` перед instantiate, если `callee` — precompile address,
dispatch в precompile impl, skip WASM execution. Каждая precompile — отдельный модуль
с input/output format из ADR-0014.

**Задачи:**

1. `crates/strangecoin-vm-wasm/src/precompiles/` — один файл на precompile:
   - `0x01_ecrecover.rs` — secp256k1 recovery: input `[hash (32), v (1), r (32), s (32)]`,
     returns `[pubkey (33) or empty]`; gas: 3000
   - `0x02_sha256.rs` — input: `Vec<u8>`, returns: `[u8; 32]`; gas: 50 + 12 per 32-byte word
   - `0x03_keccak.rs` — input: `Vec<u8>`, returns: `[u8; 32]`; gas: 30 + 6 per 32-byte word
   - `0x04_blake3.rs` — input: `Vec<u8>`, returns: `[u8; 32]`; gas: 50 + 6 per 64-byte chunk
   - `0x05_ed25519_verify.rs` — input: `[pubkey (32), signature (64), msg]`, returns: `bool`; gas: 2000
   - `0x06_bls_aggregate.rs` — returns `NotImplemented` до Stage 7; gas: 12000 (если вызвать — error)
   - `0x07_modexp.rs` — input: `[Bsize, Esize, Msize, B, E, M]`, returns: `Vec<u8>`; gas: 50 + 50 per byte
2. `dispatch_precompile(callee: Address, data: Vec<u8>) -> Result<Vec<u8>, VmError>` —
   router, вызывается из `WasmiExecutor::execute` перед WASM instantiation.
3. Тесты: каждая precompile — unit test (valid input + invalid input + gas deduction +
   NotImplemented для bls_aggregate).

**Артефакты:**
- `crates/strangecoin-vm-wasm/src/precompiles/mod.rs` (dispatcher)
- `crates/strangecoin-vm-wasm/src/precompiles/{ecrecover,sha256,keccak,blake3,ed25519_verify,bls_aggregate,modexp}.rs`
- `crates/strangecoin-vm-wasm/tests/precompiles.rs`

**КГ (чек-лист):**
- [ ] Все 7 precompiles реализованы (6 + `bls_aggregate` NotImplemented)
- [ ] `dispatch_precompile` router работает (call по `0x01` → ecrecover)
- [ ] `WasmiExecutor::execute` dispatchает в precompiles перед WASM
- [ ] Каждая precompile: input validation + gas deduction + correct output
- [ ] `bls_aggregate` returns `NotImplemented` (no BLS12-381 dep)
- [ ] Unit tests: ≥3 теста на precompile
- [ ] Коммит `[S1.5-P11] vm-wasm: precompiles (ecrecover, sha256, keccak, blake3, ed25519, bls, modexp)`

---

### 2.6. ABI + reentrancy + limits + events (S1.5-P12–S1.5-P14)

ABI — интерфейс контракта (functions, events, errors); reentrancy guard — защита от
классической атаки (threat V-18); limits — anti-DoS (threat V-20 storage exhaustion,
V-19 integer overflow); events/logs — immutable receipts (инвариант №18).

---

### S1.5-P12. JSON ABI: encode/decode + WIT migration placeholder

**Цель:** реализовать JSON ABI (ADR-0012) для MVP: канонический encode/decode аргументов
(method selector + ABI-encoded args), `format_version: "json-v1"` обязательное поле,
WIT migration placeholder (просто `format_version: "wit-v2"` reserved, не реализовано).

**Контекст:** ADR-0012 (S1.5-P02) зафиксировал формат. ABI — JSON-документ, описывающий
interface контракта: `{ format_version, name, functions: [...], events: [...], errors: [...] }`.
Method selector — `blake3("function_name(types)")` первые 4 байта (как Ethereum, но
blake3 вместо keccak). Args encode — канонический binary format (length-prefixed для
dynamic types, fixed для static). WIT migration — через `format_version` field
(activation height на Stage 4+).

**Задачи:**

1. `crates/strangecoin-core/src/abi/mod.rs` — ABI types:
   - `AbiDocument { format_version, name, functions: Vec<AbiFunction>, events: Vec<AbiEvent>, errors: Vec<AbiError> }`
   - `AbiFunction { name, inputs: Vec<AbiParam>, outputs: Vec<AbiParam>, state_mutability }`
   - `AbiParam { name, type: AbiType }`
   - `AbiType` enum: `Uint(bits), Int(bits), Bool, Address, Bytes, String, Array(Box, len), FixedArray(Box, len), Tuple(Vec<AbiType>)`
2. `crates/strangecoin-core/src/abi/encode.rs` — `encode_args(params: &[AbiParam], values: &[AbiValue]) -> Vec<u8>`:
   - Static types: fixed-size encoding (Uint → 32 bytes big-endian, Bool → 32 bytes, Address → 32 bytes)
   - Dynamic types: length-prefixed (4-byte length + data)
   - Head/tail split (как Ethereum ABI)
3. `crates/strangecoin-core/src/abi/decode.rs` — `decode_args(params: &[AbiParam], data: &[u8]) -> Result<Vec<AbiValue>, AbiError>`
4. `method_selector(name: &str, params: &[AbiParam]) -> [u8; 4]` — `blake3(format!("{}({})", name, types))` первые 4 байта.
5. `format_version` migration: `"json-v1"` (current), `"wit-v2"` (reserved, returns `NotImplemented`).
6. Тесты: round-trip encode/decode для каждого AbiType; method selector stable (golden vector);
   `format_version: "wit-v2"` returns `NotImplemented`.

**Артефакты:**
- `crates/strangecoin-core/src/abi/mod.rs` (AbiDocument + AbiFunction + AbiParam + AbiType)
- `crates/strangecoin-core/src/abi/encode.rs` (encode_args)
- `crates/strangecoin-core/src/abi/decode.rs` (decode_args)
- `crates/strangecoin-core/src/abi/selector.rs` (method_selector)
- `crates/strangecoin-core/tests/abi_roundtrip.rs`

**КГ (чек-лист):**
- [ ] `AbiDocument` с `format_version` обязательным полем
- [ ] `AbiType` enum с ≥9 variants (Uint, Int, Bool, Address, Bytes, String, Array, FixedArray, Tuple)
- [ ] `encode_args`/`decode_args` round-trip для каждого типа
- [ ] `method_selector` — blake3-based, 4 bytes, golden vector (stable)
- [ ] `format_version: "wit-v2"` returns `NotImplemented`
- [ ] Коммит `[S1.5-P12] core/abi: JSON ABI encode/decode + method_selector + WIT placeholder`

---

### S1.5-P13. Reentrancy guard + Limits (code/storage/depth/stack/memory)

**Цель:** реализовать reentrancy guard (threat V-18) и limits (threat V-19, V-20):
`reentrant: bool` в `ExecutionContext`, `nonReentrant` modifier pattern, `max_code_size`
(24KB), `max_storage_per_contract` (1MB), `max_call_depth` (1024), `max_stack_size`,
`max_memory_size`. Все limits — из ARCHITECT3 §3.9.

**Контекст:** ARCHITECT3 §3.9: "Reentrancy guard, лимиты на размер кода/хранилища/стек/память".
Threat V-18: контракта вызывает сам себя (reentrancy) — mitigation: `reentrant: bool` flag
в `ExecutionContext`; `nonReentrant` pattern (function decorator-like) — host function
`call` проверяет, не является ли callee == caller (если `reentrant: true` — `ReentrancyDetected`).
Threat V-20: storage exhaustion — mitigation: `max_storage_per_contract` (1MB), enforced
в `storage_write`. Threat V-19: integer overflow — Rust checked arithmetic (default), но
explicit checked operations в WASM-контрактах через precompiles `modexp`.

**Задачи:**

1. `ExecutionContext.reentrant: bool` — установлен в `true` если caller == callee (или
   если callee — caller of caller in same call stack); checked в host function `call`.
2. `nonReentrant` pattern: контракт помечает function как `non_reentrant` в ABI; при
   `call` с `non_reentrant` flag — `reentrant: true` → `ReentrancyDetected` error.
3. `max_code_size: 24 * 1024` (24 KB) — enforced в `CreateTx` validation (init_code.len()).
4. `max_storage_per_contract: 1024 * 1024` (1 MB) — enforced в `storage_write` (sum of
   value sizes for contract ≤ 1MB).
5. `max_call_depth: 1024` — enforced в `call`/`delegatecall`/`staticcall` (depth+1 > 1024 →
   `MaxCallDepth` error).
6. `max_stack_size` (wasmi config) — enforced в wasmi engine config.
7. `max_memory_size` (wasmi config, e.g. 16 pages × 64KB = 1MB) — enforced в wasmi engine
   config; `memory.grow` beyond limit → `OutOfMemory`.
8. Тесты: reentrancy attack (contract calls itself) → rejected; code > 24KB → rejected;
   storage > 1MB → rejected; call depth > 1024 → rejected; memory > 1MB → rejected.

**Артефакты:**
- `crates/strangecoin-core/src/vm/context.rs` (reentrant field)
- `crates/strangecoin-vm-wasm/src/host/calls/call.rs` (reentrancy check + depth check)
- `crates/strangecoin-vm-wasm/src/host/storage/storage_write.rs` (max_storage_per_contract)
- `crates/strangecoin-core/src/consensus.rs` (max_code_size in CreateTx validation)
- `crates/strangecoin-vm-wasm/src/lib.rs` (wasmi engine config: max_stack_size, max_memory_size)
- `crates/strangecoin-vm-wasm/tests/reentrancy_and_limits.rs`

**КГ (чек-лист):**
- [ ] `ExecutionContext.reentrant: bool` field
- [ ] `nonReentrant` pattern работает (function flag в ABI)
- [ ] Reentrancy attack → `ReentrancyDetected`
- [ ] `max_code_size` 24KB enforced в CreateTx
- [ ] `max_storage_per_contract` 1MB enforced в storage_write
- [ ] `max_call_depth` 1024 enforced в call/delegatecall/staticcall
- [ ] `max_stack_size` enforced (wasmi config)
- [ ] `max_memory_size` enforced (wasmi config, default 1MB)
- [ ] Тесты для каждого limit — green
- [ ] Коммит `[S1.5-P13] vm-wasm: reentrancy guard + limits (code/storage/depth/stack/memory)`

---

### S1.5-P14. Events/logs: Log + Receipt

**Цель:** реализовать `Log { address, topics, data }` и `Receipt { gas_used, logs, status }`
(инвариант №18: receipts неизменны). `emit_log` host function (S1.5-P10) добавляет Log в
`ExecutionResult.logs`; `block_executor` собирает receipts из всех tx в блоке.

**Контекст:** ARCHITECT3 §3.9: "Events/logs: `Log { address, topics, data }` в receipt;
`Receipt { gas_used, logs, status }`; receipts неизменны (инвариант №18)". `Log` —
immutable record of contract event; `topics` — indexed fields (для быстрого поиска в
explorer); `data` — non-indexed fields. `Receipt` — per-tx summary; immutable (если
block finalized — receipts нельзя изменить, даже при reorg они пересчитываются из
chain). Инвариант №18 — впервые enforce на Stage 1.5.

**Задачи:**

1. `crates/strangecoin-core/src/vm/result.rs` — `Log` и `Receipt`:
   ```rust
   pub struct Log {
       pub address: Address,
       pub topics: Vec<[u8; 32]>,  // max 4 topics (Ethereum-style)
       pub data: Vec<u8>,
   }
   pub enum TxStatus { Success, Reverted, OutOfGas, Failed }
   pub struct Receipt {
       pub txid: [u8; 32],
       pub gas_used: u64,
       pub logs: Vec<Log>,
       pub status: TxStatus,
       pub block_height: u64,
       pub tx_index: u32,
   }
   ```
2. `emit_log` host function (S1.5-P10) — добавляет Log в `ExecutionResult.logs`.
3. `block_executor::commit_block` — собирает receipts из всех tx; хранит в `state.receipts`
   (отдельная sub-trie в SMT, чтобы receipts были verifiable).
4. `max_topics: 4` — enforced в `emit_log`.
5. Receipts immutability: после `commit_block` — receipts нельзя изменить (only via reorg
   → пересчёт из chain).
6. Тесты: contract emits Log → Log в Receipt; receipts immutable (tamper attempt → reject);
   max_topics 4 enforced; receipts verifiable via SMT root.

**Артефакты:**
- `crates/strangecoin-core/src/vm/result.rs` (Log + Receipt + TxStatus)
- `crates/strangecoin-vm-wasm/src/host/logs/emit_log.rs` (max_topics check)
- `src/blockchain/block_executor.rs` (receipts collection)
- `crates/strangecoin-core/src/state/inner.rs` (receipts sub-trie)
- `crates/strangecoin-vm-wasm/tests/events_and_receipts.rs`

**КГ (чек-лист):**
- [ ] `Log` с `address`, `topics` (max 4), `data`
- [ ] `Receipt` с `txid`, `gas_used`, `logs`, `status`, `block_height`, `tx_index`
- [ ] `TxStatus` enum (Success, Reverted, OutOfGas, Failed)
- [ ] `emit_log` добавляет Log в `ExecutionResult.logs`
- [ ] `block_executor::commit_block` собирает receipts
- [ ] `max_topics: 4` enforced
- [ ] Receipts immutable (инвариант №18)
- [ ] Receipts verifiable via SMT root (receipts sub-trie)
- [ ] Тесты — green
- [ ] Коммит `[S1.5-P14] vm-wasm: events/logs (Log + Receipt) + инвариант #18`

---

### 2.7. State integration + block gas (S1.5-P15)

Contract storage в общем state trie (Sparse Merkle Tree) — главный интеграционный
промпт. Без него контракты изолированы; с ним — `state.root_after(block) ==
block.state_root` (инвариант №19) покрывает и contract storage.

---

### S1.5-P15. Contract storage в SMT + block gas limit enforcement

**Цель:** интегрировать contract storage в общий Sparse Merkle Tree. `state.apply_call`
обновляет storage trie контракта; `state.root_after(block) == block.state_root` (инвариант
№19) — extension до contract storage. Block gas limit 1M (инвариант №15) enforced в
`block_executor::validate_and_apply`.

**Контекст:** На Stage 1 (BUG-S1-011 closed) — SMT depth 256 для account state. На Stage 1.5 —
contract storage overlay: каждый контракт имеет свой sub-SMT (root хранится в
`ContractAccount.storage_root`). `storage_write` — добавляет в overlay; `storage_read` —
читает из overlay (если есть) или из SMT (если нет). После `commit_block` — overlay
flush в SMT; `ContractAccount.storage_root` обновляется; global state_root пересчитывается.

**Задачи:**

1. `crates/strangecoin-core/src/state/contract_storage.rs` — `ContractStorageOverlay`:
   - `HashMap<[u8; 32], Option<Vec<u8>>>` (key → Some(value) for write, None for delete)
   - `apply_to_smt(smt: &mut SparseMerkleTrie)` — flush overlay в SMT
   - `root(&self) -> [u8; 32]` — вычислить SMT root из overlay + parent SMT
2. `state::apply_call(call: CallTx, ctx: ExecutionContext) -> ExecutionResult`:
   - Get `ContractAccount` for callee
   - Load contract storage SMT (sub-trie rooted at `storage_root`)
   - Execute WASM with overlay
   - Flush overlay в SMT, update `storage_root`
   - Update global state SMT (contract account record)
3. `state::root_after(block) == block.state_root` — extension: теперь включает contract
   storage_roots в `ContractAccount` records.
4. Block gas limit (S1.5-P09) — `sum(tx.gas_used) <= 1_000_000` enforced в
   `validate_and_apply` (инвариант №15).
5. `unapply_call` (для reorg) — reverse overlay, restore `storage_root`.
6. Тесты: deploy contract → call → storage_write → `storage_root` changes → global
   `state_root` changes (инвариант №19); reorg → storage_root restored; block > 1M gas →
   reject (инвариант №15).

**Артефакты:**
- `crates/strangecoin-core/src/state/contract_storage.rs` (ContractStorageOverlay)
- `crates/strangecoin-core/src/state/mod.rs` (apply_call + root_after extension)
- `crates/strangecoin-core/src/state/inner.rs` (ContractAccount storage_root update)
- `src/blockchain/block_executor.rs` (block gas limit enforcement, инвариант №15)
- `crates/strangecoin-core/tests/contract_storage_smt.rs`

**КГ (чек-лист):**
- [ ] `ContractStorageOverlay` с `apply_to_smt` и `root`
- [ ] `state::apply_call` — end-to-end (deploy + call + storage update)
- [ ] `state::root_after(block) == block.state_root` — extension для contract storage
- [ ] `unapply_call` — reverse overlay (reorg support)
- [ ] Block gas limit 1M enforced (инвариант №15)
- [ ] Тест: deploy → call → storage_root changes → global state_root changes
- [ ] Тест: reorg → storage_root restored
- [ ] Тест: block > 1M gas → `BlockGasLimitExceeded`
- [ ] Коммит `[S1.5-P15] core/state: contract storage in SMT + block gas limit + инварианты #15/#19`

---

### 2.8. Fuzzing + differential testing (S1.5-P16)

ARCHITECT3 §6 threat V-21: "malicious WASM; mitigation: wasmi sandbox, host functions
whitelist, gas metering". Fuzzing — единственный способ найти edge cases в gas metering,
host functions, storage trie operations. Differential testing (wasmi vs wasmtime) —
validates wasmi correctness.

---

### S1.5-P16. Fuzzing harness + differential testing wasmi vs wasmtime

**Цель:** реализовать fuzzing harness для host functions, gas metering, storage trie
operations. Differential testing: эталонный интерпретатор wasmtime vs wasmi — тот же
WASM bytecode, тот же input → тот же output. Подготовка для Stage 6 (formal verification
с K-framework).

**Контекст:** На Stage 0/1 — один fuzz-target (canonical-decode, 600s на CI, 25.9M
inputs / 0 panics). Для Stage 1.5 нужно: (1) fuzz-target для host functions (random
input → assert no panic + deterministic output); (2) fuzz-target для gas metering
(random WASM → assert `OutOfGas` корректно); (3) fuzz-target для storage trie (random
ops → assert root deterministic). Differential testing: wasmtime — production-grade JIT
runtime, эталон; wasmi — наш runtime; если результаты расходятся — bug в wasmi или в
нашем integration.

**Задачи:**

1. `fuzz/fuzz_targets/host_functions.rs` — cargo-fuzz target: random host function call
   with random input → assert no panic + deterministic output (same input → same output).
2. `fuzz/fuzz_targets/gas_metering.rs` — cargo-fuzz target: random WASM bytecode with
   random gas_limit → assert either `OutOfGas` or `Success` (no panic, no infinite loop).
3. `fuzz/fuzz_targets/storage_trie.rs` — cargo-fuzz target: random `storage_read`/
   `storage_write` sequence → assert root deterministic (same sequence → same root).
4. `fuzz/fuzz_targets/differential_wasmi_vs_wasmtime.rs` — generate random WASM, run
   in wasmi and wasmtime, assert same output (or both fail with same error).
5. CI: extend `fuzz-canonical-decode` job — add 3 new targets, each 600s on Linux runner.
6. `fuzz/README.md` — document fuzzing strategy, coverage, results.

**Артефакты:**
- `fuzz/fuzz_targets/{host_functions,gas_metering,storage_trie,differential_wasmi_vs_wasmtime}.rs`
- `fuzz/Cargo.toml` (4 new targets)
- `.github/workflows/ci.yml` (extended `fuzz-canonical-decode` job — 4 targets × 600s)
- `fuzz/README.md` (strategy + coverage + results)

**КГ (чек-лист):**
- [ ] 4 fuzz-targets созданы (host_functions, gas_metering, storage_trie, differential)
- [ ] CI `fuzz-canonical-decode` job extended (4 targets × 600s on Linux)
- [ ] 10-min local soak для каждого target — 0 panics
- [ ] Differential testing: wasmi vs wasmtime — same output (or both fail)
- [ ] `fuzz/README.md` документирует стратегию + coverage + результаты
- [ ] Коммит `[S1.5-P16] fuzz: 4 new targets (host_functions, gas, storage, differential wasmi vs wasmtime)`

---

### 2.9. Верификация и закрытие Stage 1.5 (S1.5-P17–S1.5-P19)

Последние 3 промпта — закрытие Stage 1.5: интеграционные тесты (end-to-end), регрессия
инвариантов + threat model актуализация, DoD-верификация + тег `v1.5.0-stage1.5`.
После тега — Stage 2 (network) и Stage 3 (RocksDB) могут стартовать.

---

### S1.5-P17. Интеграционные тесты Stage 1.5: end-to-end deploy + call + transfer

**Цель:** end-to-end интеграционные тесты для Stage 1.5: deploy contract → call method →
emit event → verify receipt → transfer SC между EOA и contract. Все сценарии — на regtest
(chain_id=3), temp dirs, без `sleep` > 1 сек.

**Контекст:** S1.5-P07 (Account model), S1.5-P08 (Tx types), S1.5-P10 (host functions),
S1.5-P11 (precompiles), S1.5-P12 (ABI), S1.5-P15 (state integration) — все unit-tested.
Но integration gaps: deploy+call+event+receipt — одна цепочка, без unit-теста. Также:
reentrancy attack end-to-end (deploy malicious contract → call → verify rejected).

**Задачи:**

1. `tests/vm_deploy_call.rs` — end-to-end:
   - Deploy contract (CreateTx) — assert contract account created, code_hash matches
   - Call method (CallTx) — assert return value, gas_used, no errors
   - Emit event — assert Log in Receipt, topics match
   - Storage update — assert `storage_root` changes, `state_root` changes (инвариант №19)
2. `tests/vm_transfer_to_contract.rs` — transfer SC to contract address:
   - TransferTx to contract address — assert contract.balance incremented
   - Call contract method that uses balance — assert correct balance read
3. `tests/vm_reentrancy_attack.rs` — malicious contract that tries to reenter:
   - Deploy malicious contract — ok
   - Call method that tries to reenter — assert `ReentrancyDetected` error
4. `tests/vm_gas_exhaustion.rs` — contract with heavy computation:
   - CallTx with gas_limit=100000 — assert `OutOfGas` after ~100k gas
   - CallTx with gas_limit=2_000_000 — assert `BlockGasLimitExceeded` (per-block 1M)
5. `tests/vm_precompiles_integration.rs` — call each precompile from contract:
   - call(0x01, ecrecover data) — assert correct pubkey recovered
   - call(0x06, bls_aggregate data) — assert `NotImplemented`

**Артефакты:**
- `tests/{vm_deploy_call,vm_transfer_to_contract,vm_reentrancy_attack,vm_gas_exhaustion,vm_precompiles_integration}.rs`
- `tests/common/vm_helpers.rs` (compile WASM, deploy, call helpers)

**КГ (чек-лист):**
- [ ] 5 integration test files созданы, green
- [ ] `vm_deploy_call.rs` — end-to-end (deploy → call → event → receipt → storage)
- [ ] `vm_transfer_to_contract.rs` — transfer to contract works
- [ ] `vm_reentrancy_attack.rs` — attack rejected
- [ ] `vm_gas_exhaustion.rs` — `OutOfGas` + `BlockGasLimitExceeded`
- [ ] `vm_precompiles_integration.rs` — all 7 precompiles callable from contract
- [ ] Все тесты на regtest (chain_id=3), temp dirs, без `sleep` > 1 сек
- [ ] `cargo test --workspace` green, < 180 сек
- [ ] Коммит `[S1.5-P17] tests: vm integration (deploy/call/transfer/reentrancy/gas/precompiles)`

---

### S1.5-P18. Регрессия 22 инвариантов + threat model актуализация

**Цель:** обновить `docs/stage1/INVARIANTS_ENFORCED.md` для Stage 1.5: инварианты #15
(block gas limit), #18 (receipts) — теперь enforced; инвариант №19 (state root) —
extension до contract storage. Обновить `docs/security/THREAT_MODEL.md` для новых
векторов (reentrancy V-18, integer overflow V-19, storage exhaustion V-20, code injection
V-21) — теперь митигированы.

**Контекст:** На Stage 0 (BUG-S1-014 closed) — INVARIANTS_ENFORCED.md = 17/5/0. На Stage 1
(S1-P20) — все 22 enforced (после S1-P06 state root, S1-P05 consensus version). На
Stage 1.5 — #15 (gas limit), #18 (receipts) — впервые enforced; #19 (state root) —
extension до contract storage; остальные — без регрессии. Threat model: V-18 (reentrancy),
V-19 (integer overflow), V-20 (storage exhaustion), V-21 (code injection) — теперь
митигированы (см. S1.5-P10, S1.5-P13, S1.5-P15).

**Задачи:**

1. Создать `docs/stage1.5/INVARIANTS_ENFORCED.md`:
   - Таблица 1:1 ARCHITECT3 §5, столбцы "Stage 0 enforced / Stage 1 enforced / Stage 1.5 enforced"
   - #15 (block gas limit) — теперь enforced (`block_executor::validate_and_apply`,
     S1.5-P09, S1.5-P15)
   - #18 (event log / Receipt) — теперь enforced (`emit_log`, `Receipt`, S1.5-P14)
   - #19 (state root match) — extension: теперь покрывает contract storage (S1.5-P15)
   - Остальные — без изменений, regression green
2. Обновить `docs/security/THREAT_MODEL.md`:
   - V-18 Reentrancy — mitigation: `reentrant: bool` + `nonReentrant` pattern (S1.5-P13);
     тест `tests/vm_reentrancy_attack.rs`
   - V-19 Integer overflow — mitigation: Rust checked arithmetic + `modexp` precompile (S1.5-P11)
   - V-20 Storage exhaustion — mitigation: `max_storage_per_contract` 1MB (S1.5-P13)
   - V-21 Code injection — mitigation: wasmi sandbox + host functions whitelist + gas
     metering (S1.5-P06, S1.5-P09, S1.5-P10, S1.5-P16)
3. Residual risks для Stage 2+:
   - Block-STM (parallel execution) — Stage 3+ (anti-goal Stage 1.5)
   - Account Abstraction — Stage 5
   - EIP-1559 dynamic base fee — Stage 5 (сейчас фиксированный gas limit)
4. Тесты: regression матрица (Stage 0/1 tests + Stage 1.5 tests) — все green.

**Артефакты:**
- `docs/stage1.5/INVARIANTS_ENFORCED.md` (22/22/22, столбцы Stage 0/1/1.5)
- `docs/security/THREAT_MODEL.md` (updated V-18..V-21 mitigations)
- `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` (резюме Stage 1.5-specific threats)

**КГ (чек-лист):**
- [ ] `docs/stage1.5/INVARIANTS_ENFORCED.md` создан (22/22/22)
- [ ] Инвариант #15 — enforced (block gas limit)
- [ ] Инвариант #18 — enforced (Receipts)
- [ ] Инвариант #19 — extension для contract storage
- [ ] THREAT_MODEL.md — V-18..V-21 обновлены с mitigation ссылками
- [ ] Residual risks для Stage 2+ — перечислены (Block-STM, AA, EIP-1559)
- [ ] Regression: Stage 0/1/1.5 tests all green
- [ ] Коммит `[S1.5-P18] docs: invariants 22/22/22 + threat model stage 1.5 update`

---

### S1.5-P19. DoD-верификация Stage 1.5 + тег v1.5.0-stage1.5

**Цель:** независимая сверка DoD из `ROADMAP3.md` Этап 1.5. Поставить тег
`v1.5.0-stage1.5` — **Gate Stage 2/3/4**: после этого промпта Stage 2 (network crate
migration) и Stage 3 (RocksDB) могут стартовать; Stage 4 (dev-experience: SDK, devnet,
indexer, JSON-RPC eth_*) требует Stage 1.5 завершённым.

**Контекст:** ROADMAP3.md Этап 1.5 DoD — 14 критериев: crates/strangecoin-vm-wasm
создан; все host functions реализованы и протестированы; gas metering работает; tx
types transfer/create/call работают; precompiles реализованы; JSON ABI round-trip;
reentrancy guard работает; limits enforced; events/logs работают; contract storage в
Verkle Trie; differential testing проходит; all Stage 0+1 invariants still enforced;
ADR-0011..0014 написаны; Changelog обновлён.

**Задачи:**

1. Создать `docs/stage1.5/STAGE1_5_SUMMARY.md` — 14 DoD критериев с ✅/🟡/❌ +
   evidence (ссылки на коммиты/тесты).
2. Обновить `Changelog.md`: секция `1.5.0 — WASM smart contracts (basic)` со списком
   изменений.
3. Обновить `AGENTS.md`: убрать "Stage 1.5 is complete" до фактического прохождения
   этого промпта (вернуть формулировку только после тега).
4. Обновить `Cargo.toml` version: `1.1.0 → 1.5.0`.
5. Прогнать `cargo test --workspace` — все тесты green (включая Stage 0/1/1.5);
   `cargo clippy -- -D warnings` green.
6. Поставить аннотированный тег `v1.5.0-stage1.5` — строго ПОСЛЕ прохождения всех
   пунктов КГ этого промпта.
7. Обязательства для Stage 2+ (зафиксировать в STAGE1_5_SUMMARY.md):
   - Block-STM parallel execution — Stage 3+ (anti-goal Stage 1.5)
   - EIP-1559 dynamic base fee — Stage 5
   - Account Abstraction — Stage 5
   - WIT ABI migration — Stage 4+
   - BLS12-381 dependency + `bls_aggregate` precompile impl — Stage 7
   - K-framework formal verification — Stage 6
   - ZK-VM integration — Stage 6+ (research)
   - 2 ops-gate из Stage 1 (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) —
     остаются human ops gates для mainnet freeze; не код

**Артефакты:**
- `docs/stage1.5/STAGE1_5_SUMMARY.md` (14 DoD критериев)
- `docs/stage1.5/INVARIANTS_ENFORCED.md` (22/22/22 — см. S1.5-P18)
- `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` (см. S1.5-P18)
- `Changelog.md` (секция 1.5.0)
- `AGENTS.md` (updated)
- `Cargo.toml` (version 1.5.0)
- Git tag `v1.5.0-stage1.5`

**КГ (чек-лист):**
- [ ] `docs/stage1.5/STAGE1_5_SUMMARY.md` — 14 DoD критериев с ✅ + evidence
- [ ] `crates/strangecoin-vm-wasm/` создан, wasmi интегрирован (S1.5-P06)
- [ ] Все 13 host functions реализованы и протестированы (S1.5-P10)
- [ ] Gas metering работает (S1.5-P09)
- [ ] Tx types transfer/create/call работают (S1.5-P08)
- [ ] 7 precompiles реализованы (6 + bls_aggregate NotImplemented) (S1.5-P11)
- [ ] JSON ABI round-trip работает (S1.5-P12)
- [ ] Reentrancy guard работает (S1.5-P13)
- [ ] Limits enforced (S1.5-P13)
- [ ] Events/logs работают (S1.5-P14)
- [ ] Contract storage в SMT (S1.5-P15)
- [ ] Differential testing проходит wasmi vs wasmtime (S1.5-P16)
- [ ] All Stage 0+1 invariants still enforced — no regressions (S1.5-P18)
- [ ] ADR-0011..0014 написаны (S1.5-P01..S1.5-P04)
- [ ] `Changelog.md` — секция 1.5.0
- [ ] `AGENTS.md` — updated
- [ ] `Cargo.toml` version = 1.5.0
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy -- -D warnings` green
- [ ] Git tag `v1.5.0-stage1.5` поставлен ПОСЛЕ прохождения чек-листа
- [ ] Коммит `[S1.5-P19] docs: stage1.5 dod verification + summary + tag v1.5.0-stage1.5`

---

## 3. Сводная таблица промптов

| ID | Заголовок | Ключевые артефакты | Зависимости |
|----|-----------|--------------------|-------------|
| S1.5-P01 | ADR-0011: WASM (wasmi) vs RISC-V | docs/ADR/0011-wasm-wasmi-vs-risc-v.md | (Gate: v1.1.0-stage1 + bugfixes closed) |
| S1.5-P02 | ADR-0012: JSON ABI vs WIT | docs/ADR/0012-json-abi-for-mvp-vs-wit.md | S1.5-P01 |
| S1.5-P03 | ADR-0013: Gas metering strategy | docs/ADR/0013-gas-metering-strategy.md | S1.5-P01 |
| S1.5-P04 | ADR-0014: Precompiles list | docs/ADR/0014-precompiles-list.md | S1.5-P01 |
| S1.5-P05 | VmExecutor trait + types в core | core/vm/{traits,context,result,error,gas}.rs, tests/vm_trait_mock.rs | S1.5-P01..P04 |
| S1.5-P06 | crates/strangecoin-vm-wasm/ + wasmi | crates/strangecoin-vm-wasm/{Cargo.toml,src/{lib,error}.rs}, Cargo.toml, main.rs | S1.5-P05 |
| S1.5-P07 | Account model: EOA + Contract | core/types.rs, core/state/{mod,inner}.rs, tests/account_model.rs | S1.5-P06 |
| S1.5-P08 | Tx types: transfer/create/call | core/{types,serialize,consensus}.rs, mempool/mod.rs, tests/tx_types.rs | S1.5-P07 |
| S1.5-P09 | Gas metering + gas_left() + block gas | vm-wasm/{lib,gas}.rs, host/gas_left.rs, block_executor.rs, tests/gas_metering.rs | S1.5-P03, S1.5-P06 |
| S1.5-P10 | Host functions (crypto + storage + calls + context + logs) | vm-wasm/src/host/**/*.rs (13 files), tests/host_functions.rs | S1.5-P06, S1.5-P09 |
| S1.5-P11 | Precompiles (7 шт) | vm-wasm/src/precompiles/*.rs (8 files), tests/precompiles.rs | S1.5-P10 |
| S1.5-P12 | JSON ABI encode/decode + WIT placeholder | core/abi/{mod,encode,decode,selector}.rs, tests/abi_roundtrip.rs | S1.5-P02, S1.5-P10 |
| S1.5-P13 | Reentrancy guard + Limits | vm/context.rs, host/calls/call.rs, host/storage/storage_write.rs, lib.rs, tests/reentrancy_and_limits.rs | S1.5-P07, S1.5-P10 |
| S1.5-P14 | Events/logs: Log + Receipt | core/vm/result.rs, host/logs/emit_log.rs, block_executor.rs, tests/events_and_receipts.rs | S1.5-P10 |
| S1.5-P15 | Contract storage в SMT + block gas | core/state/contract_storage.rs, state/{mod,inner}.rs, block_executor.rs, tests/contract_storage_smt.rs | S1.5-P07, S1.5-P09, S1.5-P14 |
| S1.5-P16 | Fuzzing harness + differential testing | fuzz/fuzz_targets/{host_functions,gas_metering,storage_trie,differential_wasmi_vs_wasmtime}.rs, ci.yml, fuzz/README.md | S1.5-P10, S1.5-P11 |
| S1.5-P17 | Интеграционные тесты Stage 1.5 | tests/vm_{deploy_call,transfer_to_contract,reentrancy_attack,gas_exhaustion,precompiles_integration}.rs, tests/common/vm_helpers.rs | S1.5-P08..P15 |
| S1.5-P18 | Регрессия 22 инвариантов + threat model | docs/stage1.5/{INVARIANTS_ENFORCED,THREAT_MODEL_STAGE1_5}.md, docs/security/THREAT_MODEL.md | S1.5-P17 |
| S1.5-P19 | DoD-верификация Stage 1.5 + тег v1.5.0-stage1.5 | docs/stage1.5/STAGE1_5_SUMMARY.md, Changelog.md, AGENTS.md, Cargo.toml, тег | S1.5-P18 |

Параллельные треки: (a) ADRs S1.5-P01→P02→P03∥P04 — все 4 ADR после P01; (b) Runtime
S1.5-P05→P06 — после ADRs; (c) Account/Tx S1.5-P07→P08 — после runtime; (d) Host functions
S1.5-P10→P11 — после runtime + gas; (e) ABI/Limits/Events S1.5-P12→P13→P14 — после host
functions + account model; (f) State integration S1.5-P15 — после account+gas+events;
(g) Fuzzing S1.5-P16 — после host functions + precompiles; (h) Verification S1.5-P17→P18→P19
— после всех proper промптов.

---

## 4. Покрытие Definition of Done Этапа 1.5 (ROADMAP3)

| # | Критерий DoD Этапа 1.5 (ROADMAP3) | Промпт | Evidence |
|---|-----------------------------------|--------|----------|
| 1 | `crates/strangecoin-vm-wasm/` создан, wasmi интегрирован | S1.5-P06 | Cargo.toml workspace member, wasmi pinned |
| 2 | Все host functions реализованы и протестированы | S1.5-P10 | tests/host_functions.rs (13 funcs × ≥3 tests) |
| 3 | Gas metering работает (test: `gas_left()` корректно уменьшается) | S1.5-P09 | tests/gas_metering.rs |
| 4 | Tx types `transfer/create/call` работают | S1.5-P08 | tests/tx_types.rs |
| 5 | Precompiles реализованы и протестированы | S1.5-P11 | tests/precompiles.rs (7 precompiles) |
| 6 | JSON ABI работает (test: round-trip encode/decode) | S1.5-P12 | tests/abi_roundtrip.rs |
| 7 | Reentrancy guard работает (test: malicious contract не может reenter) | S1.5-P13 | tests/reentrancy_and_limits.rs |
| 8 | Limits enforced (test: превышение → `OutOfGas`/`OutOfMemory`) | S1.5-P13 | tests/reentrancy_and_limits.rs |
| 9 | Events/logs работают (test: emitted log in receipt) | S1.5-P14 | tests/events_and_receipts.rs |
| 10 | Contract storage в Verkle Trie (test: `state.root_after(block) == block.state_root`) | S1.5-P15 | tests/contract_storage_smt.rs |
| 11 | Differential testing проходит (wasmi vs wasmtime, same result) | S1.5-P16 | fuzz/fuzz_targets/differential_wasmi_vs_wasmtime.rs |
| 12 | All Stage 0+1 invariants still enforced (no regressions) | S1.5-P18 | docs/stage1.5/INVARIANTS_ENFORCED.md (22/22/22) |
| 13 | ADR-0011..0014 написаны | S1.5-P01..P04 | docs/ADR/0011..0014-*.md |
| 14 | `Changelog.md` обновлён: `1.5.0 — WASM smart contracts (basic)` | S1.5-P19 | Changelog.md, Cargo.toml 1.5.0 |

Плюс обязательство для Stage 2+: тег `v1.5.0-stage1.5` (S1.5-P19) — Gate для Stage 2/3/4.

---

## 5. Покрытие инвариантов ARCHITECT3 §5 на Stage 1.5

| # | Инвариант | Статус на Stage 1.5 | Где enforce / тест |
|---|-----------|-------------------|--------------------|
| 1 | Валидность — из цепочки | Усилен (contract storage в chain) | state_cache::rebuild_from_chain, contract_storage_smt.rs (S1.5-P15) |
| 2 | Каждая tx подписана, sender == pubkey | Расширено (transfer/create/call) | consensus::verify_transaction per-variant (S1.5-P08) |
| 3 | `hash <= target` для каждого блока | Без изменений | regression (S1.5-P18) |
| 4 | `apply_block`/`unapply_block` — обратные чистые | Расширено (apply_call/unapply_call) | state::apply_call + unapply_call (S1.5-P15) |
| 5 | Хэш/подпись — на канонических байтах | Расширено (per-variant serialization) | serialize_transaction_signed per-variant (S1.5-P08) |
| 6 | Блок не содержит непроверимых tx/наград | Без изменений | regression (S1.5-P18) |
| 7 | Размеры сообщений/блоков ограничены | Усилено (size check в validate_and_apply) | block_executor (BUG-S1-006 closed), max_code_size (S1.5-P13) |
| 8 | Генезис детерминирован | Без изменений (chain_id from Config) | regression (S1.5-P18) |
| 9 | Секреты не пишутся на диск | Без изменений | regression (BUG-S1-015 closed code-side) |
| 10 | Replay protection (chain_id) | Расширено (per-variant) | tx types per-variant chain_id (S1.5-P08) |
| 11 | Nonce инкрементируется | Расширено (EOA + Contract) | Account model nonce (S1.5-P07) |
| 12 | Transaction hash = commitment | Расширено (per-variant) | serialize (S1.5-P08) |
| 13 | Mempool basic rules | Расширено (per-variant) | mempool::insert per-variant (S1.5-P08) |
| 14 | Graceful shutdown | Без изменений | regression (S1.5-P18) |
| **15** | **Block gas limit** | **Enforce впервые (Stage 1.5)** | block_executor::validate_and_apply (S1.5-P09, S1.5-P15) |
| 16 | P2P framing | Без изменений | regression (S1.5-P18) |
| 17 | Rate limiting | Без изменений (Stage 2 — расширение) | regression (S1.5-P18) |
| **18** | **Event log / Receipt** | **Enforce впервые (Stage 1.5)** | emit_log + Receipt (S1.5-P14) |
| **19** | **State root match** | **Extension (contract storage)** | state::apply_call + root_after (S1.5-P15) |
| 20 | Fee invariant | Deferred (Stage 5) | — (fixed fee proxy) |
| 21 | Consensus versioning | Без изменений | regression (S1.5-P18) |
| 22 | Reproducible builds | Без изменений (release pipeline работает) | regression (BUG-S1-001 closed) |

**Итог Stage 1.5:** 22/22 invariants enforced (3 новых: #15, #18, #19 extension; 1 deferred
до Stage 5: #20). Regression: S1.5-P18.

---

## 6. Примечания по использованию

1. **Порядок выполнения:** строго по карте зависимостей (§1). Гейт №1 — подтверждение тега
   `v1.1.0-stage1` и закрытия всех P0/P1 багов из `bugfixes-stage0.md` / `bugfixes-stage1.md`
   (см. предусловие в шапке: 14 код-багов закрыты, 2 ops-gate не блокируют). Гейт №2 —
   ADR-0011..0014 (S1.5-P01..04) — без них код Stage 1.5 proper не начинается. Гейт №3 —
   тег `v1.5.0-stage1.5` (S1.5-P19); без него Stage 2/3/4 не начинаются (мягкая зависимость
   для Stage 2, жёсткая для Stage 4).

2. **Размер промпта:** каждый промпт — ≤200 строк markdown, реализация помещается в
   контекст 32k-token агента. Если промпт кажется большим — делить на a/b (например,
   S1.5-P10a: crypto host functions; S1.5-P10b: storage + calls + context + logs).

3. **Контекст:** вставлять блок "Общий контекст" (§0) в начало каждого промпта при
   отправке агенту.

4. **Артефакты и измеримость:** прогресс = число созданных файлов / общее число ожидаемых.
   КГ обязательны: нельзя закрывать промпт без прохождения всех пунктов; если КГ не
   проходит — итерировать тот же промпт, не переходить к следующему.

5. **Процессные страховки из retro §8.1** (обязательны к применению):
   - 1 промпт = 1 атомарный коммит с ID промпта в сообщении;
   - Changelog/README/AGENTS.md обновляются только коммитом, в котором артефакты уже
     существуют;
   - "done" объявления — только через DoD-промпт S1.5-P19, никогда — через ощущение
     завершённости;
   - факты, которые агент не может проверить (CI-run, fuzz-статистика, differential
     testing), фиксируются как "не верифицировано + причина", а не молча опускаются.

6. **Связь с чек-листом ARCHITECT3 §17:** после S1.5-P19 прогнать чек-лист §17 и
   убедиться, что все применимые к Stage 1.5 пункты отмечены (включая "VM trait в core,
   runtime отдельно (wasmi)").

7. **Детерминизм — критический инвариант:** каждая host function (S1.5-P10) обязана
   быть детерминированной (no time, no random, no network I/O). Ревью каждой host
   function на детерминизм — обязательный пункт КГ (threat V-21).

8. **Wasmi version pinning:** wasmi = "=0.31.0" (не "^", не patch) — gas metering
   API может меняться между версиями; pin в Cargo.toml (S1.5-P06); обновление только
   через SCIP после mainnet freeze.

9. **Gas constant updates:** все gas constants из ADR-0013 (S1.5-P03) — hardcoded; после
   mainnet freeze их изменение — только через SCIP + activation height (не прямой
   commit). До mainnet — прямой коммит с обновлением ADR-0013.

10. **ECONOMICS.md §2.6 (Stage 1.5+):** после Stage 1.5 завершения — основатель
    становится единственным носителем глубокой экспертизы по ABI и внутренностям VM;
    это открывает канал дохода "разработка контрактов под ключ" + "платные интеграции
    SDK" (см. ECONOMICS.md §2.6). Не код-задача, но экономическое основание для
    приоритизации Stage 1.5.

11. **2 ops-gate из bugfixes-stage1.md** (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis
    key) — НЕ блокируют Stage 1.5 (regtest/devnet scope); становятся обязательными gates
    только для mainnet freeze. Зафиксировать в STAGE1_5_SUMMARY.md как residual ops gates.

---

## 7. Что НЕ входит в Stage 1.5 (явно отложено)

Следующие задачи появляются на более поздних стадиях; если промпт случайно затрагивает
их — остановить и переориентировать на Stage 1.5 scope:

- **Block-STM (parallel execution)** — Stage 3+ (ARCHITECT3 §15: "сначала корректная
  последовательная валидация (Stage 1.5), потом Block-STM (Stage 3+)");
- **Account Abstraction (social recovery, multisig, session keys, paymaster)** —
  Stage 5 (ARCHITECT3 §15: "AA на Stage 5 после VM и dev-experience");
- **EIP-1559 dynamic base fee** — Stage 5 (ARCHITECT3 §15: "фиксированный gas limit
  для MVP; EIP-1559 на Stage 5");
- **Multidimensional fees (EIP-7706 analog)** — Stage 6+ research;
- **WIT/Wasm Component Model для ABI** — Stage 4+ (сейчас только `format_version`
  placeholder, S1.5-P12);
- **K-framework формальная верификация WASM subset** — Stage 6 (ARCHITECT3 §3.9:
  "K-framework спецификация WASM subset");
- **ZK-VM integration (RISC Zero / SP1)** — Stage 6+ research;
- **State rent** — Stage 3+ опционально через activation height (ARCHITECT3 §15);
- **BLS12-381 dependency + `bls_aggregate` precompile impl** — Stage 7 (PoS-фаза;
  сейчас precompile возвращает `NotImplemented`, S1.5-P11);
- **Noise Protocol Framework, Erlay, compact blocks, gossip-оптимизации** — Stage 2;
- **RocksDB миграция** — Stage 3;
- **EncryptedMempool (threshold encryption) реализация** — Stage 5 (placeholder
  допустим);
- **SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet** — Stage 4 (требует VM
  завершённым, S1.5-P19);
- **PoS миграция, Casper FFG, BLS consensus signatures** — Stage 7;
- **External audit, formal verification, reproducible builds на Stage 1.5 scope** —
  Stage 6 (security track; сейчас только fuzzing harness, S1.5-P16);
- **Перенос legacy-threads майнинга в async** — Stage 2;
- **TLA+ формальная верификация VM** — Stage 6 (currently только consensus.tla,
  актуализация свойств — по мере новых инвариантов в S1.5-P18).
