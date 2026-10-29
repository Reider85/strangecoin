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
   - Value-типы (canonical bytes через core-serialize? — core-dep запрещён: storage не зависит от core (независимые крейты). Решение: storage хранит **opaque bytes**; encode/decode блоков/tx/state — на стороне monolith (S3-P07). Schema-кодек отвечает только за KEY structure)
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
- [ ] Нет зависимости storage→core (opaque bytes) — проверено grep'ом
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
