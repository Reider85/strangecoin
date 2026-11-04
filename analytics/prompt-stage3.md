# prompt-stage3.md — Промпты для реализации Stage 3 (Хранение и масштабируемость)

**Версия:** 1.0
**Дата:** 2026-10-30
**Источник:** `ROADMAP3.md` (Этап 3 — Хранение и масштабируемость (RocksDB, stateless): цели, задачи, DoD, риски, ADRs required), `ARCHITECT3.md` (§3.3 stateless/witness, §3.4 state_cache, §3.7 storage/pruning/snapshot, §10.5 strangler Stage 3, §15 anti-goals, §17 PR-checklist), `prompt-stage2.md` (шаблон/формат/§0 контекст-блок/§1 карта зависимостей/§2 промпты Цель/Контекст/Задачи/Артефакты/КГ/Коммит; §6 процессные страховки; §7 anti-goals), `docs/ADR/0008-rocksdb-plan.md` (Accepted — RocksDB vs redb уже решён, Stage 3 конкретизирует схему), `docs/ADR/0006-verkle-trie-vs-smt.md`, `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (Stage 3 attack surface: snapshot sync, RocksDB crash-recovery, witness wire, Block-STM, pruning), `docs/stage2/STAGE2_SUMMARY.md` (Stage 2 DoD 12/12), `docs/stage1.5/INVARIANTS_ENFORCED.md` + `docs/stage2/INVARIANTS_ENFORCED.md` (22/22 invariants baseline), `src/storage/mod.rs` + `src/blockchain/state_cache.rs` (фактическая схема LevelDB: 4 JSON blob-ключа + mempool full-scan), `Cargo.toml` (root version 2.0.0, `rusty-leveldb = "4.0.0"`), `git ls-remote --tags origin` (подтверждение `v2.0.0-stage2` → `346e3c0`).

**Предусловие:** Stage 2 завершён и помечен тегом `v2.0.0-stage2` (S2-P24). Сверка по кодовой базе (HEAD `db116f9` на 2026-10-30, working tree clean):

- **Stage 2 closure:** 24/24 промптов S2-P01..S2-P24 закрыты; DoD 12/12 с evidence в `docs/stage2/STAGE2_SUMMARY.md`; invariants 22/22 (`docs/stage2/INVARIANTS_ENFORCED.md`, extensions #16/#17/#22); threat model v3.2 (`docs/stage2/THREAT_MODEL_STAGE2.md`); residuals зафиксированы (`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1 — Stage 3 attack surface: snapshot sync / RocksDB crash-recovery / witness wire / Block-STM / pruning).
- **Тег:** `v2.0.0-stage2` подтверждён локально и на origin (`git ls-remote --tags origin` → `refs/tags/v2.0.0-stage2`, target `346e3c0`). Тег-гейт Stage 3 **открыт** (жёсткий гейт №1 из `prompt-stage2.md` §3).
- **Workspace:** `Cargo.toml` members = `[".", "crates/strangecoin-core", "crates/strangecoin-vm-wasm", "crates/strangecoin-net"]` — `strangecoin-storage` **не создан** (Stage 3 не начат).
- **Version:** root `2.0.0`, `strangecoin-core = 2.0.0`, `strangecoin-vm-wasm = 0.2.0`, `strangecoin-net = 0.1.0` (S3-P15 поднимет root/core до `3.0.0`).
- **Storage baseline (verified):** `src/storage/mod.rs` (38 строк) — `Arc<Mutex<rusty_leveldb::DB>>`, get/put/new_iter/flush; схема: 4 константных JSON blob-ключа (`chain` = ВСЯ цепочка `Vec<Block>` одним blob'ом, `balances`, `contract_storage`, `difficulty`) + mempool-ключи `"{sender}:{nonce}"`; mempool — full-scan без delete; schema version отсутствует; column families отсутствуют; `config.storage.path` (`./data/leveldb`) — **мёртвый ключ** (фактический путь `<exe_dir>/blockchain_db_{port}`); `rusty-leveldb = "4.0.0"` (root only, не pinned); `crates/strangecoin-storage/` отсутствует; call-sites LevelDB — только `src/storage/mod.rs`, `src/blockchain/state_cache.rs`, `src/blockchain/blockchain_facade.rs`, `src/blockchain/chain_selector.rs`, `src/error.rs`, `tests/address_migration.rs`.
- **Witness baseline (verified):** core уже содержит `StateWitness`/`AccountProof`/`build_witness`/`verify_block_stateless` (`crates/strangecoin-core/src/state/witness.rs`, S1-P07, BUG-S1-003/004) — Stage 3 добавляет wire-формат + node-side light path + e2e test, НЕ изобретает witness с нуля.
- **ADR-0008 conflict resolution:** ROADMAP3 Этап 3 «ADRs required» планировал «ADR-0020: RocksDB vs redb», но ADR-0020..0023 заняты Stage 2, а сам выбор RocksDB уже зафиксирован и принят в **ADR-0008** (`docs/ADR/0008-rocksdb-plan.md`, разделы Decision + Alternatives включают redb/sled/keep-leveldb). **Stage 3 переименовывает свои ADR на ADR-0024..0026**; «RocksDB vs redb» НЕ дублируется — S3-P01 пишет ADR-0024 (schema & StorageEngine trait), который конкретизирует и амендит ADR-0008 (см. §2.1).

**Замечание про `BACKLOG3.md`:** файл `analytics/BACKLOG3.md` по-прежнему **не существует**. ROADMAP3.md Этап 3 («Задачи» + «Definition of Done» + «Риски» + «ADRs required») используется как de-facto Stage 3 backlog, аналогично Stage 2. Все ссылки на «бэклог Stage 3» означают `ROADMAP3.md` §«Этап 3 — Хранение и масштабируемость».

**Цель:** перенести storage в отдельный крейт `crates/strangecoin-storage/` с RocksDB backend (LevelDB удалена), ввести column families + schema versioning + O(1) индексы + batch writes + crash-recovery, stateless validation через wire-witnesses (light client проверяет блоки без полного state), state pruning (EIP-4444 analog, implementation-level) + archival mode, snapshot sync (file-based export/import с верификацией commitment'ов). **15 промптов со сквозной нумерацией S3-P01..S3-P15** — только Stage 3 proper work. Каждый промпт — самостоятельное задание для ИИ-агента, завершающееся конкретными артефактами и чек-листом КГ. После S3-P15 ставится тег `v3.0.0-stage3` — Gate для Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*, faucet) и Stage 5+ (fee market, AA).

---

## 0.1. Оценка предусловия (краткое заключение)

| Пункт | Статус | Evidence |
|-------|--------|----------|
| Тег `v2.0.0-stage2` на origin | ✅ | `git ls-remote --tags origin` → `2577f40… refs/tags/v2.0.0-stage2` → target `346e3c0` (annotated, S2-P24) |
| Stage 2 DoD 12/12 | ✅ | `docs/stage2/STAGE2_SUMMARY.md` §1 (evidence per criterion) |
| Stage 2 инварианты 22/22 | ✅ | `docs/stage2/INVARIANTS_ENFORCED.md` (3 extensions #16/#17/#22) |
| Stage 2 residuals задокументированы | ✅ | `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (§1 Stage 3 attack surface) |
| Workspace не содержит storage-крейта | ✅ | members = 4 крейта; `crates/strangecoin-storage/` отсутствует |
| LevelDB ещё в коде (start state) | ✅ | `rusty-leveldb = "4.0.0"` в root `Cargo.toml:16`; `src/storage/mod.rs` |
| ADR-0008 принят (RocksDB vs redb решён) | ✅ | `docs/ADR/0008-rocksdb-plan.md` Status: Accepted |
| SCIP-0001 / ops-gates BUG-S1-013/015 | остаются вне scope | mainnet freeze track; НЕ блокируют Stage 3 (regtest/devnet) |

**Итог:** все precondition'ы Stage 3 выполнены; тег-гейт открыт; ADR-слоты 0024..0026 свободны.

**Уроки Stage 2, перенесённые в Stage 3:**
1. ADR-слоты не перезаписывать — при конфликте переименовывать (Stage 2: 0016→0020; Stage 3: ROADMAP3-план 0020..0022 → фактические 0024..0026).
2. Все отклонения от первичной спеки фиксировать явно в разделе «Решение»/«Отклонения» каждого промпта (прецедент FBP03/FBP07, S2-P15..S2-P20).
3. Не-локально-верифицируемые факты (CI-run, cross-сборка rocksdb на 3 платформах, benchmarks) помечать как «не верифицировано + причина» (прецедент FBP08/FBP11).
4. Pin policy: все протокол/консенсус-зависимые crate — через `=` (Stage 2: snow/ed25519-dalek/hkdf; Stage 3: rocksdb — C++ dep с API churn + consensus-adjacent data layout).
5. Детерминизм: pruning/snapshot-триггеры — по block height, НЕ по wall clock (паттерн `tick_at` Stage 2); key-encoding — pure functions.
6. Windows host: C++ сборка rocksdb требует того же host-preamble (`C:\msys64\mingw64\bin` в PATH того же вызова), что и blake3/secp256k1/wasmtime; НЕ выставлять `CC_*`/`AR_*` (инвалидация fingerprints — AGENTS.md Common Gotchas).

---

## 0. Общий контекст (вставлять в начало каждого промпта)

```
Проект: Strangecoin, Rust, edition 2021, версия 2.0.0 (тег v2.0.0-stage2).
Структура (после Stage 2):
  Монолит: src/main.rs + src/{lib,consensus,economics,mempool,network,
    blockchain,storage,config,cli,wallet,address,error,events,api,gui}.rs +
    подпапки blockchain/{mod,block_executor,chain_selector,state_cache,
    blockchain_facade,consensus_manager}.rs, network/mod.rs (re-export shim),
    mempool/mod.rs, consensus/mod.rs, economics/mod.rs, governance/mod.rs,
    api/mod.rs, gui/mod.rs, storage/mod.rs, cli/mod.rs.
  Core-крейт: crates/strangecoin-core (pure, 0 I/O): types, serialize,
    consensus, chain_selector, address, abi/*, state/{mod,inner,sparse_merkle,
    witness,contract_storage}, vm/*, economics/*, governance/scip.rs.
    state/witness.rs: StateWitness/AccountProof/build_witness/
    verify_block_stateless (S1-P07) — Stage 3 добавит wire-формат.
  VM-крейт: crates/strangecoin-vm-wasm (wasmi =2.0.0, default-on).
  Net-крейт: crates/strangecoin-net (feature net, default-on): noise/*,
    protocol/*, discovery/*, peer/*, gossip/* (erlay/), rate_limiter,
    sync, sync_engine, shadow/*.
  Workspace: members = [".", "crates/strangecoin-core",
    "crates/strangecoin-vm-wasm", "crates/strangecoin-net"].
    crates/strangecoin-storage/ — НЕ создан (Stage 3 не начат; S3-P04 создаст).
  Storage baseline (LevelDB): src/storage/mod.rs = Arc<Mutex<rusty_leveldb::
    DB>> (get/put/new_iter/flush); схема: JSON blob-ключи b"chain" (ВСЯ
    цепочка Vec<Block> одним blob'ом — O(N) write amplification на блок),
    b"balances", b"contract_storage", b"difficulty"; mempool-ключи
    "{sender}:{nonce}" (full-scan load, НЕТ delete — stale tx копятся);
    schema version отсутствует; CF отсутствуют; config.storage.path —
    мёртвый ключ (факт: <exe_dir>/blockchain_db_{port}); rusty-leveldb =
    "4.0.0" (root, не pinned). state_cache: in-memory HashMap (balances/
    contract-storage) + write-behind полным snapshot'ом; state root
    пересчитывается in-memory из HashMap (SMT узлы НЕ персистятся);
    total_work/receipts/codes — derived, не в DB; rebuild_from_chain —
    invariant #1.
  Тесты: tests/ (~45 файлов net_*.rs + vm_*.rs + rpc и др.),
    crates/strangecoin-core/tests/ (12), crates/strangecoin-vm-wasm/tests/
    (8), crates/strangecoin-net/tests/ (+6 net_*.rs интеграционных);
    fuzz/fuzz_targets/ (5 core/VM + 4 network = 9 targets CI).
  Теги: v1.1.0-stage1, v1.5.0-stage1.5, v2.0.0-stage2 (annotated, target
  346e3c0, verified). Ожидается после S3-P15: strangecoin=3.0.0,
  strangecoin-core=3.0.0, strangecoin-storage=0.1.0 (первый release),
  strangecoin-vm-wasm=0.2.0 (unchanged), strangecoin-net=0.1.0 (unchanged).
Стейдж-документация: docs/stage2/{STAGE2_SUMMARY.md (12/12 ✅),
  INVARIANTS_ENFORCED.md (22/22), THREAT_MODEL_STAGE2.md, RESIDUAL_RISKS_
  STAGE3_PLUS.md} — все созданы. docs/stage3/ — НЕ создан (S3-P14/S3-P15
  создадут).
Известные прорехи (по ROADMAP3 Этап 3 и residual risks Stage 2):
  - crates/strangecoin-storage/ — не создан (S3-P04). Storage живёт в
    монолите src/storage/, LevelDB через rusty-leveldb.
  - Схема LevelDB: chain-blob (вся цепочка одним JSON — O(N) write
    amplification), mempool full-scan без delete, нет schema version,
    нет CF, нет индексов (txid→block = linear scan по цепочке).
  - config.storage.path не wired — мёртвый ключ (S3-P07 починит).
  - Crash-recovery: LOCK-файлы LevelDB, нет crash-test suite (S3-P10).
  - Stateless validation: core-функция verify_block_stateless есть
    (S1-P07), но нет wire-формата witness и node-side light path
    (S3-P11 предстоит).
  - State pruning: не реализован; исторических state-данных нет вообще
    (current-only) — EIP-4444 analog неприменим 1:1 (S3-P02/S3-P12).
  - Snapshot sync: не реализован (S3-P13 предстоит; формат — ADR-0026).
  - Verkle: ADR-0006 (verkle vs smt) — consensus-breaking замена SMT,
    SCIP-gated, НЕ делается в Stage 3 (witness-over-SMT — S3-P11).
  - Block-STM: не реализован (Stage 3+ anti-goal в этом документе).
Принципы:
  - 1 промпт = 1 атомарный коммит, ID промпта в сообщении, формат:
    «[S3-PNN] <area>: <short description>».
  - ADR пишется ДО того, как решение затронет код. Для Stage 3:
    ADR-0024..0026 (S3-P01..S3-P03) — обязательный гейт ДО S3-P04
    (crate scaffolding); ADR-0025 — гейт ДО S3-P12 (pruning);
    ADR-0026 — гейт ДО S3-P13 (snapshot).
  - Любое изменение консенсусных правил — через SCIP + activation height.
    Mainnet НЕ запущен; regtest/devnet — прямые правки допустимы до тега
    v3.0.0-stage3, каждая правка фиксируется в Changelog 3.0.0.
  - Валидация на входе (размеры, подписи, время, генезис, replay, rate,
    gas) не ослабляется НИ одним промптом (DoD: «All Stage 0-2 invariants
    still enforced», 22/22, regression S3-P14).
  - Determinism: key-encoding — pure functions; pruning/snapshot триггеры
    — по block height, НЕ wall clock (паттерн tick_at Stage 2); snapshot
    content детерминирован (sorted accounts). Ревью на детерминизм —
    обязательный пункт КГ (threat V-21 extension).
  - Strangler pattern: core остаётся чистым (0 I/O); storage runtime
    живёт в отдельном крейте strangecoin-storage (S3-P04); monolith
    остаётся оркестратором.
  - Pin policy Stage 3: rocksdb — через «=» (C++ API churn + data
    layout); обновление — только через SCIP после mainnet freeze.
  - После каждого промпта: cargo check и cargo test --workspace
    обязаны проходить. clippy -D warnings — green. Windows host
    preamble (PATH C:\msys64\mingw64\bin) — обязателен перед cargo.
  - Логирование — только tracing; println! запрещён.
  - Секреты — только в keystore/env; в config не писать.
  - Запрещено обновлять Changelog/README/AGENTS.md о работе, для которой
    ещё нет коммита с файлами (урок retro §8.1, прецедент FBP10).
Сокращения: КГ — критерии готовности.
```

---

## 1. Карта зависимостей промптов

```
STAGE 3 PROPER (старт только после подтверждения тега v2.0.0-stage2 —
см. §0.1; verified 2026-10-30):

ADRs (ДО кода — обязательный гейт):
S3-P01 (ADR-0024: Storage schema + StorageEngine trait; аменда ADR-0008) ─┐
S3-P02 (ADR-0025: State pruning strategy, EIP-4444 analog) ───────────────┤── все 3 ADR
S3-P03 (ADR-0026: Snapshot sync format) ─────────────────────────────────┘   параллельны
                                                                              после §0.1
                                                                                  │
                                                                                  ▼
CRATE + SCHEMA + BACKEND (последовательный):
S3-P01 ──▶ S3-P04 (crates/strangecoin-storage/ + workspace + Cargo.toml
                   + lib.rs skeleton + StorageEngine trait + feature)
                │
                ▼
          S3-P05 (schema.rs: KeyPrefix enum + key encoding + column
                  families + schema versioning m/schema_version)
                │
                ▼
          S3-P06 (RocksDbEngine: open/CF/get/put/delete/WriteBatch/
                  iterate_prefix/flush + crash-recovery basics; pin rocksdb)

MIGRATION + INDICES + PERFORMANCE (последовательный):
S3-P06 ──▶ S3-P07 (Миграция монолита: state_cache/facade → strangecoin-
                   storage; chain-blob → per-block keys; mempool prefix +
                   delete; wire config.storage.path + storage.engine;
                   state_history CF skeleton)
                │
                ▼
          S3-P08 (migrations.rs: LevelDB → RocksDB one-shot migration
                  format-converting; удаление rusty-leveldb — DoD
                  «LevelDB удалён»)
                │
                ▼
          S3-P09 (Индексы: txid→(height,index), height→hash,
                  contract_address→storage_root; O(1) lookups)
                │
                ▼
          S3-P10 (Performance: batch validation (WriteBatch) ≥10x microbench
                  + crash-recovery test suite + LRU/кэши)

STATELESS + PRUNING + SNAPSHOT (параллельные после S3-P10):
S3-P10 + S3-P01 ──▶ S3-P11 (Stateless validation: wire StateWitness
                             (canonical binary) + node light path +
                             e2e «light client верифицирует block»)
S3-P10 + S3-P02 ──▶ S3-P12 (State pruning: state_history prune по height,
                             archival mode, state_expiry marker; disk-
                             usage test)
S3-P10 + S3-P03 ──▶ S3-P13 (Snapshot sync: export/import CLI + format per
                             ADR-0026 + верификация commitments + new-node
                             sync test)

VERIFICATION (строго после всех proper):
S3-P11..S3-P13 ──▶ S3-P14 (Регрессия 22 инвариантов + Stage 3 extensions +
                            threat model update: snapshot forging, RocksDB
                            crash-recovery, witness wire)
                │
                ▼
          S3-P15 (DoD-верификация Stage 3 + Changelog 3.0.0 +
                  tag v3.0.0-stage3 — Gate Stage 4/5)
```

**Параллельные треки:** (a) ADRs S3-P01..S3-P03 — parallel after tag gate; (b) Crate S3-P04 → S3-P05 → S3-P06 sequential; (c) Migration S3-P07 → S3-P08 → S3-P09 → S3-P10 sequential; (d) Feature-track S3-P11 ∥ S3-P12 ∥ S3-P13 parallel after S3-P10 (+ свои ADR); (e) Verification S3-P14 → S3-P15 strictly after all.

---

## 2. Список промптов

### 2.1. ADRs — решения до кода (S3-P01–S3-P03)

Эти 3 ADR — обязательный гейт для последующих промптов Stage 3. Принцип ARCHITECT3 §16: решения фиксируются в `docs/ADR/` до того, как затронут код. ADR-0008 (RocksDB as Persistent Storage Engine) уже **Accepted** и закрывает развилку «RocksDB vs redb» — Stage 3 ADRs не дублируют его, а конкретизируют схему/прюнинг/snapshot.

**Замечание по нумерации ADR:** ROADMAP3 Этап 3 «ADRs required» планировал ADR-0020 (RocksDB vs redb), ADR-0021 (State pruning), ADR-0022 (Snapshot sync). ADR-0020..0023 заняты Stage 2 (Noise/Erlay/Scoring/RateLimit), а «RocksDB vs redb» уже решён в ADR-0008 (Accepted, включая §Alternatives: redb/sled/keep-leveldb). **Stage 3 переименовывает: ADR-0024 (schema & trait, аменда ADR-0008), ADR-0025 (pruning), ADR-0026 (snapshot format).** Решение зафиксировано в S3-P01.

---

### S3-P01. ADR-0024: Storage schema + StorageEngine trait (аменда ADR-0008)

**Цель:** зафиксировать целевую storage-схему Stage 3 — column families, KeyPrefix/key-encoding, schema versioning, семантику StorageEngine trait (без exposure raw handle — ADR-0008 Decision), config surface (`storage.engine`, починка мёртвого `storage.storage.path`), формат per-block записей (ликвидация chain-blob write amplification). Это решение определяет S3-P04..S3-P10: без него код пишется «на ощупь», а смена схемы позже потребует SCIP-grade миграции.

**Контекст:** ADR-0008 (Accepted) решил «migrate to RocksDB via rust-rocksdb + StorageEngine trait + migrations.rs» и оставил схему открытой. Фактическая LevelDB-схема (verified): 4 JSON blob-ключа + mempool full-scan; `b"chain"` хранит ВСЮ цепочку `Vec<Block>` — каждый `save_state()` переписывает весь blob (O(N) write amplification); mempool-ключи никогда не удаляются; schema version отсутствует (runtime-хаки при открытии); `config.storage.path` не используется. ROADMAP3 Этап 3 требует CF: `chain`, `state`, `transactions`, `nonces`, `meta`, `peers`, `mempool_cache`. Альтернативы схемы: (1) **RocksDB column families (ROADMAP3 list, adapted)** — нативная изоляция пространств, независимая compaction per-CF, iterate_prefix без prefix-scan cost; (2) **одна CF + KeyPrefix байты** — проще open/backup, но смешанная compaction и prefix-scan на всём keyspace; (3) **JSON blob-статус-кво** — O(N) write amplification недопустим на 10k+ блоков; (4) **персистентный SMT (узлы trie в DB)** — отдельное большое решение (меняет invariant #1 rebuild-семантику), не требуется ROADMAP3 DoD Stage 3 — отложить (residual). Решение: CF-схема (адаптированная ROADMAP3-list, см. отклонения), `StorageEngine` trait, schema versioning через запись `schema_version` в CF `meta`, per-block/per-account/per-tx ключи.

**Задачи:**

1. Создать `docs/ADR/0024-storage-schema-and-engine.md` по шаблону ADR-0001:
   - Status: Accepted
   - Context: фактическая LevelDB-схема (blob-ключи, full-scan, нет version), требования ROADMAP3 (CF, versioning, индексы, crash-recovery), связь с ADR-0008 (trait + migration уже решены; этот ADR амендит его §Decision, дополняя concrete schema)
   - Decision:
     - **Column families** (7): `meta` (schema_version, tip_height, tip_hash, difficulty, chain_id), `blocks` (height→canonical block bytes; hash→height index), `state` (address→AccountState canonical bytes), `transactions` (txid→(height, tx_index) index + raw tx), `contract_storage` (address||slot_key→value), `state_history` (height||address→pre-block AccountState snapshot, для pruning/archival — ADR-0025), `mempool` (sender||nonce→tx)
     - **KeyPrefix enum** + чистые key-encoding функции (без wall clock/RNG — детерминизм)
     - **Schema versioning**: `meta.schema_version: u32` (Stage 3 = 1; LevelDB-blob legacy = 0); upgrades через `migrations.rs` (S3-P08); отсутствие ключа на свежей БД → init genesis-состояние version=1
     - **StorageEngine trait**: `get/put/delete/write_batch/iterate_prefix/flush` (+CF как параметр); raw `DBWithThreadMode` НЕ покидает крейт (ADR-0008)
     - **Config surface**: `storage.engine = "rocksdb"` (default; `"leveldb"` — transitional только до S3-P08, затем удаляется), `storage.path` — **wired по-настоящему** (default `<data_dir>/rocksdb`; миграция legacy `<exe_dir>/blockchain_db_{port}` — S3-P08)
     - **Персистентность derived-данных**: receipts/codes/total_work остаются derived (rebuild from chain — invariant #1 сохраняется)
   - Consequences: Positive (write amplification O(1) на блок, O(1) индексы, независимая compaction, versioned upgrades); Negative (C++ dep + build time, multi-CF backup сложнее single-file, state_history растёт — pruning в ADR-0025); Neutral (peers/nonces CF ROADMAP3 НЕ вводятся — peers живут в network.json (stranger-реализация Stage 2), nonce входит в AccountState в CF `state`)
   - Alternatives considered: одна CF + prefixes; JSON blob status-quo; персистентный SMT узлы (residual); sled/redb (отклонены ADR-0008, не пересматриваются)
2. Зафиксировать отклонения от ROADMAP3-CF-list явно (FBP-прецедент): нет `nonces` (nonce ⊂ AccountState), нет `peers` (network.json Stage 2), добавлены `state_history` + hash→height index.
3. Pin policy: `rocksdb` — pin `=` (версия резолвится в S3-P06 по актуальному стабильному rust-rocksdb, затем фиксируется в этом ADR строкой «Pinned: rocksdb = x.y.z» и в AGENTS.md Pin Policy).

**Артефакты:**
- `docs/ADR/0024-storage-schema-and-engine.md`

**КГ:**
- [ ] ADR-0024 создан по template ADR-0001 (Status/Context/Decision/Consequences/Alternatives/Related)
- [ ] 7 CF перечислены с назначением ключ→значение; ROADMAP3-deviations задокументированы явно
- [ ] StorageEngine trait-семантика зафиксирована (без raw handle exposure)
- [ ] Schema versioning (legacy=0 → stage3=1, migrations.rs) зафиксирован
- [ ] Config surface: engine + path (wiring mёртвого ключа) зафиксирован
- [ ] Ссылка на ADR-0008 как на superseded-in-part / амендируемый baseline
- [ ] Коммит `[S3-P01] adr-0024: storage schema + StorageEngine trait (amend ADR-0008)`

---

### S3-P02. ADR-0025: State pruning strategy (EIP-4444 analog)

**Цель:** зафиксировать pruning-стратегию — что прюнится (implementation-level historical data), что НЕ прюнится (консенсус-необходимый current state), archival mode, retention policy по height (не wall clock), relationship к state expiry (consensus-changing → SCIP-gated, НЕ в Stage 3). Это решение определяет S3-P12.

**Контекст:** ROADMAP3 Этап 3: «Удаление исторического state старше N блоков (EIP-4444 analog); State revival через Verkle witnesses; state_expiry_check в state.apply_block; опционально state rent». Резерв Stage 2 (`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1/§3): pruning ослабляет replay-verifiability — «Stage 3+ через SCIP (consensus-changing)»; receipts prune / contract storage prune — Stage 3+ SCIP. Ключевой факт baseline: текущая схема хранит **только current** AccountState (HashMap → blob), исторических версий нет — EIP-4444 (удаление исторических trie nodes Ethereum) **неприменим 1:1**. Stage 3 вводит `state_history` CF (ADR-0024) — исторические снапшоты per block, — и прюнит именно их. Альтернативы: (1) **Implementation-level pruning historical data (height-window)** — full-нода хранит history длиной retention_window (default 5000 блоков), старше — delete; consensus не меняется (история восстановима из blocks + re-execution); archival-нода хранит всё; (2) **State expiry (dormant account removal из current state)** — consensus-breaking (revival требует witness-семантики на wire) → SCIP-gated, Stage 3 только marker-подготовка, НЕ удаление; (3) **Receipts prune** — SCIP-gated (residual), НЕ в Stage 3; (4) **no pruning** — ROADMAP3 DoD невыполним (disk-usage test). Решение: вариант (1) + marker-подготовка (2), явный SCIP-path для (2)/(3).

**Задачи:**

1. Создать `docs/ADR/0025-state-pruning-strategy.md`:
   - Status: Accepted
   - Context: EIP-4444 analog, replay-verifiability trade-off, current-only baseline, archival vs full node modes (ROADMAP3 risk: «State pruning breaks light clients» → archival)
   - Decision:
     - **Full node** (default): прюнит `state_history` записи старше `pruning.retention_blocks` (default 5000, config.toml); blocks/state/txs/mempool НЕ прюнятся; триггер — по height (каждые `pruning.sweep_every_blocks`, default 100), НЕ wall clock
     - **Archival node** (`node_mode = "archival"` в config): history хранится полностью; отвечает на исторические запросы (future Stage 4 indexer)
     - **Pruning никогда не трогает**: current `state` CF, `blocks` CF (replay chain), `meta` tip
     - **State expiry**: dormant-account removal — consensus-changing; Stage 3 ограничивается touch-tracking marker'ом в `state_history` (данные для будущего SCIP), НЕ удаляет current-state записи
     - **Receipts/contract-storage prune**: остаются SCIP-gated residual (не Stage 3)
   - Consequences: Positive (disk bounded для full-нод, archival-режим сохраняет auditability, no consensus change); Negative (full-нода не отвечает на глубокие исторические запросы; retention_window — tuning knob без универсального оптимума); Neutral (pruning — implementation detail; верификация цепочки остаётся через blocks replay — invariant #1)
   - Alternatives considered: state expiry (SCIP-gated), receipts prune (SCIP-gated), no pruning (DoD fail), wall-clock TTL (антидетерминизм — threat V-21)
2. Sketch config surface: `[pruning] enabled = true, retention_blocks = 5000, sweep_every_blocks = 100` + `[node] mode = "full" | "archival"`.

**Артефакты:**
- `docs/ADR/0025-state-pruning-strategy.md`

**КГ:**
- [ ] ADR-0025 создан по template
- [ ] Раздел Alternatives ≥3 альтернатив
- [ ] Retention policy по height (не wall clock) зафиксирована — детерминизм
- [ ] Что прюнится / что НЕ прюнится — исчерпывающий список
- [ ] Archival mode + SCIP-path для state expiry/receipts prune зафиксированы
- [ ] Ссылка на residual risks Stage 2 §1/§3 (replay-verifiability trade-off)
- [ ] Коммит `[S3-P02] adr-0025: state pruning strategy (height-window, archival mode)`

---

### S3-P03. ADR-0026: Snapshot sync format

**Цель:** зафиксировать snapshot file format + верификацию commitments (height, block_hash, state_root, receipts_root) ДО применения + cadence генерации + threat mitigations (forged snapshot, withholding). Это решение определяет S3-P13.

**Контекст:** ROADMAP3 Этап 3: «Быстрая синхронизация новых узлов без скачивания всей цепочки; Snapshot file format; Snapshot generation every N blocks». Residual Stage 2 §1: snapshot sync добавляет векторы forged snapshot / snapshot withholding / split-brain — «требует отдельного threat-моделинга + верификации snapshot commitment (state root + height) перед применением». Baseline: state root считается из HashMap через `SparseMerkleTrie::compute_root`; snapshot должен содержать сериализованный state (accounts + contract storage), достаточный для пересчёта state_root. Альтернативы: (1) **File-based snapshot (export/import CLI)** — детерминированный файл, верификация локально, транспорт вне P2P (scp/https/file); (2) **Wire snapshot sync через P2P (GET_SNAPSHOT)** — требует протокольного расширения strangecoin-net (новый message, rate-limit class, Erlay/compact interaction) — Scope-explosion для Stage 3, отложить (residual / Stage 4+); (3) **State-only dump без commitment-верификации** — уязвим к forged snapshot (V-residual Stage 2), недопустимо; (4) **Переносить все blocks в snapshot (block+state)** — не «быстрая» синхронизация, не отличается от replay. Решение: вариант (1) с обязательной commitment-верификацией; wire-транспорт — residual.

**Задачи:**

1. Создать `docs/ADR/0026-snapshot-sync-format.md`:
   - Status: Accepted
   - Context: bootstrap cost, forged/withheld snapshot threats, canonical serialization baseline (serialize.rs, blake3), SMT compute_root
   - Decision:
     - **Format**: directory либо single file `snapshot-{height}-{state_root_hex}.scsnap` — header (magic `SCSNAP01`, format_version, chain_id, height, block_hash, state_root, receipts_root) + body: accounts sorted lexicographically по address (canonical bytes), contract storage sorted (address, slot); trailer: blake3 whole-file checksum
     - **Верификация перед применением**: (a) checksum файла; (b) chain_id match; (c) **state_root пересчитывается** из accounts через `SparseMerkleTrie::compute_root` и сверяется с header.state_root — mismatch → reject (forged-snapshot mitigation); (d) block_hash сверяется с header-цепочкой после catch-up
     - **Generation**: каждые `snapshot.every_blocks` (default 1000), только на archival-нодах (default) либо по явной команде оператора; генератор детерминирован (sorted order)
     - **Transport**: Stage 3 — file-based (CLI `sc snapshot export` / `sc snapshot import`, S3-P13); P2P GET_SNAPSHOT — residual (Stage 4+)
     - **Применение**: import → write state CF + meta tip = snapshot.height; далее headers-first sync (S1) догоняет до tip; нода СТАНОВИТСЯ trusted только после (c) + (d)
   - Consequences: Positive (bootstrap O(state) вместо O(chain), commitment-верификация снимает forged-snapshot vector); Negative (snapshot withholding — нода без файла не синхронизируется быстро; split-brain snapshot-vs-replay — лечится block_hash catch-up); Neutral (file-based транспорт — ops-ответственность; wire — residual)
   - Alternatives considered: wire GET_SNAPSHOT (scope), без верификации (vulnerable), full dump (не fast), state rent внутри snapshot (нет)
2. Snapshot-формат обязан использовать canonical binary serialization core (не serde_json для body; JSON допустим только как optional manifest для tooling).

**Артефакты:**
- `docs/ADR-0026` → `docs/ADR/0026-snapshot-sync-format.md`

**КГ:**
- [ ] ADR-0026 создан по template
- [ ] Header-поля + body ordering + checksum зафиксированы
- [ ] 4-step верификация (checksum/chain_id/state_root recompute/block_hash) зафиксирована
- [ ] Cadence + transport (file-based Stage 3, wire residual) зафиксированы
- [ ] Threat mitigations (forged/withholding/split-brain) задокументированы со ссылкой на residual risks Stage 2 §1
- [ ] Коммит `[S3-P03] adr-0026: snapshot sync format (commitment-verified, file-based)`

---

### 2.2. Crate + schema + backend (S3-P04–S3-P06)

Stage 3 начинается с создания `crates/strangecoin-storage/` (strangler, ARCHITECT3 §10.5) по образцу S2-P05/S2-P12: scaffolding → schema (pure) → RocksDB backend. Миграция монолита (S3-P07) идёт ПОСЛЕ backend'а.

---

### S3-P04. crates/strangecoin-storage/ + workspace + StorageEngine trait skeleton

**Цель:** создать storage-крейт: workspace member, Cargo.toml (pinned rocksdb dep, feature `rocksdb` default-on), lib.rs skeleton с `StorageEngine` trait + `StorageError` (по образцу `NetError` S2-P06) + ре-экспортами. Никакого LevelDB-кода в крейте — только чистые типы + trait.

**Контекст:** ADR-0024 (S3-P01) зафиксировал trait-семантику; strangler pattern Stage 2 (S2-P05 → S2-P06) показал рабочий порядок. Windows host: rocksdb = C++ через cc-rs — preamble PATH обязателен; cold build минуты. `rusty-leveldb` остаётся в monolith до S3-P08.

**Задачи:**

1. `crates/strangecoin-storage/Cargo.toml`: name `strangecoin-storage`, version `0.1.0`, edition 2021; deps: `rocksdb` (pin `=`, версия резолвится здесь — зафиксировать в ADR-0024 «Pinned» + AGENTS.md Pin Policy), `thiserror`, `tracing`, `serde` (только для manifest-типов, НЕ для hot-path encoding), `blake3` (checksums); dev-deps: `tempfile`.
2. `crates/strangecoin-storage/src/lib.rs`: `pub mod engine; pub mod schema;` (schema — реализуется S3-P05; здесь только `pub mod` declarations по образцу S2-P05), `pub use engine::{StorageEngine, WriteBatchOp, StorageError}`; module docs со ссылкой на ADR-0024/ADR-0008.
3. `crates/strangecoin-storage/src/engine.rs`: trait `StorageEngine`:
   - `get(&self, cf: Cf, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError>`
   - `put/delete` (convenience, = write_batch одного op)
   - `write_batch(&self, ops: Vec<WriteBatchOp>) -> Result<(), StorageError>` (атомарный batch)
   - `iterate_prefix(&self, cf: Cf, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError>` (sorted by key — детерминизм)
   - `flush(&self) -> Result<(), StorageError>`
   - `enum Cf` — 7 variants per ADR-0024 (кодек в S3-P05)
   - `enum StorageError { Io, Rocksdb, Corrupt(&'static str), SchemaVersion{found,expected}, ... }` + `From<rocksdb::Error>` (feature-gated при необходимости)
4. Workspace `Cargo.toml`: добавить `"crates/strangecoin-storage"` в members; root crate НЕ зависит от storage ещё (S3-P07 подключит).
5. Тесты: trait-уровня — mock in-memory engine (Vec of BTreeMap per CF) + unit-тесты iteration order; реальный RocksDbEngine — S3-P06.

**Артефакты:**
- `crates/strangecoin-storage/Cargo.toml`
- `crates/strangecoin-storage/src/lib.rs`
- `crates/strangecoin-storage/src/engine.rs`
- `Cargo.toml` (workspace members)

**КГ:**
- [ ] Workspace member добавлен; `cargo check --workspace` зелёный (Windows preamble!)
- [ ] `StorageEngine` trait + `Cf` (7 variants) + `StorageError` объявлены per ADR-0024
- [ ] rocksdb dep pinned `=` (версия зафиксирована в ADR-0024 + AGENTS.md Pin Policy в S3-P15)
- [ ] In-memory mock engine + iteration-order тесты зелёные
- [ ] `cargo test -p strangecoin-storage` зелёный; clippy -D warnings green
- [ ] Коммит `[S3-P04] crate: strangecoin-storage scaffolding + StorageEngine trait`

---

### S3-P05. schema.rs: KeyPrefix + key encoding + schema versioning

**Цель:** реализовать чистый key-encoding слой: `KeyPrefix`/`CfKey` типы, конструкторы ключей для каждой CF (height, hash, address, txid, slot, sender||nonce), schema version constants (`SCHEMA_VERSION = 1`), чтение/инициализация `meta.schema_version`. Без I/O — pure functions (детерминизм, легко тестируется).

**Контекст:** ADR-0024 зафиксировал CF + versioning. Ключевые инварианты кодека: (a) одинаковый input → одинаковый key bytes; (b) lexicographic order ключей в CF `blocks`/`state_history` должен соответствовать числовому order height — big-endian encoding; (c) mempool-ключи должны поддерживать prefix-iterate всех tx одного sender.

**Задачи:**

1. `crates/strangecoin-storage/src/schema.rs`:
   - `pub const SCHEMA_VERSION: u32 = 1;` (+ doc: legacy LevelDB blob = 0)
   - key-конструкторы (все `fn ... -> Vec<u8>`, pure):
     - `meta_key(name: &str)`, известные имена: `schema_version`, `tip_height`, `tip_hash`, `difficulty`, `chain_id`
     - `block_by_height_key(height: u64)` — BE u64; `block_hash_index_key(hash: &[u8;32])` → height bytes
     - `state_key(address: &str)`; `state_history_key(height: u64, address: &str)` — BE height ‖ address
     - `tx_index_key(txid: &[u8;32])` → (height, index); `tx_raw_key(txid)` 
     - `contract_storage_key(address: &str, slot: &[u8])`
     - `mempool_key(sender: &str, nonce: u64)` — BE nonce; `mempool_sender_prefix(sender)`
   - Value-типы: storage хранит **opaque bytes**; encode/decode блоков/tx/state — на стороне monolith (S3-P07). Schema-кодек отвечает только за KEY structure. Зависимость storage→core **допустима** (core — pure, 0 I/O; нужна snapshot'ам S3-P13 и witness S3-P11), запрещена обратная (core→storage) — ADR-0024 §Decision; сам `schema.rs` core-типы НЕ использует (pure byte functions)
   - `pub fn encode_schema_version(v: u32) -> [u8;4]` / decode
2. Тесты: round-trip key-функций; **order test** (height BE: key(9) < key(10)); sender-prefix мемпула (все nonce одного sender — contiguous); fuzz-style proptest на address/slot bytes (нет коллизий между CF-кодеками).
3. `lib.rs`: `pub mod schema;` уже есть — ре-экспорт key-конструкторов.

**Артефакты:**
- `crates/strangecoin-storage/src/schema.rs`
- `crates/strangecoin-storage/src/schema.rs` unit tests (в файле) + возможно `crates/strangecoin-storage/tests/schema.rs`

**КГ:**
- [ ] Все key-конструкторы per ADR-0024 реализованы как pure functions (нет SystemTime/RNG/IO)
- [ ] BE-height ordering протестирован (lexicographic == numeric)
- [ ] Mempool sender-prefix contiguity протестирована
- [ ] SCHEMA_VERSION=1 + encode/decode протестированы
- [ ] `schema.rs` не использует core-типы (pure byte encoding); зависимость crate-level storage→core допустима per ADR-0024, обратная (core→storage) запрещена — проверено grep'ом
- [ ] `cargo test -p strangecoin-storage` зелёный; clippy green
- [ ] Коммит `[S3-P05] schema: KeyPrefix encoding + schema versioning v1`

---

### S3-P06. RocksDbEngine: open/CF/get/put/delete/WriteBatch/iterate + crash-recovery basics

**Цель:** реализовать `RocksDbEngine` (impl StorageEngine) на `rocksdb` crate: открытие с 7 CF, atomic WriteBatch, prefix iteration, flush, обработка corrupt/schema-mismatch ошибок; тесты на temp-директориях; базовый crash-recovery smoke (kill simulation через drop без flush → WAL restore).

**Контекст:** ADR-0024/ADR-0008: `Options::set_level_compaction_dynamic_level_bytes(true)`; CF create при первом open; missing `meta.schema_version` на свежей БД → инициализация `SCHEMA_VERSION` (writer: `init_fresh`), на существующей → сверка (mismatch → `StorageError::SchemaVersion`). Windows: rocksdb C++ build — preamble; pin уже в Cargo.toml (S3-P04).

**Задачи:**

1. `crates/strangecoin-storage/src/rocks.rs`:
   - `pub struct RocksDbEngine { db: rocksdb::DB }` (rocksdb::DB thread-safe — снимает `Arc<Mutex<>>` Stage 1; документировать улучшение lock-ordering: DB-mutex уровня больше нет)
   - `RocksDbEngine::open(path) -> Result<Self, StorageError>`: `open_cf` со всеми 7 CF (+default), create_if_missing, dynamic_level_bytes; прочитать/инициализировать schema_version (mismatch → error, НЕ auto-upgrade — upgrades = migrations.rs S3-P08)
   - impl StorageEngine: get/put/delete через `cf_handle`; `write_batch` → `rocksdb::WriteBatch` (атомарно, default WriteOptions sync=false + документированный `flush()` contract); `iterate_prefix` → PrefixIterator, собирает в Vec (sorted by key天然); `flush` → `flush(&default_cf)`
   - `impl Drop` — flush (по образцу Stage 1 Storage::drop)
2. Тесты (`crates/strangecoin-storage/tests/rocks_engine.rs`, tempfile):
   - open/create, put/get/delete round-trip per CF
   - write_batch атомарность (batch из 100 ops, все или ни одного после «отката» — проверка через второе открытие)
   - iterate_prefix: порядок ключей, изоляция CF
   - schema_version: fresh init = 1; ручная порча ключа → open возвращает SchemaVersion error
   - crash smoke: put без flush → drop процесса (std::mem::forget +reopen в новом engine) → WAL восстановил запись (документировать, что forget ≠ real kill; real kill-тест — S3-P10)
3. Feature-flag: `rocksdb` default-on в Cargo.toml крейта; in-memory mock остаётся для тестов schema'ы.

**Артефакты:**
- `crates/strangecoin-storage/src/rocks.rs`
- `crates/strangecoin-storage/tests/rocks_engine.rs`
- `crates/strangecoin-storage/Cargo.toml` (feature wiring)

**КГ:**
- [ ] RocksDbEngine: open с 7 CF, все методы trait'а реализованы
- [ ] Schema-version guard: fresh init=1, mismatch → error (без auto-migration)
- [ ] Тесты tempfile: round-trip, batch atomicity, prefix order, CF isolation — зелёные
- [ ] Crash smoke (forget+reopen) зелёный; ограничения задокументированы
- [ ] `cargo test -p strangecoin-storage` зелёный (preamble!); clippy green; build time зафиксирован в коммите (Windows cold ≈ N мин — measurement note)
- [ ] Коммит `[S3-P06] backend: RocksDbEngine (CF, WriteBatch, prefix iter, schema guard)`

---

### 2.3. Migration + indices + performance (S3-P07–S3-P10)

После backend'а: перевести монолит на strangecoin-storage, убрать LevelDB, добавить индексы и performance-слой.

---

### S3-P07. Миграция монолита: state_cache/facade → strangecoin-storage (per-block keys, mempool delete, config wiring)

**Цель:** переключить monolith (`src/storage/`, `src/blockchain/state_cache.rs`, facade, chain_selector, error) на `strangecoin-storage`: новая схема per-block/per-account (ликвидация chain-blob), mempool в CF с delete, починить `config.storage.path` (мёртвый ключ) + `storage.engine`, `state_history` запись на каждом adopt. LevelDB-код МОЖЕТ остаться transitional (engine="leveldb") до S3-P08, но новый путь — default.

**Контекст:** фактические call-sites (verified): `state_cache.rs` save_state/load_persisted_state/debug_db (blob-ключи, full-scan), `blockchain_facade.rs:124` open_local, `chain_selector.rs:135` serde-открытие, `error.rs:36` LeveldbError, `tests/address_migration.rs` (прямые seed legacy-базы). Invariant #1 (rebuild_from_chain) сохраняется: receipts/codes/total_work — derived. Lock ordering: DB уровня больше нет (rocksdb thread-safe) — обновить docs в `src/error.rs` при необходимости. Config: `db_path_for_port` (exe-dir) упраздняется как единственный путь — остаётся fallback при engine=leveldb.

**Задачи:**

1. `src/storage/mod.rs`: ре-экспорт `strangecoin_storage::{StorageEngine, RocksDbEngine, Cf, schema::*}`; `Storage`-обёртка monolith-уровня (open по config) — или прямое использование engine в state_cache (выбрать минимальный diff); удалить rusty-leveldb из этого файла (переход на engine).
2. `state_cache.rs`:
   - `save_state()`: убрать chain-blob; писать: tip → meta; блоки → `blocks` CF (per-height при adopt'е, не полный re-write); accounts → `state` CF (upsert changed accounts — diff vs HashMap, не full rewrite); contract storage → `contract_storage` CF; mempool → `mempool` CF (put новых + **delete** confirmed/mined (sender,nonce)); difficulty → meta; **state_history**: снапшот затронутых адресов ПЕРЕД apply → `state_history` CF (height||addr)
   - `load_persisted_state()`: читать tip → blocks по heights → state CF full-load (archival/current), mempool prefix-iterate (БЕЗ full-scan всей БД)
   - `debug_db` — заменить на health-check (tip presence + version)
3. `blockchain_facade.rs`/`chain_selector.rs`/`block_executor.rs`/`gui/mod.rs`: открытие через новый путь (`config.storage.path` + engine); `open_local` — сохранить сигнатуру (temp-директории ~25 тестов), внутри — RocksDbEngine.
4. `src/config.rs`: default path → `./data/rocksdb`; `storage.engine` поле; валидация; документировать legacy fallback.
5. `src/error.rs`: `StrangecoinError::Storage(#[from] StorageError)`; LeveldbError — оставить transitional (S3-P08 удалит).
6. `src/lib.rs`: `pub mod storage` shim обновить; test_support `blockchain_db_path_for_port` — совместимость с tests/address_migration.rs (либо обновить тест под новую схему, зафиксировать deviation).
7. Тесты: существующие ~25 файлов через `create_test_blockchain*` должны пройти без изменений (open_local signature stable); новый тест: «блок adopt не перезаписывает всю цепочку» (micro-assert: количество put-операций O(изменённых accounts), не O(цепочка) — через counting-mock engine); mempool: mined tx удаляется из CF (не копится).

**Артефакты:**
- `src/storage/mod.rs`, `src/blockchain/state_cache.rs`, `src/blockchain/blockchain_facade.rs`, `src/blockchain/chain_selector.rs`, `src/config.rs`, `src/error.rs`, `src/lib.rs` (правки)
- `tests/mempool_persistence.rs` (или дополнение существующего rbf/mempool теста)

**КГ:**
- [ ] Monolith открывает/пишет через strangecoin-storage (новый путь default); chain-blob удалён
- [ ] Per-block writes: O(изменённых записей) на adopt — подтверждено counting-mock тестом
- [ ] Mempool: prefix-load + delete после минта — протестировано
- [ ] `config.storage.path` wired (default ./data/rocksdb) + `storage.engine` — протестировано
- [ ] `state_history` пишется при adopt (pre-apply snapshot затронутых адресов)
- [ ] ~25 существующих тестов (create_test_blockchain*) зелёные; `cargo test --workspace` зелёный (чанками per AGENTS.md); clippy green
- [ ] Отклонения (если address_migration.rs изменён и т.п.) зафиксированы в коммите
- [ ] Коммит `[S3-P07] migrate: monolith → strangecoin-storage (per-block keys, mempool CF, config wiring)`

---

### S3-P08. migrations.rs: LevelDB → RocksDB migration + удаление rusty-leveldb

**Цель:** one-shot миграция legacy LevelDB-базы (`<exe_dir>/blockchain_db_{port}`, blob-схема version 0) → RocksDB (per-block схема version 1): конвертация формата (chain blob → per-block, balances → state, mempool scan → mempool CF), запуск при первом открытии с `engine="rocksdb"` при наличии legacy-директории + явный CLI-флаг; затем удалить `rusty-leveldb` из workspace (DoD «LevelDB удалён»).

**Контекст:** ADR-0024: legacy=0 → stage3=1 через migrations.rs. Формат legacy (verified): JSON blob `chain` (Vec<Block>), `balances` (HashMap<String, AccountState> или legacy HashMap<String,u64>), `contract_storage`, `difficulty`, mempool sender:nonce. Runtime-миграции Stage 1 (bech32 и т.п.) уже применяются при load — мигратор должен их учитывать (читать через тот же deserialize-path). Rollback: legacy-директория переименовывается в `blockchain_db_{port}.leveldb.bak`, НЕ удаляется (ADR-0008 «rollback capability»).

**Задачи:**

1. `src/storage/migrations.rs` (или `crates/strangecoin-storage/src/migrations.rs` + monolith glue — предпочтительно в крейте с trait-доступом к legacy Reader; deviation зафиксировать):
   - `pub fn migrate_leveldb_to_rocksdb(legacy_path, new_path) -> Result<MigrationReport, StorageError>`
   - Read: iterate legacy DB (rusty-leveldb iterator), парсить 4 blob-ключа + mempool-ключи; deserialize через monolith types (blockchain/state_cache path)
   - Write: batched WriteBatch в RocksDB (блоки per-height, accounts → state, contract storage, mempool, tip/difficulty → meta; schema_version=1)
   - `MigrationReport { blocks, accounts, mempool_txs }` — tracing::info
   - **Auto-run**: при `Blockchain::new` с engine=rocksdb + существующей legacy-директории + отсутствующей/new rocksdb — миграция + rename legacy → `.bak`
   - CLI: `sc storage migrate-leveldb` (explicit, без авто) — опционально, если auto-run покрыт тестом
2. Удаление LevelDB: `rusty-leveldb` dep из root Cargo.toml; `StrangecoinError::LeveldbError` → удалить (или deprecated alias); `engine="leveldb"` config-value удалить; `src/storage/mod.rs` полностью на strangecoin-storage; tests/address_migration.rs — переписать под legacy-fixtures (seed через in-memory LevelDB-fixture в dev-dep? — rusty-leveldb как dev-dep только для фикстур, если auto-run path должен тестироваться; deviation: dev-only legacy reader допустим и документируется)
3. Тесты: fixture legacy DB (seed старой схемы) → migrate → reopen → состояние эквивалентно (balances, chain length, tip, mempool); idempotency (повторный open — no-op); corrupt legacy → fail-closed (без partial apply: migrate в temp, атомарный rename).

**Артефакты:**
- `src/storage/migrations.rs` (или `crates/strangecoin-storage/src/migrations.rs`)
- `tests/leveldb_migration.rs`
- `Cargo.toml` (rusty-leveldb удалён из runtime deps)
- `src/error.rs` (LeveldbError cleanup)

**КГ:**
- [ ] Миграция legacy blob → per-block схема: fixture-тест зелёный (блоки/счета/мемпул/tip сверены)
- [ ] Auto-run при первом открытии + rename → .bak (rollback preserved) — протестировано
- [ ] Fail-closed на corrupt legacy (без partial apply)
- [ ] `rusty-leveldb` удалён из runtime workspace deps; `cargo tree -i rusty-leveldb` пуст (или только dev-deps — с пометкой)
- [ ] `cargo test --workspace` зелёный (чанками); clippy green
- [ ] Deviation (dev-dep fixture и т.п.) зафиксирован
- [ ] Коммит `[S3-P08] migrate: LevelDB → RocksDB one-shot migration + leveldb removal`

---

### S3-P09. Индексы: txid→(height,index), height→hash, contract_address→storage_root

**Цель:** O(1) индексы, поддерживаемые при adopt'е: `txid → (height, tx_index)` (+ raw tx в transactions CF), `block hash → height`, `contract_address → storage_root` (кэш; storage_root пересчитываемый — кэш инвалидируется при записи контракт-storage). Ликвидировать linear-scan `chain_has_tx` / `block_by_hash`.

**Контекст:** ROADMAP3 Этап 3 Indices: «address → UTXO/balance», «txid → block_height», «contract_address → storage_root». Baseline (verified): address→balance — in-memory HashMap (уже O(1)); txid→block — linear scan (`chain_selector.rs:37-44`); block_by_hash — scan. Deviation: address→balance индекс НЕ создаётся отдельно (state CF уже O(1) по address) — документировать.

**Задачи:**

1. Индексная поддержка в S3-P07 path расширить (или отдельный `indexer.rs` в state_cache):
   - При adopt block: для каждой tx — `tx_index_key(txid) → (height BE ‖ index BE)`, `tx_raw_key(txid) → canonical tx bytes`; `blocks` hash-index: `block_hash_index_key(hash) → height`
   - При изменении contract storage контракта: `contract_storage_root_key(addr) → root` (после apply)
   - При prune (S3-P12) — tx-индексы НЕ прюнятся (история транзакций нужна навсегда; прюнится только state_history)
2. API facade: `fn tx_location(txid) -> Option<(u64,u32)>`, `fn block_by_hash(hash) -> Option<Block>` — O(1) (заменить scan); `chain_has_tx` через tx_location.
3. Тесты: insert 100 блоков → random txid lookup O(1) корректен; reorg/adopt_candidate — индексы консистентны (unapply удаляет/перезаписывает); negative lookup; benchmark-смoke (10k tx lookup < ms — на десктопе, не assert-time в CI, только local note).

**Артефакты:**
- Правки `src/blockchain/{state_cache,blockchain_facade,chain_selector}.rs`
- `tests/indices.rs`

**КГ:**
- [ ] txid→(height,index) + raw tx, hash→height, contract storage_root кэш — пишутся при adopt
- [ ] `block_by_hash`/`chain_has_tx` — через индексы (scan удалён — проверено grep'ом)
- [ ] Reorg-консистентность индексов протестирована (adopt_candidate + revert)
- [ ] address→balance deviation (state CF уже O(1)) зафиксирована в коммите
- [ ] `cargo test --workspace` зелёный; clippy green
- [ ] Коммит `[S3-P09] indices: txid/block-hash/storage-root O(1) lookups`

---

### S3-P10. Performance: batch validation microbench + crash-recovery suite + cache hardening

**Цель:** закрыть ROADMAP3 Performance-задачи: «Пакетная валидация (batch validation) ≥10x быстрее sequential» (microbench WriteBatch vs per-tx put), «Кэши состояния (LRU)», «Crash-recovery без ручного удаления LOCK» (тест suite: kill во время writes → чистый reopen, OS-released lock).

**Контекст:** write_batch уже в trait (S3-P06); state cache — in-memory HashMap (full current state, не LRU-ограничен — это осознанно: current state обязан быть в RAM для state root). LRU применим к block/tx кэшу при чтении с диска. Crash-recovery: RocksDB WAL + OS-level LOCK (auto-released при смерти процесса) — протестировать на уровне «reopen без ручного вмешательства».

**Задачи:**

1. Bench (`crates/strangecoin-storage/benches/write_batch.rs`, criterion dev-dep или простой `#[test]` c Instant — выбрать criterion если build-time приемлем; deviation если test-based):
   - 1000 random account-upsert: (a) per-op `put`+flush, (b) один `write_batch`+flush — замер local, assert `b_time < a_time` (мягкий) + зафиксировать ratio в коммите (ROADMAP3 ≥10x — измерить, «не верифицировано в CI» если только local)
2. Crash-recovery suite (`tests/crash_recovery.rs`):
   - child process (spawn самого тест-бинаря с env-флагом) пишет N батчей и убивается (`Child::kill`) на random точке → parent reopen → все batch'и либо целиком, либо отсутствуют (атомарность), tip консистентен
   - stale LOCK: симулируя «процесс умер» (kill -9 child) — reopen успешен без ручного удаления LOCK-файла
3. Cache: block-by-hash read path — in-memory LRU (capacity config, default 4096) поверх transactions/blocks CF; документировать, что state CF не кэшируется (всё в RAM StateCache).
4. `src/config.rs`: `[storage] block_cache_capacity` (опционально).

**Артефакты:**
- `crates/strangecoin-storage/benches/` или microbench test
- `crates/strangecoin-storage/tests/crash_recovery.rs`
- LRU-кэш (правки engine/monolith read path)

**КГ:**
- [ ] Batch-vs-sequential microbench написан; ratio измерен и зафиксирован (≥10x или deviation с данными)
- [ ] Crash suite: kill child → reopen чистый, atomic batches, LOCK auto-release — зелёные
- [ ] LRU block cache capacity-bounded, документировано
- [ ] `cargo test --workspace` зелёный (чанками); clippy green
- [ ] Не-локально-верифицируемое (CI-time benches) помечено
- [ ] Коммит `[S3-P10] perf: batch write microbench + crash-recovery suite + LRU cache`

---

### 2.4. Stateless + pruning + snapshot (S3-P11–S3-P13)

Параллельные feature-промпты после S3-P10; каждый требует своего ADR (S3-P01/P02/P03 соответственно — уже готовы).

---

### S3-P11. Stateless validation: wire StateWitness + node light path + e2e

**Цель:** довести существующий core witness (`StateWitness`, `verify_block_stateless`, S1-P07) до wire-уровня: canonical binary encode/decode witness'а, node-side API (построение witness'а по запросу + верификация входящего блока с witness), e2e-тест «light client (без полного state) верифицирует блок». Без Verkle (consensus-breaking → SCIP, ADR-0006 amendment residual).

**Контекст:** core: `witness.rs` — `StateWitness { pre_state_root, proofs: HashMap<String, AccountProof> }`, `build_witness(&State, &Block)`, `verify_block_stateless(parent_root, block, chain_id, witness)` (полная проверка Merkle proof + touched-coverage + apply). ROADMAP3: «StateWitness для Verkle proofs; Light-клиенты верифицируют блоки без полного state; verify_block(block, witness) -> bool в consensus». Отклонение (FBP): Verkle замена SMT — consensus-breaking, SCIP-gated (ADR-0006 + residual Stage 2 §3); Stage 3 делает wire+node path поверх **текущего SMT** — функциональный light-client DoD достигается, Verkle — отдельный SCIP.

**Задачи:**

1. `crates/strangecoin-core/src/state/witness.rs`: canonical binary encode/decode для `StateWitness`/`AccountProof` (формат: version byte + sorted-by-address proofs, повторяя детерминированный порядок; format_version = FORMAT_VERSION-согласованный или отдельный WITNESS_FORMAT_VERSION=1); round-trip тесты + golden vector.
2. Monolith node path (`src/blockchain/`):
   - `BlockchainFacade::build_witness_for_block(height) -> StateWitness` (реконструировать pre-state на height через replay до height-1 — expensive, документировать; или из state_history CF если покрывает window — optimization, документировать fallback)
   - `verify_incoming_block_with_witness(block, witness) -> Result<()>` — для будущих stateless relay-путей (wire-integration — residual Stage 4+; здесь — API + тесты)
3. E2E (`tests/stateless_light_client.rs`): node A майнят цепочку; node B (temp dir, **без** загрузки state) получает блок + witness (in-process), `verify_block_stateless` → Ok; повреждённый witness (flip proof byte) → Err (WitnessVerificationFailed); witness без touched address → Err.
4. Wire: НЕ интегрируется в strangecoin-net message enum (Stage 4 residual, зафиксировать).

**Артефакты:**
- `crates/strangecoin-core/src/state/witness.rs` (+ encode/decode)
- `src/blockchain/blockchain_facade.rs` (witness API)
- `tests/stateless_light_client.rs`

**КГ:**
- [ ] Canonical binary witness codec + round-trip + golden vector (core, 0 I/O соблюдён)
- [ ] Facade: build_witness_for_block + verify_incoming_block_with_witness
- [ ] E2E: light verification Ok + negative-тесты (corrupt/omitted proof) — зелёные
- [ ] Verkle-deviation (SMT-based, SCIP-gated Verkle) зафиксирована явно
- [ ] Wire-integration — residual (Stage 4+) зафиксирован
- [ ] `cargo test --workspace` зелёный (чанками); clippy green
- [ ] Коммит `[S3-P11] stateless: witness wire codec + light client e2e (SMT-based)`

---

### S3-P12. State pruning: state_history height-window + archival mode + expiry markers

**Цель:** реализовать pruning по ADR-0025: full-нода прюнит `state_history` старше retention_blocks; `node_mode = "archival"` хранит всё; state-expiry **markers** (touch-tracking для будущего SCIP), БЕЗ удаления current state; disk-usage test (рост замедляется/уменьшается после prune sweep).

**Контекст:** ADR-0025 — гейт (готов). `state_history` CF пишется с S3-P07. Триггер — height-based sweep (`pruning.sweep_every_blocks`), инжектируемый height (детерминизм, паттерн tick_at).

**Задачи:**

1. `src/storage/pruning.rs` (или strangecoin-storage/src/pruning.rs — prefer крейт, monolith вызывает):
   - `pub fn sweep(engine: &dyn StorageEngine, tip_height: u64, cfg: PruningConfig) -> PruneReport` — iterate `state_history` prefix, delete записи с height < tip − retention; batch delete; sorted deterministic
   - Archival check: mode archival → sweep = no-op (возвращает skipped)
   - `PruneReport { pruned_keys, freed_bytes_estimate }` — tracing::info
2. Интеграция: вызов из adopt-path каждые sweep_every_blocks (height-based); config wiring `[pruning]` + `[node] mode`.
3. Expiry markers: при sweep — для addresses, не видевших touch retention-окно, писать `expiry_marker` (meta или state_history sentinel; НЕ удалять state) — данные для будущего SCIP state expiry; документировать.
4. Тесты: 6000 блоков (или smaller window в тесте: retention=100, 300 блоков) → sweep → state_history keys < window удалены, current state цел, blocks целы; archival — ничего не прюнится; disk-usage smoke: DB size после prune < size без prune (при одинаковой нагрузке — мягкий assert через估算 ключей, не bytes, т.к. compaction асинхронна — deviation зафиксировать).

**Артефакты:**
- `crates/strangecoin-storage/src/pruning.rs`
- `src/config.rs` (`[pruning]`, `[node] mode`)
- `tests/pruning.rs`

**КГ:**
- [ ] sweep: height-window deletes, archival no-op, deterministic order — протестированы
- [ ] Current state/blocks не прюнятся (негативный тест)
- [ ] Expiry markers пишутся, current state НЕ удаляется (SCIP-boundary соблюдён)
- [ ] Config wiring `[pruning]`/`[node] mode` протестирован
- [ ] Disk-usage smoke + deviation (keys vs bytes) зафиксированы
- [ ] `cargo test --workspace` зелёный; clippy green
- [ ] Коммит `[S3-P12] pruning: state_history height-window sweep + archival mode`

---

### S3-P13. Snapshot sync: export/import CLI + commitment verification + new-node sync test

**Цель:** реализовать snapshot per ADR-0026: `sc snapshot export/import` (CLI), формат `.scsnap` (header + sorted accounts/storage + blake3 trailer), 4-step верификация перед применением, generation каждые N blocks (по команде/default cadence hook), e2e «new node из snapshot догоняет tip без full chain download».

**Контекст:** ADR-0026 — гейт (готов). CLI живёт в `src/cli/` (прецедент shadow replay S2-P20: `sc shadow replay`, `#[cfg(feature="net")]` не нужен — storage tooling). Import: state CF + tip = snapshot.height → headers-first sync (Stage 1) догоняет.

**Задачи:**

1. `crates/strangecoin-storage/src/snapshot.rs` (format + verify + apply) + `src/cli/snapshot.rs` (export/import subcommands, ре-экспорт в `src/cli/mod.rs` + main dispatch):
   - `export(engine, out_path, cfg)`: header + accounts (sorted) + contract storage (sorted) + trailer checksum
   - `verify_file(path, expected_chain_id) -> SnapshotHeader` (4-step per ADR: checksum → chain_id → state_root recompute → готовность к apply)
   - `import(engine, path)`: verify → write state/contract CF (batch) + meta tip → report
   - Generation hook: `--every N` при export; автогенерация по cadence — опциональный фоновый hook в Node (если trivial; иначе CLI-only + deviation «auto-generation — ops cron»)
2. E2E (`tests/snapshot_sync.rs`): node A 200 блоков → export → node B fresh → import → assert: state_root B == state_root A@height, tip == height; node B sync (in-process network или direct block application оставшихся блоков) → tip A; **forged test**: flip account balance в файле → state_root mismatch → reject; corrupt checksum → reject; wrong chain_id → reject.
3. Tooling: отдельный бинарь не нужен — `sc snapshot ...` subcommand (прецедент `sc shadow replay`).

**Артефакты:**
- `crates/strangecoin-storage/src/snapshot.rs`
- `src/cli/snapshot.rs` (+ dispatch)
- `tests/snapshot_sync.rs`

**КГ:**
- [ ] Export/import + format per ADR-0026 (magic, header fields, sorted body, checksum)
- [ ] 4-step verify: forged/corrupt/wrong-chain_id negative-тесты — зелёные
- [ ] E2E: new node из snapshot → verify state_root → догоняет tip (без full replay)
- [ ] Generation cadence: CLI + (hook или deviation «ops cron»)
- [ ] Wire GET_SNAPSHOT — residual (Stage 4+) зафиксирован
- [ ] `cargo test --workspace` зелёный; clippy green
- [ ] Коммит `[S3-P13] snapshot: export/import + commitment verification + sync e2e`

---

### 2.5. Verification + закрытие Stage 3 (S3-P14–S3-P15)

---

### S3-P14. Регрессия 22 инвариантов + threat model Stage 3

**Цель:** regression-проверка всех Stage 0/1/1.5/2 инвариантов (22/22) на новом storage backend + расширение инвариантов Stage 3 (schema versioning, crash recovery, pruning boundary, snapshot verification) + threat model update (`docs/stage3/THREAT_MODEL_STAGE3.md` + `docs/security/THREAT_MODEL.md` → v3.3: snapshot forging/withholding, RocksDB crash-recovery divergence, witness wire forgery) + residuals (`docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md`: wire snapshot, Verkle SCIP, state expiry SCIP, Block-STM, receipts prune SCIP).

**Контекст:** прецедент S2-P23 (invariants + threat + residuals в одном промпте). DoD Stage 3: «All Stage 0-2 invariants still enforced» — 22/22 + новые. Особое внимание: invariant #1 (rebuild from chain побеждает кеш) на RocksDB; #16/#17 не трогаются (net); #22 reproducible builds — v3.0.0 release pipeline (S3-P15).

**Задачи:**

1. `docs/stage3/INVARIANTS_ENFORCED.md`: таблица 22 baseline + Stage 3 extensions (новые: #23 schema_version guard — open с mismatched version fail-closed; #24 crash atomicity — kill во время write → reopen консистентен; #25 snapshot commitment — import без верификации невозможен; #26 pruning boundary — current state/blocks не прюнятся) → итог 26/26 (или 22 + 4 extensions, формат как в `docs/stage2/INVARIANTS_ENFORCED.md`).
2. `docs/stage3/THREAT_MODEL_STAGE3.md`: V-snapshot-forged, V-snapshot-withholding, V-witness-forgery (wire omission), V-rocksdb-crash-divergence, V-pruning-replay-weakness — mitigations per prompt.
3. `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md`: wire snapshot sync (Stage 4+), Verkle SCIP (ADR-0006 amendment), state expiry SCIP, receipts/contract-storage prune SCIP, Block-STM (Stage 3+), state rent (SCIP), TLA+ network (Stage 6), Erlay full minisketch SCIP (carry-over), SCIP-0001 + ops-gates (carry-over mainnet).
4. `docs/security/THREAT_MODEL.md`: bump v3.2 → v3.3, добавить Stage 3 vectors (строка в соответствующей секции).
5. Прогон: `cargo test --workspace` (чанками) — все существующие тесты green (regression evidence: счётчики в коммите).

**Артефакты:**
- `docs/stage3/INVARIANTS_ENFORCED.md`
- `docs/stage3/THREAT_MODEL_STAGE3.md`
- `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md`
- `docs/security/THREAT_MODEL.md` (v3.3)

**КГ:**
- [ ] INVARIANTS_ENFORCED: 22 baseline + Stage 3 extensions (≥4 новых) — evidence paths
- [ ] THREAT_MODEL_STAGE3: ≥5 vector'ов с mitigations + cross-ref residual risks Stage 2 §1
- [ ] RESIDUAL_RISKS_STAGE4_PLUS: carry-over + новые (SCIP-gated items перечислены)
- [ ] THREAT_MODEL.md v3.3 обновлён
- [ ] Полный workspace test-run green (чанками, счётчики в коммите); clippy green
- [ ] Коммит `[S3-P14] docs: invariants + threat model update (Stage 3 snapshot/rocksdb/witness/pruning)`

---

### S3-P15. DoD-верификация Stage 3 + Changelog 3.0.0 + тег v3.0.0-stage3

**Цель:** финальная верификация 11 DoD-критериев ROADMAP3 Этап 3 с evidence; `docs/stage3/STAGE3_SUMMARY.md`; Changelog 3.0.0; bump версий (root 3.0.0, core 3.0.0, storage 0.1.0 unchanged «первый release»); AGENTS.md Stage 3 sections (по FBP10 — в этом же коммите, артефакты уже существуют); annotated tag `v3.0.0-stage3` — Gate Stage 4/5.

**Контекст:** ROADMAP3 Этап 3 DoD (11 критериев, см. §4 ниже). Прецедент S2-P24 (DoD summary + release + tag). Tag message format — по образцу S2-P24 (аннотированный, prompt references, DoD status).

**Задачи:**

1. `docs/stage3/STAGE3_SUMMARY.md`: Header (status, date, S3-P01..S3-P15 refs), DoD table 11 строк с evidence (test file + line + результат), residual obligations (SCIP-gated items, wire snapshot, Verkle, ops-gates BUG-S1-013/015 + SCIP-0001 carry-over), release section (versions, platforms, pipeline run id — «не верифицировано локально + причина» если нет gh CLI).
2. `Changelog.md`: секция `3.0.0 — RocksDB storage + stateless validation + pruning + snapshot sync` (date, highlights, breaking: storage format v1, LevelDB removed, storage path default change, `storage.engine` flag removed после миграции).
3. Version bumps: root `3.0.0`, `crates/strangecoin-core` `3.0.0` (witness codec), `crates/strangecoin-storage` остаётся `0.1.0`; vm-wasm/net — unchanged; `Cargo.lock` синхронизирован.
4. `AGENTS.md`: Stage 3 sections (§Active Stage → complete, Dependency Map, Pin Policy rocksdb, Determinism notes, Architecture update storage crate, DoD table, pitfalls Windows-rocksdb, Forbidden Updates FBP10) — согласованно с фактическими артефактами.
5. Tag `v3.0.0-stage3` annotated (message: Stage 3 complete, prompts S3-P01..S3-P15, DoD 11/11, gate Stage 4/5) — строго после зелёного `cargo test --workspace` + clippy; push tag (как S2-P24).
6. ARCHITECT3 §17 checklist — прогнать применимые к Stage 3 пункты (strangler ✓, storage schema versioned ✓, tests ✓, reproducible builds ✓ regression).

**Артефакты:**
- `docs/stage3/STAGE3_SUMMARY.md`
- `Changelog.md` (3.0.0)
- `Cargo.toml` + `crates/strangecoin-core/Cargo.toml` (+ Cargo.lock)
- `AGENTS.md` (Stage 3 sections)
- annotated tag `v3.0.0-stage3`

**КГ:**
- [ ] STAGE3_SUMMARY.md: все 11 DoD с evidence (test/line) — сверено с фактическими файлами
- [ ] Residual obligations перечислены (SCIP-gated: Verkle/expiry/receipts-prune; wire snapshot; ops-gates; Block-STM)
- [ ] Changelog 3.0.0 (breaking changes перечислены)
- [ ] Версии: root/core 3.0.0, storage 0.1.0, vm-wasm/net unchanged; Cargo.lock зелёный
- [ ] `cargo test --workspace` (чанками) + `cargo clippy --all-targets -- -D warnings` — green ДО тега
- [ ] AGENTS.md Stage 3 секции — только факты, подтверждённые коммитами (FBP10)
- [ ] Annotated tag `v3.0.0-stage3` создан и запушен; git ls-remote подтверждает
- [ ] Коммит `[S3-P15] release: stage3 dod verification + summary + tag v3.0.0-stage3`

---

## 3. Жёсткие гейты (dependency gates)

1. **Tag gate = Gate Stage 3**: ни один промпт S3-P01+ не начинается до подтверждения тега `v2.0.0-stage2`. ✅ Precondition verified 2026-10-30 (§0.1).
2. **ADR до кода**: S3-P04 не без S3-P01; S3-P12 не без S3-P02; S3-P13 не без S3-P03. ADR-0008 — superseded-in-part by ADR-0024 (не перезаписывается).
3. **Crate scaffolding до schema/backend**: S3-P05 не без S3-P04; S3-P06 не без S3-P05.
4. **Backend до миграции**: S3-P07 не без S3-P06; S3-P08 не без S3-P07 (мигратор пишет в новую схему — схема должна жить); S3-P09 не без S3-P08; S3-P10 не без S3-P09.
5. **Feature-track после storage core**: S3-P11/S3-P12/S3-P13 — после S3-P10 (∥ между собой).
6. **All proper до verification**: S3-P14 не без S3-P11..S3-P13; S3-P15 не без S3-P14.
7. **S3-P15 = Gate Stage 4/5**: Stage 4 (dev-experience) не начинается до тега `v3.0.0-stage3` (мягкая зависимость Stage 3∥Stage 4 допустима только по storage-независимым задачам Stage 4; indexer/JSON-RPC требуют storage crate).

---

## 4. Покрытие DoD Этапа 3 (ROADMAP3) после S3-P01..S3-P15

| # | Критерий | Промпт | Evidence (после закрытия) |
|---|----------|--------|---------------------------|
| 1 | RocksDB миграция завершена, LevelDB удалён | S3-P07 + S3-P08 | monolith на strangecoin-storage; rusty-leveldb удалён из runtime deps |
| 2 | Stateless validation работает (light client верифицирует block без полного state) | S3-P11 | `tests/stateless_light_client.rs` |
| 3 | State pruning работает (disk usage уменьшается после N блоков) | S3-P12 | `tests/pruning.rs` (keys/prune report + size smoke) |
| 4 | Snapshot sync работает (new node без full chain download) | S3-P13 | `tests/snapshot_sync.rs` |
| 5 | Indices работают (O(1) lookups) | S3-P09 | `tests/indices.rs` (txid→height, hash→height, storage_root) |
| 6 | Performance: batch validation ≥10x быстрее sequential | S3-P10 | microbench (ratio в STAGE3_SUMMARY; CI — не верифицируемо локально) |
| 7 | `crates/strangecoin-storage/` создан, storage перенесён | S3-P04..S3-P07 | workspace member + monolith wiring |
| 8 | All Stage 0-2 invariants still enforced | S3-P14 | `docs/stage3/INVARIANTS_ENFORCED.md` (22 baseline + extensions) |
| 9 | ADR «Why RocksDB instead of redb» написан | S3-P01 | **уже**: `docs/ADR/0008-rocksdb-plan.md` (Accepted) + аменда ADR-0024 |
| 10 | ADR «State pruning strategy» написан | S3-P02 | `docs/ADR/0025-state-pruning-strategy.md` |
| 11 | `Changelog.md` обновлён: `3.0.0 — RocksDB + stateless + pruning` | S3-P15 | Changelog 3.0.0 + tag `v3.0.0-stage3` |

Плюс ADR-0026 (snapshot format) — ROADMAP3 «ADRs required», S3-P03.

---

## 5. Process rules (§6 Stage 2 precedent, adapted)

1. **Порядок执行**: строго по карте зависимостей (§1). Гейты — §3.
2. **КГ обязательны**: нельзя закрывать промпт без всех пунктов; не прошёл — итерируй тот же промпт.
3. **§0 «Общий контекст»** — вставлять в каждый промпт при отправке агенту; НЕ дублировать в файле промпта.
4. **Измеримость**: прогресс = созданные файлы / ожидаемые; коммиты по одному на промпт, формат `[S3-PNN] <area>: <short description>` (areas: `adr`, `crate`, `schema`, `backend`, `migrate`, `indices`, `perf`, `stateless`, `pruning`, `snapshot`, `invariants`, `release`).
5. **Страховки retro §8.1**: Changelog/README/AGENTS.md — только коммитом с уже существующими артефактами (FBP10); «done» — только через S3-P15 (тег); не-локально-верифицируемое (CI cross-build rocksdb 3 платформ, bench ratio в CI, release pipeline) — «не верифицировано + причина»; отклонения — явно в коммите/ADR.
6. **ARCHITECT3 §17**: прогнать после S3-P15 (strangler ✓, storage schema versioned ✓, tests ✓, reproducible builds ✓).
7. **Determinism**: key-encoding — pure; prune/snapshot триггеры — height-based; witness codec — sorted deterministic order. Ревью на детерминизм — пункт КГ S3-P05/S3-P11/S3-P12/S3-P13 (V-21 extension).
8. **Pin policy Stage 3**: `rocksdb = "=x.y.z"` (резолв в S3-P06, фиксация в ADR-0024 + AGENTS.md); обновление — SCIP после mainnet freeze. Прочие новые deps (lru, criterion dev) — pinned тоже, если входят в storage-крейт.
9. **Consensus boundary**: storage — implementation detail (не consensus); pruning current-state / state expiry / receipts prune / Verkle — SCIP-gated (mainnet не запущен, но эти изменения СТАНОВЯТСЯ consensus-relevant при запуске — фиксировать SCIP-path в ADR).
10. **Ops-gates carry-over**: BUG-S1-013/015 + SCIP-0001 — НЕ блокируют Stage 3, фиксируются в STAGE3_SUMMARY.md residual (прецедент S2-P24).
11. **Windows host**: preamble PATH перед каждым cargo (rocksdb C++ — blake3/secp256k1-прецедент); запрещено CC_*/AR_* env-vars; долгие команды — `cmd /c "... > target\x.log 2>&1"`; тесты чанками (`-p strangecoin-storage` → `-p strangecoin` → `-p strangecoin-core` → ...).

---

## 6. Что НЕ входит в Stage 3 (явно отложено)

Если промпт случайно затрагивает — остановить и переориентировать:

- **Verkle trie замена SMT** — SCIP-gated (ADR-0006 amendment; consensus-breaking); Stage 3 = witness-over-SMT (S3-P11).
- **State expiry (dormant account removal из current state)** — SCIP-gated; Stage 3 = markers only (S3-P12).
- **Receipts prune / contract storage prune** — SCIP-gated (residual Stage 1.5/2); не Stage 3.
- **State rent** — SCIP, optional Stage 3+; не Stage 3.
- **Block-STM (parallel execution)** — Stage 3+; anti-goal здесь (differential serial-vs-parallel — отдельный трек).
- **Wire snapshot sync (GET_SNAPSHOT P2P)** — Stage 4+ residual (S3-P13 file-based только).
- **Wire witness integration в gossip/sync** — Stage 4+ residual (S3-P11 node API только).
- **SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet, cargo-strangecoin, HD wallets, Ledger** — Stage 4.
- **Account Abstraction, EIP-1559, threshold encryption mempool** — Stage 5.
- **WIT/Wasm Component Model ABI** — Stage 4+.
- **K-framework, ZK-VM, external audit, TLA+ network** — Stage 6+.
- **BLS12-381 + bls_aggregate, PoS/Casper, inclusion lists, validator ops** — Stage 7.
- **Multidimensional fees, PBS, batch auctions** — Stage 5+/6+ research.
- **Metrics (Prometheus), Docker, systemd** — Stage 4 operations.
- **Wallet перенос в отдельный крейт** (ARCHITECT3 §10.5 упоминает вместе со storage) — Stage 4+; Stage 3 scope = только storage (ROADMAP3 задачи Этап 3 не содержат wallet).
- **Персистентный SMT (trie-узлы в DB)** — residual ADR-0024; не Stage 3 (invariant #1 rebuild сохраняется).
- **Erlay full minisketch impl (impl_id=1)** — SCIP-gated carry-over Stage 2.
- **SCIP-0001, BUG-S1-013, BUG-S1-015** — mainnet freeze track, вне Stage 3.

---

## 7. Связь с другими документами

- `ROADMAP3.md` — de-facto Stage 3 backlog. См. «Этап 3 — Хранение и масштабируемость (RocksDB, stateless)» (задачи/DoD/риски/ADRs required).
- `ARCHITECT3.md` — целевая архитектура. См. §3.3 stateless/witness, §3.4 state_cache, §3.7 storage/pruning/snapshot, §10.5 strangler Stage 3, §15 anti-goals, §17 PR-checklist.
- `prompt-stage2.md` — шаблон/формат; §0 контекст-блок; §6 process rules; §7 anti-goals — образец для этого документа.
- `docs/ADR/0008-rocksdb-plan.md` — Accepted; RocksDB vs redb решён; Stage 3 амендит через ADR-0024.
- `docs/ADR/0006-verkle-trie-vs-smt.md` — Verkle baseline; SCIP-gated (§6 anti-goals).
- `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` — Stage 3 attack surface (snapshot/RocksDB/witness/Block-STM/pruning) — источник threat-моделинга S3-P14.
- `docs/stage2/INVARIANTS_ENFORCED.md` — 22/22 baseline (расширяется S3-P14 → `docs/stage3/INVARIANTS_ENFORCED.md`).
- `docs/stage2/STAGE2_SUMMARY.md` — Stage 2 closure; residual obligations carry-over.
- `docs/security/THREAT_MODEL.md` — STRIDE v3.2 → v3.3 (S3-P14).
- `docs/SCIP/scip-0000-process.md` — SCIP process (для Verkle/expiry/prune после mainnet freeze).
- `Changelog.md` — секции 2.0.0 (Stage 2); Stage 3 добавит 3.0.0 (S3-P15).
- `AGENTS.md` — Stage 2 complete; Stage 3 sections добавит S3-P15 (FBP10).

---

# 8. Extension prompts — AI Agent orientation + Professional UI (Stage 3+ / Stage 4)

**Версия расширения:** 1.0
**Дата добавления:** 2026-11-04
**Заказчик:** pivot позиционирования — Strangecoin как «криптовалюта для ИИ-агентов».
**Статус:** design-time only — промпты ниже НЕ являются частью DoD Stage 3 (S3-P01..S3-P15, §4) и НЕ блокируют тег `v3.0.0-stage3`. Это forward-looking design track, который фиксирует ADRs, спецификации и промпты для Stage 4 (AI agents / MCP / ACP / professional UI), чтобы дизайн был settled ДО открытия Stage 4 implementation gate. Все промпты имеют префикс `AI-P##` (agent integration) или `UI-P##` (professional UI) — нумерация НЕ пересекается с `S3-P##`.

**Мотивация расширения (из обновлённых `README.md` / `analytics/ARCHITECT.md` §19 / `analytics/ROADMAP.md` Этап 4):** Strangecoin позиционируется как L1, в котором автономные ИИ-агенты — первоклассные экономические акторы. Agent identity = wallet identity (secp256k1, как у MetaMask/Ledger), chain state экспонируется через **MCP (Model Context Protocol)** — любой MCP-совместимый LLM (Claude, GPT, Gemini, open-weight) читает блоки и сабмитит транзакции без custom-plugin, agent-to-agent платежи и координация задач — через **ACP (Agent Communication Protocol)** поверх существующего Noise P2P (Stage 2), demo-UI `eframe`/`egui` заменяется на профессиональный. Расширение инвариантов (#23..#27) и threat-векторов (V-43..V-48) зафиксировано в `ARCHITECT.md` §19.5/§19.6.

**Замечание про `analytics/BACKLOG.md`:** создан этим же патчем (см. §13) — сквозной бэклог AI/UI work items, который обновляется по мере закрытия `AI-P##` / `UI-P##`. До этого «бэклог Stage 4» жил в `ROADMAP.md` §«Этап 4» (de-facto), теперь — отдельный файл для traceability.

## 8.1. Общий контекст расширения (вставлять в начало каждого AI-P## / UI-P## промпта)

```
Проект: Strangecoin, Rust, edition 2021, версия 3.0.0 (после S3-P15, ожидаемо).
Позиционирование: криптовалюта для ИИ-агентов (ADR-0028).
Архитектурный baseline:
  - Stage 0..2 done (v2.0.0-stage2): PoW + secp256k1 EOA + bech32 + SMT state root
    + WASM VM (wasmi) + Noise P2P + BIP-152 compact blocks + Erlay + peer scoring
    + rate limit + 22 invariants enforced.
  - Stage 3 (в работе, S3-P01..S3-P15): RocksDB migration + stateless witnesses
    (wire StateWitness) + state pruning (EIP-4444 analog, height-window) + snapshot
    sync (file-based, commitment-verified) + O(1) indices + schema versioning.
  - Stage 4 (target этого расширения): SDK + devnet + indexer + JSON-RPC eth_* +
    agent_* + MCP server + ACP transport + professional UI + agent devnet/faucet.
Workspace (после Stage 3): ["." , crates/strangecoin-core, -vm-wasm, -net,
  -storage]. Stage 4 добавит: crates/strangecoin-mcp, -sdk, -ui (новые); -net
  расширяется ACP message handlers; src/api/ расширяется agent_* JSON-RPC;
  src/gui/ становится тонким launcher'ом поверх strangecoin-ui (strangler).
AI-agent design baseline (ADR-0028..0031, ARCHITECT.md §19):
  - Agent identity = wallet identity (secp256k1 EOA, HD path m/44'/1234'/agent_id').
  - MCP server (ADR-0029): tools get_block/get_balance/get_nonce/submit_tx/
    call_contract/estimate_fee/subscribe_events; resources chain://tip,
    chain://block/{h}, chain://tx/{txid}, chain://contract/{addr}; transports
    stdio (local) + HTTP+SSE (remote, Noise-XX auth); output determinism #23
    (pure-core reuse, signed by node_id Ed25519).
  - ACP (ADR-0030): wire messages ACP_NEGOTIATE / ACP_TASK / ACP_SETTLE /
    ACP_REVOKE поверх Noise P2P; escrow ссылается on-chain txid (#24); replay
    protection monotonic agent_nonce per (agent_id, counterparty) (#25);
    rate limit reuse Stage 2 token-bucket (V-44).
  - Agent JSON-RPC: agent_getSession, agent_signAndSubmitBatch,
    agent_sponsorTx (Stage 5 full, Stage 4 stub), agent_estimateTaskCost,
    agent_getActivity.
  - Professional UI (ADR-0031): crates/strangecoin-ui — design system +
    component library + 5 screens (wallet, node dashboard, agent activity,
    tx detail, settings); dark/light themes; WCAG AA accessibility (#27,
    axe-core CI gate); strangler migration src/gui/ → launcher.
Invariants baseline: 22 (Stage 0..2) + extensions #23..#27 (ARCHITECT.md §19.5).
Threat vectors: V-1..V-42 (Stage 0..2) + extensions V-43..V-48 (ARCHITECT.md
  §19.6).
Теги: v2.0.0-stage2 (verified, target 346e3c0); v3.0.0-stage3 (expected after
  S3-P15); v4.0.0-stage4 (expected after AI-P05 + UI-P05 + Stage 4 core).
Формат коммитов: [S4-AI-P##] <area>: <short description> (areas: ai-adr,
  mcp, acp, agent-rpc, agent-sdk, agent-devnet, ui-adr, ui-design-system,
  ui-screen, ui-migration, release).
```

## 8.2. Карта зависимостей промптов расширения

```
AI-P01 (ADR-0028: vision + agent account model)
   │
   ├──▶ AI-P02 (ADR-0029 + MCP server scaffold + tools/resources/transports)
   │       │
   │       └──▶ UI-P03..UI-P05 (UI screens потребляют MCP для agent activity feed)
   │
   ├──▶ AI-P03 (ADR-0030 + ACP wire messages + escrow + replay protection)
   │       │
   │       └──▶ AI-P04 (agent_* JSON-RPC зависит от ACP escrow txid)
   │
   ├──▶ AI-P04 (agent_* JSON-RPC: getSession, signAndSubmitBatch, sponsorTx stub,
   │            estimateTaskCost, getActivity)
   │       │
   │       └──▶ AI-P05 (SDK + agent devnet + multi-agent test использует все
   │                    agent_* методы + ACP)
   │
   └──▶ AI-P05 (Rust + TS + Python SDK; agent devnet + faucet; 3-agent ACP chain
                settle e2e test)

UI-P01 (ADR-0031: render backend selection + design system principles)
   │
   ├──▶ UI-P02 (design system + component library + design tokens)
   │       │
   │       ├──▶ UI-P03 (wallet screen migration)
   │       ├──▶ UI-P04 (node dashboard + tx detail)
   │       └──▶ UI-P05 (agent activity + settings + a11y audit + strangler closeout)
   │
   └──▶ UI-P05 (strangler closeout: src/gui/ → launcher; tests/two_clients.rs
                остаётся зелёным)
```

**Параллельность:** `AI-P02` и `AI-P03` — параллельны после `AI-P01`. `AI-P04` — после `AI-P03` (нужен ACP escrow txid). `AI-P05` — после `AI-P04`. `UI-P02` — после `UI-P01`. `UI-P03..UI-P05` — параллельны после `UI-P02`, но `UI-P05` закрывает strangler migration (нужны `UI-P03` + `UI-P04` готовы).

## 8.3. Жёсткие гейты расширения

1. **Stage 3 gate:** ни один `AI-P##` / `UI-P##` не начинается до тега `v3.0.0-stage3` (S3-P15). Stage 4 design track (ADRs + specs + промпты) можно писать параллельно Stage 3 implementation, но код — только после тега. Exception: ADRs `0028`..`0031` и spec-файлы (`docs/spec/agent_integration.md`, `mcp_server.md`, `acp_messages.md`, `ui_professional.md`) могут быть созданы во время Stage 3 как design-only artefacts — без зависимостей от кода.
2. **ADR до кода:** `AI-P02` код не пишет без `AI-P01` (ADR-0028) и `AI-P02` (ADR-0029). `AI-P03` — без `AI-P01` и своего ADR-0030. `UI-P02..UI-P05` — без `UI-P01` (ADR-0031).
3. **Pure-core boundary:** MCP handler и ACP message handler — тонкие shell'ы над pure-core; вся business logic — в `strangecoin-core` (extending trait bound `McpBackend` / `AcpBackend` impl'ится в `-net`/`-mcp`, не в core). MCP output determinism (#23) — pure-function requirement.
4. **No consensus change:** `AI-P##` / `UI-P##` промпты НЕ трогают consensus rules. Если промпт случайно затрагивает — остановить и переориентировать на SCIP (как Stage 3 §6 anti-goal precedent).
5. **`tests/two_clients.rs` green:** UI migration (`UI-P03..UI-P05`) НЕ ломает headline test. Strangler pattern — `src/gui/` остаётся launcher, `tests/two_clients.rs` не переписывается.
6. **WCAG AA gate:** `UI-P05` не закрывается без axe-core audit ≥ WCAG AA (#27) в CI.
7. **AI-P05 = Stage 4 agent-track gate:** Stage 5 paymaster full impl не начинается до `AI-P05` (stub достаточно для Stage 4, full impl — Stage 5).

## 8.4. Что НЕ входит в extension scope (явно отложено)

- **On-chain agent registry** — agent = любой EOA; регистрация off-chain (indexer, MCP). Консенсус не знает про «агентов». (anti-goal ARCHITECT.md §19.7)
- **Custom LLM-plugin protocol** — MCP — единый стандарт; один раз описали tools, любой MCP-client работает. (anti-goal §19.7)
- **ACP поверх plain TCP (без Noise)** — Noise XX + ChaCha20-Poly1305 уже Stage 2; не дублировать crypto. (anti-goal §19.7)
- **Paymaster full impl** — Stage 5 (Stage 4 — stub `agent_sponsorTx`, returns "not implemented").
- **Account Abstraction full impl** — Stage 5 (Stage 4 — только batched tx через WASM contract, без AA protocol-level).
- **EIP-1559 fee market** — Stage 5 (`agent_estimateTaskCost` на Stage 4 использует fixed gas price).
- **Wire ACP-over-gossip (relay через third peer)** — Stage 4 — direct peer-to-peer только; relayed ACP — Stage 5+ research.
- **ZK-proofs для ACP escrow** — Stage 6+ research; Stage 4 — on-chain txid verification только.
- **Multi-language SDK (Go, Java, C++)** — Stage 5+; Stage 4 — Rust + TS + Python только.
- **Mobile UI (iOS/Android)** — Stage 5+; Stage 4 — desktop only (retain eframe/egui per ADR-0031).
- **PoS validator agent integration** — Stage 7 (PoS миграция); Stage 4 — agent = EOA, не validator.

---

## 9. AI Agent integration prompts (AI-P01..AI-P05)

### AI-P01. ADR-0028: Strangecoin as cryptocurrency for AI agents (vision + account model)

**Цель:** зафиксировать архитектурное решение «Strangecoin — криптовалюта для ИИ-агентов»: agent identity = wallet identity (secp256k1 EOA, без отдельного registry), HD-деривация для sub-agents/session keys, key isolation (operator vs agent keystore), batched/sponsored tx через WASM-контракты (без consensus change), агент-нативные политики (multisig / session-key validators) — off-chain / contract-layer. Это решение определяет AI-P02..AI-P05 и UI-P03..UI-P05: без него MCP/ACP/UI пишутся «на ощупь», а смена account model позже потребует consensus-change.

**Контекст:** Stage 0..2 доставили secp256k1 EOA + bech32 + SMT state root + WASM VM (Stage 1.5) + Noise P2P (Stage 2). Stage 1.5 уже даёт batched tx через WASM (один `call` вызывает N внутренних `call`s) и programmable policy wallets через contract code. Не хватает: (1) явного позиционирования — agents как first-class actors; (2) HD-деривации для sub-agents (BIP-44 path `m/44'/1234'/<agent_id>'`); (3) key isolation — operator wallet (node-оператор) vs agent wallets (программные); (4) session-key pattern (короткий TTL, scoped permissions); (5) agent-tx features surfaced в JSON-RPC и MCP. Альтернативы: (1) **On-chain agent registry** — consensus-breaking, требует нового account type, отклонено (anti-goal §19.7); (2) **Custom LLM-plugin protocol per vendor** — lock-in, отклонено (§19.7); (3) **Separate agent chain (L2/sidechain)** — adds bridge risk, отклонено для Stage 4 (research Stage 6+); (4) **Account Abstraction protocol-level** — Stage 5, не Stage 4. Решение: agent = EOA с HD-деривацией + WASM-contract policy wallets + off-chain registry (indexer/MCP), без consensus change.

**Задачи:**

1. Создать `docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md` по шаблону ADR-0001:
   - Status: Accepted (после review; Proposed → Accepted по факту land'а AI-P02)
   - Context: Stage 0..2 baseline (secp256k1 EOA, WASM, Noise, peer scoring); отсутствие agent-orientation в current state; market pull (MCP, A2A, ACP frameworks); Stage 1.5 WASM-contract capability для batched/sponsored tx без consensus change; threat vectors V-43..V-48 (ARCHITECT.md §19.6)
   - Decision:
     - **Agent identity = wallet identity.** Agent — это secp256k1 EOA, owned by agent runtime (не operator). Никакого нового account type, никакого on-chain registry.
     - **HD derivation.** BIP-44 path `m/44'/1234'/<agent_id>'/0/<sub_id>` для sub-agents и rotating session keys. `coin_type = 1234` (SLIP-44 registered — placeholder; S4-AI-P05 обрабатывает SLIP-44 registration).
     - **Key isolation.** Wallet trait делится на `OperatorKeystore` (доступ к node-оператор wallet) и `AgentKeystore` (доступ к agent keys, sandboxed). MCP/ACP handlers НЕ имеют доступа к `OperatorKeystore`. Compile-time enforcement через Rust type system (separate types, no shared trait object).
     - **Session keys.** Optional pattern: agent derives short-TTL session keypair (`m/44'/1234'/agent_id'/1/<session_id>`), scoped to specific contracts/methods/max-value, signed by master agent key. Validated by on-chain WASM policy wallet (Stage 1.5 contract).
     - **Batched tx.** One signature, N contract calls — implemented as WASM contract `BatchExecutor` that takes signed calldata batch and executes N internal `call`s. No consensus change.
     - **Sponsored tx (paymaster).** Stage 5 full impl; Stage 4 — stub `agent_sponsorTx` returns "not implemented". Paymaster contract pattern: WASM contract pays gas for agent tx, agent signs intended action, paymaster signs sponsorship.
     - **Off-chain agent registry.** Indexer (Stage 4) indexes agent activity (ACP messages, batched txs, sponsor txs); MCP `agent_getActivity` reads it. No on-chain registry.
   - Consequences: Positive (no consensus change, agent = EOA → MetaMask/Ledger compat preserved, MCP/ACP ride on existing Stage 2 transport, SDK reuses existing wallet primitives); Negative (HD-derivation adds key-management complexity для agent runtimes, session-key WASM contract needs audit, off-chain indexer becomes trust-source для agent reputation); Neutral (agent reputation — out of scope; indexer может быть replaced/swapped)
   - Alternatives considered: on-chain agent registry (consensus-breaking, отклонено); custom LLM-plugin protocol per vendor (lock-in, отклонено); separate agent L2 (bridge risk, Stage 6+ research); AA protocol-level (Stage 5)
   - Related: ADR-0029 (MCP server), ADR-0030 (ACP), ADR-0031 (Professional UI), ARCHITECT.md §19, ROADMAP.md Этап 4
2. Создать `docs/spec/agent_integration.md` — high-level spec: agent identity, HD path, key isolation, session keys, batched tx, sponsored tx (stub), off-chain registry, relationship к MCP (ADR-0029) / ACP (ADR-0030) / agent_* JSON-RPC (AI-P04) / SDK (AI-P05) / UI (ADR-0031).
3. Pin policy:HD derivation через `bip32` crate (или `secp256k1` builtin) — pinned `=x.y.z` (резолв в AI-P05, фиксация в этом ADR + AGENTS.md Pin Policy).
4. Зафиксировать в ADR явные отклонения от возможных альтернатив (FBP-прецедент): no AA protocol-level (Stage 5), no L2 (Stage 6+), no on-chain registry (never).

**Артефакты:**
- `docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md`
- `docs/spec/agent_integration.md`

**КГ:**
- [ ] ADR-0028 создан по template ADR-0001 (Status/Context/Decision/Consequences/Alternatives/Related)
- [ ] Agent identity = wallet identity зафиксировано (no new account type, no on-chain registry)
- [ ] HD path `m/44'/1234'/agent_id'` зафиксирован (с placeholder SLIP-44, AI-P05 обработает registration)
- [ ] Key isolation `OperatorKeystore` vs `AgentKeystore` зафиксировано (compile-time enforcement)
- [ ] Session-key pattern зафиксирован (TTL, scoped, WASM policy wallet)
- [ ] Batched tx — через WASM contract `BatchExecutor` (no consensus change) зафиксировано
- [ ] Sponsored tx — Stage 5 full, Stage 4 stub — зафиксировано
- [ ] Off-chain agent registry (indexer/MCP) зафиксировано
- [ ] `docs/spec/agent_integration.md` создан, cross-link к MCP/ACP/agent_*/SDK/UI specs
- [ ] Threat vectors V-43..V-48 referenced (ARCHITECT.md §19.6)
- [ ] Коммит `[S4-AI-P01] ai-adr-0028: strangecoin as cryptocurrency for ai agents + spec`

---

### AI-P02. ADR-0029 + MCP server scaffold (tools/resources/transports)

**Цель:** реализовать MCP (Model Context Protocol) сервер в новом крейте `crates/strangecoin-mcp/` — экспонирование chain state как MCP tools (`get_block`, `get_balance`, `get_nonce`, `submit_tx`, `call_contract`, `estimate_fee`, `subscribe_events`) и resources (`chain://tip`, `chain://block/{h}`, `chain://tx/{txid}`, `chain://contract/{addr}`), с двумя transport'ами — stdio (local, для LLM IDE plugins) и HTTP+SSE (remote, Noise-XX auth reusing Stage 2 transport). Output determinism (#23) — pure functions от chain state, signed by node_id Ed25519 key (V-43 mitigation).

**Контекст:** MCP (modelcontextprotocol.io) — открытый стандарт Anthropic, поддерживаемый Claude, GPT (via MCP-bridge), Gemini, open-weight LLMs через `mcp-cli`. JSON-RPC 2.0 base, three primitives: tools (callable functions), resources (read-only URIs), prompts (templates — опционально, отложить). Strangecoin node уже имеет JSON-RPC surface (Stage 4 вводит eth_* + agent_*); MCP — дополнительный facade, не replacement. Pure-core (Stage 1) уже даёт `Block`, `AccountState`, `Transaction`, `Receipt` — MCP handler только сериализует их в MCP-compatible JSON. Noise-XX handshake (Stage 2, ADR-0020) уже имплементирован в `crangecoin-net` — reuse для HTTP+SSE auth. Альтернативы: (1) **MCP-over-stdio only** — безопаснее (no network), но не работает для remote agents; (2) **MCP-over-HTTP+SSE only** — универсальнее, но требует auth; (3) **Custom JSON-RPC schema (non-MCP)** — теряем LLM ecosystem interop; (4) **MCP-over-WebSocket** — дополнительно к HTTP+SSE, но spec MCP 2024-11-05 fixирует stdio + HTTP+SSE — WebSocket не в spec. Решение: оба transport'а (stdio default для local IDE, HTTP+SSE для remote agents), auth через Noise-XX reuse для HTTP+SSE.

**Задачи:**

1. Создать `docs/ADR/0029-mcp-server.md`:
   - Status: Accepted (после AI-P02 implementation)
   - Context: MCP standard (modelcontextprotocol.io, spec version 2024-11-05), LLM ecosystem interop, pure-core reuse, Noise-XX auth reuse, output determinism requirement (#23), V-43 threat (MCP tool poisoning)
   - Decision:
     - **New crate** `crates/strangecoin-mcp/` (workspace member, feature `mcp` default-on Stage 4)
     - **Tools** (7): `get_block(height)`, `get_balance(address)`, `get_nonce(address)`, `submit_tx(signed_tx_bytes)`, `call_contract(address, method, args)`, `estimate_fee(tx)`, `subscribe_events(filter)` (SSE stream)
     - **Resources** (4): `chain://tip` (latest block header), `chain://block/{height_or_hash}`, `chain://tx/{txid}`, `chain://contract/{address}` (code + storage root)
     - **Transports**: stdio (default, local only, no auth — relies on process isolation), HTTP+SSE (remote, requires Noise-XX handshake reuse from Stage 2 `strangecoin-net::noise`)
     - **Output determinism (#23)**: every tool output is a pure function of chain state — no `SystemTime::now()`, no random, no peer-state. Same `get_block(h)` returns byte-identical JSON on every node. Enforced by `cargo test -p strangecoin-mcp --features determinism-fuzz` (N-node cross-check).
     - **Response signing (V-43)**: every MCP response carries `signature: Ed25519(node_id, canonical_json(response))` — client verifies against peer's advertised node_id (Stage 2 `HELLO` message). Unsigned responses — rejected by spec-compliant client.
     - **MCP spec compliance**: implement MCP `initialize`/`initialized`/`tools/list`/`tools/call`/`resources/list`/`resources/read`/`notifications/*` lifecycle; reference impl `mcp-cli` passes round-trip
     - **Auth for HTTP+SSE**: reuse Noise-XX handshake (ADR-0020) — agent presents its `node_id` (or ephemeral key derived from agent keypair), node responds with Noise-XX, transport encrypted with ChaCha20-Poly1305
   - Consequences: Positive (LLM ecosystem interop без custom-plugin, pure-core reuse сохраняет determinism, Noise-XX reuse без new crypto); Negative (stdio — local only, HTTP+SSE требует running server, MCP spec churn risk — spec 2024-11-05 может смениться); Neutral (MCP не replacement для JSON-RPC eth_* — параллельный facade для LLM-consumers)
   - Alternatives considered: stdio-only, HTTP+SSE-only, custom non-MCP schema, MCP-over-WebSocket
   - Related: ADR-0028 (agent vision), ADR-0020 (Noise), ARCHITECT.md §19.3
2. Создать `crates/strangecoin-mcp/`:
   - `Cargo.toml`: `strangecoin-mcp = { version = "0.1.0", features = ["stdio", "http-sse"] }`, deps `strangecoin-core = { path = "../strangecoin-core" }`, `serde_json`, `tokio` (for SSE), `snow` (Noise-XX reuse, pin policy from Stage 2)
   - `src/lib.rs`: `McpBackend` trait (impl in `McpNode` over `Arc<BlockchainFacade>`); `McpServer` — JSON-RPC 2.0 handler, dispatches tools/resources
   - `src/stdio.rs`: stdio transport (tokio stdin/stdout, newline-delimited JSON-RPC)
   - `src/http_sse.rs`: HTTP+SSE transport (axum or hyper, SSE for `subscribe_events`)
   - `src/tools/{get_block,get_balance,get_nonce,submit_tx,call_contract,estimate_fee,subscribe_events}.rs` — pure functions over `McpBackend` trait
   - `src/resources/{tip,block,tx,contract}.rs` — URI handlers
   - `src/auth.rs`: Noise-XX reuse from `strangecoin-net::noise`, agent keypair → ephemeral session key
3. Создать `docs/spec/mcp_server.md` — spec: tool schemas (JSON), resource URI schemes, transport details, auth flow, determinism requirement, response signing, lifecycle (`initialize`/`initialized`/etc.)
4. Создать `tests/mcp_server.rs`:
   - `mcp_stdio_get_block_round_trip` — stdio transport, `tools/call get_block(1)` → JSON response, byte-identical across N=3 nodes (#23)
   - `mcp_http_sse_auth` — HTTP+SSE requires Noise-XX; reject unauthenticated
   - `mcp_response_signature` — every response carries Ed25519 signature; client verifies
   - `mcp_cli_reference_compat` — round-trip с reference `mcp-cli` client (документировать setup)
5. CI: `cargo test -p strangecoin-mcp` green; `cargo clippy -p strangecoin-mcp -- -D warnings` green; `cargo +nightly fuzz run mcp_determinism -- -max_total_time=600` (target: N-node cross-check determinism)

**Артефакты:**
- `docs/ADR/0029-mcp-server.md`
- `docs/spec/mcp_server.md`
- `crates/strangecoin-mcp/Cargo.toml`, `src/{lib,stdio,http_sse,auth}.rs`, `src/tools/*.rs`, `src/resources/*.rs`
- `tests/mcp_server.rs`
- `fuzz/fuzz_targets/mcp_determinism.rs`

**КГ:**
- [ ] ADR-0029 создан по template; Status: Accepted
- [ ] `crates/strangecoin-mcp/` — workspace member; `cargo build -p strangecoin-mcp` green
- [ ] 7 tools + 4 resources реализованы; schemas в `docs/spec/mcp_server.md`
- [ ] stdio + HTTP+SSE transports работают; auth для HTTP+SSE через Noise-XX reuse
- [ ] Output determinism (#23) — `cargo test --features determinism-fuzz` green; 3-node cross-check
- [ ] Response signing Ed25519 (#V-43 mitigation) — `tests/mcp_server.rs::mcp_response_signature` green
- [ ] `mcp-cli` reference client round-trip проходит (documented setup)
- [ ] `cargo clippy -p strangecoin-mcp -- -D warnings` green
- [ ] `cargo +nightly fuzz run mcp_determinism -- -max_total_time=600` no panic
- [ ] Pin policy: `snow` pinned (Stage 2 precedent); `serde_json` pinned (storage precedent)
- [ ] Коммит `[S4-AI-P02] mcp: server scaffold + 7 tools + 4 resources + stdio/http-sse transports`

---

### AI-P03. ADR-0030 + ACP (Agent Communication Protocol) wire messages + escrow + replay protection

**Цель:** реализовать ACP — семейство wire-сообщений `ACP_NEGOTIATE` / `ACP_TASK` / `ACP_SETTLE` / `ACP_REVOKE` поверх существующего Noise-authenticated P2P-слоя (Stage 2, `strangecoin-net`), чтобы агенты могли peer-to-peer договариваться о задачах, котировать и платить за услуги, эскроуить micropayments — без централизованного marketplace. Escrow ссылается on-chain txid (#24); replay protection monotonic `agent_nonce` per `(agent_id, counterparty)` (#25); rate limit reuse Stage 2 token-bucket (V-44).

**Контекст:** Stage 2 доставил Noise-XX + ChaCha20-Poly1305 transport с peer scoring + rate limit. ACP — application-layer поверх этого transport, не новый transport. Сообщения reference on-chain txid для escrow (agent A платит agent B за задачу: `ACP_NEGOTIATE` (quote) → `ACP_TASK` (assignment + escrow txid) → `ACP_SETTLE` (release escrow) или `ACP_REVOKE` (refund escrow после timeout/dispute)). Replay protection — monotonic `agent_nonce` per `(agent_id, counterparty)` (in-memory map, backed by RocksDB `meta` CF). Альтернативы: (1) **On-chain escrow (WASM contract)** — publicly verifiable, но gas cost per message недопустим для micropayments; (2) **Off-chain HTLC-style** — дешевле, но требует indexer trust; (3) **Centralized marketplace** — отклонено (anti-goal §19.7 — без центра); (4) **ACP-over-plain-TCP без Noise** — отклонено (anti-goal §19.7). Решение: ACP-over-Noise (Stage 2 transport), escrow reference on-chain txid (verification через `StorageEngine::get_tx(txid)`, invariant #24), off-chain settlement (HTLC-like optional Stage 5+).

**Задачи:**

1. Создать `docs/ADR/0030-acp-agent-communication-protocol.md`:
   - Status: Accepted (после AI-P03 implementation)
   - Context: Stage 2 Noise transport, agent-to-agent use cases (task delegation, micropayment, escrow), threat V-44 (ACP spam), V-45 (escrow fraud), invariants #24/#25, anti-goal §19.7 (no central marketplace, no plain TCP)
   - Decision:
     - **Message family** (4): `ACP_NEGOTIATE` (agent A → B: «what's your quote for task T?»), `ACP_TASK` (A → B: «assigned T, escrow txid=X, deadline=D»), `ACP_SETTLE` (A → B: «task done, release escrow to B»), `ACP_REVOKE` (A or B: «cancel, refund escrow to A»)
     - **Wire format**: extend `strangecoin-net::protocol` enum with `ACP_NEGOTIATE/ACP_TASK/ACP_SETTLE/ACP_REVOKE` variants; payload: serde canonical JSON (signed by agent keypair Ed25519/secp256k1, NOT node_id); `agent_nonce: u64` per `(agent_id, counterparty)`; `escrow_txid: Option<[u8;32]>` for SETTLE/REVOKE
     - **Transport**: ACP-over-Noise (reuse Stage 2 `strangecoin-net::transport`); ACP messages ride existing peer connection (no new handshake); rate limit reuse Stage 2 token-bucket per-message-type sub-bucket for `ACP_*` (V-44)
     - **Escrow verification (#24)**: handler validates `escrow_txid` via `StorageEngine::get_tx(txid)` — must exist, must be `transfer` tx from agent A to escrow contract (WASM `EscrowVault` Stage 4 deliverable) with `amount ≥ quote`; reject + ban peer via Stage 2 peer scoring extension (V-45)
     - **Replay protection (#25)**: in-memory `HashMap<(agent_id, counterparty), last_nonce>`; reject if `agent_nonce <= last_seen`; backed by RocksDB `meta` CF (key `acp_nonce:{agent_id}:{counterparty}`) for restart-survival
     - **Settlement flow**: `ACP_SETTLE` → on-chain tx from EscrowVault → agent B (release); `ACP_REVOKE` → on-chain tx from EscrowVault → agent A (refund, after `deadline` timeout or mutual signature)
     - **No on-chain ACP message storage** — ACP messages are transport-layer only; indexer (Stage 4) indexes them off-chain for `agent_getActivity`
   - Consequences: Positive (no central marketplace, micropayments feasible, escrow publicly verifiable via txid, Stage 2 transport reuse без new crypto); Negative (EscrowVault WASM contract needs audit, ACP message loss → escrow lock until timeout, in-memory nonce map state loss без RocksDB backup); Neutral (ACP — application layer, not consensus; agents могут не использовать ACP и платить напрямую)
   - Alternatives considered: on-chain escrow (gas cost), off-chain HTLC (indexer trust), central marketplace (anti-goal), ACP-over-plain-TCP (anti-goal)
   - Related: ADR-0028 (agent vision), ADR-0020 (Noise), ADR-0022 (peer scoring), ARCHITECT.md §19.3/§19.5/§19.6
2. Расширить `crates/strangecoin-net/src/protocol/`:
   - `acp.rs` (new): `AcpMessage` enum, `AcpNegotiate`/`AcpTask`/`AcpSettle`/`AcpRevoke` structs, canonical serialization (serde + signed bytes)
   - `mod.rs`: extend `Message` enum with `ACP_*` variants; dispatch to `AcpHandler`
3. Создать `crates/strangecoin-net/src/acp/handler.rs`:
   - `AcpHandler` — processes incoming `ACP_*` messages, validates escrow txid (#24), checks nonce (#25), updates peer score on violation
   - Backed by `Arc<Mutex<HashMap<(AgentId, AgentId), u64>>>` for nonce map + `Arc<StorageEngine>` for txid lookup + `Arc<EscrowVaultContract>` for settle/revoke
4. Создать `crates/strangecoin-vm-wasm/examples/escrow_vault.rs` — WASM contract `EscrowVault`:
   - `deposit(agent_a, agent_b, amount, deadline) → escrow_id` (called by agent A)
   - `release(escrow_id, signature_a, signature_b) → transfer to agent_b` (called by AcpHandler on SETTLE)
   - `refund(escrow_id, signature_a OR deadline_passed) → transfer to agent_a` (called by AcpHandler on REVOKE)
5. Создать `docs/spec/acp_messages.md` — spec: message schemas, wire format, escrow flow diagrams, replay protection semantics, rate limit policy, peer scoring extensions
6. Создать `tests/acp_messages.rs`:
   - `acp_negotiate_task_settle_e2e` — two-agent session: A negotiates, B quotes, A assigns + escrows, B fulfills, A settles → on-chain release to B
   - `acp_revoke_after_timeout` — A assigns, B no response, deadline passes, A revokes → on-chain refund to A
   - `acp_escrow_fraud_rejected` — A sends SETTLE with fake txid → rejected + peer banned (#24, V-45)
   - `acp_replay_rejected` — A replays old `ACP_NEGOTIATE` with stale nonce → rejected (#25)
   - `acp_spam_rate_limited` — A floods `ACP_NEGOTIATE` → rate-limited by Stage 2 token-bucket (V-44)
7. CI: `cargo test -p strangecoin-net --features acp` green; `cargo clippy -p strangecoin-net -- -D warnings` green; `cargo +nightly fuzz run acp_handler -- -max_total_time=600`

**Артефакты:**
- `docs/ADR/0030-acp-agent-communication-protocol.md`
- `docs/spec/acp_messages.md`
- `crates/strangecoin-net/src/protocol/acp.rs`
- `crates/strangecoin-net/src/acp/handler.rs`
- `crates/strangecoin-vm-wasm/examples/escrow_vault.rs`
- `tests/acp_messages.rs`
- `fuzz/fuzz_targets/acp_handler.rs`

**КГ:**
- [ ] ADR-0030 создан по template; Status: Accepted
- [ ] 4 message types (`ACP_NEGOTIATE/TASK/SETTLE/REVOKE`) реализованы; canonical serialization
- [ ] Escrow txid verification (#24) — `tests/acp_messages.rs::acp_escrow_fraud_rejected` green
- [ ] Replay protection (#25) — `tests/acp_messages.rs::acp_replay_rejected` green
- [ ] Rate limit reuse Stage 2 (V-44) — `tests/acp_messages.rs::acp_spam_rate_limited` green
- [ ] EscrowVault WASM contract компилируется, unit tests pass
- [ ] `tests/acp_messages.rs::acp_negotiate_task_settle_e2e` green (full flow)
- [ ] Peer scoring extension для ACP violations — documented в ADR + `src/peer/scoring.rs` updated
- [ ] `docs/spec/acp_messages.md` создан, message schemas + flow diagrams
- [ ] `cargo clippy -p strangecoin-net -- -D warnings` green
- [ ] `cargo +nightly fuzz run acp_handler -- -max_total_time=600` no panic
- [ ] Коммит `[S4-AI-P03] acp: wire messages + handler + escrow vault + tests + spec`

---

### AI-P04. Agent-native JSON-RPC surface (agent_* methods + batched/sponsored tx helpers)

**Цель:** расширить Stage 4 JSON-RPC surface (`src/api/`) namespaced-методами `agent_*`: `agent_getSession`, `agent_signAndSubmitBatch`, `agent_sponsorTx` (Stage 5 full impl, Stage 4 — stub returning "not implemented"), `agent_estimateTaskCost`, `agent_getActivity`. Методы сосуществуют с `eth_*` (Ethereum tooling interop сохраняется) и `sc_*` (Strangecoin-specific). Reuse: `agent_signAndSubmitBatch` — thin wrapper над `BatchExecutor` WASM contract (AI-P01); `agent_estimateTaskCost` — gas estimator + ACP escrow cost (AI-P03); `agent_getActivity` — indexer query.

**Контекст:** Stage 4 core deliverable включает JSON-RPC `eth_*` (`eth_sendTransaction`, `eth_getBalance`, `eth_call`, `eth_getLogs`, `eth_subscribe`). Agent-runtime consumers (LangChain, AutoGen, CrewAI agents, custom Python/Rust agents) не хотят возиться с low-level eth_* + signing — им нужен high-level API: открыть session, submit batch одним вызовом, оценить cost, посмотреть свою активность. MCP (AI-P02) даёт LLM-friendly facade; JSON-RPC `agent_*` — programmatic facade для code-first agents. Альтернативы: (1) **Только MCP, без agent_*** — теряем code-first ergonomics; MCP optimized для LLM tool-calling, не для programmatic loops; (2) **Extend eth_* instead of new namespace** — ломает Ethereum tooling compat (`eth_*` должен соответствовать Ethereum schema); (3) **Separate port/server для agent_*** — ops complexity, no reuse JSON-RPC server; (4) **gRPC вместо JSON-RPC** — terяем browser/wget compatibility. Решение: тот же JSON-RPC server, новый namespace `agent_*`, methods reuse existing infra (signing, mempool, indexer).

**Задачи:**

1. Расширить `src/api/mod.rs` (или `crates/strangecoin-api/`, если Stage 4 уже вынес в крейт):
   - `agent_getSession(agent_keypair_pubkey) → session_id` — открыть session для agent, return session_id (random u128, scoped to agent pubkey + TTL default 1h, config `agent.session_ttl_sec`). Session даёт доступ к `agent_signAndSubmitBatch` / `agent_sponsorTx` / `agent_estimateTaskCost` / `agent_getActivity`. Auth: caller подписывает challenge своим agent keypair, server verifies secp256k1.
   - `agent_signAndSubmitBatch(session_id, [unsigned_tx; N]) → [txid; N]` — server использует `BatchExecutor` WASM contract (AI-P01): caller specifies N `unsigned_tx` (transfer / call / create); server constructs batch calldata, signs с agent keypair (session-scoped subkey), submits одним on-chain tx. Returns N txids (one per internal call, derived via `txid = blake3(batch_tx || index)`). Caller не подписывает каждый tx отдельно — одно signature на batch.
   - `agent_sponsorTx(session_id, unsigned_tx, paymaster_address) → txid | "not implemented"` — Stage 4: stub, returns `{"code": -32601, "message": "agent_sponsorTx not implemented until Stage 5 paymaster"}`. Stage 5: full impl через paymaster contract pattern.
   - `agent_estimateTaskCost(session_id, task_spec) → { gas_units, fee_sc, acp_escrow_cost_sc, total_sc }` — gas estimation через existing `eth_estimateGas` logic + ACP escrow overhead (deposit gas + settle gas, см. AI-P03 EscrowVault).
   - `agent_getActivity(session_id, agent_address, [filter]) → [activity_item; N]` — queries Stage 4 indexer (AI-P05 dependencies: indexer indexes agent txs + ACP messages off-chain); returns recent activity (transfers, batched txs, ACP negotiates/tasks/settles, sponsor txs).
2. Создать `src/api/agent/session.rs` — `SessionStore` (in-memory `HashMap<SessionId, AgentSession>`, TTL eviction via background task), `AgentSession { agent_pubkey, subkey_pubkey, expires_at, scope }`
3. Создать `src/api/agent/batch.rs` — `BatchExecutor` client (calls WASM contract `BatchExecutor.execute(calldata_batch, signature)` — deployed в AI-P05 devnet)
4. Создать `src/api/agent/activity.rs` — `ActivityQuery` (delegates to indexer `crates/strangecoin-indexer`)
5. Создать `tests/agent_rpc.rs`:
   - `agent_get_session_auth` — open session with valid secp256k1 signature, get session_id
   - `agent_get_session_reject_wrong_sig` — reject session с invalid signature
   - `agent_sign_and_submit_batch_3_txs` — submit 3-tx batch, verify 3 txids on-chain, all 3 settle correctly
   - `agent_sponsor_tx_stub` — Stage 4 stub returns "not implemented"
   - `agent_estimate_task_cost` — estimate gas for `transfer 100 SC + ACP escrow deposit 50 SC` — returns `{ gas_units, fee_sc, acp_escrow_cost_sc, total_sc }` matching on-chain actuals
   - `agent_get_activity` — submit batch + ACP settle, then `agent_getActivity` returns both items
6. CI: `cargo test --test agent_rpc` green; `cargo clippy -- -D warnings` green

**Артефакты:**
- `src/api/agent/{mod,session,batch,activity}.rs` (или `crates/strangecoin-api/src/agent/`)
- `tests/agent_rpc.rs`

**КГ:**
- [ ] 5 методов `agent_*` реализованы (getSession, signAndSubmitBatch, sponsorTx stub, estimateTaskCost, getActivity)
- [ ] Session auth через secp256k1 signature, TTL eviction
- [ ] `agent_signAndSubmitBatch` — `BatchExecutor` WASM contract reuse (AI-P01), N txids returned
- [ ] `agent_sponsorTx` — Stage 4 stub returns -32601 "not implemented"
- [ ] `agent_estimateTaskCost` — gas + ACP escrow cost accurate (test cross-checks with on-chain actuals)
- [ ] `agent_getActivity` — delegates to indexer, returns transfers + batched txs + ACP messages
- [ ] `eth_*` methods остаются byte-identical (Ethereum tooling compat preserved)
- [ ] `tests/agent_rpc.rs` — all 6 tests green
- [ ] `cargo clippy -- -D warnings` green
- [ ] JSON-RPC schema documented в `docs/spec/agent_integration.md` (updated by AI-P01 → cross-link)
- [ ] Коммит `[S4-AI-P04] agent-rpc: 5 agent_* methods + session store + batch executor + activity query`

---

### AI-P05. Agent SDK (Rust + TS + Python) + agent devnet + faucet + multi-agent e2e test

**Цель:** доставить agent SDK в трёх языках (Rust canonical, TypeScript и Python — generated из Rust через `ts-rs` / `pyo3`), one-command agent devnet (`cargo strangecoin agent devnet`) с faucet, который funds N agent wallets из seed, и end-to-end multi-agent test: 3 agents выполняют ACP task chain (A → B → C) с on-chain settle. Закрывает Stage 4 agent-track gate (§8.3 hard gate #7): Stage 5 paymaster full impl не начинается до закрытия AI-P05.

**Контекст:** Stage 4 доставил MCP server (AI-P02), ACP transport (AI-P03), `agent_*` JSON-RPC (AI-P04). Agent-runtime consumers (LangChain/AutoGen/CrewAI на Python, custom TypeScript/Node agents, Rust-native agents) хотят SDK: high-level classes/functions вместо raw JSON-RPC calls. Cookbook examples нужны для onboarding. Multi-agent e2e test нужен для regression — что весь stack (wallet + signing + batch + ACP + indexer) работает end-to-end. Альтернативы: (1) **Rust SDK только** — теряем Python ML ecosystem (LangChain/CrewAI) и TypeScript web ecosystem; (2) **Manual wrappers per language** — divergence risk, no single source of truth; (3) **No devnet, manual node setup** — agent onboarding friction; (4) **Skip multi-agent e2e** — regression risk. Решение: Rust canonical + TS/Python generated, one-command devnet, e2e multi-agent test.

**Задачи:**

1. Создать `crates/strangecoin-sdk/`:
   - `Cargo.toml`: `strangecoin-sdk = "0.1.0"`, features `["rust", "ts", "python"]`; deps `strangecoin-core`, `secp256k1`, `tokio`, `reqwest` (for JSON-RPC client), `serde_json`
   - `src/lib.rs`: `AgentClient` struct (high-level: open session, sign batch, submit, estimate, get activity), `AgentWallet` (HD derivation, key isolation `AgentKeystore`)
   - `src/rust/` — Rust-native API (canonical)
   - `src/ts/` — TypeScript bindings via `ts-rs` (`#[derive(TS)]` on shared types)
   - `src/python/` — Python bindings via `pyo3` (PyO3 wrapper around `AgentClient`)
   - `src/mcp_client.rs` — MCP client helpers (для agents, которые хотят MCP вместо JSON-RPC)
   - `src/acp_client.rs` — ACP client helpers (для agents, которые хотят peer-to-peer без JSON-RPC)
2. Создать `tools/devnet/agent/`:
   - `cargo-strangecoin-agent-devnet` binary: `cargo strangecoin agent devnet --agents N --fund 1000SC`
   - Spins up local Strangecoin node (regtest), derives N agent wallets from seed `m/44'/1234'/{0..N}'`, funds each from faucet (coinbase tx), prints agent pubkeys + RPC endpoints
   - Faucet: reuses Stage 4 faucet infra (`tools/faucet/`), but auto-requests for devnet agent wallets
3. Создать `examples/agent_cookbook/`:
   - `rust/single_agent_on_chain_read.rs` — agent reads `get_balance` via MCP, logs result
   - `rust/batch_submit.rs` — agent submits 3-tx batch via `agent_signAndSubmitBatch`
   - `rust/acp_task_settle.rs` — agent A negotiates with agent B, escrows, settles
   - `ts/single_agent_on_chain_read.ts` — TypeScript equivalent
   - `ts/acp_task_settle.ts` — TypeScript ACP example
   - `python/single_agent_on_chain_read.py` — Python (LangChain tool wrapper)
   - `python/multi_agent_crewai.py` — CrewAI integration: 3 CrewAI agents coordinate via ACP, settle on Strangecoin
4. Создать `tests/agent_e2e_multi_agent.rs`:
   - `three_agent_acp_chain_settle` — A→B→C task chain: A assigns task to B (ACP_NEGOTIATE → ACP_TASK with escrow), B fulfills and assigns sub-task to C (ACP_NEGOTIATE → ACP_TASK with sub-escrow), C fulfills, B settles with C, A settles with B; verify on-chain: escrow released B→C, escrow released A→B, all three agent balances correct
   - `agent_devnet_one_command` — `cargo strangecoin agent devnet --agents 5 --fund 1000SC` — verify 5 wallets funded with 1000 SC each, RPC endpoints reachable
5. SLIP-44 registration: open PR to [satoshilabs/slips#slip-44](https://github.com/satoshilabs/slips) for `coin_type = 1234` (Strangecoin placeholder from ADR-0028). Document in ADR-0028 amendment if accepted, else use unregistered placeholder with explicit note.
6. CI: `cargo test -p strangecoin-sdk` green; `cargo test --test agent_e2e_multi_agent` green; `npm test` in `examples/agent_cookbook/ts/` green; `pytest examples/agent_cookbook/python/` green

**Артефакты:**
- `crates/strangecoin-sdk/Cargo.toml`, `src/{lib,rust,ts,python,mcp_client,acp_client}.rs`
- `tools/devnet/agent/` (binary `cargo-strangecoin-agent-devnet`)
- `examples/agent_cookbook/{rust,ts,python}/`
- `tests/agent_e2e_multi_agent.rs`
- SLIP-44 registration PR (external; reference link in ADR-0028 amendment)

**КГ:**
- [ ] `crates/strangecoin-sdk/` — workspace member; `cargo build -p strangecoin-sdk --all-features` green
- [ ] `AgentClient` high-level API в Rust: open session, sign batch, submit, estimate, get activity
- [ ] TypeScript bindings via `ts-rs` — `npm run build` в `examples/agent_cookbook/ts/` green
- [ ] Python bindings via `pyo3` — `pytest examples/agent_cookbook/python/` green
- [ ] `cargo strangecoin agent devnet --agents 5 --fund 1000SC` — one-command, 5 wallets funded
- [ ] `examples/agent_cookbook/` — ≥6 examples (3 rust + 2 ts + 2 python, including CrewAI integration)
- [ ] `tests/agent_e2e_multi_agent.rs::three_agent_acp_chain_settle` green (A→B→C, all settles on-chain)
- [ ] `tests/agent_e2e_multi_agent.rs::agent_devnet_one_command` green
- [ ] SLIP-44 PR открыт (link в ADR-0028 amendment), либо placeholder documented
- [ ] `cargo clippy -p strangecoin-sdk -- -D warnings` green
- [ ] Коммит `[S4-AI-P05] agent-sdk: rust+ts+python + devnet + faucet + 3-agent e2e + cookbook`

---

## 10. Professional UI prompts (UI-P01..UI-P05)

### UI-P01. ADR-0031: Professional UI replacement (render backend + design system principles)

**Цель:** зафиксировать архитектурное решение о замене demo-UI `eframe`/`egui` на профессиональный UI: выбор render backend (retain eframe/egui vs Tauri+Web vs iced — выбрано retain eframe/egui), new crate `crates/strangecoin-ui/` (headless design system + component library + render-backend-agnostic core), strangler migration `src/gui/` → launcher (по образцу `src/storage/` → `strangecoin-storage` Stage 3), WCAG AA accessibility CI gate (#27), dark/light themes, i18n (en/ru/zh). Это решение определяет UI-P02..UI-P05.

**Контекст:** Current state (verified): `src/gui/` — `eframe`/`egui 0.22` immediate-mode GUI; login screen + wallet view (address, balance, recipient, amount, Send button); Windows `winapi` features declared explicitly. Pain points: demo-only aesthetic, no node operator dashboard (peers/sync/mempool/blocks), no agent activity view, no transaction detail with receipts/witness proofs, no settings screen, no accessibility audit, no theme/i18n. ROADMAP.md Этап 4 явно требует «professional UI». Альтернативы render backend: (1) **Retain eframe/egui** — Rust-native, immediate-mode, existing team expertise, no new deps, но accessibility support ограничен (no native screen reader hooks, manual tab-order); (2) **Tauri+Web (HTML/CSS)** — best accessibility (browser a11y tree), best design ecosystem (Tailwind, shadcn), но massive new dependency (webview), non-trivial IPC bridge Rust↔JS, builds heavier; (3) **iced (retained Elm-style)** — Rust-native, retained mode (better for complex state), accessibility via platform APIs, но smaller ecosystem, learning curve, less battle-tested; (4) **slint** — declarative Rust-native, но commercial license for some features. Решение: **retain eframe/egui**, invest в custom design system + component library поверх egui, manual WCAG AA compliance via axe-core audit на rendered screenshots (headless render + DOM-emulation for a11y check). Trade-off: accessibility effort manual, но no new heavyweight dep.

**Задачи:**

1. Создать `docs/ADR/0031-professional-ui-replacement.md`:
   - Status: Accepted (после UI-P01 land'а)
   - Context: current egui demo state, pain points (no dashboard/a11y/themes/i18n), ROADMAP.md Этап 4 requirement, threat V-47 (accessibility regression), invariant #27 (WCAG AA CI gate)
   - Decision:
     - **Render backend: retain eframe/egui 0.22+** — Rust-native, no new heavyweight dep; upgrade path tracked в ADR (if egui accessibility story matures → adopt; if not → Stage 5+ consider Tauri+Web migration as separate ADR)
     - **New crate** `crates/strangecoin-ui/` — headless design system + component library, render-backend-agnostic core (trait `UiBackend` with `eframe_egui` impl)
     - **Design system**: design tokens (color, spacing, typography, radius, shadow), light/dark themes (config `ui.theme = "light" | "dark" | "system"`), i18n (`en`, `ru`, `zh` — initial set, extensible via `fluent` crate)
     - **Screens (5)**: Wallet (send/history/contracts), Node Dashboard (peers/sync/mempool/blocks), Agent Activity (ACP messages, batched txs, sponsor txs — from indexer via AI-P04 `agent_getActivity`), Transaction Detail (receipts, witness proofs — from AI-P03 wire witnesses Stage 3), Settings (network/peers/keystore/config/MCP/ACP toggles)
     - **Strangler migration** (§10.5 precedent): `src/gui/` → thin launcher (`egui::App` impl делегирует в `strangecoin-ui::App`); `tests/two_clients.rs` остаётся зелёным весь migration (гейт §8.3 #5)
     - **WCAG AA accessibility (#27)**: axe-core audit в CI gate (`cargo test -p strangecoin-ui --features a11y-audit`); headless render to screenshot + DOM-emulation for axe-core; manual tab-order, focus indicators, color contrast ≥ 4.5:1, keyboard navigation all screens
     - **i18n**: `fluent` crate (L20n-style), locale files в `crates/strangecoin-ui/locales/{en,ru,zh}/main.ftl`; `ui.locale = "en" | "ru" | "zh" | "system"` config
   - Consequences: Positive (no new heavyweight dep, Rust-native story preserved, team expertise reuse, deterministic builds unchanged); Negative (accessibility effort manual, axe-core requires DOM-emulation layer, i18n adds translation workflow); Neutral (design system — reusable для future explorer/faucet web UIs Stage 5+)
   - Alternatives considered: Tauri+Web (heavyweight dep, IPC complexity), iced (smaller ecosystem, learning curve), slint (license risk)
   - Related: ADR-0028 (agent vision — Agent Activity screen), ADR-0029 (MCP — Settings toggles), ADR-0030 (ACP — Agent Activity screen), ARCHITECT.md §19.3/§19.5/§19.6, ROADMAP.md Этап 4
2. Создать `docs/spec/ui_professional.md` — spec: design tokens (color palette, spacing scale, typography scale, radius, shadow), component list (Button, Input, Card, Table, Modal, Drawer, Toast, Tabs, Breadcrumb, Badge, Avatar, Stat, Chart, CodeBlock, etc.), screen layouts (wireframes в ASCII or mermaid), accessibility checklist (WCAG AA criteria per screen), i18n workflow, theme switcher semantics
3. Pin policy: `eframe`/`egui` — pinned `=0.22.x` (Stage 2 precedent; upgrade requires ADR amendment); `fluent` — pinned `=x.y.z`; `axe-core` (via `cargo test` headless audit) — dev-dep only

**Артефакты:**
- `docs/ADR/0031-professional-ui-replacement.md`
- `docs/spec/ui_professional.md`

**КГ:**
- [ ] ADR-0031 создан по template; Status: Accepted
- [ ] Render backend choice (retain eframe/egui) зафиксирован с rationale + alternatives
- [ ] 5 screens перечислены (Wallet, Dashboard, Agent Activity, Tx Detail, Settings)
- [ ] Design system principles (tokens, themes, i18n) зафиксированы
- [ ] WCAG AA CI gate (#27) зафиксирован (axe-core audit approach)
- [ ] Strangler migration plan (`src/gui/` → launcher, `tests/two_clients.rs` green) зафиксирован
- [ ] `docs/spec/ui_professional.md` создан (design tokens, component list, screen wireframes, a11y checklist, i18n workflow)
- [ ] Pin policy: eframe/egui/fluent зафиксированы
- [ ] Коммит `[S4-UI-P01] ui-adr-0031: professional ui replacement + spec`

---

### UI-P02. Design system + component library + design tokens (crates/strangecoin-ui/)

**Цель:** реализовать `crates/strangecoin-ui/` — headless design system + component library поверх eframe/egui: design tokens (color palette, spacing scale, typography scale, radius, shadow), light/dark themes, 12+ reusable components (Button, Input, Card, Table, Modal, Drawer, Toast, Tabs, Breadcrumb, Badge, Avatar, Stat, Chart, CodeBlock), i18n setup (`fluent` crate, locales en/ru/zh). Render-backend-agnostic core (trait `UiBackend`) с `eframe_egui` impl.

**Контекст:** ADR-0031 (UI-P01) зафиксировал retain eframe/egui, new crate `crates/strangecoin-ui/`, design system principles. UI-P02 реализует foundation: tokens, components, themes, i18n — без которых UI-P03..UI-P05 (screen migration) невозможны. Альтернативы: (1) **Skip design system, write screens ad-hoc** — divergence risk, no theme consistency, accessibility per-screen ad-hoc (V-47 risk); (2) **Use external design system** — нет готовых Rust-native egui design systems (для web есть shadcn/MUI, для egui — нет); (3) **Generate design system from Figma tokens** — adds Figma dependency, not in repo workflow. Решение: hand-roll design system в `crates/strangecoin-ui/`, design tokens в `tokens.rs` (single source of truth), components в `src/components/`.

**Задачи:**

1. Создать `crates/strangecoin-ui/`:
   - `Cargo.toml`: `strangecoin-ui = "0.1.0"`, features `["eframe_egui", "a11y-audit"]`; deps `eframe = "=0.22.x"`, `egui = "=0.22.x"`, `fluent = "=x.y.z"`, `fluent-bundle`, `unic-langid`
   - `src/lib.rs`: `UiBackend` trait (render-backend-agnostic core); `EframeEguiBackend` impl; `App` struct (top-level entry, manages screen routing, theme, locale)
   - `src/tokens.rs`: design tokens — `ColorPalette` (primary, secondary, accent, success, warning, error, neutral — 10 shades each), `SpacingScale` (4/8/12/16/24/32/48/64 px), `TypographyScale` (xs/sm/base/lg/xl/2xl, font family, weight, line-height), `Radius` (none/sm/md/lg/full), `Shadow` (none/sm/md/lg)
   - `src/theme.rs`: `Theme` enum (Light/Dark/System), `ThemeManager` (resolve `System` via `eframe::Visuals::dark()`, persist to config `ui.theme`)
   - `src/i18n.rs`: `Localization` struct (loads `locales/{locale}/main.ftl` via `fluent-bundle`), `tr(key, args)` method; locale files в `locales/{en,ru,zh}/main.ftl`
   - `src/components/{button,input,card,table,modal,drawer,toast,tabs,breadcrumb,badge,avatar,stat,chart,codeblock}.rs` — 14 reusable components, each with: light/dark variants, focus indicators (WCAG 2.4.7), min touch target 44×44 px (WCAG 2.5.5), ARIA-like labels via `egui::Widget::id()` + accessibility metadata
2. Создать `crates/strangecoin-ui/locales/{en,ru,zh}/main.ftl` — initial translation strings for all UI labels (≥100 strings: button labels, screen titles, error messages, etc.)
3. Создать `crates/strangecoin-ui/tests/a11y_audit.rs` — axe-core audit harness: headless render of each component to screenshot + DOM-emulation, run `axe-core` ruleset, fail CI on WCAG AA violations
4. Создать `crates/strangecoin-ui/tests/component_snapshot.rs` — snapshot tests for each component (light/dark variants) — catches visual regressions
5. Создать `crates/strangecoin-ui/examples/gallery.rs` — design system gallery (renders all components in light/dark, all locales) — для visual review
6. CI: `cargo test -p strangecoin-ui` green (incl. a11y-audit feature); `cargo clippy -p strangecoin-ui -- -D warnings` green; snapshot tests green

**Артефакты:**
- `crates/strangecoin-ui/Cargo.toml`, `src/{lib,tokens,theme,i18n}.rs`, `src/components/*.rs` (14 files)
- `crates/strangecoin-ui/locales/{en,ru,zh}/main.ftl`
- `crates/strangecoin-ui/tests/{a11y_audit,component_snapshot}.rs`
- `crates/strangecoin-ui/examples/gallery.rs`

**КГ:**
- [ ] `crates/strangecoin-ui/` — workspace member; `cargo build -p strangecoin-ui` green
- [ ] `UiBackend` trait + `EframeEguiBackend` impl — render-backend-agnostic core
- [ ] Design tokens: `ColorPalette` (10 shades × 7 hues), `SpacingScale` (8 steps), `TypographyScale` (6 sizes), `Radius` (5), `Shadow` (4)
- [ ] 14 components: Button, Input, Card, Table, Modal, Drawer, Toast, Tabs, Breadcrumb, Badge, Avatar, Stat, Chart, CodeBlock — each with light/dark variants, focus indicators, 44×44 min touch target
- [ ] `Theme` enum (Light/Dark/System) + `ThemeManager` (persists to config)
- [ ] `Localization` via `fluent` — locales en/ru/zh, ≥100 strings, `tr(key, args)` method
- [ ] `tests/a11y_audit.rs` — axe-core audit, all components pass WCAG AA (#27)
- [ ] `tests/component_snapshot.rs` — snapshot tests green (light + dark)
- [ ] `examples/gallery.rs` — design system gallery renders all components
- [ ] `cargo clippy -p strangecoin-ui -- -D warnings` green
- [ ] Коммит `[S4-UI-P02] ui: design system + 14 components + tokens + themes + i18n + a11y audit`

---

### UI-P03. Wallet screen migration (send / history / contracts)

**Цель:** мигрировать wallet screen из demo-`src/gui/` в новый `crates/strangecoin-ui/`: три таба — Send (recipient, amount, fee, submit), History (recent txs с status, gas, block height), Contracts (deployed contracts list, call interface, ABI viewer). Reuse design system (UI-P02). WCAG AA compliant. Replaces existing login+wallet-view demo screens.

**Контекст:** Current wallet screen (verified): single egui view — address (read-only), balance, recipient + amount inputs, Send button. No history, no contracts tab, no fee control, no ABI viewer. ROADMAP.md Этап 4 явно требует wallet migration. ADR-0031 (UI-P01) зафиксировал strangler pattern: `src/gui/` → launcher. UI-P03 мигрирует wallet screen first (most-used screen). Альтернативы: (1) **Rewrite from scratch, no migration** — теряем working code, no fallback; (2) **Migrate all screens at once (UI-P03+UI-P04+UI-P05 in one PR)** — too big, regression risk; (3) **Keep demo wallet, add new screens alongside** — UX confusion. Решение: incremental migration, wallet first (UI-P03), then dashboard + tx detail (UI-P04), then agent activity + settings + strangler closeout (UI-P05).

**Задачи:**

1. Создать `crates/strangecoin-ui/src/screens/wallet/`:
   - `mod.rs` — `WalletScreen` struct, manages tab state (Send/History/Contracts)
   - `send.rs` — Send tab: recipient input (bech32 validation), amount input (SC + atomic units), fee input (default = estimate from `agent_estimateTaskCost` AI-P04), Submit button (calls JSON-RPC `eth_sendTransaction` or `agent_signAndSubmitBatch`), success/error toast
   - `history.rs` — History tab: table of recent txs (columns: txid, type, from, to, amount, fee, status, block height), filter (sent/received/failed), pagination, click row → tx detail (UI-P04)
   - `contracts.rs` — Contracts tab: list of deployed contracts (address, name, ABI), click → call interface (method selector, args inputs, "Read" / "Write" buttons calling `eth_call` / `eth_sendTransaction`), ABI viewer (JSON)
2. Wire wallet screen to JSON-RPC client (`crates/strangecoin-api` или inline `src/api/`):
   - `eth_getBalance` for balance display
   - `eth_getTransactionByHash` for history
   - `eth_call` / `eth_sendTransaction` for contract interaction
   - `agent_signAndSubmitBatch` for batch send (AI-P04)
3. Migrate `src/gui/` → thin launcher:
   - `src/gui/mod.rs` — `egui::App` impl delegates to `strangecoin_ui::App`
   - Remove old `src/gui/wallet_view.rs` etc. (replaced by `strangecoin-ui/src/screens/wallet/`)
   - `tests/two_clients.rs` remains green (гейт §8.3 #5)
4. WCAG AA: tab-order logical, focus indicators on all inputs, color contrast ≥ 4.5:1, keyboard navigation (Tab/Enter/Esc), screen reader labels via `egui::Widget::id()` + metadata
5. i18n: all labels localized via `fluent` (`wallet.send.recipient`, `wallet.send.amount`, `wallet.history.title`, etc.)
6. Создать `crates/strangecoin-ui/tests/screens/wallet_a11y.rs` — axe-core audit for wallet screen (Send/History/Contracts tabs)

**Артефакты:**
- `crates/strangecoin-ui/src/screens/wallet/{mod,send,history,contracts}.rs`
- `crates/strangecoin-ui/tests/screens/wallet_a11y.rs`
- `src/gui/mod.rs` (thin launcher)

**КГ:**
- [ ] `WalletScreen` с 3 tabs (Send/History/Contracts) реализован
- [ ] Send tab: bech32 validation, amount + fee inputs, submit → JSON-RPC `eth_sendTransaction` (or batch via `agent_signAndSubmitBatch`)
- [ ] History tab: table с filter + pagination, click row → tx detail (wire к UI-P04)
- [ ] Contracts tab: list + call interface + ABI viewer
- [ ] `src/gui/` — thin launcher (delegates to `strangecoin_ui::App`); old wallet_view.rs removed
- [ ] `tests/two_clients.rs` green (гейт §8.3 #5 — strangler migration не ломает headline test)
- [ ] WCAG AA: tab-order, focus indicators, contrast ≥ 4.5:1, keyboard nav — `tests/screens/wallet_a11y.rs` green (#27)
- [ ] i18n: all labels localized (en/ru/zh)
- [ ] `cargo clippy -p strangecoin-ui -- -D warnings` green
- [ ] Коммит `[S4-UI-P03] ui: wallet screen migration (send/history/contracts) + strangler launcher`

---

### UI-P04. Node operator dashboard + transaction detail (receipts / witness proofs)

**Цель:** реализовать Node Dashboard screen — operator-facing view (peers, sync status, mempool, blocks) — и Transaction Detail screen (receipts, witness proofs, gas breakdown, event logs). Node Dashboard даёт operator visibility в running node; Tx Detail — auditability для любой транзакции (на чем основана её валидность).

**Контекст:** ROADMAP.md Этап 4 + ADR-0031 требуют dashboard и tx detail. Tx Detail критичен для agent txs — agents хотят видеть receipts (success/fail, gas used, logs) и witness proofs (stateless verification — Stage 3 deliverable S3-P11) для своих batched/sponsored txs. Node Dashboard закрывает operator pain: сейчас operator не видит peers/sync/mempool/blocks без логов. Reuse: `NodeEvent` bus (Stage 1) → UI subscription; `StorageEngine` indices (Stage 3 S3-P09) → tx lookup; `StateWitness` wire (Stage 3 S3-P11) → witness proof display. Альтернативы: (1) **Dashboard как separate binary (sc-dashboard)** — ops complexity, no shared state с GUI; (2) **Tx Detail как modal only** — limited for complex txs (batched); (3) **Skip witness proofs** — теряем stateless verification visibility. Решение: dashboard как screen в main UI app, tx detail как full screen (navigated from history row click).

**Задачи:**

1. Создать `crates/strangecoin-ui/src/screens/dashboard/`:
   - `mod.rs` — `DashboardScreen` struct
   - `peers.rs` — Peers panel: table (peer_id, addr, latency EWMA, score, direction, capabilities), color-coded score (green > 0.5, yellow 0.2-0.5, red < 0.2 — Stage 2 peer scoring bands)
   - `sync.rs` — Sync panel: current height, tip height, sync %, headers/blocks queue depth, sync engine state (idle/headers-first/blocks/catch-up)
   - `mempool.rs` — Mempool panel: tx count, total fee, sorted-by-fee table (top 10), RBF conflicts highlighted
   - `blocks.rs` — Blocks panel: recent 10 blocks (height, hash, tx count, gas used, miner, timestamp), click → block detail modal
2. Создать `crates/strangecoin-ui/src/screens/tx_detail/`:
   - `mod.rs` — `TxDetailScreen` struct, takes `txid` param
   - `overview.rs` — Overview: txid, type (transfer/call/create), from, to, amount, fee, gas used, status, block height, timestamp
   - `receipt.rs` — Receipt panel: status (success/fail), gas used, gas limit, logs (decoded ABI events), revert reason (if fail)
   - `witness.rs` — Witness panel: `StateWitness` wire proof (Stage 3 S3-P11 deliverable) — account proof (Merkle path to state_root), contract storage proof (Merkle path to contract_storage_root); "Verify" button — calls `verify_block_stateless` (Stage 1 + Stage 3 wire) client-side
   - `events.rs` — Events panel: decoded event logs (using ABI from indexer)
3. Wire screens to data sources:
   - Dashboard: `NodeEvent` bus subscription (Stage 1) → live updates
   - Tx Detail: `StorageEngine::get_tx(txid)` (Stage 3 S3-P09 indices) + `get_receipt(txid)` + `get_witness(txid)` (Stage 3 S3-P11)
4. WCAG AA: tab-order, focus indicators, keyboard nav, color contrast (incl. color-coded peer score — must have text label too, not color-only — WCAG 1.4.1)
5. i18n: all labels localized
6. Создать `crates/strangecoin-ui/tests/screens/{dashboard,tx_detail}_a11y.rs` — axe-core audits

**Артефакты:**
- `crates/strangecoin-ui/src/screens/dashboard/{mod,peers,sync,mempool,blocks}.rs`
- `crates/strangecoin-ui/src/screens/tx_detail/{mod,overview,receipt,witness,events}.rs`
- `crates/strangecoin-ui/tests/screens/{dashboard,tx_detail}_a11y.rs`

**КГ:**
- [ ] `DashboardScreen` с 4 panels (Peers, Sync, Mempool, Blocks) реализован
- [ ] Peers panel: live updates via `NodeEvent` bus, color-coded score + text label (WCAG 1.4.1)
- [ ] Sync panel: current/tip height, sync %, sync engine state
- [ ] Mempool panel: tx count, top-10-by-fee table, RBF conflicts highlighted
- [ ] Blocks panel: recent 10 blocks, click → block detail modal
- [ ] `TxDetailScreen` с 4 panels (Overview, Receipt, Witness, Events) реализован
- [ ] Witness panel: `StateWitness` proof display + "Verify" button calling `verify_block_stateless`
- [ ] Tx Detail navigated from Wallet History row click (UI-P03 wire)
- [ ] WCAG AA — `tests/screens/{dashboard,tx_detail}_a11y.rs` green (#27)
- [ ] i18n: all labels localized (en/ru/zh)
- [ ] `cargo clippy -p strangecoin-ui -- -D warnings` green
- [ ] Коммит `[S4-UI-P04] ui: node dashboard + tx detail (receipts/witness proofs)`

---

### UI-P05. Agent activity feed + settings + accessibility audit + strangler closeout

**Цель:** реализовать Agent Activity screen (ACP messages, batched txs, sponsor txs from indexer via AI-P04 `agent_getActivity`) и Settings screen (network/peers/keystore/config/MCP/ACP toggles). Запустить full axe-core audit на всех 5 screens, закрыть strangler migration (`src/gui/` → pure launcher, no business logic remains), обновить `tests/two_clients.rs` для использования new UI launcher (если нужно), обновить Changelog и AGENTS.md.

**Контекст:** UI-P03 (wallet) + UI-P04 (dashboard + tx detail) мигрированы. UI-P05 закрывает migration: Agent Activity (depends on AI-P04 `agent_getActivity` + AI-P03 ACP messages + AI-P02 MCP), Settings (operator config surface), strangler closeout (`src/gui/` → pure launcher), a11y audit (all 5 screens), Changelog 4.0.0. Альтернативы: (1) **Skip Agent Activity screen** — теряем agent-monitoring UX; (2) **Settings via config.toml only, no UI** — operator friction; (3) **Skip full a11y audit** — V-47 risk. Решение: full impl, closeout.

**Задачи:**

1. Создать `crates/strangecoin-ui/src/screens/agent_activity/`:
   - `mod.rs` — `AgentActivityScreen` struct
   - `feed.rs` — Activity feed: chronological list of (timestamp, agent_address, event_type, details) — event types: ACP_NEGOTIATE/ACP_TASK/ACP_SETTLE/ACP_REVOKE, batched_tx, sponsor_tx (stub), transfer; click → tx detail (UI-P04) or ACP message detail modal
   - `filter.rs` — Filter sidebar: by agent_address, by event_type, by time range
2. Создать `crates/strangecoin-ui/src/screens/settings/`:
   - `mod.rs` — `SettingsScreen` struct
   - `network.rs` — Network settings: listen_addr, seeds, max_peers, network_id (read-only after start)
   - `peers.rs` — Peer management: list, ban/unban, add seed
   - `keystore.rs` — Keystore management: list wallets (operator + agents), import/export, password change; key isolation (`OperatorKeystore` vs `AgentKeystore` — ADR-0028) reflected в UI (separate tabs)
   - `config.rs` — Raw config.toml editor (advanced users; with validation on save)
   - `mcp.rs` — MCP toggles (ADR-0029): enable/disable, transport select (stdio/HTTP+SSE), view connected MCP clients
   - `acp.rs` — ACP toggles (ADR-0030): enable/disable, view active ACP sessions, ban agent
3. Strangler closeout:
   - Audit `src/gui/` — ensure no business logic remains, only launcher (`egui::App` impl delegates to `strangecoin_ui::App`)
   - Remove any leftover demo files (`src/gui/wallet_view.rs`, etc.)
   - `tests/two_clients.rs` — verify still green (гейт §8.3 #5); if test breaks due to UI changes, fix test to use new launcher (but do not regress test coverage)
4. Full axe-core a11y audit:
   - Run `cargo test -p strangecoin-ui --features a11y-audit` — must pass all 5 screens (Wallet, Dashboard, Tx Detail, Agent Activity, Settings)
   - Document any WCAG AA criteria requiring manual review (e.g., dynamic content live-regions) in `docs/spec/ui_professional.md`
5. Update `Changelog.md`:
   - `4.0.0 — dev-experience + AI agents + MCP + ACP + professional UI`
   - List breaking changes (if any): `src/gui/` API removed (use `strangecoin-ui` crate)
   - List new features: 5 screens, MCP server, ACP transport, agent_* JSON-RPC, SDK (Rust/TS/Python)
6. Update `AGENTS.md`:
   - Stage 4 sections — only facts confirmed by commits (FBP10 precedent from Stage 3)
   - Pin policy updated: eframe/egui, fluent, snow (Stage 2 carry), bip32 (AI-P01)
   - Common gotchas: eframe/egui upgrade requires ADR amendment; MCP spec version churn (pin to 2024-11-05)

**Артефакты:**
- `crates/strangecoin-ui/src/screens/agent_activity/{mod,feed,filter}.rs`
- `crates/strangecoin-ui/src/screens/settings/{mod,network,peers,keystore,config,mcp,acp}.rs`
- `crates/strangecoin-ui/tests/screens/{agent_activity,settings}_a11y.rs`
- `src/gui/` — pure launcher (audit confirms no business logic)
- `Changelog.md` — `4.0.0` section
- `AGENTS.md` — Stage 4 sections

**КГ:**
- [ ] `AgentActivityScreen` реализован: feed (ACP/batched/sponsor/transfer), filter sidebar, click → tx detail / ACP modal
- [ ] `SettingsScreen` реализован: 7 sub-screen (network/peers/keystore/config/mcp/acp), key isolation reflected (OperatorKeystore vs AgentKeystore separate tabs)
- [ ] `src/gui/` audit confirms pure launcher (no business logic); old demo files removed
- [ ] `tests/two_clients.rs` green (гейт §8.3 #5)
- [ ] Full a11y audit: `cargo test -p strangecoin-ui --features a11y-audit` — all 5 screens pass WCAG AA (#27)
- [ ] Manual WCAG AA review items documented в `docs/spec/ui_professional.md`
- [ ] `Changelog.md` 4.0.0 section added (breaking changes + new features listed)
- [ ] `AGENTS.md` Stage 4 sections added (facts only, FBP10 precedent)
- [ ] Pin policy updated: eframe/egui, fluent, bip32 added; snow (Stage 2 carry)
- [ ] `cargo clippy -p strangecoin-ui -- -D warnings` green
- [ ] Коммит `[S4-UI-P05] ui: agent activity + settings + a11y audit + strangler closeout + changelog 4.0.0`

---

## 11. Покрытие DoD Stage 4 (расширение ROADMAP.md «Этап 4») после AI-P01..AI-P05 + UI-P01..UI-P05

| # | Критерий (из обновлённого ROADMAP.md «Этап 4» DoD) | Промпт | Evidence (после закрытия) |
|---|----------------------------------------------------|--------|----------------------------|
| 1 | JSON-RPC eth_* работает с MetaMask | (Stage 4 core, не extension) | `tests/eth_rpc.rs` (Stage 4 core track) |
| 2 | HD Wallet работает (BIP-39/44) | (Stage 4 core) | `tests/hd_wallet.rs` |
| 3 | SDK работает (cargo strangecoin new → deploy → call) | (Stage 4 core) | `tests/sdk_e2e.rs` |
| 4 | Devnet работает | (Stage 4 core) | `tests/devnet.rs` |
| 5 | Explorer работает | (Stage 4 core) | manual + `tools/explorer/` |
| 6 | Faucet работает | (Stage 4 core) | `tests/faucet.rs` |
| 7 | Indexer работает | (Stage 4 core, extension: agent activity) | `tests/indexer.rs` + `tests/agent_rpc.rs::agent_get_activity` |
| 8 | Metrics exposed | (Stage 4 core) | `curl /metrics` smoke |
| 9 | **MCP server работает** (deterministic #23) | AI-P02 | `tests/mcp_server.rs` + `mcp-cli` round-trip |
| 10 | **ACP transport работает** (escrow #24, replay #25) | AI-P03 | `tests/acp_messages.rs` |
| 11 | **Agent JSON-RPC работает** (5 agent_* methods) | AI-P04 | `tests/agent_rpc.rs` |
| 12 | **Agent SDK работает** (Rust + TS + Python) | AI-P05 | `examples/agent_cookbook/` + `tests/agent_e2e_multi_agent.rs` |
| 13 | **Agent devnet работает** (N wallets, ACP chain settle) | AI-P05 | `tests/agent_e2e_multi_agent.rs::three_agent_acp_chain_settle` |
| 14 | **Professional UI работает** (5 screens, WCAG AA #27) | UI-P02..UI-P05 | `tests/screens/*_a11y.rs` + `tests/two_clients.rs` green |
| 15 | **crates/strangecoin-mcp/ создан** | AI-P02 | workspace member + `cargo build -p strangecoin-mcp` green |
| 16 | **crates/strangecoin-sdk/ создан** | AI-P05 | workspace member + `cargo build -p strangecoin-sdk --all-features` green |
| 17 | **crates/strangecoin-ui/ создан** | UI-P02 | workspace member + `cargo build -p strangecoin-ui` green |
| 18 | **ADR-0028 (cryptocurrency for AI agents) написан** | AI-P01 | `docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md` |
| 19 | **ADR-0029 (MCP server) написан** | AI-P02 | `docs/ADR/0029-mcp-server.md` |
| 20 | **ADR-0030 (ACP) написан** | AI-P03 | `docs/ADR/0030-acp-agent-communication-protocol.md` |
| 21 | **ADR-0031 (Professional UI) написан** | UI-P01 | `docs/ADR/0031-professional-ui-replacement.md` |
| 22 | **specs: agent_integration / mcp_server / acp_messages / ui_professional созданы** | AI-P01..AI-P03, UI-P01 | `docs/spec/{agent_integration,mcp_server,acp_messages,ui_professional}.md` |
| 23 | **BACKLOG.md создан и обновляется** | (этот патч) | `analytics/BACKLOG.md` (см. §12) |
| 24 | `Changelog.md` обновлён: `4.0.0 — dev-experience + AI agents + MCP + ACP + professional UI` | UI-P05 | Changelog 4.0.0 + tag `v4.0.0-stage4` |

**Stage 4 closure tag:** `v4.0.0-stage4` ставится после закрытия **всех** Stage 4 core DoD + extension DoD (AI-P01..AI-P05 + UI-P01..UI-P05). Tag-gate для Stage 5 (fee market + AA + paymaster full impl) — soft dependency: Stage 5 paymaster full impl требует `AI-P05` closed (stub sufficient до Stage 5).

---

## 12. Связь с BACKLOG.md

Файл `analytics/BACKLOG.md` создан этим же патчем (см. ниже) — сквозной бэклог AI/UI work items, который обновляется по мере закрытия `AI-P##` / `UI-P##`. Структура BACKLOG.md:

- **§1 Active work items** — `AI-P##` / `UI-P##` в работе (status: in-progress), owner, ETA
- **§2 Next up** — upcoming prompts (queued, not started)
- **§3 Blocked** — prompts blocked on dependency (e.g., AI-P05 blocked on AI-P04)
- **§4 Done** — closed prompts with commit hash + close date
- **§5 Stage 4 residual obligations** — carry-over to Stage 5+ (paymaster full impl, AA protocol-level, EIP-1559, mobile UI, multi-lang SDK Go/Java/C++, relayed ACP, ZK-proofs for ACP escrow)
- **§6 Cross-stage carry-over** — SCIP-0001 (genesis key), BUG-S1-013/015 (ops-gates), Stage 3 residuals (Verkle/SMT, state expiry, receipts prune, Block-STM, wire snapshot sync)

До этого патча «бэклог Stage 3» жил в `ROADMAP3.md` (referenced в `prompt-stage3.md` §0 Замечание про BACKLOG3.md), «бэклог Stage 4» — в `ROADMAP.md` §«Этап 4» (de-facto). Теперь AI/UI work items живут в `analytics/BACKLOG.md` для traceability — single source of truth для agent-track и UI-track.

---

## 13. Связь с другими документами (расширение §7)

К существующему списку (§7) добавляются:

- `analytics/BACKLOG.md` — сквозной бэклог AI/UI work items (см. §12).
- `docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md` — ADR для AI-agent vision (AI-P01).
- `docs/ADR/0029-mcp-server.md` — ADR для MCP server (AI-P02).
- `docs/ADR/0030-acp-agent-communication-protocol.md` — ADR для ACP (AI-P03).
- `docs/ADR/0031-professional-ui-replacement.md` — ADR для professional UI (UI-P01).
- `docs/spec/agent_integration.md` — high-level spec для AI agent integration (AI-P01).
- `docs/spec/mcp_server.md` — spec для MCP server tools/resources/transports (AI-P02).
- `docs/spec/acp_messages.md` — spec для ACP message family (AI-P03).
- `docs/spec/ui_professional.md` — spec для professional UI design system + screens (UI-P01).
- `analytics/ARCHITECT.md` §19 — AI Agent orientation (principles, context diagram, subsystems, invariants #23..#27, threat vectors V-43..V-48, anti-goals, open questions).
- `analytics/ROADMAP.md` §«Этап 4» (расширенный) — Stage 4 tasks/DoD/risks/ADRs required с AI/MCP/ACP/UI work items.
- `README.md` (root) — обновлён: AI-agent orientation в intro + AI agents feature subsection + roadmap row Stage 4 «+ AI agents» + project structure (new ADRs/specs/BACKLOG.md) + acknowledgements footer.
- `docs/security/THREAT_MODEL.md` — STRIDE v3.3 → v3.4 (extension: V-43..V-48, AI/UI attack surface; обновляется в UI-P05 closeout или отдельным security-patch).

---

## 14. Stage 4 gate closure (tag `v4.0.0-stage4`)

**Precondition:** Stage 4 closure требует:

1. Stage 4 core DoD closed (JSON-RPC eth_*, HD wallet, SDK core, devnet, explorer, faucet, indexer, metrics — из оригинального ROADMAP.md «Этап 4» DoD, не extension).
2. Extension DoD closed (§11 этой секции, AI-P01..AI-P05 + UI-P01..UI-P05).
3. All Stage 0..3 invariants (#1..#22) still enforced (`docs/stage4/INVARIANTS_ENFORCED.md`, расширение до #27).
4. Threat model v3.4 (`docs/stage4/THREAT_MODEL_STAGE4.md`, V-1..V-48).
5. `cargo test --workspace` (чанками по precedent Stage 3 §5 #11) + `cargo clippy --all-targets -- -D warnings` green ДО тега.
6. Annotated tag `v4.0.0-stage4` создан и запушен; `git ls-remote --tags origin` подтверждает.
7. Changelog 4.0.0 (breaking changes перечислены: `src/gui/` API removed).
8. AGENTS.md Stage 4 sections — только факты, подтверждённые коммитами (FBP10 precedent).

**Soft gates (Stage 5):**

- Stage 5 paymaster full impl не начинается до `AI-P05` closed (stub sufficient до Stage 5).
- Stage 5 AA protocol-level не начинается до `AI-P04` closed (batched tx via WASM contract sufficient до Stage 5).
- Stage 5 EIP-1559 не начинается до Stage 4 core `eth_estimateGas` stable.

**Hard gates (Stage 6+):**

- Stage 6 external audit не начинается до `v4.0.0-stage4`.
- Stage 6 TLA+ network spec не начинается до ACP transport formally specified (AI-P03 spec → TLA+ model).
- Stage 7 PoS migration не начинается до `v4.0.0-stage4` + SCIP-0001 closed (genesis key replacement).

---

*Конец расширения §8..§14 (добавлено 2026-11-04). Оригинальный Stage 3 content (§0..§7, S3-P01..S3-P15) сохранён без изменений.*
