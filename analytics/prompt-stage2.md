# prompt-stage2.md — Промпты для реализации Stage 2 (Сетевая зрелость)

**Версия:** 1.0
**Дата:** 2026-10-21
**Источник:** `ARCHITECT3.md` (§3.6 network/p2p, §4.2 sync, §5 инварианты #16/#17, §6 угрозы V-1/V-2/V-15/V-17/V-24, §10.4 strangler Stage 2, §13 headless, §14 network_id, §15 anti-goals, §17 PR-checklist), `ROADMAP3.md` (Этап 2 — Сетевая зрелость: цели, задачи, DoD, риски, ADRs required; «Критические развилки → Stage 2»; «Сквозные треки»), `prompt-stage1.5.md` (шаблон/формат/§0 контекст-блок/§1 карта зависимостей/§2 список промптов с разделами Цель/Контекст/Задачи/Артефакты/КГ/Коммит; §6 процессные страховки из retro §8.1; §7 anti-goals), `bugfixes-stage1.5.md` (v1.2 — закрытие S1.5-FBP01..FBP11, верификация тега `v1.5.0-stage1.5`), `Changelog.md` (секции 1.5.0..1.5.6 — Stage 1.5 closure), `crates/strangecoin-core/Cargo.toml` (version 1.5.0), `crates/strangecoin-vm-wasm/Cargo.toml` (version 0.2.0), `Cargo.toml` (workspace root, version 1.5.0), `docs/ADR/0016-receipts-root-in-block-header.md` (занятый ADR-0016 слот — Stage 2 переименовывает ADR-0016..0019 → ADR-0020..0023), `docs/spec/consensus.tla` (TLA+ baseline, 35,207 состояний PASS, 10 инвариантов, включая `ReceiptsRootMatch`), `git ls-remote --tags origin` (подтверждение `v1.5.0-stage1.5` на коммите `1f133c7`).

**Предусловие:** Stage 1.5 завершён и помечен тегом `v1.5.0-stage1.5` (S1.5-FBP11). Сверка `bugfixes-stage1.5.md` v1.2 по коду (HEAD `1b4dbcc` на origin, tag `v1.5.0-stage1.5` → `1f133c7`):

- **`bugfixes-stage1.5.md` v1.2 (11 промптов S1.5-FBP01..FBP11):** все 11 промптов отмечены **fixed** с evidence (commit hash, тест-счётчики, bump версии, push тега). Артефакты сверены с кодовой базой (см. §0.1 ниже для деталей). Все отклонения от первичной спеки явно задокументированы в каждом FBP в разделе «Решение» — это урок §8.1 retro: «отклонения фиксируются, а не замалчиваются». 2 residual-пункта не локально-верифицируемы (Windows-хост без `cargo-fuzz`/`cosign`/`slsa-verifier`/`gh` CLI — прецедент FBP07/FBP09): fuzz 5×600s CI-side, cosign/SLSA blob verify tooling-side. Тег-гейт Stage 2/3/4 **открыт**: tag pushed + release pipeline run 39110855701 success 13/13 jobs, 26 assets, SHA256SUMS сверен.
- **Workspace:** `Cargo.toml` members = `[".", "crates/strangecoin-core", "crates/strangecoin-vm-wasm"]` — `strangecoin-net` **не создан** (Stage 2 не начат).
- **Version:** `1.5.0` (Stage 1.5 complete; S2-P24 поднимет до `2.0.0`).
- **ADR-0016 конфликт:** ROADMAP3 §«Критические развилки → Stage 2» первоначально планировал ADR-0016 для Noise Protocol Framework, но Stage 1.5 FBP07 использовал ADR-0016 для «Receipts Root in Block Header» (commit `f217314` + ADR-0016 от `768fc99`, файл `docs/ADR/0016-receipts-root-in-block-header.md`). Stage 2 переименовывает свои 4 ADR на ADR-0020..0023 (примыкающие к занятому ADR-0016). Это решение зафиксировано в §2.1 ниже и в S2-P01.

**Замечание про `BACKLOG3.md`:** файл `analytics/BACKLOG3.md` **не существует** в репозитории на коммите HEAD `1b4dbcc` (поиск `find . -iname "*BACKLOG*"` пуст). ROADMAP3.md Этап 2 (раздел «Задачи» + «Definition of Done» + «Риски» + «ADRs required») используется как de-facto Stage 2 бэклог — он содержит полный список tasks с checkboxes и явный DoD, что функционально эквивалентно бэклогу. Все ссылки в этом документе на «бэклог Stage 2» означают `ROADMAP3.md` §«Этап 2 — Сетевая зрелость». Если позже появится `BACKLOG3.md`, промпты S2-P01..S2-P24 нужно переразметить по нему 1:1.

**Цель:** реализовать production-grade P2P в отдельном крейте `crates/strangecoin-net/`: gossip через inventory, compact blocks (BIP 152), Erlay reconciliation, Noise Protocol Framework (handshake XX + ChaCha20-Poly1305 transport + Ed25519 internal auth), peer discovery через hard-coded seed IPs + DNSSEC + ADDR gossip, peer scoring + rate limiting, deprecated v0.8.6 messages cleanup, async-migration network workers, shadow network tooling (Stage 6 prep). **24 промпта со сквозной нумерацией S2-P01..S2-P24** — только новый Stage 2 proper work (аналогично `prompt-stage1.5.md`, который содержал 19 промптов S1.5-P01..P19). Каждый промпт — самостоятельное задание для ИИ-агента, завершающееся конкретными артефактами и чек-листом КГ. После S2-P24 ставится тег `v2.0.0-stage2` — Gate для Stage 3 (RocksDB) и Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*, faucet).

---

## 0.1. Оценка реализации bugfixes-stage1.5.md (краткое заключение)

Сверка всех 11 промптов `bugfixes-stage1.5.md` v1.2 (S1.5-FBP01..FBP11) по фактической кодовой базе (`/home/z/my-project/research/strangecoin` после `git clone --depth 1`, HEAD `1b4dbcc`, origin tag `v1.5.0-stage1.5` → `1f133c7`):

| FBP | Статус | Артефакт-верификация (ls) | Commit (claim) | Отклонения от первичной спеки | Residual |
|-----|--------|---------------------------|----------------|--------------------------------|----------|
| S1.5-FBP01 | fixed | `crates/strangecoin-core/src/abi/{mod,encode,decode,selector}.rs` + `tests/abi_roundtrip.rs` (15/15) ✅ | `b925f71` + `10e9acc` | `AbiErrorDecl`/`AbiError` split, `AbiEventParam.indexed` per-param, serde `Deserialize` вместо `AbiDocument::parse`, `NonCanonicalOffset` строже спеки | нет |
| S1.5-FBP02 | fixed | `crates/strangecoin-core/src/abi/selector.rs` + golden vectors literal hex в `tests/abi_roundtrip.rs` ✅ | `b925f71` + `eee7b7b` | `AbiDocument::parse(raw: &str)` не реализован как метод — parsing через serde `Deserialize` (calldata-байты в core, JSON parsing tooling-side) | нет |
| S1.5-FBP03 | fixed | `crates/strangecoin-vm-wasm/src/lib.rs` (ENTRY_POINT удалён, dispatch через `resolve_dispatch_entry`), `host/context/input_read.rs` ✅ | `f4dadda` | dual-export `main`/`guarded` вместо fallback на `main` (строгое `MissingExport` для ABI-hit без экспорта — контракт-баг, не fallback); `HostCallWeight::InputRead = 3` | нет |
| S1.5-FBP04 | fixed | `crates/strangecoin-core/src/state/inner.rs::apply_create` (исполнение конструктора), `core/src/types.rs::CreateTx.gas_limit + constructor_params`, FORMAT_VERSION 6, genesis hashes + golden vectors перегенерированы ✅ | `f217314` | `CreateTx.gas_limit` — новое поле (спека предполагала `create.gas_limit` уже существующим); failure = failed-receipt семантика (спека противоречила сама себе); `constructor_params` на wire, MVP диспатчит по имени | нет |
| S1.5-FBP05 | fixed | `crates/strangecoin-vm-wasm/src/host/calls/{call,delegatecall,staticcall}.rs`, `core/src/vm/result.rs::ExecutionResult.value_transfers`, `core/src/state/inner.rs::commit_value_transfers`/`reverse_value_transfers` ✅ | Changelog 1.5.3 | `value_transfers` trail в `Receipt` + `value_transfers_undo` accumulating map (не только receipts, как в тексте промпта — chained rollbacks иначе теряли данные); top-level transfer селтится в `run_frame`, не в `nested_call` | нет |
| S1.5-FBP06 | fixed | `crates/strangecoin-core/src/state/contract_storage.rs` (`SharedContractStorage = Arc<RwLock<ContractStorageMap>>`), `core/src/vm/context.rs`, manual deep `State::clone` + deep-copy на границе `StateCache` ✅ | `06c3444` | `Arc` вместо `Rc` (Send/Sync сохранён), ручной deep-clone `State` (инвариант #4 round-trip сохранён); overlay unchanged; flush/undo in place через `write()` | нет |
| S1.5-FBP07 | fixed | `crates/strangecoin-core/src/types.rs::BlockHeader.receipts_root: [u8;32]`, FORMAT_VERSION 7 (HEADER_WIRE_LEN 221), `genesis.json`/`genesis-testnet.json` rebuild, `tests/serialize_golden.rs` regenerate, TLA+ `ReceiptsRootMatch` PASS 35,207 states, `docs/ADR/0016-receipts-root-in-block-header.md` ✅ | `768fc99` | вариант (b) `BlockHeader.receipts_root` (спека сама рекомендовала (b), не fold в global state_root); TLA+ инвариант — abstract fold, не SMT (структура — Rust-тесты, честно задокументировано в README residual) | нет |
| S1.5-FBP08 | fixed | `docs/stage1.5/{INVARIANTS_ENFORCED,THREAT_MODEL_STAGE1_5,RESIDUAL_RISKS_STAGE2_PLUS}.md` — 22/22 enforced (extensions #4/#5/#8/#11/#15/#18/#19, #20 degenerate), residuals V-18/V-19/V-21 ✅ | `768fc99` | #20 — enforced degenerate (fee=0 → тривиальный сплит, gas через #15; настоящий fee market Stage 5) | fuzz 5×600s CI-side, локально не верифицировано (Windows-хост, cargo-fuzz недоступен) — прецедент FBP07/FBP09 |
| S1.5-FBP09 | fixed | `docs/stage1.5/STAGE1_5_SUMMARY.md` существует, 14 DoD ✅; drift-sync `100a388` актуализировал DoD #2/#4/#6/#7/#10/#12, §2 FBP01..08, §4 residuals #12/#13 done, §4.1 anti-goals 16/16 ✅ | `1f133c7` + `100a388` | выполнен как часть S1.5-P19 (не FBP-коммит), что противоречит первоначальной нумерации FBP01..FBP11 — это задокументировано в «Статусе каталога» | нет |
| S1.5-FBP10 | fixed | root `Cargo.toml` 1.1.0→1.5.0 ✅, `crates/strangecoin-core/Cargo.toml` 1.1.0→1.5.0 ✅, `crates/strangecoin-vm-wasm/Cargo.toml` 0.1.0→0.2.0 ✅, `Cargo.lock` + `fuzz/Cargo.lock` синхронизированы ✅ | `1f133c7` + `3826a44` | большая часть выполнена S1.5-P19 (`1f133c7`); residual — bump версий крейтов `3826a44` | нет |
| S1.5-FBP11 | fixed | annotated tag `v1.5.0-stage1.5` на origin → `1f133c7` ✅ (подтверждено `git ls-remote --tags origin`), release-run 39110855701 success 13/13 jobs, 26 assets, SHA256SUMS сверен ✅ | tag push + `1b4dbcc` | residual закрыт 2026-10-20 через GitHub REST API (без `gh` CLI) | cosign/slsa-verifier/`gh` CLI не установлены на Windows-хосте — verify-команды зафиксированы для независимого повтора |

**Итог сверки:** 11/11 fixed, все заявленные артефакты присутствуют в кодовой базе, версии корректны (root/core 1.5.0, vm-wasm 0.2.0), тег `v1.5.0-stage1.5` реально есть на origin (target `1f133c7`, tagger Reider85), последний коммит `1b4dbcc` `[S1.5-FBP11] release: release pipeline verified for v1.5.0-stage1.5 (run 39110855701)`. 2 residual-пункта (fuzz CI-side, cosign/slsa blob verify) не локально-верифицируемы по причине Windows-хоста — это задокументировано в FBP08/FBP11, и не блокирует tag-gate для Stage 2 (CI-side верификация подтверждена через GitHub REST API). Тег-гейт Stage 2/3/4 **открыт**.

**Уроки, перенесённые в Stage 2:**
1. ADR-0016 конфликт — Stage 2 переименовывает свои ADR на 0020..0023, не пытаясь перезаписать `docs/ADR/0016-receipts-root-in-block-header.md` (см. §2.1).
2. Все отклонения от первичной спеки фиксируются явно в разделе «Решение» каждого промпта (прецедент FBP03/FBP04/FBP05/FBP07).
3. Не-локально-верифицируемые факты (CI-run, fuzz stats, release pipeline) помечаются как «не верифицировано + причина» (прецедент FBP08/FBP11).
4. 2 ops-gate из bugfixes-stage1 (BUG-S1-013 PGP-key offline keygen, BUG-S1-015 offline genesis key air-gapped keygen) остаются human-ops gates для mainnet freeze; **не блокируют Stage 2** (regtest/devnet scope) и **не входят в scope Stage 2** (отдельный ops+code track, см. §7).

---

## 0. Общий контекст (вставлять в начало каждого промпта)

```
Проект: Strangecoin, Rust, edition 2021, версия 1.5.0 (тег v1.5.0-stage1.5).
Структура (после Stage 1.5):
  Монолит: src/main.rs + src/{lib,consensus,economics,mempool,network,
    blockchain,storage,config,cli,wallet,address,error,events,api,gui}.rs +
    подпапки blockchain/{mod,block_executor,chain_selector,state_cache,
    blockchain_facade,consensus_manager}.rs, network/{mod,protocol,rate_limiter,
    sync,sync_engine}.rs, mempool/mod.rs, consensus/mod.rs, economics/mod.rs,
    governance/mod.rs, api/mod.rs, gui/mod.rs, storage/mod.rs, cli/mod.rs.
  Core-крейт: crates/strangecoin-core/src/{lib,types,serialize,consensus,
    chain_selector,address,error,abi/{mod,encode,decode,selector}}.rs + state/
    {mod,inner,sparse_merkle,witness,contract_storage}.rs + vm/{mod,traits,
    context,result,error,gas,limits}.rs + economics/{mod,emission,fee_market}.rs
    + governance/scip.rs.
  VM-крейт: crates/strangecoin-vm-wasm/src/{lib,error,gas}.rs + host/{mod,abi,
    state}.rs + host/crypto/{mod,secp256k1_verify,bls_verify,blake3,keccak,
    sha256}.rs + host/storage/{mod,storage_read,storage_write}.rs + host/calls/
    {mod,call,delegatecall,staticcall}.rs + host/context/{mod,block_context,
    gas_left,input_read}.rs + host/logs/{mod,emit_log}.rs + precompiles/{mod,
    ecrecover,sha256,keccak,blake3,ed25519_verify,bls_aggregate,modexp}.rs.
  Workspace: Cargo.toml с [workspace] members = [".", "crates/strangecoin-core",
    "crates/strangecoin-vm-wasm"]. crates/strangecoin-net/ — НЕ создан (Stage 2
    не начат; S2-P05 создаст).
  Тесты: tests/ (35 файлов, включая vm_deploy_call, vm_transfer_to_contract,
    vm_reentrancy_attack, vm_gas_exhaustion, vm_precompiles_integration,
    block_executor, receipts, state_root, sync_engine, rate_limit, two_clients,
    network, sync_headers, network_id, rbf, double_spend, reorg, concurrency);
    crates/strangecoin-core/tests/ (12 файлов: account_model, contract_storage_smt,
    receipts, tx_types, vm_trait_mock, state_roundtrip, state_root, witness,
    serialize_golden, merkle, u256, consensus_proptest, abi_roundtrip);
    crates/strangecoin-vm-wasm/tests/ (8 файлов: basic_execution, contract_storage,
    differential, events_and_receipts, gas_metering, host_functions, precompiles,
    reentrancy_and_limits).
  Fuzz: fuzz/fuzz_targets/{canonical_decode, host_functions, gas_metering,
    storage_trie, differential_wasmi_vs_wasmtime}.rs — 5 таргетов, 600s каждый
    в CI (matrix: ubuntu-latest, cargo-fuzz 0.13.2).
Теги: v0.0.1-rc1, v1.0.0-stage0, v1.1.0-stage1, v1.5.0-stage1.5 (annotated,
  target 1f133c7, verified через git ls-remote --tags origin). Ожидается после
  S2-P24: strangecoin=2.0.0, strangecoin-core=2.0.0, strangecoin-vm-wasm=0.2.0
  (unchanged — VM не трогается в Stage 2), strangecoin-net=0.1.0 (первый release).
Версия Cargo.toml (подтверждено): strangecoin=1.5.0, strangecoin-core=1.5.0,
  strangecoin-vm-wasm=0.2.0.
Стейдж-документация: docs/stage1.5/{STAGE1_5_SUMMARY.md (14 DoD ✅),
  INVARIANTS_ENFORCED.md (22/22 enforced), THREAT_MODEL_STAGE1_5.md (V-18/V-19/
  V-21 mitigated), RESIDUAL_RISKS_STAGE2_PLUS.md} — все созданы в Stage 1.5.
  docs/stage2/ — НЕ создан (Stage 2 не начат; S2-P23/S2-P24 создадут).
Bug-трекинг: analytics/bugfixes-stage0.md (35 багов, все закрыты),
  analytics/bugfixes-stage1.md (16 багов BUG-S1-001..016: 14 кодом, 2 ops-gate),
  analytics/bugfixes-stage1.5.md (11 FBP, все closed см. §0.1).
Известные прорехи (по ROADMAP3 Этап 2 и ARCHITECT3 §3.6):
  - crates/strangecoin-net/ — не создан (S2-P05). Страдает strangler pattern:
    network/* всё ещё в монолите src/network/, импортируется напрямую из
    src/main.rs и src/blockchain/*.rs.
  - P2P transport: сырой TCP без encryption (threat V-24 Network MITM не
    митигирован). Noise Protocol Framework не реализован (ADR-0020 предстоит).
  - Gossip: только announce через существующие sync.rs/sync_engine.rs (основан
    на GET_HEADERS/GET_BLOCKS), нет Bitcoin-style INV/GET_DATA/TX/BLOCK
    (S2-P13 предстоит).
  - Compact blocks (BIP 152): не реализован (S2-P14 предстоит). Текущая
    broadcast — full block bytes, bandwidth O(N) на 100 узлах.
  - Erlay: не реализован (S2-P15 предстоит). Reconciliation-based relay
    отсутствует, эффективный relay на 10k+ узлов невозможен.
  - Peer discovery: DNS-only seeds (без hard-coded IP fallback, threat V-16
    Eclipse via DNS poisoning не митигирован), нет ADDR gossip, нет DNSSEC
    validation (S2-P11 предстоит).
  - Peer scoring: инлайн в src/network/mod.rs (без разделения peer_store/
    peer_manager/connection_pool), баны только temporary, без persistent в
    network.json (S2-P12 + S2-P17 предстоит).
  - Rate limiting: Stage 1 rate_limiter.rs (per-message-type caps), но без
    интеграции с peer scoring, без token bucket (S2-P16 предстоит).
  - Legacy messages: GET_BLOCKCHAIN/BLOCKCHAIN (v0.8.6) всё ещё в protocol.rs
    (S2-P18 предстоит — cleanup).
  - Async migration: tokio введён в Stage 1 (ADR-0011), но network workers
    всё ещё на std::thread в нескольких местах (S2-P19 предстоит — finish).
  - Shadow network: не реализован (S2-P20 предстоит — Stage 6 prep).
  - ADR-0016 занят Receipts Root in Block Header (Stage 1.5 FBP07); Stage 2
    ADRs переименованы на 0020..0023 (см. §2.1).
  - 2 ops-gate из bugfixes-stage1 (BUG-S1-013 PGP-key, BUG-S1-015 offline
    genesis key) — НЕ блокируют Stage 2 (regtest/devnet scope); остаются
    human ops gates для mainnet freeze.
Принципы:
  - 1 промпт = 1 атомарный коммит, ID промпта в сообщении, формат:
    «[S2-PNN] <area>: <short description>».
  - ADR пишется ДО того, как решение затронет код (если промпт затрагивает
    архитектуру — ADR обязателен). Для Stage 2: ADR-0020..0023 (S2-P01..S2-P04)
    — обязательный гейт ДО S2-P05 (crate scaffolding).
  - Любое изменение консенсусных правил — через SCIP + activation height. На
    Stage 2 mainnet НЕ запущен (mainnet freeze gate = BUG-S1-013/015 ops gates),
    цепочка regtest/devnet — прямые правки допустимы до тега v2.0.0-stage2, но
    каждая правка фиксируется в Changelog.
  - Валидация на входе (размеры, подписи, время, генезис, replay, rate, gas)
    не ослабляется НИ одним промптом (DoD: «All Stage 0/1/1.5 invariants still
    enforced — no regressions», 22/22 enforced, regression S2-P23).
  - Determinism: Noise handshake (snow crate) и Erlay reconciliation sketches
    ДОЛЖНЫ быть детерминированными (одинаковый input → одинаковый output).
    Ephemeral keys генерируются на старте узла из long-term seed (keystore),
    не из system RNG в момент handshake. Ревью каждой новой функции на
    детерминизм — обязательный пункт КГ (threat V-21 extension для network).
  - Strangler pattern: core остаётся чистым (0 I/O); network runtime живёт в
    отдельном крейте strangecoin-net и импортируется через feature flag
    (default `["net"]`, серверная сборка `--no-default-features` не
    поддерживается на Stage 2 — net обязателен; Stage 4+ — feature для
    «headless + offline-tools» mode).
  - После каждого промпта: cargo check и cargo test --workspace обязаны
    проходить. clippy -D warnings — green.
  - Логирование — только через tracing (structured fields); println! запрещён.
  - Секреты — только в keystore/env (Ed25519 long-term node keypair — в
    keystore, NOT в config.toml); в config не писать.
  - Запрещено обновлять Changelog/README/AGENTS.md о работе, для которой
    ещё нет коммита с файлами (урок retro §8.1, прецедент FBP10).
Сокращения: КГ — критерии готовности.
```

---

## 1. Карта зависимостей промптов

```
STAGE 2 PROPER (старт только после подтверждения тега v1.5.0-stage1.5 и
сверки bugfixes-stage1.5.md v1.2 — 11/11 fixed, см. §0.1):

ADRs (ДО кода — обязательный гейт):
S2-P01 (ADR-0020: Noise Protocol Framework) ──┐
S2-P02 (ADR-0021: Erlay integration) ───────────┤
S2-P03 (ADR-0022: Peer scoring algorithm) ─────┤── все 4 ADR параллельны
S2-P04 (ADR-0023: Rate limiting strategy) ──────┘    после подтверждения
                                                      §0.1 (tag gate)
                                                              │
                                                              ▼
CRATE SCAFFOLDING:
S2-P01..S2-P04 ──▶ S2-P05 (crates/strangecoin-net/ + workspace + Cargo.toml
                              + lib.rs skeleton + feature flag)
                              │
                              ▼
                        S2-P06 (Миграция network/* в strangecoin-net; src/network/
                                mod.rs → re-export shim)
                              │
                              ▼
NOISE TRANSPORT (последовательный):
S2-P01 + S2-P06 ──▶ S2-P07 (Noise handshake XX pattern, snow crate, state machine)
                              │
                              ▼
                        S2-P08 (Noise transport encryption: ChaCha20-Poly1305
                                framing, MITM detection)
                              │
                              ▼
                        S2-P09 (node_id auth via Ed25519 internal P2P identity,
                                keystore integration)
                              │
                              ▼
PEER MANAGEMENT (последовательный):
S2-P06 + S2-P10 ──▶ S2-P10 (HELLO message + capabilities negotiation + version reject)
                              │
                              ▼
                        S2-P11 (Peer discovery: hard-coded seed IPs + DNSSEC +
                                ADDR gossip + multi-source bootstrap)
                              │
                              ▼
                        S2-P12 (peer_store + peer_manager + connection_pool split;
                                integration with rate_limiter stub)
                              │
                              ▼
GOSSIP (последовательный):
S2-P12 ──▶ S2-P13 (Gossip protocol: announce_tx/announce_block via INV,
                   GET_DATA(hash) → TX|BLOCK, broadcast)
                              │
                              ▼
                S2-P14 (Compact blocks BIP 152: siphash short tx IDs,
                        CompactBlock message, fill-up from mempool)
                              │
                              ▼
                S2-P15 (Erlay reconciliation: mincut protocol + fallback to
                        full relay per ADR-0021)
                              │
                              ▼
LIMITS (последовательный, после S2-P12):
S2-P12 + S2-P04 ──▶ S2-P16 (Rate limiting: token bucket per peer, per-message
                            caps, ban on exceed; extends Stage 1 rate_limiter)
                              │
                              ▼
S2-P12 + S2-P03 ──▶ S2-P17 (Peer scoring algorithm: factors + thresholds +
                            ban expiry; integrates with peer_manager)
                              │
                              ▼
CLEANUP + ASYNC (последовательный, после S2-P06):
S2-P06 ──▶ S2-P18 (Deprecated messages cleanup: remove GET_BLOCKCHAIN/BLOCKCHAIN)
                              │
                              ▼
                S2-P19 (Async migration: std::thread → tokio::spawn для network
                        workers, ADR-0011 continuation)
                              │
                              ▼
SHADOW (опциональный, но рекомендуется; после S2-P06):
S2-P06 ──▶ S2-P20 (Shadow network tooling: replay mainnet blocks on staging,
                    consensus divergence detector; Stage 6 preparation)
                              │
                              ▼
VERIFICATION (строго после всех proper промптов):
S2-P10..S2-P20 ──▶ S2-P21 (Fuzzing harness: 4 new network targets — noise_handshake,
                            gossip_inv, compact_blocks_fill_up, erlay_reconciliation)
                              │
                              ▼
                S2-P22 (Integration tests: noise handshake, gossip propagation,
                        compact block fill-up, erlay reconciliation, peer scoring,
                        rate limiting — 6 new test files + tests/common/net_helpers.rs)
                              │
                              ▼
S2-P01..S2-P22 ──▶ S2-P23 (Regression 22 invariants + threat model update:
                          V-1/V-2/V-15/V-17/V-24 mitigations; Stage 2 INVARIANTS_ENFORCED.md)
                              │
                              ▼
                S2-P24 (DoD-верификация Stage 2 + тег v2.0.0-stage2 — Gate для
                        Stage 3 (RocksDB) и Stage 4 (dev-experience))
```

Параллельные треки (независимые линии):
- **ADRs-трек:** S2-P01..S2-P04 — все 4 ADR параллельны после подтверждения §0.1 (tag gate). ADR-0020 (Noise) определяет scope S2-P07..S2-P09; ADR-0021 (Erlay) определяет scope S2-P15; ADR-0022 (Peer scoring) — S2-P17; ADR-0023 (Rate limiting) — S2-P16.
- **Crate-трек:** S2-P05 → S2-P06 — последовательный, scaffolding до миграции. После S2-P06 — несколько параллельных треков.
- **Noise-трек:** S2-P07 → S2-P08 → S2-P09 — последовательный, handshake до transport до auth.
- **Peer-трек:** S2-P10 → S2-P11 → S2-P12 — последовательный, handshake до discovery до peer management.
- **Gossip-трек:** S2-P13 → S2-P14 → S2-P15 — последовательный, gossip до compact blocks до Erlay (Erlay зависит от компактных block-aware mempool).
- **Limit-трек:** S2-P16 → S2-P17 — последовательный, rate limiting до scoring (scoring интегрируется с rate limit bans).
- **Cleanup-трек:** S2-P18 → S2-P19 — последовательный, deprecated cleanup до async migration.
- **Shadow-трек:** S2-P20 — параллелен после S2-P06, опциональный, но рекомендуется.
- **Verification-трек:** S2-P21 → S2-P22 → S2-P23 → S2-P24 — строго после всех proper промптов; S2-P24 — последний, gate для Stage 3/4.

Жёсткие гейты:
1. **Tag gate = Gate Stage 2:** ни один промпт S2-P01+ не начинается до подтверждения тега `v1.5.0-stage1.5` и закрытия всех S1.5-FBP01..FBP11 (см. §0.1 — все 11 fixed, tag на origin confirmed).
2. **ADR до кода:** S2-P05 (crate scaffolding) не начинается без ADR-0020..0023 (S2-P01..S2-P04); S2-P07 (Noise handshake) не начинается без ADR-0020; S2-P15 (Erlay) не без ADR-0021; S2-P17 (scoring) не без ADR-0022; S2-P16 (rate limiting) не без ADR-0023.
3. **Crate scaffolding до миграции:** S2-P06 не без S2-P05 (workspace member).
4. **Noise до peer management:** S2-P12 не без S2-P07..S2-P09 (peer connection requires encrypted transport).
5. **Peer management до gossip:** S2-P13 не без S2-P12 (gossip использует peer_manager для broadcast).
6. **Gossip до Erlay:** S2-P15 не без S2-P14 (Erlay reconciliation строится поверх compact-block-aware mempool).
7. **All proper до verification:** S2-P21 не без S2-P01..S2-P20; S2-P22 не без S2-P21; S2-P23 не без S2-P22; S2-P24 не без S2-P23.
8. **S2-P24 = Gate Stage 3/4:** Stage 3 (RocksDB) и Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*, faucet) не начинаются до тега `v2.0.0-stage2` (мягкая зависимость: Stage 3 может идти параллельно с Stage 4 согласно ROADMAP3 карта зависимостей, но формальное завершение Stage 2 — обязательный гейт для Stage 4, который требует network).

---

## 2. Список промптов

### 2.1. ADRs — решения до кода (S2-P01–S2-P04)

Эти 4 ADR — обязательный гейт для всех последующих промптов Stage 2. Принцип ARCHITECT3 §16: «решения должны быть зафиксированы в `docs/ADR/` до того, как затронут консенсус или протокол». На Stage 2 mainnet не запущен, но Noise Protocol, Erlay, peer scoring, rate limiting — все затрагивают будущий протокол (по activation height после mainnet freeze). Без ADR код пишется «на ощупь» — позже оказывается, что выбор был suboptimal, и миграция требует SCIP. Каждый ADR — короткий документ (1–2 экрана) с разделами Status / Context / Decision / Consequences / Alternatives considered по шаблону `docs/ADR/0001-template.md`.

**Замечание по нумерации ADR:** ROADMAP3 §«Критические развилки → Stage 2» первоначально планировал ADR-0016..0019 для Noise/Erlay/PeerScoring/RateLimiting. Однако Stage 1.5 FBP07 (commit `f217314` + ADR-0016 от `768fc99`) уже использовал ADR-0016 для «Receipts Root in Block Header» (файл `docs/ADR/0016-receipts-root-in-block-header.md`). Поскольку ADR-0016 уже занят и ссылается на него `BlockHeader.receipts_root` (инвариант #18 extension, Changelog 1.5.5), его перезапись была бы breaking change. **Stage 2 переименовывает свои 4 ADR на ADR-0020..0023** — это примыкающие к занятому ADR-0016 слоты и не вступает в конфликт. Если позже кто-то пожелает вернуться к ROADMAP3-плану (ADR-0016..0019), потребуется отдельный ADR-0024 «ADR renumbering» + migration commit, что выходит за scope Stage 2. Это решение зафиксировано в S2-P01 commit message.

---

### S2-P01. ADR-0020: Noise Protocol Framework

**Цель:** зафиксировать выбор transport encryption — Noise Protocol Framework (XX handshake pattern, ChaCha20-Poly1305 transport, ephemeral keys для forward secrecy). Это решение определяет всю архитектуру Stage 2 transport layer: handshake state machine, peer identity, framing, MITM detection. Без ADR — последующие промпты S2-P07..S2-P09 не имеют основания.

**Контекст:** ARCHITECT3 §3.6 заявляет «Noise-шифрование с Stage 2», §6 V-24 (Network MITM) требует forward secrecy + handshake auth. Альтернативы: (1) **Noise Protocol Framework (XX pattern)** — W3C informal standard, battle-tested (WireGuard, Signal), Rust impl `snow` crate, forward secrecy, XX pattern (mutual auth without pre-shared keys); (2) **TLS 1.3** — industry standard, но требует certificate PKI (X.509), слишком тяжёлый для P2P, не используется в Bitcoin/Ethereum P2P; (3) **raw TCP без encryption** — текущее состояние, threat V-24 не митигирован; (4) **self-implemented crypto** — anti-goal ARCHITECT3 §15 («Hand-written cryptographic primitives»); (5) **NaCl box (libsodium)** — простая, но без handshake state machine, не forward secrecy по умолчанию. Решение: Noise XX pattern via `snow` crate, ChaCha20-Poly1305 transport encryption, Ed25519 static key для mutual auth (см. S2-P09), ephemeral keys per handshake (forward secrecy).

**Задачи:**

1. Создать `docs/ADR/0020-noise-protocol-framework.md` по шаблону ADR-0001:
   - Status: Accepted
   - Context: требования (P2P без certificate PKI, forward secrecy, MITM detection, peer auth via static key)
   - Decision: Noise XX pattern via `snow` crate, ChaCha20-Poly1305 transport, Ed25519 static key для peer identity
   - Consequences:
     - Positive: forward secrecy (compromised long-term key не раскрывает прошлый трафик), mutual auth без PKI, battle-tested impl, MITM detection через static key binding
     - Negative: snow crate API может меняться между majors — pin version! handshake = 3 round trips (extra latency vs raw TCP)
     - Neutral: Ed25519 для internal node-to-node auth (НЕ для EOA, см. ADR-0001); EOA остаётся secp256k1
   - Alternatives considered: TLS 1.3 (PKI overhead), raw TCP (no encryption), self-impl (anti-goal), NaCl box (no handshake state machine)
2. Зафиксировать `snow` version в `Cargo.toml` (S2-P05, не patch, чтобы избежать breaking changes в Noise API).
3. Ссылка на ADR-0020 в `crates/strangecoin-net/Cargo.toml` comments (после S2-P05).
4. Зафиксировать handshake state machine sketch (3 round trips: initiator → responder → initiator → responder; static key exchange в messages 2 и 3).

**Артефакты:**
- `docs/ADR/0020-noise-protocol-framework.md`

**КГ (чек-лист):**
- [ ] ADR-0020 создан по template ADR-0001
- [ ] Раздел «Alternatives considered» содержит ≥3 альтернативы с обоснованием
- [ ] snow crate version выбрана (placeholder для S2-P05 pin)
- [ ] Handshake state machine sketch включён (3 round trips, static key exchange)
- [ ] Ссылка на threat V-24 mitigation зафиксирована
- [ ] Коммит `[S2-P01] adr-0020: Noise Protocol Framework (XX handshake, snow crate)`

---

### S2-P02. ADR-0021: Erlay integration

**Цель:** зафиксировать выбор relay-стратегии для gossip — Erlay reconciliation-based relay (mincut protocol) с fallback на full relay. Это решение определяет bandwidth scaling для сети на 10k+ узлов: без Erlay bandwidth O(N) на каждый tx, с Erlay — O(log N). Без ADR — последующий промпт S2-P15 не имеет основания.

**Контекст:** ARCHITECT3 §3.6 заявляет «Erlay (Stage 2)», ROADMAP3 Этап 2 risk: «Erlay reconciliation может приводить к потере txs». Erlay разработан в Bitcoin Core (BIP? — не формализован, описан в paper «Erlay: Efficient Transaction Relay for Bitcoin» 2019, BIP-330 draft). Альтернативы: (1) **Erlay reconciliation** — mincut protocol, peers поддерживают sketches known-tx-sets, периодически reconcile, передают только delta; bandwidth O(log N); fallback на full relay при sketch mismatch; (2) **Full relay only** (текущее состояние, Bitcoin pre-Erlay) — каждый tx broadcast всем пирам, O(N) bandwidth на tx, недопустимо на 10k+ узлах; (3) **BIP 152 compact blocks only** — compact blocks решают проблему на block-broadcast, но не на tx-broadcast (txs всё равно full relay до включения в block); (4) **FIBRE (Fast Internet Bitcoin Relay Engine)** — closed network из доверенных relay-нод, не decentralization-friendly; (5) **Threshold relay (Dandelion++)** — privacy-first, не bandwidth-first. Решение: Erlay mincut reconciliation с обязательным fallback на full relay при sketch mismatch (mitigation ROADMAP3 risk).

**Задачи:**

1. Создать `docs/ADR/0021-erlay-integration.md`:
   - Status: Accepted
   - Context: требования (10k+ узлов scaling, bandwidth O(log N), no tx loss, fallback policy)
   - Decision: Erlay mincut reconciliation (libminisketch-style sketches) + fallback на full relay при mismatch
   - Consequences:
     - Positive: bandwidth O(log N) на 10k+ узлов (≥80% reduction vs naive broadcast), Bitcoin Core battle-tested protocol
     - Negative: reconciliation state per peer (memory overhead), sketch mismatch fallback может временно удвоить bandwidth (mitigation: monitor mismatch rate)
     - Neutral: minisketch dependency (Rust port `minisketch-rs` или FFI to C lib)
   - Alternatives considered: full relay (O(N) недопустимо), BIP 152 only (не решает tx-relay), FIBRE (closed network), Dandelion++ (privacy-first, не bandwidth-first)
2. Sketch reconciliation protocol: peer pairs reconcile every N seconds или after M new txs; sketch sizes adaptive to set difference estimate.
3. Fallback policy: if sketch decode fails → request full tx list → broadcast missed txs (per ADR-0021).

**Артефакты:**
- `docs/ADR/0021-erlay-integration.md`

**КГ (чек-лист):**
- [ ] ADR-0021 создан по template
- [ ] Раздел «Alternatives considered» содержит ≥3 альтернативы
- [ ] Reconciliation protocol sketch включён (sketch sizes, reconcile interval)
- [ ] Fallback policy documented (sketch mismatch → full relay)
- [ ] minisketch dependency approach зафиксирован (Rust port или FFI)
- [ ] Коммит `[S2-P02] adr-0021: Erlay integration (mincut reconciliation + fallback)`

---

### S2-P03. ADR-0022: Peer scoring algorithm

**Цель:** зафиксировать peer scoring алгоритм — weighted sum факторов (latency EWMA, invalid_blocks_txs_sent, protocol_violations, uptime), ban thresholds (warn/ban), temporary + persistent bans. Это решение определяет, как сеть защищается от DoS (threat V-15), sybil (V-17), eclipse (V-1). Без ADR — последующие промпты S2-P12 (peer management) и S2-P17 (scoring impl) не имеют основания.

**Контекст:** ARCHITECT3 §3.6 заявляет «peer_store.rs, peer_manager.rs, connection_pool.rs — разделены», §6 V-15 (invalid blocks DoS), V-17 (sybil) требуют scoring + ban. ROADMAP3 Этап 2 risk: «Peer scoring может ban legitimate peers». Альтернативы: (1) **Weighted scoring (multi-factor)** — каждый фактор нормализован, weighted sum, thresholds tunable в config.toml; ban state machine (warn → temp ban → persistent ban); persistent bans в network.json; (2) **Hard ban after single violation** — простая, но false positives (legitimate peer под плохой сетью); (3) **No scoring** (текущее состояние, Stage 1) — no sybil defense, threat V-17 не митигирован; (4) **Reputation tokens / stake-based** — overengineered для MVP, требует on-chain reputation contract (Stage 7+ PoS); (5) **Sliding-window rate-based** — только на rate violations, не на invalid blocks. Решение: weighted scoring с tunable thresholds (config.toml), ban state machine (warn → temp ban 1h → persistent ban 30d), persistent bans в network.json.

**Задачи:**

1. Создать `docs/ADR/0022-peer-scoring-algorithm.md`:
   - Status: Accepted
   - Context: требования (DoS protection, sybil defense, false-positive mitigation, persistent bans survive restart)
   - Decision: weighted scoring с 4 факторами (latency EWMA 0..100ms → 0..1; invalid_blocks_txs_sent counter; protocol_violations counter; uptime percentage 0..100% → 0..1); weights в config.toml; ban state machine
   - Consequences:
     - Positive: tunable (оператор может подстроить thresholds), false-positive path (high latency but valid messages) → not banned
     - Negative: weights require tuning (no universal optimal); persistent bans в network.json может устареть (peer сменил node_id — но node_id = Ed25519 static key, не меняется)
     - Neutral: scoring интегрируется с rate_limiter (S2-P16) — rate violation → protocol_violation score increment
   - Alternatives considered: hard ban (false positives), no scoring (no sybil defense), reputation tokens (overengineered), sliding-window only (no invalid block defense)
2. Sketch scoring formula + weights: `score = w_latency * (1 - latency_norm) + w_invalid * invalid_count + w_violation * violation_count + w_uptime * uptime_norm`; initial weights: `w_latency=0.2, w_invalid=0.5, w_violation=0.2, w_uptime=0.1`.
3. Ban thresholds: warn at `score < 0.4`, temp ban at `score < 0.2`, persistent ban at `score < 0.05` (tunable в config.toml).
4. network.json schema extension: `banned_peers: [{ node_id, ban_until, reason, banned_at }]`.

**Артефакты:**
- `docs/ADR/0022-peer-scoring-algorithm.md`

**КГ (чек-лист):**
- [ ] ADR-0022 создан по template
- [ ] Раздел «Alternatives considered» содержит ≥3 альтернативы
- [ ] Scoring formula + initial weights documented
- [ ] Ban thresholds + state machine (warn → temp → persistent) documented
- [ ] network.json schema extension specified
- [ ] False-positive mitigation path documented (high latency but valid → not banned)
- [ ] Коммит `[S2-P03] adr-0022: Peer scoring algorithm`

---

### S2-P04. ADR-0023: Rate limiting strategy

**Цель:** зафиксировать rate limiting стратегию — token bucket per peer с per-message-type caps, ban on exceed (threshold from ADR-0022). Это решение определяет, как сеть защищается от spam (threat V-14: spam txs). Без ADR — последующий промпт S2-P16 не имеет основания.

**Контекст:** ARCHITECT3 §3.6 заявляет «rate_limiter.rs: токены на сообщения/сек от одного пира», инвариант #17 (Rate limiting: лимит сообщений/сек от одного пира; нарушение → бан). Stage 1 уже реализовала per-message-type caps (100 INV / 30 BLOCK per 10s per peer). Альтернативы: (1) **Token bucket per peer** — классическая, support burst (до bucket size), затем steady-state rate; per-message-type sub-buckets (extend Stage 1); интеграция с peer scoring (rate violation → protocol_violation score increment); (2) **Global rate limit** — no per-peer DoS defense, attacker может забить лимит для всех; (3) **Leaky bucket** — похож на token bucket, но сложнее рассуждать о burst; (4) **Fixed window** — boundary burst (sender может послать 2× rate в момент границы окна); (5) **Sliding window log** — точный, но memory-heavy (хранить все timestamps). Решение: token bucket per peer + per-message-type sub-buckets (extension Stage 1), ban on exceed (threshold от ADR-0022).

**Задачи:**

1. Создать `docs/ADR/0023-rate-limiting-strategy.md`:
   - Status: Accepted
   - Context: требования (per-peer DoS defense, burst support, per-message-type caps, integration with peer scoring)
   - Decision: token bucket per peer (capacity = burst size, refill rate = steady-state rate) + per-message-type sub-buckets (extend Stage 1 rate_limiter.rs); ban on exceed threshold (config.toml: `rate_limit_ban_threshold`)
   - Consequences:
     - Positive: burst support (legitimate peer с коротким burst не banned), steady-state rate enforcement, integration with peer scoring
     - Negative: token bucket state per peer (memory overhead ~100 bytes × N peers); per-message-type sub-buckets удваивает state
     - Neutral: ban on exceed интегрируется с ADR-0022 (rate violation → protocol_violation score increment, что опускает общий peer score)
   - Alternatives considered: global rate (no per-peer DoS defense), leaky bucket (harder to reason about), fixed window (boundary burst), sliding window log (memory-heavy)
2. Sketch parameters: main bucket capacity = 1000 tokens, refill rate = 100 tokens/sec; per-message-type sub-buckets: INV 100/10s, BLOCK 30/10s, TX 1000/10s (extend Stage 1); ban threshold = 5 violations in 60s.
3. Integration с peer scoring: rate violation → `peer.scoring.protocol_violations += 1` (через peer_manager API из S2-P12).

**Артефакты:**
- `docs/ADR/0023-rate-limiting-strategy.md`

**КГ (чек-лист):**
- [ ] ADR-0023 создан по template
- [ ] Раздел «Alternatives considered» содержит ≥3 альтернативы
- [ ] Token bucket parameters documented (capacity, refill rate, per-message-type caps)
- [ ] Ban threshold documented (5 violations / 60s — initial)
- [ ] Integration с peer scoring (ADR-0022) зафиксирована
- [ ] Коммит `[S2-P04] adr-0023: Rate limiting strategy`

---

### 2.2. Crate scaffolding + миграция (S2-P05–S2-P06)

Stage 2 начинается с создания крейта `crates/strangecoin-net/` (strangler pattern, ARCHITECT3 §10.4) и миграции в него существующих network-модулей из монолита `src/network/`. S2-P05 создаёт пустой крейт с Cargo.toml и lib.rs skeleton; S2-P06 мигрирует существующие модули (mod/protocol/rate_limiter/sync/sync_engine) — без behavioral changes, чистый move. После S2-P06 запускаются параллельные треки: Noise transport (S2-P07..S2-P09), peer management (S2-P10..S2-P12), cleanup (S2-P18..S2-P19), shadow network (S2-P20).

---

### S2-P05. crates/strangecoin-net/ + workspace + Cargo.toml + lib.rs skeleton

**Цель:** создать пустой крейт `crates/strangecoin-net/` с минимальным Cargo.toml, lib.rs skeleton, добавить в workspace members, сделать optional зависимостью root-крейта `strangecoin` за feature flag `net` (default on). Это первый шаг strangler pattern для network subsystem.

**Контекст:** ROADMAP3 Этап 2 DoD: «`crates/strangecoin-net/` создан, сеть перенесена». ARCHITECT3 §10.4: «Stage 2: Перенос сети в отдельный крейт — Создаётся `crates/strangecoin-net/`; Переносятся: `protocol`, `p2p`, `sync`, `gossip`, `noise`; Вводится tokio (вместо threads); `main.rs` обновляется, чтобы использовать `net` как зависимость.» На этом промпте — только scaffolding; миграция модулей — следующий S2-P06. ADR-0020 (snow crate) и ADR-0021 (Erlay) определяют будущие зависимости; ADR-0022 и ADR-0023 — алгоритмы, а не зависимости. Этот промпт НЕ добавляет snow или minisketch — это будет в S2-P07 и S2-P15.

**Задачи:**

1. Создать `crates/strangecoin-net/Cargo.toml`:
   - `name = "strangecoin-net"`
   - `version = "0.1.0"` (первый release; S2-P24 поднимет до 0.1.0 final)
   - `edition = "2021"`
   - `description = "Strangecoin network crate: P2P protocol, Noise transport, gossip, compact blocks, Erlay, peer management (Stage 2)"`
   - `[dependencies]` (пока минимальный, snow и minisketch добавятся в S2-P07/S2-P15):
     - `strangecoin-core = { path = "../strangecoin-core", version = "1.5.0" }` (для типов: Address, Transaction, Block, Hash)
     - `tokio = { version = "1", features = ["rt", "net", "sync", "time", "macros"] }`
     - `tracing = "0.1"`
     - `serde = { version = "1.0", features = ["derive"] }`
     - `blake3 = "1"`
     - `hex = "0.4"`
   - Ссылка на ADRs в comments: `# See docs/ADR/0020-noise-protocol-framework.md (Noise), 0021-erlay-integration.md (Erlay), 0022-peer-scoring-algorithm.md (scoring), 0023-rate-limiting-strategy.md (rate limit)`
2. Создать `crates/strangecoin-net/src/lib.rs` — skeleton:
   - `pub mod error;` (новый `error.rs` с `NetError` enum: `Handshake`, `Transport`, `Peer`, `Protocol`, `RateLimit`)
   - `pub mod protocol;` (stub — мигрируется в S2-P06)
   - `pub mod rate_limiter;` (stub — мигрируется в S2-P06)
   - `pub mod sync;` (stub — мигрируется в S2-P06)
   - `pub mod sync_engine;` (stub — мигрируется в S2-P06)
   - `pub mod noise;` (stub — реализуется в S2-P07..S2-P09)
   - `pub mod peer;` (stub — реализуется в S2-P12)
   - `pub mod gossip;` (stub — реализуется в S2-P13..S2-P15)
   - `pub mod discovery;` (stub — реализуется в S2-P11)
   - `pub mod shadow;` (stub — реализуется в S2-P20, опциональный)
3. Создать `crates/strangecoin-net/src/error.rs` — `NetError` enum с variants выше + `From<io::Error>`, `From<snow::Error>` (forward-compat для S2-P07).
4. Обновить root `Cargo.toml`:
   - `[workspace] members = [".", "crates/strangecoin-core", "crates/strangecoin-vm-wasm", "crates/strangecoin-net"]`
   - В `[dependencies]` strangecoin package: `strangecoin-net = { path = "crates/strangecoin-net", version = "0.1.0", optional = true }`
   - `[features] default = ["net", "vm-wasm", "gui"]`; `net = ["dep:strangecoin-net"]`
5. Создать `crates/strangecoin-net/tests/skeleton_smoke.rs` — минимальный тест: `use strangecoin_net;` компилируется, `NetError::Protocol` доступен.
6. Запустить `cargo check -p strangecoin-net` и `cargo test -p strangecoin-net` — green.

**Артефакты:**
- `crates/strangecoin-net/Cargo.toml`
- `crates/strangecoin-net/src/{lib,error}.rs` (skeleton)
- `crates/strangecoin-net/tests/skeleton_smoke.rs`
- `Cargo.toml` (workspace members + strangecoin-net dep + feature flag)
- `crates/strangecoin-net/README.md` (1 параграф: что это, ссылка на ARCHITECT3 §3.6 + ROADMAP3 Этап 2)

**КГ (чек-лист):**
- [ ] `crates/strangecoin-net/Cargo.toml` создан с deps (tokio, tracing, serde, blake3, hex, strangecoin-core path)
- [ ] `crates/strangecoin-net/src/lib.rs` skeleton с pub mod объявлениями (8 stub modules)
- [ ] `crates/strangecoin-net/src/error.rs` с `NetError` enum (5 variants + From impls)
- [ ] Root `Cargo.toml` workspace members включает `crates/strangecoin-net`
- [ ] Root `Cargo.toml` `[features] default = ["net", "vm-wasm", "gui"]` (net — default on)
- [ ] `cargo check -p strangecoin-net` green
- [ ] `cargo test -p strangecoin-net` green (skeleton_smoke)
- [ ] `cargo check --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P05] net: create crates/strangecoin-net/ + workspace member + feature flag`

---

### S2-P06. Миграция network/* в crates/strangecoin-net/ (strangler pattern)

**Цель:** мигрировать `src/network/{mod,protocol,rate_limiter,sync,sync_engine}.rs` в `crates/strangecoin-net/src/` — чистый move без behavioral changes. После миграции `src/network/mod.rs` становится 5-строчным re-export shim для backward compat с `src/main.rs` и `src/blockchain/*.rs` импортами. Этот промпт — чистый strangler pattern step: новый крейт «удушает» монолит по network-подсистеме.

**Контекст:** ARCHITECT3 §10.4: «Stage 2: Перенос сети в отдельный крейт». S2-P05 создал scaffolding. Этот промпт — фактическая миграция. Принцип: 0 behavioral changes, только move + re-export shim. Если в `src/network/*.rs` есть `use crate::` ссылки на не-network модули (например, `use crate::blockchain::BlockchainInner`), их нужно переключить на `use strangecoin_core::` (если тип в core) или сделать функцию generic over trait (если тип в монолите). Это может потребовать minor refactoring в src/blockchain/, что фиксируется в этом же коммите.

**Задачи:**

1. Скопировать (не удалить!) `src/network/{mod,protocol,rate_limiter,sync,sync_engine}.rs` в `crates/strangecoin-net/src/{mod,protocol,rate_limiter,sync,sync_engine}.rs` (заменить stubs из S2-P05).
2. В каждом перенесённом файле:
   - Заменить `use crate::` (если указывает на network-модуль) на `use crate::` (теперь внутри strangecoin-net).
   - Заменить `use crate::blockchain::*` / `use crate::mempool::*` / `use crate::consensus::*` (если они ссылаются на типы в `src/` монолита) на `use strangecoin_core::*` (если тип в core) или на trait-bound generic (если тип в монолите).
   - Если тип из монолита (например, `BlockchainInner`) — сделать `pub trait BlockchainView { fn add_block(...); fn get_tip(...); }` в strangecoin-net, монолит реализует его для `BlockchainInner`. Это разрывает цикл network↔blockchain (ARCHITECT3 §17 PR-checklist: «Sync engine: разрыв цикла network↔blockchain»).
3. Заменить `src/network/mod.rs` на 5-строчный re-export shim:
   ```rust
   #[cfg(feature = "net")]
   pub use strangecoin_net::{
       mod::*, protocol::*, rate_limiter::*, sync::*, sync_engine::*,
   };
   ```
4. Обновить `src/network/{protocol,rate_limiter,sync,sync_engine}.rs` — удалить (теперь в strangecoin-net).
5. Запустить `cargo check --workspace` — green (или итеративно чинить imports).
6. Запустить `cargo test --workspace` — все тесты green: tests/two_clients.rs, tests/sync_engine.rs, tests/rate_limit.rs, tests/network.rs, tests/sync_headers.rs, tests/network_id.rs.

**Артефакты:**
- `crates/strangecoin-net/src/{mod,protocol,rate_limiter,sync,sync_engine}.rs` (мигрированные)
- `src/network/mod.rs` (5-line re-export shim)
- `src/network/{protocol,rate_limiter,sync,sync_engine}.rs` — удалены
- (опционально) `crates/strangecoin-net/src/blockchain_view.rs` (trait для разрыва цикла network↔blockchain)
- `src/blockchain/*.rs` (если потребовалось переключить на trait)

**КГ (чек-лист):**
- [ ] `crates/strangecoin-net/src/{mod,protocol,rate_limiter,sync,sync_engine}.rs` существуют, не stubs
- [ ] `src/network/mod.rs` — 5-line re-export shim
- [ ] `src/network/{protocol,rate_limiter,sync,sync_engine}.rs` — удалены
- [ ] Цикл network↔blockchain разорван (через trait или иным способом)
- [ ] `cargo check --workspace` green
- [ ] `cargo test --workspace` green (особенно tests/two_clients, sync_engine, rate_limit, network, sync_headers, network_id)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Regression: все Stage 0/1/1.5 тесты green — no behavioral changes
- [ ] Коммит `[S2-P06] net: migrate network modules to strangecoin-net crate (strangler pattern)`

---

### 2.3. Noise transport (S2-P07–S2-P09)

S2-P07..S2-P09 реализуют Noise Protocol Framework (ADR-0020): S2-P07 — handshake state machine (XX pattern via snow); S2-P08 — ChaCha20-Poly1305 transport encryption для всех post-handshake frames + MITM detection; S2-P09 — Ed25519-based node_id auth (internal P2P identity, не EOA). После S2-P09 каждое TCP-соединение в Strangecoin P2P зашифровано, peer identity верифицирована, MITM невозможен без компрометации long-term static key (threat V-24 mitigation).

---

### S2-P07. Noise handshake XX pattern (snow crate, state machine)

**Цель:** реализовать Noise XX handshake state machine в `crates/strangecoin-net/src/noise/{mod,handshake}.rs` с использованием `snow` crate. Handshake = 3 round trips: initiator → responder (e), responder → initiator (e, ee, s, es), initiator → responder (s, se), responder → initiator (transport ready). После handshake обе стороны имеют shared secret + transport state (ChaCha20-Poly1305).

**Контекст:** ADR-0020 (S2-P01) зафиксировал Noise XX pattern + snow crate. snow — Rust implementation Noise Protocol Framework, поддерживает XX/IK/PSK patterns. XX pattern: mutual auth без pre-shared keys, 3 round trips, ephemeral keys для forward secrecy, static keys для identity (см. S2-P09). После handshake `snow::TransportState` предоставляет `encrypt/decrypt` для всех последующих сообщений. Без S2-P07 — S2-P08 (transport encryption) не имеет состояния для шифрования.

**Задачи:**

1. Добавить `snow = "=x.y.z"` (pin конкретную версию, не ^) в `crates/strangecoin-net/Cargo.toml [dependencies]`.
2. Создать `crates/strangecoin-net/src/noise/mod.rs` — pub mod declarations + `NoiseError` (если не в `error.rs`).
3. Создать `crates/strangecoin-net/src/noise/handshake.rs`:
   - `pub struct NoiseHandshake { state: snow::HandshakeState, role: NoiseRole }`
   - `pub enum NoiseRole { Initiator, Responder }`
   - `impl NoiseHandshake`:
     - `pub fn initiator(static_key: &[u8; 32]) -> Result<Self, NoiseError>` — `snow::Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse()?).local_private_key(static_key).build_initiator()?`
     - `pub fn responder(static_key: &[u8; 32]) -> Result<Self, NoiseError>` — аналогично `build_responder()`
     - `pub fn write_message(&mut self, payload: &[u8], out: &mut [u8]) -> Result<usize, NoiseError>`
     - `pub fn read_message(&mut self, payload: &[u8], out: &mut [u8]) -> Result<usize, NoiseError>`
     - `pub fn is_finished(&self) -> bool`
     - `pub fn into_transport(self) -> Result<NoiseTransport, NoiseError>` — `self.state.into_transport_mode()?`
4. State machine (3 round trips):
   - Round 1: initiator → responder, payload empty (or minimal: `node_id` hash, см. S2-P09), message contains ephemeral key
   - Round 2: responder → initiator, payload = responder's static key encrypted, message contains responder's ephemeral + DH (ee, es)
   - Round 3: initiator → responder, payload = initiator's static key encrypted, DH (se)
   - После round 3: handshake finished, `into_transport()` возвращает `NoiseTransport`
5. Тесты в `crates/strangecoin-net/tests/noise_handshake.rs`:
   - Handshake completes in 3 round trips — both sides reach `is_finished() == true`
   - Same static keys on both sides → same shared secret (deterministic)
   - Invalid handshake message (corrupted bytes) → `read_message` returns `Err(NoiseError::Decrypt)`
   - Wrong static key on one side → handshake fails at `se` step (auth check)
6. Запустить `cargo test -p strangecoin-net --test noise_handshake` — green.

**Артефакты:**
- `crates/strangecoin-net/Cargo.toml` (snow pinned dependency)
- `crates/strangecoin-net/src/noise/{mod,handshake}.rs`
- `crates/strangecoin-net/tests/noise_handshake.rs`

**КГ (чек-лист):**
- [ ] `snow = "=x.y.z"` pinned в Cargo.toml (не ^, не patch)
- [ ] `NoiseHandshake` struct с `initiator`/`responder` constructors
- [ ] State machine: 3 round trips → `is_finished() == true`
- [ ] Deterministic: same static keys → same shared secret
- [ ] Invalid handshake message → `Err(NoiseError::Decrypt)`
- [ ] Wrong static key → handshake fails at `se` step
- [ ] `cargo test -p strangecoin-net --test noise_handshake` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P07] net/noise: XX handshake state machine (snow crate, forward secrecy)`

---

### S2-P08. Noise transport encryption (ChaCha20-Poly1305 framing)

**Цель:** зашифровать все post-handshake TCP-фреймы ChaCha20-Poly1305 через `snow::TransportState`. Обновить `crates/strangecoin-net/src/protocol.rs` — length-prefixed framing теперь заворачивается в encrypted envelope: `[length: u32 BE][nonce: u64][ciphertext + AEAD tag]`. Nonce монотонный per-session, reject out-of-order. MITM detection: handshake auth (S2-P09) связывает peer static key с ожидаемым node_id; mismatch → drop connection.

**Контекст:** S2-P07 дал handshake → `NoiseTransport`. Этот промпт использует `NoiseTransport` для шифрования всех фреймов. ARCHITECT3 §6 V-24 (Network MITM) требует: handshake auth + transport encryption. MITM detection в этом промпте — на transport layer: если AEAD tag не валидируется → connection drop. S2-P09 добавит Ed25519 node_id auth на handshake layer. Без S2-P08 — после handshake фреймы идут plaintext, что эквивалентно TLS handshake без record-layer encryption.

**Задачи:**

1. Создать `crates/strangecoin-net/src/noise/transport.rs`:
   - `pub struct NoiseTransport { state: snow::TransportState, send_nonce: u64, recv_nonce: u64 }`
   - `impl NoiseTransport`:
     - `pub fn encrypt(&mut self, plaintext: &[u8], out: &mut [u8]) -> Result<usize, NoiseError>` — increment `send_nonce`, AEAD encrypt
     - `pub fn decrypt(&mut self, ciphertext: &[u8], out: &mut [u8]) -> Result<usize, NoiseError>` — verify AEAD tag, reject if `recv_nonce` не monotonic (anti-replay)
     - `pub fn rekey(&mut self)` — periodic rekey (после N сообщений или M байт, mitigation key compromise)
2. Обновить `crates/strangecoin-net/src/protocol.rs`:
   - New wire format: `[length: u32 BE][nonce: u64][ciphertext + AEAD tag: length - 8]`
   - `pub fn encrypt_frame(transport: &mut NoiseTransport, msg: &Message, out: &mut Vec<u8>) -> Result<(), NetError>`
   - `pub fn decrypt_frame(transport: &mut NoiseTransport, raw: &[u8]) -> Result<Message, NetError>`
   - Length-prefix check ДО аллокации (инвариант #16) — `MAX_MESSAGE_SIZE` cap (1 MB)
3. MITM detection: на handshake (S2-P07) static key проверяется против expected `node_id` (S2-P09 даст expected). Mismatch → drop connection до начала transport. Этот промпт готовит API: `NoiseTransport::new()` принимает `expected_node_id: Option<[u8; 32]>` — если Some, проверяется после handshake.
4. Тесты в `crates/strangecoin-net/tests/noise_transport.rs`:
   - Легитимный peer → encrypt/decrypt round-trip работает
   - MITM attacker (own Noise keys) → handshake fails at auth step (см. S2-P09) — этот тест full path только после S2-P09; на этом промпте — encrypt/decrypt round-trip + AEAD tamper detection
   - Tampered ciphertext (1 byte flipped) → `decrypt` returns `Err(NoiseError::Decrypt)`
   - Out-of-order nonce (replay) → `decrypt` returns `Err(NoiseError::Replay)`
   - Rekey after N messages → new key, old ciphertext rejected
5. Запустить `cargo test -p strangecoin-net --test noise_transport` — green.

**Артефакты:**
- `crates/strangecoin-net/src/noise/transport.rs`
- `crates/strangecoin-net/src/protocol.rs` (обновлён: encrypt_frame/decrypt_frame)
- `crates/strangecoin-net/tests/noise_transport.rs`

**КГ (чек-лист):**
- [ ] `NoiseTransport` struct с encrypt/decrypt + monotonic nonces
- [ ] Wire format updated: `[length][nonce][ciphertext + AEAD tag]`
- [ ] `MAX_MESSAGE_SIZE` cap enforced ДО аллокации (инвариант #16 сохранён)
- [ ] AEAD tamper detection: 1 byte flip → `Err(NoiseError::Decrypt)`
- [ ] Replay protection: out-of-order nonce → `Err(NoiseError::Replay)`
- [ ] Rekey after N messages → new key, old ciphertext rejected
- [ ] `cargo test -p strangecoin-net --test noise_transport` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P08] net/noise: ChaCha20-Poly1305 transport encryption + MITM detection API`

---

### S2-P09. node_id auth via Ed25519 (internal P2P identity)

**Цель:** реализовать Ed25519-based node_id auth для Noise handshake. Каждый узел имеет long-term Ed25519 keypair (stored in keystore, НЕ для EOA — только для internal node-to-node auth per ARCHITECT3 §3.8). `node_id = blake3(pubkey)`. Handshake auth message подписывается Ed25519 static key; peer верифицирует подпись против expected node_id (из peer_store или hard-coded seed). Mismatch → drop connection до transport layer.

**Контекст:** ARCHITECT3 §3.8: «Ed25519 — опционально для internal node-to-node auth (не для транзакций)». S2-P07/S2-P08 дали Noise handshake + transport encryption, но без identity verification: MITM с собственными Noise keys может перехватить handshake (если нет separate auth layer). Этот промпт добавляет Ed25519 подпись на handshake auth message (3rd round trip) — peer верифицирует подпись против expected `node_id`. Если `node_id` unknown (новый peer из ADDR gossip) — верификация всё равно проходит (signature валидна против pubkey), но peer помечается как «unverified» в peer_store (до первого handshake). Ed25519 здесь — internal node identity, НЕ для EOA transactions (EOA остаётся secp256k1, ADR-0001).

**Задачи:**

1. Добавить `ed25519-dalek = "=2.0"` (pin) в `crates/strangecoin-net/Cargo.toml [dependencies]`.
2. Создать `crates/strangecoin-net/src/noise/auth.rs`:
   - `pub struct NodeId([u8; 32]);` — `node_id = blake3(pubkey)`
   - `pub struct NodeKeypair { secret: ed25519_dalek::SigningKey, public: ed25519_dalek::VerifyingKey }`
   - `impl NodeKeypair`:
     - `pub fn from_keystore(keystore: &Keystore) -> Result<Self, NetError>` — load long-term Ed25519 keypair
     - `pub fn node_id(&self) -> NodeId` — `NodeId(blake3(self.public.to_bytes()))`
     - `pub fn sign(&self, msg: &[u8]) -> ed25519_dalek::Signature`
   - `pub fn verify_node_id(signature: &Signature, msg: &[u8], expected_node_id: &NodeId) -> Result<(), NetError>` — verify signature, derive pubkey, derive node_id, compare
3. Обновить `crates/strangecoin-net/src/noise/handshake.rs` (S2-P07):
   - `NoiseHandshake::initiator(static_key, identity_key: &NodeKeypair, expected_node_id: Option<NodeId>)` — payload в round 3 содержит `identity_key.sign(handshake_hash)`. `expected_node_id` — для known peers (из peer_store); для новых peers из ADDR gossip — None, подпись всё равно проверяется, peer помечается «unverified».
   - `NoiseHandshake::responder(static_key, identity_key: &NodeKeypair, expected_node_id: Option<NodeId>)` — аналогично
   - После `into_transport()` возвращает `Result<(NoiseTransport, NodeId), NetError>` — `NodeId` верифицирован (или None для unverified)
4. MITM detection test (полный path): MITM с собственными Noise keys пытается handshake → handshake завершается, но `NodeId` не совпадает с `expected_node_id` → drop connection до transport.
5. Тесты в `crates/strangecoin-net/tests/noise_auth.rs`:
   - Peer с valid Ed25519 auth → connection accepted, returns correct `NodeId`
   - Peer с invalid signature → handshake fails at auth step
   - Peer с mismatched `NodeId` (MITM) → connection dropped
   - New peer (None expected_node_id) → accepted, marked «unverified»
6. Запустить `cargo test -p strangecoin-net --test noise_auth` — green.

**Артефакты:**
- `crates/strangecoin-net/Cargo.toml` (ed25519-dalek pinned)
- `crates/strangecoin-net/src/noise/auth.rs`
- `crates/strangecoin-net/src/noise/handshake.rs` (обновлён: identity_key param + NodeId return)
- `crates/strangecoin-net/tests/noise_auth.rs`

**КГ (чек-лист):**
- [ ] `ed25519-dalek = "=2.0"` pinned в Cargo.toml
- [ ] `NodeId` struct с `blake3(pubkey)` derivation
- [ ] `NodeKeypair` с `from_keystore`, `sign`, `verify_node_id`
- [ ] `NoiseHandshake::initiator/responder` принимают `identity_key` + `expected_node_id`
- [ ] `into_transport()` возвращает `Result<(NoiseTransport, NodeId), NetError>`
- [ ] MITM с own Noise keys → connection dropped (NodeId mismatch)
- [ ] New peer (None expected_node_id) → accepted, marked «unverified»
- [ ] `cargo test -p strangecoin-net --test noise_auth` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P09] net/noise: Ed25519 node_id auth (internal P2P, not EOA)`

---

### 2.4. Peer handshake + discovery + менеджмент (S2-P10–S2-P12)

S2-P10..S2-P12 реализуют peer management subsystem (ARCHITECT3 §3.6): S2-P10 — HELLO message + capabilities negotiation (version/network_id/capabilities/height, reject incompatible); S2-P11 — peer discovery (hard-coded seed IPs + DNSSEC + ADDR gossip + multi-source bootstrap); S2-P12 — split peer_store/peer_manager/connection_pool (архитектурное разделение ARCHITECT3 §3.6). После S2-P12 каждый peer в сети имеет persistent state (score, ban history, last_seen), peer_manager оркестрирует соединения, connection_pool управляет TCP lifecycle.

---

### S2-P10. HELLO message + capabilities negotiation

**Цель:** реализовать HELLO message в `crates/strangecoin-net/src/protocol/hello.rs` — отправляется сразу после Noise handshake (S2-P09) завершён. Содержит `{version, network_id, capabilities (bitfield), height}`. Reject peers с `version < MIN_PROTO_VERSION` (initial = 2 для Stage 2). Reject peers с mismatched `network_id` (инвариант #10 replay protection). Capabilities negotiation: обе стороны advertise, используется common subset.

**Контекст:** ARCHITECT3 §3.6: «`HELLO { version, network_id, capabilities, height }`. Capabilities negotiation (what features this peer supports). Reject peers with `version < MIN_PROTO_VERSION`.» ROADMAP3 Этап 2 DoD: «Handshake works (test: incompatible peers отвергаются)». Capabilities bitfield (первые 8 бит): NOISE=0x01, COMPACT_BLOCKS=0x02, ERLAY=0x04, SHADOW=0x08 (для Stage 6). Без S2-P10 — peer connection остаётся без version/capabilities negotiation, что блокирует дальнейшие промпты (gossip/Erlay/compact blocks) — им нужны capabilities flags.

**Задачи:**

1. Создать `crates/strangecoin-net/src/protocol/hello.rs`:
   - `pub struct Hello { version: u32, network_id: u32, capabilities: u32, height: u64 }` — wire-формат (length-prefixed, после Noise encryption)
   - `pub const MIN_PROTO_VERSION: u32 = 2;` — Stage 2 protocol version (Stage 1.5 был 1)
   - `pub enum Capability { Noise = 0x01, CompactBlocks = 0x02, Erlay = 0x04, Shadow = 0x08 }`
   - `pub fn negotiate_capabilities(local: u32, remote: u32) -> u32` — `local & remote` (common subset)
   - `pub fn validate_hello(hello: &Hello, local_network_id: u32) -> Result<(), NetError>` — reject if `hello.version < MIN_PROTO_VERSION`, reject if `hello.network_id != local_network_id`
2. Wire format (после Noise encrypted framing): `[version: u32 BE][network_id: u32 BE][capabilities: u32 BE][height: u64 BE]` (20 bytes fixed-size message)
3. Интеграция в peer connection flow: после S2-P09 `into_transport()` обе стороны обмениваются HELLO через `encrypt_frame`/`decrypt_frame` (S2-P08). Если validate_hello возвращает Err — drop connection.
4. Тесты в `crates/strangecoin-net/tests/hello.rs`:
   - Compatible peers (same version, same network_id) → connection accepted, negotiated capabilities correct
   - Incompatible version (peer version 1, local MIN_PROTO_VERSION 2) → connection rejected
   - Incompatible network_id (peer network_id 1, local 3) → connection rejected (инвариант #10)
   - Capability negotiation: peer advertises 0x07 (NOISE+Compact+Erlay), local 0x05 (NOISE+Compact) → negotiated 0x05 (subset)
   - HELLO round-trip через Noise encrypted framing works end-to-end
5. Запустить `cargo test -p strangecoin-net --test hello` — green.

**Артефакты:**
- `crates/strangecoin-net/src/protocol/hello.rs`
- `crates/strangecoin-net/tests/hello.rs`

**КГ (чек-лист):**
- [ ] `Hello` struct с version/network_id/capabilities/height
- [ ] `MIN_PROTO_VERSION = 2` const
- [ ] `Capability` enum (Noise/CompactBlocks/Erlay/Shadow bitfield)
- [ ] `negotiate_capabilities` возвращает common subset
- [ ] `validate_hello` reject incompatible version + mismatched network_id
- [ ] HELLO exchange через Noise encrypted framing работает end-to-end
- [ ] Тесты: incompatible version rejected, mismatched network_id rejected, capability negotiation correct
- [ ] `cargo test -p strangecoin-net --test hello` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P10] net/protocol: HELLO + capabilities negotiation + version reject`

---

### S2-P11. Peer discovery (seed IPs + DNSSEC + ADDR gossip)

**Цель:** реализовать peer discovery в `crates/strangecoin-net/src/discovery/{mod,seeds,dnssec,addr_gossip}.rs` — hard-coded seed IPs в `network.json` (не только DNS), DNSSEC validation для DNS-resolved seeds, ADDR gossip (peers обмениваются списками адресов), multi-source bootstrap (try ≥3 sources, succeed with 1 down). Цель: new node находит ≥8 peers за ≤30 сек.

**Контекст:** ARCHITECT3 §3.6: «Peer discovery through seed nodes + addr gossip, peer scoring». §6 V-16 (Eclipse via DNS poisoning): «Hard-coded seed IPs (not just DNS); DNSSEC validation; rotation of seeds». ROADMAP3 Этап 2 DoD: «Peer discovery: new node finds ≥8 peers in ≤30 sec». Текущее состояние (Stage 1.5): DNS-only seeds без DNSSEC validation — threat V-16 не митигирован. Этот промпт добавляет: hard-coded seed IPs в network.json, DNSSEC validation, ADDR gossip message, multi-source bootstrap.

**Задачи:**

1. Создать `crates/strangecoin-net/src/discovery/mod.rs` — pub mod declarations.
2. Создать `crates/strangecoin-net/src/discovery/seeds.rs`:
   - `pub struct SeedConfig { hard_coded_ips: Vec<IpAddr>, dns_seeds: Vec<String>, dnssec_required: bool }`
   - Загрузка из `network.json` (новый раздел `seed_config`)
   - `pub fn load_seeds(config_path: &Path) -> Result<SeedConfig, NetError>`
3. Создать `crates/strangecoin-net/src/discovery/dnssec.rs`:
   - `pub fn resolve_with_dnssec(domain: &str) -> Result<Vec<IpAddr>, NetError>` — использует `hickory-resolver` crate с DNSSEC validation
   - Reject responses без DNSSEC validation (если `dnssec_required = true` в config)
4. Создать `crates/strangecoin-net/src/discovery/addr_gossip.rs`:
   - `ADDR` message: `{addrs: Vec<(NodeId, IpAddr, Port)>}` — peers обмениваются списками известных addresses
   - `pub fn handle_addr_message(addrs: Vec<(NodeId, IpAddr, Port)>, peer_store: &mut PeerStore)` — добавить new addrs в peer_store (marked «unverified»)
   - `pub fn broadcast_known_addrs(peer_manager: &PeerManager, max_count: usize)` — отправить top-K known addrs случайным пирам
5. Multi-source bootstrap algorithm: try ≥3 sources (hard-coded IPs, DNS seeds, ADDR gossip from initial peers); succeed if ≥1 source returns ≥8 peers. Total time budget 30 сек.
6. Тесты в `crates/strangecoin-net/tests/discovery.rs` (simulated, no real network):
   - New node с only seed IPs → finds ≥8 peers in ≤30s (simulated clock)
   - DNSSEC validation rejects poisoned DNS (mock DNS resolver returning invalid DNSSEC)
   - ADDR gossip propagates addresses through simulated network
   - Multi-source bootstrap succeeds with 1 of 3 sources down (mocked failure)
7. Запустить `cargo test -p strangecoin-net --test discovery` — green.

**Артефакты:**
- `crates/strangecoin-net/src/discovery/{mod,seeds,dnssec,addr_gossip}.rs`
- `network.json` (schema extension: `seed_config` section)
- `crates/strangecoin-net/Cargo.toml` (hickory-resolver dep)
- `crates/strangecoin-net/tests/discovery.rs`

**КГ (чек-лист):**
- [ ] `SeedConfig` с hard_coded_ips + dns_seeds + dnssec_required
- [ ] DNSSEC validation rejects non-DNSSEC responses
- [ ] `ADDR` gossip message implemented
- [ ] Multi-source bootstrap algorithm (≥3 sources, succeed with 1 down)
- [ ] Test: new node finds ≥8 peers in ≤30s (simulated)
- [ ] Test: DNSSEC validation rejects poisoned DNS (V-16 mitigation)
- [ ] Test: ADDR gossip propagates addresses
- [ ] Test: multi-source bootstrap with 1/3 sources down
- [ ] `cargo test -p strangecoin-net --test discovery` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P11] net/discovery: seed IPs + DNSSEC + ADDR gossip + multi-source bootstrap`

---

### S2-P12. peer_store + peer_manager + connection_pool split

**Цель:** разделить peer management на 3 модуля в `crates/strangecoin-net/src/peer/{store,manager,pool}.rs` (ARCHITECT3 §3.6): `peer_store.rs` — persistent peer db + scoring state (из ADR-0022); `peer_manager.rs` — active connection orchestration + ban logic; `connection_pool.rs` — TCP connection lifecycle. Scoring factors из ADR-0022: latency EWMA, invalid_blocks_txs_sent, protocol_violations, uptime. Ban threshold tunable в `config.toml: peer_ban_threshold`. Persistent bans в `network.json: banned_peers`.

**Контекст:** ARCHITECT3 §3.6: «peer_store.rs / peer_manager.rs / connection_pool.rs — разделены как в ARCHITECT.md §11.6». §6 V-15 (invalid blocks DoS), V-17 (sybil) требуют scoring + ban. Текущее состояние (Stage 1.5 после S2-P06 миграции): вся peer-management логика в `crates/strangecoin-net/src/mod.rs` (бывший `src/network/mod.rs`) — monolithic, без разделения. Этот промпт делает архитектурное разделение + scoring (full impl в S2-P17, здесь только skeleton + state).

**Задачи:**

1. Создать `crates/strangecoin-net/src/peer/mod.rs` — pub mod declarations + `Peer` struct.
2. Создать `crates/strangecoin-net/src/peer/store.rs`:
   - `pub struct PeerStore { peers: HashMap<NodeId, PeerRecord>, ban_state: HashMap<NodeId, BanRecord> }`
   - `pub struct PeerRecord { node_id: NodeId, addr: SocketAddr, last_seen: Instant, score: f32, latency_ewma: Duration, invalid_count: u32, violation_count: u32, uptime_seconds: u64, verified: bool }`
   - `pub struct BanRecord { until: Instant, reason: BanReason, banned_at: Instant, persistent: bool }`
   - `pub enum BanReason { InvalidBlocks, ProtocolViolation, RateLimit, Manual }`
   - API: `add_peer`, `remove_peer`, `get_peer`, `update_score`, `ban_peer`, `unban_peer`, `load_from_network_json`, `save_to_network_json`
3. Создать `crates/strangecoin-net/src/peer/manager.rs`:
   - `pub struct PeerManager { store: PeerStore, pool: ConnectionPool, ban_threshold: f32, max_connections: usize }`
   - API: `connect_to_peer(addr, expected_node_id)`, `disconnect_peer(node_id)`, `broadcast_message(msg)`, `tick()` (periodic scoring update, ban expiry check)
   - Integration с rate_limiter (S2-P16 даёт `rate_violation_count` → `peer.violation_count += 1`)
4. Создать `crates/strangecoin-net/src/peer/pool.rs`:
   - `pub struct ConnectionPool { connections: HashMap<NodeId, Arc<Connection>>, max_per_ip: usize }` — limit connections per IP range (sybil defense V-17)
   - `pub struct Connection { transport: NoiseTransport, peer_addr: SocketAddr, node_id: NodeId, capabilities: u32 }`
   - API: `add_connection`, `remove_connection`, `get_connection`, `count_by_ip_range`
5. Перенести peer-management logic из `crates/strangecoin-net/src/mod.rs` в 3 новых модуля.
6. `network.json` schema: `peers: [{ node_id, addr, last_seen, score, ... }]` + `banned_peers: [{ node_id, until, reason, ... }]` — persistent state.
7. Тесты в `crates/strangecoin-net/tests/peer_management.rs`:
   - 3 modules: PeerStore.save_to_network_json → restart → PeerStore.load_from_network_json recovers all peers
   - Malicious peer sending invalid blocks → score drops → banned after N violations (threshold from config.toml)
   - Ban expiry respected (temp ban 1h → unbanned after 1h simulated)
   - Persistent ban survives restart
   - False-positive path (high latency but valid messages) → NOT banned
   - Connection pool limit per IP range enforced (sybil defense V-17)
8. Запустить `cargo test -p strangecoin-net --test peer_management` — green.

**Артефакты:**
- `crates/strangecoin-net/src/peer/{mod,store,manager,pool}.rs`
- `crates/strangecoin-net/src/mod.rs` (peer-management logic removed)
- `network.json` (schema extension: peers + banned_peers + seed_config)
- `config.toml` (schema extension: peer_ban_threshold, max_connections, max_per_ip_range)
- `crates/strangecoin-net/tests/peer_management.rs`

**КГ (чек-лист):**
- [ ] 3 modules split: peer_store, peer_manager, connection_pool
- [ ] `PeerStore` с PeerRecord + BanRecord + load/save to network.json
- [ ] `PeerManager` с connect/disconnect/broadcast/tick API
- [ ] `ConnectionPool` с per-IP-range limit (V-17 mitigation)
- [ ] Persistent peer state survives restart
- [ ] Ban state machine (warn → temp ban 1h → persistent ban 30d)
- [ ] False-positive path tested (high latency but valid → not banned)
- [ ] `cargo test -p strangecoin-net --test peer_management` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P12] net/peer: split peer_store/manager/pool + scoring skeleton + ban state machine`

---

### 2.5. Gossip protocol + compact blocks + Erlay (S2-P13–S2-P15)

S2-P13..S2-P15 реализуют gossip subsystem (ARCHITECT3 §3.6): S2-P13 — Bitcoin-style gossip (INV/GET_DATA/TX/BLOCK, broadcast to all peers); S2-P14 — BIP 152 compact blocks (siphash short tx IDs + fill-up from mempool, ≥50% bandwidth reduction); S2-P15 — Erlay reconciliation (mincut protocol + fallback to full relay, ≥80% bandwidth reduction на 10k+ узлов). После S2-P15 network scaling с 100 до 10k+ узлов становится feasible.

---

### S2-P13. Gossip protocol (announce_tx/announce_block via INV)

**Цель:** реализовать Bitcoin-style gossip в `crates/strangecoin-net/src/gossip/{mod,inv,get_data,broadcast}.rs`: `announce_tx(txid)` и `announce_block(block_hash)` через INV message; `GET_DATA(hash) → TX|BLOCK` response; broadcast new INV всем connected peers (через peer_manager из S2-P12). Цель: txs распространяются за ≤1 сек на 100 узлах.

**Контекст:** ARCHITECT3 §3.6: «`gossip.rs`: announce + инвентарь (как Bitcoin `inv`), compact blocks (BIP 152), Erlay (Stage 2)». ROADMAP3 Этап 2 DoD: «Gossip работает (test: txs распространяются за ≤1 сек на 100 узлах)». Bitcoin-стиль: каждый peer, получивший new tx/block, broadcast INV всем своим пирам; peers, не имеющие объекта, отвечают GET_DATA; оригинальный peer отвечает TX/BLOCK. Без S2-P13 — S2-P14 (compact blocks) и S2-P15 (Erlay) не имеют baseline для bandwidth reduction comparison.

**Задачи:**

1. Создать `crates/strangecoin-net/src/gossip/mod.rs` — pub mod declarations + `GossipMessage` enum.
2. Создать `crates/strangecoin-net/src/gossip/inv.rs`:
   - `INV` message: `{items: Vec<InventoryItem>}`
   - `pub struct InventoryItem { kind: InvKind, hash: [u8; 32] }`
   - `pub enum InvKind { Tx, Block }`
3. Создать `crates/strangecoin-net/src/gossip/get_data.rs`:
   - `GET_DATA` message: `{items: Vec<InventoryItem>}` — same shape as INV
   - Response: `TX { tx: Transaction}` или `BLOCK { block: Block }` или `NOTFOUND { items: Vec<InventoryItem> }`
4. Создать `crates/strangecoin-net/src/gossip/broadcast.rs`:
   - `pub fn announce_tx(txid: [u8; 32], peer_manager: &PeerManager)` — broadcast INV(tx) всем non-banned peers
   - `pub fn announce_block(block_hash: [u8; 32], peer_manager: &PeerManager)` — broadcast INV(block) всем non-banned peers
   - `pub fn handle_inv(inv: Inv, peer_manager: &PeerManager, mempool: &Mempool, blockchain: &dyn BlockchainView) -> Result<Vec<GossipMessage>, NetError>` — для каждого item, если неизвестно → запросить GET_DATA
   - `pub fn handle_get_data(get_data: GetData, mempool: &Mempool, blockchain: &dyn BlockchainView) -> Vec<GossipMessage>` — для каждого item, если известен → ответить TX/BLOCK, иначе NOTFOUND
5. INV deduplication: не broadcast INV для объектов, которые peer уже видел (по peer's known-set tracking)
6. Тесты в `crates/strangecoin-net/tests/gossip.rs`:
   - 1 node creates tx → propagates to 100 simulated peers in ≤1 sec (simulated clock, no real network)
   - INV deduplication: peer не запрашивает tx, который уже имеет
   - GET_DATA для unknown hash → NOTFOUND response
   - GET_DATA для known tx → TX response
7. Запустить `cargo test -p strangecoin-net --test gossip` — green.

**Артефакты:**
- `crates/strangecoin-net/src/gossip/{mod,inv,get_data,broadcast}.rs`
- `crates/strangecoin-net/tests/gossip.rs`

**КГ (чек-лист):**
- [ ] INV/GET_DATA/TX/BLOCK/NOTFOUND messages implemented
- [ ] `announce_tx`/`announce_block` broadcast через peer_manager
- [ ] INV deduplication per peer (no GET_DATA для known objects)
- [ ] GET_DATA → TX/BLOCK/NOTFOUND response
- [ ] Test: 1 node → 100 peers propagation ≤1s (simulated)
- [ ] `cargo test -p strangecoin-net --test gossip` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P13] net/gossip: inv/GET_DATA announce + broadcast`

---

### S2-P14. Compact blocks (BIP 152) — siphash short tx IDs + fill-up

**Цель:** реализовать BIP 152 compact blocks в `crates/strangecoin-net/src/gossip/compact_blocks.rs`: siphash-2-4 short tx IDs (8 bytes, derived from txid + per-block nonce), `CompactBlock` message `{header, nonce, short_ids[], prefilled_txs[]}`, receiver fills from mempool, requests missing via `GET_BLOCK_TXN`, responder sends `BLOCK_TXN`. Bandwidth reduction ≥50% vs full block broadcast.

**Контекст:** ARCHITECT3 §3.6: «compact blocks (BIP 152)». ROADMAP3 Этап 2 DoD: «Compact blocks работают (test: bandwidth reduction ≥50% vs naive)». BIP 152 (Bitcoin Improvement Proposal 152): вместо broadcast full block (например 1 MB с 1000 txs), broadcaster отправляет CompactBlock с header + siphash short IDs (8 bytes × 1000 = 8 KB вместо 1 MB); receiver восстанавливает из mempool, для недостающих txs запрашивает GET_BLOCK_TXN. Без S2-P14 — bandwidth на 100 узлах неприемлемо для production (10 MB на каждый block × 100 peers = 1 GB).

**Задачи:**

1. Добавить `siphasher = "x.y"` (или `siphash24` crate) в `crates/strangecoin-net/Cargo.toml [dependencies]`.
2. Создать `crates/strangecoin-net/src/gossip/compact_blocks.rs`:
   - `short_id(txid: &[u8; 32], block_nonce: u64) -> [u8; 8]` — siphash-2-4 of txid XOR block_nonce, first 8 bytes
   - `pub struct CompactBlock { header: BlockHeader, nonce: u64, short_ids: Vec<[u8; 8]>, prefilled_txs: Vec<(usize, Transaction)> }` — prefilled_txs для coinbase и txs, которые broadcaster knows, что receiver не имеет (e.g., new txs just arrived)
   - `pub struct BlockTxn { block_hash: [u8; 32], txs: Vec<Transaction> }` — response на GET_BLOCK_TXN
   - `pub struct GetBlockTxn { block_hash: [u8; 32], missing_indices: Vec<usize> }` — request missing txs
3. Fill-up algorithm: receiver получает CompactBlock, для каждого short_id ищет tx в mempool с matching short_id (siphash with same nonce); если найдено — добавить в block; если не найдено — добавить index в missing; request GetBlockTxn; responder отвечает BlockTxn; receiver completes block.
4. Short_id collision handling: если 2 txs в mempool дают same short_id (extremely rare, 64-bit ID space) — fallback to full block request.
5. Тесты в `crates/strangecoin-net/tests/compact_blocks.rs`:
   - 100-tx block, all txs in mempool → CompactBlock fills ≥80% from mempool, no GetBlockTxn needed
   - 100-tx block, 20 txs not in mempool → GetBlockTxn round-trip completes block
   - Short_id collision (2 txs with same siphash) → fallback to full block (no false block reconstruction)
   - Bandwidth reduction test: full block 1 MB → CompactBlock ~10 KB (header + 8 bytes × 100 + prefilled 0) → ≥50% reduction measured
6. Запустить `cargo test -p strangecoin-net --test compact_blocks` — green.

**Артефакты:**
- `crates/strangecoin-net/Cargo.toml` (siphash dep)
- `crates/strangecoin-net/src/gossip/compact_blocks.rs`
- `crates/strangecoin-net/tests/compact_blocks.rs`

**КГ (чек-лист):**
- [ ] `short_id` siphash-2-4 (8 bytes, txid XOR block_nonce)
- [ ] `CompactBlock` struct (header, nonce, short_ids, prefilled_txs)
- [ ] Fill-up from mempool algorithm
- [ ] `GetBlockTxn`/`BlockTxn` round-trip для missing txs
- [ ] Short_id collision → fallback to full block
- [ ] Bandwidth reduction ≥50% measured in test (1 MB block → ~10 KB CompactBlock)
- [ ] `cargo test -p strangecoin-net --test compact_blocks` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P14] net/gossip: BIP 152 compact blocks (siphash short IDs + fill-up)`

---

### S2-P15. Erlay reconciliation (mincut protocol + fallback)

**Цель:** реализовать Erlay reconciliation-based relay в `crates/strangecoin-net/src/gossip/erlay/{mod,reconcile,mincut,fallback}.rs`: peers поддерживают sketches known-tx-sets, периодически reconcile (every N sec или after M new txs), передают только delta; sketch mismatch → fallback на full relay (per ADR-0021). Цель: ≥80% bandwidth reduction на 10k+ узлах vs naive broadcast.

**Контекст:** ARCHITECT3 §3.6: «Erlay (Stage 2)». ADR-0021 (S2-P02) зафиксировал Erlay mincut + fallback. ROADMAP3 Этап 2 risk: «Erlay reconciliation может приводить к потере txs» → mitigation: fallback на full relay при sketch mismatch. Erlay paper (BIP-330 draft): peers поддерживают minisketch sketches (PinSketch code-based set reconciliation), periodically exchange sketches, decode set difference, transmit only delta. Без S2-P15 — Stage 2 production-grade P2P на 10k+ узлах невозможен (bandwidth O(N) на каждый tx недопустимо).

**Задачи:**

1. Добавить `minisketch-rs = "x.y"` (Rust port) или `minisketch = "x.y"` (FFI to C lib) в `crates/strangecoin-net/Cargo.toml [dependencies]`.
2. Создать `crates/strangecoin-net/src/gossip/erlay/mod.rs` — pub mod declarations.
3. Создать `crates/strangecoin-net/src/gossip/erlay/reconcile.rs`:
   - `pub struct ReconciliationState { known_set: HashSet<TxId>, last_reconciled: Instant, peer_state: HashMap<NodeId, PeerReconciliation> }`
   - `pub struct PeerReconciliation { last_set_size: usize, last_sketch: Vec<u8>, reconciliation_interval: Duration }`
   - `pub fn reconcile(&mut self, peer: &NodeId, peer_sketch: &[u8]) -> Result<Vec<TxId>, NetError>` — decode set difference, return delta
4. Создать `crates/strangecoin-net/src/gossip/erlay/mincut.rs`:
   - `pub fn create_sketch(set: &HashSet<TxId>, capacity: usize) -> Vec<u8>` — minisketch encode
   - `pub fn decode_difference(local_sketch: &[u8], remote_sketch: &[u8]) -> Result<Vec<TxId>, NetError>` — minisketch decode
5. Создать `crates/strangecoin-net/src/gossip/erlay/fallback.rs`:
   - `pub fn fallback_full_relay(missing_txs: Vec<TxId>, peer_manager: &PeerManager, mempool: &Mempool)` — если reconciliation failed (sketch decode error), запросить missing txs через INV/GET_DATA (S2-P13)
6. Reconciliation protocol: peer pairs reconcile every N seconds (default 5 sec) или after M new txs (default 5000); sketch sizes adaptive to set difference estimate (start with capacity 100, increase if decode fails).
7. Тесты в `crates/strangecoin-net/tests/erlay.rs`:
   - 2 peers with 1000 txs each, 800 shared → reconciliation transmits only 200 unknown (≥80% reduction vs full INV broadcast)
   - Reconciliation failure (mismatched sketches, decode error) → fallback to full relay works (no tx loss)
   - 10k-peer simulation (mocked peer count) shows ≥80% bandwidth reduction vs naive broadcast (compute bandwidth math, not full network sim)
   - Adaptive sketch capacity: if decode fails 2× in a row → increase capacity (from 100 to 200, etc.)
8. Запустить `cargo test -p strangecoin-net --test erlay` — green.

**Артефакты:**
- `crates/strangecoin-net/Cargo.toml` (minisketch dep)
- `crates/strangecoin-net/src/gossip/erlay/{mod,reconcile,mincut,fallback}.rs`
- `crates/strangecoin-net/tests/erlay.rs`

**КГ (чек-лист):**
- [ ] `minisketch-rs` (or FFI) pinned в Cargo.toml
- [ ] `ReconciliationState` + `PeerReconciliation` per-peer state
- [ ] `create_sketch` + `decode_difference` minisketch wrappers
- [ ] Reconciliation interval (5 sec / 5000 new txs — defaults)
- [ ] Fallback to full relay on sketch mismatch (no tx loss)
- [ ] Adaptive sketch capacity (increase on decode failure)
- [ ] Test: 2 peers 1000 txs, 800 shared → only 200 transmitted (≥80% reduction)
- [ ] Test: reconciliation failure → fallback to full relay works
- [ ] Test: 10k-peer bandwidth math shows ≥80% reduction
- [ ] `cargo test -p strangecoin-net --test erlay` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P15] net/gossip: Erlay reconciliation (mincut + fallback)`

---

### 2.6. Rate limiting + peer scoring (S2-P16–S2-P17)

S2-P16..S2-P17 реализуют protection subsystem: S2-P16 — rate limiting (token bucket per peer, per-message-type caps, ban on exceed) per ADR-0023, extends Stage 1 rate_limiter; S2-P17 — peer scoring algorithm (weighted sum of factors, ban state machine) per ADR-0022, integrates with peer_manager (S2-P12). После S2-P17 network защищена от DoS (V-15), sybil (V-17), spam (V-14).

---

### S2-P16. Rate limiting (token bucket per peer, ban on exceed)

**Цель:** реализовать token bucket rate limiter per peer в `crates/strangecoin-net/src/rate_limiter.rs` (rewrite после S2-P06 миграции): main bucket (capacity 1000, refill 100/sec) + per-message-type sub-buckets (INV 100/10s, BLOCK 30/10s, TX 1000/10s — extend Stage 1); ban on exceed threshold (config.toml: `rate_limit_ban_threshold = 5` violations in 60s); integration с peer scoring через `peer.violation_count += 1` на каждое violation.

**Контекст:** ADR-0023 (S2-P04) зафиксировал token bucket + per-message-type caps + ban threshold. ARCHITECT3 §3.6: «`rate_limiter.rs`: токены на сообщения/сек от одного пира». Инвариант #17: «Rate limiting: лимит сообщений/сек от одного пира; нарушение → бан». Текущее состояние (Stage 1.5 после S2-P06): Stage 1 rate_limiter без token bucket (per-message-type sliding window), без integration с peer scoring. Этот промпт — full rewrite per ADR-0023.

**Задачи:**

1. Rewrite `crates/strangecoin-net/src/rate_limiter.rs`:
   - `pub struct TokenBucket { capacity: u32, tokens: u32, refill_rate: u32, last_refill: Instant }`
   - `pub fn try_consume(&mut self, count: u32) -> bool` — refill based on elapsed time, then consume if available
   - `pub struct PeerRateLimiter { main_bucket: TokenBucket, per_type_buckets: HashMap<MessageType, TokenBucket>, violation_count: u32, last_violation: Instant, ban_threshold: u32, ban_window: Duration }`
   - `pub enum MessageType { Inv, Block, Tx, Hello, GetData, CompactBlock, GetBlockTxn, BlockTxn, ErlayReconcile, Addr }`
   - `pub fn check_rate(&mut self, msg_type: MessageType) -> Result<(), RateLimitError>` — check main bucket + per-type bucket; if either exhausted → `Err(RateLimitError::Exceeded)`; on Err increment violation_count; if violation_count >= ban_threshold → return `Err(RateLimitError::Ban)`
2. `config.toml` extension: `rate_limit_ban_threshold = 5`, `rate_limit_ban_window_sec = 60`, `rate_limit_main_capacity = 1000`, `rate_limit_main_refill = 100`, `rate_limit_per_type = { inv = "100/10s", block = "30/10s", tx = "1000/10s" }` (parses "X/Ys" format).
3. Integration с peer_manager (S2-P12): на `Err(RateLimitError::Exceeded)` — `peer_manager.record_rate_violation(node_id)` → `peer.violation_count += 1`; на `Err(RateLimitError::Ban)` — `peer_manager.ban_peer(node_id, BanReason::RateLimit, temp_ban)`.
4. Тесты в `crates/strangecoin-net/tests/rate_limit.rs`:
   - Spammer sending 1000 INV/s → violation_count increments → banned after threshold (5 violations / 60s)
   - Legitimate peer under 100 INV/10s → never banned (tokens always available)
   - Burst support: 200 INV in 1s (within main bucket capacity 1000) → not exceeded; 1500 INV in 1s → exceeded (capacity exhausted)
   - Per-message-type caps: 30 BLOCK/10s OK, 31 BLOCK/10s → Exceeded
   - Ban state persists in network.json (via peer_manager S2-P12)
5. Запустить `cargo test -p strangecoin-net --test rate_limit` — green.

**Артефакты:**
- `crates/strangecoin-net/src/rate_limiter.rs` (rewritten)
- `config.toml` (rate_limit_* extension)
- `crates/strangecoin-net/tests/rate_limit.rs`

**КГ (чек-лист):**
- [ ] `TokenBucket` struct с `try_consume` + refill based on elapsed time
- [ ] `PeerRateLimiter` с main bucket + per-type sub-buckets
- [ ] `MessageType` enum (10 variants)
- [ ] `check_rate` returns Exceeded/Ban errors
- [ ] `config.toml` rate_limit_* parameters (tunable)
- [ ] Integration с peer_manager: rate violation → violation_count++; ban → peer_manager.ban_peer
- [ ] Test: spammer banned after threshold (5 violations / 60s)
- [ ] Test: legitimate peer never banned
- [ ] Test: burst support (200 INV/1s OK, 1500 INV/1s exceeded)
- [ ] Test: per-message-type caps enforced
- [ ] Test: ban state persists in network.json
- [ ] `cargo test -p strangecoin-net --test rate_limit` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P16] net/rate_limit: token bucket + per-message caps + ban threshold`

---

### S2-P17. Peer scoring algorithm (factors + thresholds + ban expiry)

**Цель:** реализовать peer scoring algorithm в `crates/strangecoin-net/src/peer/scoring.rs` per ADR-0022: 4 scoring factors (latency EWMA, invalid_blocks_txs_sent, protocol_violations, uptime percentage), weighted sum formula `score = w_latency * (1 - latency_norm) + w_invalid * invalid_count + w_violation * violation_count + w_uptime * uptime_norm`, ban state machine (warn at score < 0.4, temp ban at < 0.2, persistent ban at < 0.05), ban expiry (temp ban 1h, persistent ban 30d). Integration с peer_manager (S2-P12) и rate_limiter (S2-P16).

**Контекст:** ADR-0022 (S2-P03) зафиксировал weighted scoring + thresholds + ban state machine. ARCHITECT3 §6 V-15 (invalid blocks DoS), V-17 (sybil) требуют scoring. ROADMAP3 Этап 2 DoD: «Peer scoring: malicious peers banned after N violations». Текущее состояние (после S2-P12): skeleton PeerRecord с score field, без actual scoring algorithm. Этот промпт — full impl per ADR-0022.

**Задачи:**

1. Создать `crates/strangecoin-net/src/peer/scoring.rs`:
   - `pub struct ScoringConfig { w_latency: f32, w_invalid: f32, w_violation: f32, w_uptime: f32, warn_threshold: f32, temp_ban_threshold: f32, persistent_ban_threshold: f32, temp_ban_duration: Duration, persistent_ban_duration: Duration }`
   - Default values from ADR-0022: `w_latency=0.2, w_invalid=0.5, w_violation=0.2, w_uptime=0.1, warn_threshold=0.4, temp_ban_threshold=0.2, persistent_ban_threshold=0.05, temp_ban_duration=1h, persistent_ban_duration=30d`
   - `pub fn compute_score(record: &PeerRecord, config: &ScoringConfig) -> f32` — apply weighted formula
   - `pub fn update_score(peer: &mut PeerRecord, config: &ScoringConfig) -> BanDecision` — compute score, return `BanDecision { action: NoAction | Warn | TempBan { until: Instant } | PersistentBan { until: Instant } }`
2. Latency EWMA: `peer.latency_ewma = 0.9 * peer.latency_ewma + 0.1 * last_latency` (на каждое message)
3. Uptime tracking: `peer.uptime_seconds += elapsed_since_last_seen` (на каждое successful message); `uptime_norm = peer.uptime_seconds as f32 / MAX_UPTIME_SEC` (MAX = 30 days)
4. Integration с peer_manager (S2-P12): `PeerManager::tick()` (periodic, every 60 sec) calls `update_score` для всех peers; if BanDecision = TempBan/PersistentBan → `ban_peer` with computed until; if Warn → log warning + reduce broadcast priority (peer still connected)
5. Integration с rate_limiter (S2-P16): rate violation → `peer.violation_count += 1` (через peer_manager API)
6. Ban expiry: `PeerManager::tick()` checks `ban_state.until < now` → unban (move to historical_bans table, not delete, for audit)
7. `config.toml` extension: `peer_scoring_w_latency = 0.2`, etc. (все weights + thresholds tunable)
8. Тесты в `crates/strangecoin-net/tests/peer_scoring.rs`:
   - Peer с high latency (200ms) but valid messages → score stays high → not banned (false-positive mitigation)
   - Peer sending invalid blocks (invalid_count=10) → score drops below temp_ban_threshold → temp banned 1h
   - Peer with protocol_violations=20 → score drops below persistent_ban_threshold → persistent banned 30d
   - Warn state recovers (uptime grows, no new violations) → score increases → ban lifted after expiry
   - Persistent ban survives restart (load from network.json)
9. Запустить `cargo test -p strangecoin-net --test peer_scoring` — green.

**Артефакты:**
- `crates/strangecoin-net/src/peer/scoring.rs`
- `crates/strangecoin-net/src/peer/manager.rs` (updated: tick() calls update_score)
- `config.toml` (peer_scoring_* extension)
- `crates/strangecoin-net/tests/peer_scoring.rs`

**КГ (чек-лист):**
- [ ] `ScoringConfig` with all weights + thresholds (defaults from ADR-0022)
- [ ] `compute_score` weighted formula implementation
- [ ] Latency EWMA update on each message (0.9/0.1 alpha)
- [ ] Uptime tracking + normalization
- [ ] Ban state machine (NoAction/Warn/TempBan/PersistentBan)
- [ ] `PeerManager::tick()` calls update_score every 60s
- [ ] Ban expiry respected (temp 1h, persistent 30d)
- [ ] Integration с rate_limiter (rate violation → violation_count++)
- [ ] Persistent ban survives restart
- [ ] Test: false-positive (high latency but valid) → not banned
- [ ] Test: invalid blocks → temp ban
- [ ] Test: protocol violations → persistent ban
- [ ] Test: warn state recovers (uptime grows)
- [ ] `cargo test -p strangecoin-net --test peer_scoring` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P17] net/peer: scoring algorithm (factors + thresholds + ban expiry)`

---

### 2.7. Cleanup + async migration (S2-P18–S2-P19)

S2-P18 удаляет deprecated v0.8.6 messages (GET_BLOCKCHAIN, BLOCKCHAIN) из protocol.rs (ROADMAP3 Этап 2 task). S2-P19 завершает async migration (ADR-0011): заменяет remaining `std::thread::spawn` в network workers на `tokio::spawn`. После S2-P19 весь network stack на tokio, legacy v0.8.6 network несовместим.

---

### S2-P18. Deprecated messages cleanup (GET_BLOCKCHAIN/BLOCKCHAIN removed)

**Цель:** удалить deprecated v0.8.6 messages GET_BLOCKCHAIN и BLOCKCHAIN из `crates/strangecoin-net/src/protocol.rs` (Message enum + handlers). ROADMAP3 Этап 2 DoD: «Legacy messages удалены (no backward compatibility with v0.8.6 network)». Текущее состояние (после S2-P06 миграции): GET_BLOCKCHAIN/BLOCKCHAIN всё ещё в Message enum, но не используются (Stage 1 sync_engine использует GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS).

**Контекст:** ARCHITECT3 §3.6: «Network messages — backwards compatible через `format_version`. Deprecated сообщения (`GET_BLOCKCHAIN`, `BLOCKCHAIN` из v0.8.6) удаляются на Stage 2». ROADMAP3 Этап 2 task: «Deprecated messages cleanup: Удалить `GET_BLOCKCHAIN`, `BLOCKCHAIN` (legacy v0.8.6)». Удаление — breaking change для v0.8.6 network, но Stage 1.5 не запущен на mainnet (gate BUG-S1-013/015 ops), regtest/devnet не используют эти messages. Прямая правка допустима до тега v2.0.0-stage2.

**Задачи:**

1. В `crates/strangecoin-net/src/protocol.rs` (Message enum):
   - Удалить variants `GetBlockchain` и `Blockchain`
   - Удалить wire-format encoding/decoding для этих variants
2. Удалить handlers в `crates/strangecoin-net/src/sync.rs` или `sync_engine.rs` (если были)
3. Добавить `UnknownMessage` variant в Message enum для forward-compat (если получено unknown message type → log + return, не crash) — не путать с legacy GET_BLOCKCHAIN, просто unknown future message type
4. Тесты в `crates/strangecoin-net/tests/protocol_deprecated.rs`:
   - Sending GET_BLOCKCHAIN encoded bytes (with old message type byte) → `decode_message` returns `Ok(Message::UnknownMessage)` (или `Err(NetError::UnknownType)` — выбрать, что cleaner)
   - No code path references GetBlockchain/Blockchain variants (compile-time: removed from enum)
   - No regression in valid messages (HELLO, INV, GET_DATA, TX, BLOCK, etc. all still decode correctly)
5. Запустить `cargo test --workspace` — green.

**Артефакты:**
- `crates/strangecoin-net/src/protocol.rs` (GetBlockchain/Blockchain removed, UnknownMessage added)
- `crates/strangecoin-net/src/sync.rs` / `sync_engine.rs` (handlers removed)
- `crates/strangecoin-net/tests/protocol_deprecated.rs`

**КГ (чек-лист):**
- [ ] `GetBlockchain` и `Blockchain` variants removed from Message enum
- [ ] Handlers removed (grep `GetBlockchain` / `Blockchain` → 0 matches outside test files)
- [ ] `UnknownMessage` variant added (forward-compat)
- [ ] Test: old GET_BLOCKCHAIN encoded bytes → UnknownMessage (или UnknownType)
- [ ] No regression in valid messages (existing tests/sync_engine.rs, tests/network.rs, tests/sync_headers.rs green)
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P18] net/protocol: remove deprecated GET_BLOCKCHAIN/BLOCKCHAIN (v0.8.6 legacy)`

---

### S2-P19. Async migration network workers (std::thread → tokio::spawn, ADR-0011 continuation)

**Цель:** завершить async migration network workers (ADR-0011): заменить remaining `std::thread::spawn` в network code на `tokio::spawn`; обновить tracing integration для async context. ARCHITECT3 §3.6: «p2p.rs: сокеты (tokio с Stage 1)». ADR-0011 (Stage 1) ввёл tokio, но в Stage 1.5 network workers всё ещё на std::thread в нескольких местах.

**Контекст:** ADR-0011 (async migration, Stage 1, commit `274b6b9`) ввёл tokio для blockchain/event-bus, но не все network workers были мигрированы. ARCHITECT3 §3.6: «`p2p.rs`: сокеты (tokio с Stage 1)». ROADMAP3 Этап 2 implicit task: «Вводится tokio (вместо threads)» (ARCHITECT3 §10.4). Текущее состояние: после S2-P06 миграции в strangecoin-net, несколько мест в `crates/strangecoin-net/src/mod.rs` / `sync_engine.rs` всё ещё используют `std::thread::spawn`. Этот промпт — finish migration.

**Задачи:**

1. `grep -rn "std::thread::spawn" crates/strangecoin-net/src/` → найти все использования
2. Заменить каждое на `tokio::spawn(async move { ... })` с async block
3. Обновить blocking I/O на `tokio::net::TcpStream` (async read/write) / `tokio::io::AsyncReadExt`/`AsyncWriteExt`
4. Заменить `std::sync::mpsc` channels на `tokio::sync::mpsc` (если используются в network code)
5. Обновить tracing integration: `tracing::Instrument::instrument()` для async spans
6. Удалить `std::thread::sleep` в network code → `tokio::time::sleep`
7. Не трогать `std::thread` в CLI signal handlers (ctrlc handler может требовать sync thread) — только network code
8. Тесты в `crates/strangecoin-net/tests/async_migration.rs`:
   - `grep -rn "std::thread::spawn" crates/strangecoin-net/src/` → 0 matches (excluding tests)
   - existing tests/two_clients.rs, tests/sync_engine.rs, tests/network.rs green (no behavioral change)
   - tokio runtime properly shut down (no hung threads after test)
9. Запустить `cargo test --workspace` — green.

**Артефакты:**
- `crates/strangecoin-net/src/mod.rs` (thread::spawn → tokio::spawn)
- `crates/strangecoin-net/src/sync.rs` / `sync_engine.rs` (thread::spawn → tokio::spawn; std::sync::mpsc → tokio::sync::mpsc)
- `crates/strangecoin-net/src/peer/manager.rs` (если использовал std::thread::spawn)
- `crates/strangecoin-net/tests/async_migration.rs`

**КГ (чек-лист):**
- [ ] `grep -rn "std::thread::spawn" crates/strangecoin-net/src/` → 0 matches (excluding tests)
- [ ] `std::thread::sleep` → `tokio::time::sleep` в network code
- [ ] `std::sync::mpsc` → `tokio::sync::mpsc` в network code
- [ ] blocking I/O → async (tokio::net::TcpStream)
- [ ] tracing integration с async spans
- [ ] tokio runtime properly shut down (no hung threads)
- [ ] Existing tests green (no behavioral change)
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P19] net: async migration — std::thread → tokio::spawn (ADR-0011 continuation)`

---

### 2.8. Shadow network (Stage 6 preparation, optional but recommended)

S2-P20 реализует shadow network tooling (ROADMAP3 Этап 2 task: «Shadow network (Stage 6 подготовка): Tool для replay mainnet-блоков на staging; Тестирование апгрейдов перед deployment»). Это опциональный промпт, но рекомендуется — он готовит Stage 6 (security track) и упрощает regression testing в будущем.

---

### S2-P20. Shadow network tooling (replay mainnet blocks on staging)

**Цель:** реализовать shadow network tooling в `crates/strangecoin-net/src/shadow/{mod,replay,compare}.rs`: CLI subcommand `sc shadow replay <mainnet-blocks-file>` подключается к staging network (regtest/devnet) и replays mainnet blocks. Detects consensus divergence (block rejected on staging but accepted on mainnet). Цель: подготовить Stage 6 (external audit, formal verification) — replay tooling для testing upgrades перед deployment.

**Контекст:** ROADMAP3 Этап 2 task: «Shadow network (Stage 6 подготовка): Tool для replay mainnet-блоков на staging. Тестирование апгрейдов перед deployment.» ARCHITECT3 §6 (no explicit reference, но Stage 6 security track implies need for upgrade testing). Текущее состояние (Stage 1.5): нет tooling для consensus divergence detection. Этот промпт — first step к Stage 6.

**Задачи:**

1. Создать `crates/strangecoin-net/src/shadow/mod.rs` — pub mod declarations.
2. Создать `crates/strangecoin-net/src/shadow/replay.rs`:
   - `pub struct ReplayConfig { staging_node_url: String, mainnet_blocks_file: PathBuf, stop_on_divergence: bool, max_blocks: usize }`
   - `pub async fn replay_blocks(config: &ReplayConfig, blockchain_view: &dyn BlockchainView) -> Result<ReplayReport, NetError>` — load mainnet blocks, submit to staging node, capture accept/reject for each
3. Создать `crates/strangecoin-net/src/shadow/compare.rs`:
   - `pub struct DivergenceReport { block_height: u64, mainnet_status: BlockStatus, staging_status: BlockStatus, divergence_reason: Option<String> }`
   - `pub fn compare(mainnet_results: &[BlockResult], staging_results: &[BlockResult]) -> Vec<DivergenceReport>` — find blocks with different status
4. CLI subcommand в `src/cli/mod.rs`: `sc shadow replay --staging-url=<url> --blocks-file=<path> --max-blocks=<N>` — использует ReplayConfig
5. Тесты в `crates/strangecoin-net/tests/shadow.rs` (simulated):
   - Replay of known-valid mainnet block on regtest → accepted (no divergence)
   - Replay of known-invalid (tampered) block → rejected with same reason as mainnet (no divergence)
   - Consensus divergence detector emits warning (block rejected on staging but accepted on mainnet — simulated with mock data)
6. Запустить `cargo test -p strangecoin-net --test shadow` — green.

**Артефакты:**
- `crates/strangecoin-net/src/shadow/{mod,replay,compare}.rs`
- `src/cli/mod.rs` (shadow subcommand)
- `crates/strangecoin-net/tests/shadow.rs`

**КГ (чек-лист):**
- [ ] `ReplayConfig` + `ReplayReport` structs
- [ ] `DivergenceReport` + `compare` function
- [ ] CLI subcommand `sc shadow replay` works
- [ ] Test: known-valid block → accepted on staging (no divergence)
- [ ] Test: known-invalid block → rejected with same reason (no divergence)
- [ ] Test: divergence detector emits warning on simulated mismatch
- [ ] `cargo test -p strangecoin-net --test shadow` green
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P20] net/shadow: replay tooling + consensus divergence detector (Stage 6 prep)`

---

### 2.9. Verification + закрытие Stage 2 (S2-P21–S2-P24)

S2-P21..S2-P24 — verification-трек: S2-P21 добавляет fuzzing harness (4 new network targets); S2-P22 добавляет integration tests (6 new test files covering noise/gossip/compact_blocks/erlay/scoring/rate_limit); S2-P23 обновляет invariants + threat model (V-1/V-2/V-15/V-17/V-24 mitigations); S2-P24 — DoD-верификация + тег `v2.0.0-stage2` (Gate для Stage 3/4). После S2-P24 Stage 2 закрыт.

---

### S2-P21. Fuzzing harness — 4 new network targets

**Цель:** добавить 4 new fuzz targets для network code: `noise_handshake` (random handshake messages → no panic + deterministic state machine); `gossip_inv` (random INV sequences → no panic + mempool consistency); `compact_blocks_fill_up` (random mempool + CompactBlock → fill-up succeeds or fails cleanly); `erlay_reconciliation` (random peer sets → reconciliation terminates + bandwidth reduction verified). Extend CI fuzz job (5 existing + 4 new = 9 targets × 600s on Linux runner).

**Контекст:** ARCHITECT3 §7 (Security track): «Stage 1+: Fuzzing (`cargo-fuzz`)». ROADMAP3 Этап 2 implicit: production-grade P2P requires fuzzing. Текущее состояние (Stage 1.5): 5 fuzz targets (canonical_decode, host_functions, gas_metering, storage_trie, differential_wasmi_vs_wasmtime). Этот промпт добавляет 4 network-specific targets.

**Задачи:**

1. Создать `fuzz/fuzz_targets/noise_handshake.rs`:
   - Random handshake bytes → `NoiseHandshake::initiator/responder` + `read_message`
   - Assert: no panic; deterministic output for same input (initial state)
2. Создать `fuzz/fuzz_targets/gossip_inv.rs`:
   - Random INV sequences (random txid/block_hash + InvKind) → `handle_inv` with mock mempool + mock blockchain
   - Assert: no panic; mempool consistency (no false positive TxId announces)
3. Создать `fuzz/fuzz_targets/compact_blocks_fill_up.rs`:
   - Random mempool state + random CompactBlock (header, nonce, short_ids, prefilled) → fill-up algorithm
   - Assert: no panic; if fill-up succeeds, block is valid (matches header); if fails, GetBlockTxn returned
4. Создать `fuzz/fuzz_targets/erlay_reconciliation.rs`:
   - Random peer sets (random TxId sets with varying overlap) → `create_sketch` + `decode_difference`
   - Assert: no panic; reconciliation terminates (no infinite loop); bandwidth reduction ≥50% vs naive broadcast (compute math)
5. Update `fuzz/Cargo.toml`: add 4 new [[bin]] targets
6. Update `.github/workflows/ci.yml`: extend `fuzz-canonical-decode` job — add 4 new targets, each 600s on Linux runner (matrix: ubuntu-latest, cargo-fuzz 0.13.2)
7. Update `fuzz/README.md`: document new targets + strategy
8. Run 10-min local soak for each new target — 0 panics

**Артефакты:**
- `fuzz/fuzz_targets/{noise_handshake,gossip_inv,compact_blocks_fill_up,erlay_reconciliation}.rs`
- `fuzz/Cargo.toml` (4 new targets)
- `.github/workflows/ci.yml` (extended fuzz job — 9 targets × 600s)
- `fuzz/README.md` (updated strategy + coverage)

**КГ (чек-лист):**
- [ ] 4 fuzz-targets созданы (noise_handshake, gossip_inv, compact_blocks_fill_up, erlay_reconciliation)
- [ ] CI `fuzz-canonical-decode` job extended (9 targets × 600s on Linux)
- [ ] 10-min local soak для каждого new target — 0 panics (если cargo-fuzz доступен на хосте; если нет — note "not verified + reason", прецедент FBP08)
- [ ] `fuzz/README.md` документирует стратегию + coverage + результаты
- [ ] `cargo test --workspace` green (no regressions)
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P21] fuzz: 4 new network targets (noise/gossip/compact_blocks/erlay)`

---

### S2-P22. Integration tests — Stage 2 end-to-end

**Цель:** integration tests для Stage 2 в 6 new test files + `tests/common/net_helpers.rs` (simulated peers, deterministic clock): noise handshake end-to-end (S2-P07..S2-P09), gossip propagation (S2-P13, 1 node → 100 peers ≤1s), compact block fill-up (S2-P14, ≥50% bandwidth reduction), Erlay reconciliation (S2-P15, ≥80% bandwidth reduction), peer scoring (S2-P12+S2-P17, malicious banned), rate limiting (S2-P16, spammer banned). All tests на regtest (chain_id=3), temp dirs, no `sleep` > 1 sec.

**Контекст:** ARCHITECT3 §17 PR-checklist: «Tests: unit + property (proptest) + integration (two_clients, reorg, double_spend)». ROADMAP3 Этап 2 DoD включает 6 критериев, которые требуют integration tests. Текущее состояние (Stage 1.5): tests/two_clients.rs, tests/sync_engine.rs, tests/network.rs, tests/rate_limit.rs (Stage 1), tests/sync_headers.rs. Этот промпт добавляет Stage 2-specific integration tests.

**Задачи:**

1. Создать `tests/common/net_helpers.rs`:
   - `SimulatedNetwork` — in-memory network с N simulated peers, deterministic clock (no real time)
   - `MockPeer { node_id, keypair, addr, capabilities }`
   - `simulate_gossip_propagation(tx_origin: NodeId, network: &SimulatedNetwork) -> Vec<(NodeId, Instant)>` — returns per-peer arrival time
2. Создать `tests/net_noise_handshake.rs`:
   - 2 узла выполняют Noise handshake end-to-end (initiator + responder + transport)
   - MITM attacker (own Noise keys) → connection dropped
   - New peer (None expected_node_id) → accepted, marked «unverified»
3. Создать `tests/net_gossip.rs`:
   - 1 node creates tx → propagates to 100 simulated peers in ≤1 sec (simulated clock, no real network)
   - INV deduplication: peer не запрашивает tx, который уже имеет
4. Создать `tests/net_compact_blocks.rs`:
   - 100-tx block, all txs in mempool → CompactBlock fills ≥80% from mempool, ≥50% bandwidth reduction
   - 100-tx block, 20 missing → GetBlockTxn round-trip completes
5. Создать `tests/net_erlay.rs`:
   - 2 peers with 1000 txs each, 800 shared → reconciliation transmits only 200 (≥80% reduction)
   - Reconciliation failure → fallback to full relay (no tx loss)
6. Создать `tests/net_peer_scoring.rs`:
   - Malicious peer sending invalid blocks → banned after N violations (threshold from config.toml)
   - False-positive path (high latency but valid) → not banned
   - Persistent ban survives restart (load from network.json)
7. Создать `tests/net_rate_limit_integration.rs`:
   - Spammer sending 1000 INV/s → banned after threshold (5 violations / 60s)
   - Legitimate peer under 100 INV/10s → never banned
8. Запустить `cargo test --workspace` — green; < 240 sec total.

**Артефакты:**
- `tests/common/net_helpers.rs`
- `tests/{net_noise_handshake,net_gossip,net_compact_blocks,net_erlay,net_peer_scoring,net_rate_limit_integration}.rs`

**КГ (чек-лист):**
- [ ] 6 integration test files созданы, green
- [ ] `tests/common/net_helpers.rs` — SimulatedNetwork + MockPeer + helpers
- [ ] `net_noise_handshake.rs` — handshake + MITM + unverified peer
- [ ] `net_gossip.rs` — 100 peers ≤1s propagation + deduplication
- [ ] `net_compact_blocks.rs` — ≥50% bandwidth reduction measured
- [ ] `net_erlay.rs` — ≥80% bandwidth reduction (1000 txs, 200 transmitted)
- [ ] `net_peer_scoring.rs` — malicious banned, false-positive not banned, persistent survives restart
- [ ] `net_rate_limit_integration.rs` — spammer banned, legitimate not banned
- [ ] Все тесты на regtest (chain_id=3), temp dirs, без `sleep` > 1 сек
- [ ] `cargo test --workspace` green, < 240 сек
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Коммит `[S2-P22] tests: net integration (noise/gossip/compact_blocks/erlay/scoring/rate_limit)`

---

### S2-P23. Регрессия 22 инвариантов + threat model актуализация

**Цель:** обновить `docs/security/THREAT_MODEL.md` для Stage 2: V-24 (Network MITM) now mitigated by Noise (S2-P07/S2-P08/S2-P09); V-1 (Eclipse) improved via peer discovery + ADDR gossip (S2-P11); V-2 (Partition) improved via sync_engine ≥3 peers (already Stage 1); V-15 (invalid blocks DoS) improved via peer scoring (S2-P17); V-17 (Sybil) improved via peer scoring + connection limit (S2-P12); V-14 (spam txs) improved via rate limiting (S2-P16). Создать `docs/stage2/INVARIANTS_ENFORCED.md` (22/22 still enforced + extensions для #16 Noise encryption layer, #17 rate limiting + peer scoring integration). Создать `docs/stage2/THREAT_MODEL_STAGE2.md`. Residual risks для Stage 3+: snapshot sync, RocksDB migration, Block-STM.

**Контекст:** ARCHITECT3 §5 (22 invariants), §6 (STRIDE threat model). Stage 1.5 closed with 22/22 enforced, V-18..V-21 mitigated. Stage 2 добавляет new mitigation для network-layer threats (V-1, V-14, V-15, V-17, V-24). ROADMAP3 «Сквозные треки → Security track»: «Threat model (STRIDE) maintained» per Stage.

**Задачи:**

1. Создать `docs/stage2/INVARIANTS_ENFORCED.md`:
   - Таблица 1:1 ARCHITECT3 §5, столбцы «Stage 0 enforced / Stage 1 enforced / Stage 1.5 enforced / Stage 2 enforced»
   - #16 (P2P framing) — extension: Noise encrypted framing (S2-P08)
   - #17 (Rate limiting) — extension: integrated with peer scoring (S2-P16/S2-P17)
   - Остальные 20 — без регрессии (regression green from S2-P22 integration tests)
   - Summary: 22/22 enforced, 2 extensions, 1 still deferred (#20 fee invariant — Stage 5)
2. Обновить `docs/security/THREAT_MODEL.md`:
   - V-1 Eclipse — mitigation: peer discovery (S2-P11) + peer scoring (S2-P17) + connection limit per IP range (S2-P12) — test: `tests/net_peer_scoring.rs`
   - V-2 Partition — mitigation: sync_engine ≥3 peers (Stage 1, regression test `tests/sync_engine.rs`)
   - V-14 spam txs — mitigation: token bucket rate limiter (S2-P16) + peer scoring (S2-P17) — test: `tests/net_rate_limit_integration.rs`
   - V-15 invalid blocks DoS — mitigation: peer scoring (S2-P17, invalid_blocks_txs_sent factor) — test: `tests/net_peer_scoring.rs`
   - V-17 Sybil — mitigation: peer scoring + connection_pool per-IP limit (S2-P12) — test: `tests/net_peer_scoring.rs`
   - V-24 Network MITM — mitigation: Noise XX handshake + ChaCha20-Poly1305 transport + Ed25519 node_id auth (S2-P07/S2-P08/S2-P09) — test: `tests/net_noise_handshake.rs`
3. Создать `docs/stage2/THREAT_MODEL_STAGE2.md` — резюме Stage 2-specific threats (network-layer), cross-references to `docs/security/THREAT_MODEL.md`
4. Создать/обновить `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` — residual risks для Stage 3+:
   - Snapshot sync (Stage 3) — new attack surface
   - RocksDB migration (Stage 3) — crash-recovery testing
   - Block-STM parallel execution (Stage 3+) — concurrency bugs
   - Stateless validation witnesses (Stage 3) — witness forgery
5. Regression matrix: запустить `cargo test --workspace` + fuzz 9 targets × 600s — все green, no panics (fuzz CI-side, note "not verified + reason" if local cargo-fuzz unavailable — прецедент FBP08)
6. Запустить `cargo clippy --all-targets -- -D warnings` — green

**Артефакты:**
- `docs/stage2/INVARIANTS_ENFORCED.md` (22/22/22/22, столбцы Stage 0/1/1.5/2)
- `docs/stage2/THREAT_MODEL_STAGE2.md` (new document)
- `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (new document)
- `docs/security/THREAT_MODEL.md` (updated V-1/V-2/V-14/V-15/V-17/V-24 mitigations)

**КГ (чек-лист):**
- [ ] `docs/stage2/INVARIANTS_ENFORCED.md` создан (22/22/22/22, 4-stage columns)
- [ ] Инвариант #16 — extension для Noise encrypted framing (S2-P08)
- [ ] Инвариант #17 — extension для rate limiting + peer scoring integration
- [ ] `THREAT_MODEL.md` — V-1/V-2/V-14/V-15/V-17/V-24 обновлены с mitigation ссылками
- [ ] `THREAT_MODEL_STAGE2.md` создан (Stage 2-specific threats)
- [ ] `RESIDUAL_RISKS_STAGE3_PLUS.md` создан (Stage 3+ residual risks)
- [ ] Regression: `cargo test --workspace` green
- [ ] Regression: `cargo clippy --all-targets -- -D warnings` green
- [ ] Fuzz 9 targets × 600s — 0 panics (CI-side; if local cargo-fuzz unavailable → note "not verified + reason", прецедент FBP08)
- [ ] Коммит `[S2-P23] docs: invariants + threat model update (Stage 2 V-1/V-2/V-14/V-15/V-17/V-24 mitigations)`

---

### S2-P24. DoD-верификация Stage 2 + тег v2.0.0-stage2

**Цель:** независимая сверка DoD из `ROADMAP3.md` Этап 2 (12 DoD критериев). Поставить тег `v2.0.0-stage2` — **Gate Stage 3/4**: после этого промпта Stage 3 (RocksDB) и Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*, faucet) могут стартовать. Stage 4 требует Stage 2 завершённым (network для dev-experience); Stage 3 — мягкая зависимость (может идти параллельно с Stage 4 согласно ROADMAP3).

**Контекст:** ROADMAP3.md Этап 2 DoD — 12 критериев: gossip работает (≤1s на 100 узлах), compact blocks (≥50% bandwidth reduction), Erlay (reconciliation), peer discovery (≥8 peers ≤30s), handshake (incompatible rejected), peer scoring (malicious banned), Noise (MITM detected), rate limiting (spammer banned), legacy messages removed, `crates/strangecoin-net/` создан + сеть перенесена, ADR-0016..0019 (renumbered to ADR-0020..0023, see §2.1), `Changelog.md` 2.0.0. После S2-P23 все артефакты созданы, версия должна быть поднята. Тег `v2.0.0-stage2` — последний шаг.

**Задачи:**

1. Создать `docs/stage2/STAGE2_SUMMARY.md` со структурой:
   - Header: статус, дата, prompt references (S2-P01..S2-P24)
   - 12 DoD критериев из ROADMAP3 Этап 2 (таблица 1:1), каждый с ✅ + evidence (commit hash, test file, file path)
   - Cross-reference to bugfixes-stage1.5.md evaluation (§0.1) — Stage 1.5 closure verified
   - Residual obligations для Stage 3+ (snapshot sync, RocksDB, Block-STM, stateless validation)
   - 2 ops-gate из bugfixes-stage1 (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) — НЕ блокируют Stage 2 (regtest/devnet scope), остаются human ops gates для mainnet freeze
   - Anti-goals Stage 2 (явно отложено): see §7
2. Обновить `Changelog.md`: секция `2.0.0 — production-grade P2P (Noise, gossip, compact blocks, Erlay, peer scoring)`:
   - **Status**: Complete (pending tag S2-P24)
   - **Date**: 2026-MM-DD (when this prompt is executed)
   - **Prompts**: S2-P01..S2-P24
   - Список изменений: ADR-0020..0023, crates/strangecoin-net created, network modules migrated, Noise Protocol Framework (handshake + transport + Ed25519 auth), gossip (INV/GET_DATA), BIP 152 compact blocks, Erlay reconciliation, peer discovery (seed IPs + DNSSEC + ADDR gossip), peer scoring (4-factor weighted, ban state machine), rate limiting (token bucket + per-message caps), deprecated messages cleanup (GET_BLOCKCHAIN/BLOCKCHAIN removed), async migration complete (std::thread → tokio), shadow network tooling, fuzzing harness (4 new network targets), integration tests (6 new files), invariants 22/22/22/22 enforced, threat model V-1/V-2/V-14/V-15/V-17/V-24 mitigated
   - Not in this change: 2 ops-gate (BUG-S1-013, BUG-S1-015) — mainnet freeze gates
3. Обновить `AGENTS.md`:
   - Убрать формулировку «Stage 1.5 complete»
   - Добавить «Stage 2 complete (tag `v2.0.0-stage2` — pending S2-P24)»
   - Обновить раздел "Key docs": добавить `docs/stage2/STAGE2_SUMMARY.md`, `docs/stage2/INVARIANTS_ENFORCED.md`, `docs/stage2/THREAT_MODEL_STAGE2.md`, `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`
   - Обновить "Current Architecture": добавить `crates/strangecoin-net/` с Noise transport, gossip, peer management, compact blocks, Erlay
   - Обновить "Key Constraints": убрать «Stage 2 — network migration in progress» (теперь Stage 2 done)
4. Обновить `Cargo.toml` version: root `1.5.0 → 2.0.0`, `crates/strangecoin-core/Cargo.toml` `1.5.0 → 2.0.0`, `crates/strangecoin-vm-wasm/Cargo.toml` `0.2.0` (unchanged — VM не трогается в Stage 2), `crates/strangecoin-net/Cargo.toml` `0.1.0` (first release, unchanged)
5. Обновить `Cargo.lock` + `fuzz/Cargo.lock` (синхронизация с новыми версиями)
6. Прогнать `cargo test --workspace` — все тесты green (включая Stage 0/1/1.5/2); `cargo clippy --all-targets -- -D warnings` — green
7. Обязательства для Stage 3+ (зафиксировать в STAGE2_SUMMARY.md):
   - Stage 3: RocksDB migration (LevelDB → RocksDB), stateless validation (Verkle proofs), state pruning (EIP-4444 analog), snapshot sync, Block-STM parallel execution (Stage 3+)
   - Stage 4: SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet (требует Stage 2 завершённым)
   - Stage 5: Account Abstraction (ERC-4337), EIP-1559 dynamic base fee, threshold encryption mempool impl, fee market
   - Stage 6: External audit, formal verification (K-framework WASM subset, TLA+ network protocol), ZK-VM research
   - Stage 7: PoS migration (Casper FFG), BLS12-381 dependency, bls_aggregate precompile impl, validator set rotation, slashing
   - 2 ops-gate (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) — остаются human ops gates для mainnet freeze; не код
8. Anti-goals Stage 2 (явно отложено — §7): перенести в summary с подтверждением, что не были реализованы
9. Annotated tag `v2.0.0-stage2` — строго ПОСЛЕ прохождения всех КГ:
   ```bash
   git tag -a v2.0.0-stage2 -m "Stage 2 complete: production-grade P2P (Noise, gossip, compact blocks, Erlay)

   Prompts: S2-P01..S2-P24
   Bugfixes: bugfixes-stage1.5.md verified (11/11 fixed, tag v1.5.0-stage1.5)

   12 DoD criteria — all ✅ (docs/stage2/STAGE2_SUMMARY.md)
   22 invariants — all enforced (docs/stage2/INVARIANTS_ENFORCED.md)
   Threats V-1/V-2/V-14/V-15/V-17/V-24 — mitigated (docs/stage2/THREAT_MODEL_STAGE2.md)

   ADRs: 0020 (Noise), 0021 (Erlay), 0022 (peer scoring), 0023 (rate limiting)
   Note: ADR-0016 occupied by Receipts Root in Block Header (Stage 1.5 FBP07);
   Stage 2 renumbered to 0020..0023

   Fuzz: 9 targets × 600s, 0 panics (CI-side)
   TLA+: 35,207 states PASS + receipts_root_match invariant (unchanged from Stage 1.5)

   Gate for Stage 3 (RocksDB) and Stage 4 (dev-experience: SDK, devnet, indexer,
   JSON-RPC eth_*, faucet).

   Ops-gate (NOT blocking Stage 2, blocks mainnet):
   - BUG-S1-013: PGP-key offline keygen
   - BUG-S1-015: offline genesis key air-gapped keygen (SCIP-0001)
   "
   ```
10. Push тег на origin: `git push origin v2.0.0-stage2`
11. После push — `release.yml` CI job должен сработать (triggers on `push: tags: ['v*']`), собрать matrix артефактов + SHA256 + cosign signature + SLSA L3 provenance. Записать Actions run URL + artifact SHA256s в `docs/security/REPRODUCIBLE_BUILDS.md` (procedure BUG-S1-001).
12. Обновить `docs/stage2/STAGE2_SUMMARY.md` — добавить раздел "Release" с Actions run URL + verify commands (`cosign verify-blob`, `slsa-verifier`).
13. Зафиксировать в Changelog: «Tag v2.0.0-stage2 pushed; release pipeline run <URL>; artifacts verified».

**Артефакты:**
- `docs/stage2/STAGE2_SUMMARY.md` (12 DoD критериев с evidence)
- `docs/stage2/INVARIANTS_ENFORCED.md` (22/22/22/22 — см. S2-P23)
- `docs/stage2/THREAT_MODEL_STAGE2.md` (см. S2-P23)
- `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (см. S2-P23)
- `Changelog.md` (секция 2.0.0)
- `AGENTS.md` (updated)
- `Cargo.toml` (version 2.0.0)
- `crates/strangecoin-core/Cargo.toml` (version 2.0.0)
- `Cargo.lock` + `fuzz/Cargo.lock` (synced)
- Git tag `v2.0.0-stage2` (annotated)
- `docs/security/REPRODUCIBLE_BUILDS.md` (Actions run URL + SHA256s + verify commands)

**КГ (чек-лист):**
- [ ] `docs/stage2/STAGE2_SUMMARY.md` создан, 12 DoD критериев с ✅ + evidence
- [ ] `crates/strangecoin-net/` создан (S2-P05), сеть перенесена (S2-P06)
- [ ] ADR-0020..0023 написаны (S2-P01..S2-P04) — note renumbering from ROADMAP3 plan (ADR-0016..0019)
- [ ] Gossip работает (test: 1 node → 100 peers ≤1s, S2-P22 `net_gossip.rs`)
- [ ] Compact blocks (≥50% bandwidth reduction, S2-P22 `net_compact_blocks.rs`)
- [ ] Erlay (≥80% bandwidth reduction, S2-P22 `net_erlay.rs`)
- [ ] Peer discovery (≥8 peers ≤30s, S2-P22 `net_helpers.rs` sim)
- [ ] Handshake (incompatible rejected, S2-P22 `net_noise_handshake.rs`)
- [ ] Peer scoring (malicious banned after N violations, S2-P22 `net_peer_scoring.rs`)
- [ ] Noise encryption (MITM detected, S2-P22 `net_noise_handshake.rs`)
- [ ] Rate limiting (spammer banned, S2-P22 `net_rate_limit_integration.rs`)
- [ ] Legacy messages removed (S2-P18)
- [ ] `Changelog.md` — секция `2.0.0 — production-grade P2P`
- [ ] `AGENTS.md` — Stage 2 marked complete
- [ ] `Cargo.toml` version = 2.0.0 (root + core); vm-wasm 0.2.0 (unchanged); net 0.1.0 (first release)
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --all-targets -- -D warnings` green
- [ ] Annotated tag `v2.0.0-stage2` создан (с полным сообщением)
- [ ] Tag pushed to origin
- [ ] `release.yml` CI job сработал (triggers on `push: tags: ['v*']`)
- [ ] 6 matrix artifacts собраны + SHA256 + cosign signature + SLSA L3 (или note "not verified + reason", если cosign/slsa-verifier не установлены — прецедент FBP11)
- [ ] Actions run URL + SHA256s записаны в `REPRODUCIBLE_BUILDS.md`
- [ ] `STAGE2_SUMMARY.md` — Release section обновлён
- [ ] `Changelog.md` — tag pushed + release verified
- [ ] Коммит `[S2-P24] docs: stage2 dod verification + summary + tag v2.0.0-stage2`

---

## 3. Сводная таблица промптов

| ID | Заголовок | Ключевые артефакты | Зависимости |
|----|-----------|--------------------|-------------|
| S2-P01 | ADR-0020: Noise Protocol Framework | docs/ADR/0020-noise-protocol-framework.md | (Gate: v1.5.0-stage1.5 + bugfixes-stage1.5 verified §0.1) |
| S2-P02 | ADR-0021: Erlay integration | docs/ADR/0021-erlay-integration.md | (parallel after §0.1) |
| S2-P03 | ADR-0022: Peer scoring algorithm | docs/ADR/0022-peer-scoring-algorithm.md | (parallel after §0.1) |
| S2-P04 | ADR-0023: Rate limiting strategy | docs/ADR/0023-rate-limiting-strategy.md | (parallel after §0.1) |
| S2-P05 | crates/strangecoin-net/ + workspace + Cargo.toml + lib.rs skeleton | crates/strangecoin-net/{Cargo.toml,src/{lib,error}.rs,tests/skeleton_smoke.rs}, Cargo.toml (workspace members + features) | S2-P01..S2-P04 |
| S2-P06 | Миграция network/* в crates/strangecoin-net/ | crates/strangecoin-net/src/{mod,protocol,rate_limiter,sync,sync_engine}.rs, src/network/mod.rs (5-line shim) | S2-P05 |
| S2-P07 | Noise handshake XX pattern (snow crate) | crates/strangecoin-net/src/noise/{mod,handshake}.rs, tests/noise_handshake.rs | S2-P01, S2-P05 |
| S2-P08 | Noise transport encryption (ChaCha20-Poly1305) | crates/strangecoin-net/src/noise/transport.rs, src/protocol.rs (encrypt_frame/decrypt_frame), tests/noise_transport.rs | S2-P07 |
| S2-P09 | node_id auth via Ed25519 (internal P2P) | crates/strangecoin-net/src/noise/auth.rs, tests/noise_auth.rs | S2-P08 |
| S2-P10 | HELLO message + capabilities negotiation | crates/strangecoin-net/src/protocol/hello.rs, tests/hello.rs | S2-P06, S2-P09 |
| S2-P11 | Peer discovery (seed IPs + DNSSEC + ADDR gossip) | crates/strangecoin-net/src/discovery/{mod,seeds,dnssec,addr_gossip}.rs, network.json, tests/discovery.rs | S2-P10 |
| S2-P12 | peer_store + peer_manager + connection_pool split | crates/strangecoin-net/src/peer/{mod,store,manager,pool}.rs, network.json + config.toml, tests/peer_management.rs | S2-P11 |
| S2-P13 | Gossip protocol (INV/GET_DATA/TX/BLOCK/NOTFOUND) | crates/strangecoin-net/src/gossip/{mod,inv,get_data,broadcast}.rs, tests/gossip.rs | S2-P12 |
| S2-P14 | Compact blocks (BIP 152) | crates/strangecoin-net/src/gossip/compact_blocks.rs, tests/compact_blocks.rs | S2-P13 |
| S2-P15 | Erlay reconciliation (mincut + fallback) | crates/strangecoin-net/src/gossip/erlay/{mod,reconcile,mincut,fallback}.rs, tests/erlay.rs | S2-P14 |
| S2-P16 | Rate limiting (token bucket + per-message caps + ban) | crates/strangecoin-net/src/rate_limiter.rs (rewrite), config.toml, tests/rate_limit.rs | S2-P12, S2-P04 |
| S2-P17 | Peer scoring algorithm (4-factor weighted + ban state machine) | crates/strangecoin-net/src/peer/scoring.rs, src/peer/manager.rs (tick), config.toml, tests/peer_scoring.rs | S2-P12, S2-P03 |
| S2-P18 | Deprecated messages cleanup (GET_BLOCKCHAIN/BLOCKCHAIN removed) | crates/strangecoin-net/src/protocol.rs (variants removed), tests/protocol_deprecated.rs | S2-P06 |
| S2-P19 | Async migration network workers (std::thread → tokio::spawn) | crates/strangecoin-net/src/{mod,sync,sync_engine,peer/manager}.rs, tests/async_migration.rs | S2-P06 |
| S2-P20 | Shadow network tooling (replay + divergence detector) | crates/strangecoin-net/src/shadow/{mod,replay,compare}.rs, src/cli/mod.rs (shadow subcommand), tests/shadow.rs | S2-P06 |
| S2-P21 | Fuzzing harness — 4 new network targets | fuzz/fuzz_targets/{noise_handshake,gossip_inv,compact_blocks_fill_up,erlay_reconciliation}.rs, fuzz/Cargo.toml, .github/workflows/ci.yml, fuzz/README.md | S2-P10..S2-P20 |
| S2-P22 | Integration tests — Stage 2 end-to-end | tests/{net_noise_handshake,net_gossip,net_compact_blocks,net_erlay,net_peer_scoring,net_rate_limit_integration}.rs, tests/common/net_helpers.rs | S2-P21 |
| S2-P23 | Регрессия 22 инвариантов + threat model update | docs/stage2/{INVARIANTS_ENFORCED,THREAT_MODEL_STAGE2,RESIDUAL_RISKS_STAGE3_PLUS}.md, docs/security/THREAT_MODEL.md | S2-P22 |
| S2-P24 | DoD-верификация Stage 2 + тег v2.0.0-stage2 | docs/stage2/STAGE2_SUMMARY.md, Changelog.md (2.0.0), AGENTS.md, Cargo.toml (version 2.0.0), tag v2.0.0-stage2, REPRODUCIBLE_BUILDS.md | S2-P23 |

Параллельные треки (см. §1 для деталей): (a) ADRs S2-P01..S2-P04 — parallel after §0.1 (tag gate); (b) Crate scaffolding S2-P05 → S2-P06 sequential; (c) Noise S2-P07 → S2-P08 → S2-P09 sequential; (d) Peer S2-P10 → S2-P11 → S2-P12 sequential; (e) Gossip S2-P13 → S2-P14 → S2-P15 sequential; (f) Limits S2-P16 ∥ S2-P17 parallel after peer; (g) Cleanup S2-P18 ∥ S2-P19 parallel after scaffolding; (h) Shadow S2-P20 parallel after scaffolding (optional); (i) Verification S2-P21 → S2-P22 → S2-P23 → S2-P24 strictly after all proper prompts; S2-P24 — gate for Stage 3/4.

---

## 4. Покрытие DoD Этапа 2 (ROADMAP3) после S2-P01..S2-P24

| # | Критерий DoD Этапа 2 (ROADMAP3) | Промпт | Evidence (после закрытия) |
|---|-----------------------------------|--------|--------------------------|
| 1 | Gossip работает (test: txs распространяются за ≤1 сек на 100 узлах) | S2-P13 + S2-P22 | tests/net_gossip.rs (1 node → 100 simulated peers ≤1s, simulated clock) |
| 2 | Compact blocks работают (test: bandwidth reduction ≥50% vs naive) | S2-P14 + S2-P22 | tests/net_compact_blocks.rs (100-tx block 1 MB → CompactBlock ~10 KB, ≥50% reduction) |
| 3 | Erlay работает (test: peer reconciliation успешна) | S2-P15 + S2-P22 | tests/net_erlay.rs (2 peers 1000 txs, 800 shared → only 200 transmitted, ≥80% reduction) |
| 4 | Peer discovery: new node находит ≥8 peers за ≤30 сек | S2-P11 + S2-P22 | tests/net_helpers.rs (SimulatedNetwork, multi-source bootstrap) |
| 5 | Handshake works (test: incompatible peers отвергаются) | S2-P10 + S2-P22 | tests/net_noise_handshake.rs (incompatible version/network_id rejected) |
| 6 | Peer scoring: malicious peers banned после N violations | S2-P12 + S2-P17 + S2-P22 | tests/net_peer_scoring.rs (invalid blocks → temp ban, protocol violations → persistent ban) |
| 7 | Noise encryption works (test: MITM обнаружен) | S2-P07 + S2-P08 + S2-P09 + S2-P22 | tests/net_noise_handshake.rs (MITM with own Noise keys → NodeId mismatch → connection dropped) |
| 8 | Rate limiting works (test: spammer banned) | S2-P16 + S2-P22 | tests/net_rate_limit_integration.rs (1000 INV/s spammer → banned after 5 violations / 60s) |
| 9 | Legacy messages удалены (no backward compatibility with v0.8.6 network) | S2-P18 | crates/strangecoin-net/src/protocol.rs (GetBlockchain/Blockchain removed), tests/protocol_deprecated.rs |
| 10 | `crates/strangecoin-net/` создан, сеть перенесена | S2-P05 + S2-P06 | crates/strangecoin-net/Cargo.toml (workspace member), src/network/mod.rs (5-line re-export shim) |
| 11 | ADR-0016: «Why Noise Protocol Framework» написан | S2-P01 (renumbered to ADR-0020) | docs/ADR/0020-noise-protocol-framework.md — note: ADR-0016 occupied by Receipts Root in Block Header (Stage 1.5 FBP07); Stage 2 renumbered to 0020..0023 |
| 12 | ADR-0017: «Why Erlay» написан | S2-P02 (renumbered to ADR-0021) | docs/ADR/0021-erlay-integration.md |
| 13 | `Changelog.md` обновлён: `2.0.0 — production-grade P2P` | S2-P24 | Changelog.md (секция 2.0.0), Cargo.toml version 2.0.0, tag v2.0.0-stage2 |

**Замечание по ADR нумерации:** ROADMAP3 §«Критические развилки → Stage 2» первоначально планировал ADR-0016..0019 для Noise/Erlay/PeerScoring/RateLimiting. Однако Stage 1.5 FBP07 (commit `f217314` + ADR-0016 от `768fc99`) уже использовал ADR-0016 для «Receipts Root in Block Header» (файл `docs/ADR/0016-receipts-root-in-block-header.md`). Stage 2 переименовывает свои 4 ADR на ADR-0020..0023 — это примыкающие к занятому ADR-0016 слоты, не вступает в конфликт. Дополнительно ROADMAP3 упоминает ADR-0018 (Peer scoring) и ADR-0019 (Rate limiting) — эти тоже переименованы на ADR-0022 и ADR-0023 соответственно. Это решение зафиксировано в S2-P01 commit message и в §2.1.

Плюс: тег `v2.0.0-stage2` (S2-P24) — Gate для Stage 3 (RocksDB) и Stage 4 (dev-experience: SDK, devnet, indexer, JSON-RPC eth_*, faucet).

---

## 5. Покрытие инвариантов ARCHITECT3 §5 на Stage 2

| # | Инвариант | Статус на Stage 2 | Где enforce / тест |
|---|-----------|-------------------|--------------------|
| 1 | Валидность — из цепочки; `balances` — кэш | Без изменений (regression) | state_cache::rebuild_from_chain (Stage 1) |
| 2 | Каждая tx подписана, `sender == pubkey` (secp256k1) | Без изменений (regression) | consensus::verify_transaction (Stage 1) |
| 3 | `hash <= target` для каждого блока | Без изменений (regression) | consensus::validate_block (Stage 1) |
| 4 | `apply_block`/`unapply_block` — обратные чистые функции | Без изменений (regression) | state::apply_block + unapply_block (Stage 1.5 extension) |
| 5 | Хэш/подпись — на канонических байтах (`serialize`), не JSON | Без изменений (regression) | serialize (Stage 1.5 extension for receipts_root) |
| 6 | Блок не содержит непроверимых tx/наград | Без изменений (regression) | consensus::validate_block (Stage 1) |
| 7 | Размеры сообщений/блоков всегда ограничены до аллокации | Без изменений (regression) | protocol.rs `MAX_MESSAGE_SIZE` check ДО аллокации (S2-P08 encryption preserves this) |
| 8 | Генезис детерминирован и совпадает у всех узлов | Без изменений (regression) | EXPECTED_GENESIS_HASH (Stage 1.5 FBP07 receipts_root extension) |
| 9 | Секреты не пишутся на диск и не логируются | Без изменений (regression) | keystore, no println (Stage 0/1) |
| 10 | Replay protection (chain_id) | Без изменений (regression); HELLO rejects mismatched network_id (S2-P10) | consensus + protocol/hello.rs (S2-P10) |
| 11 | Nonce инкрементируется | Без изменений (regression) | state::apply_create/apply_call (Stage 1.5 FBP04) |
| 12 | Transaction hash = commitment | Без изменений (regression) | serialize (Stage 1.5 extension) |
| 13 | Mempool basic rules | Без изменений (regression) | mempool::insert (Stage 1) |
| 14 | Graceful shutdown | Без изменений (regression); tokio runtime shutdown (S2-P19) | node::Drop (Stage 1) + async_migration (S2-P19) |
| 15 | Block gas limit | Без изменений (regression) | block_executor::enforce_block_gas_limit (Stage 1.5 FBP04) |
| **16** | **P2P framing: length-prefixed, проверка размера ДО аллокации** | **Расширено (Noise encrypted framing)** | crates/strangecoin-net/src/protocol.rs (encrypt_frame/decrypt_frame with MAX_MESSAGE_SIZE check ДО аллокации, S2-P08); tests/noise_transport.rs |
| **17** | **Rate limiting: лимит сообщений/сек от одного пира; нарушение → бан** | **Расширено (integration с peer scoring)** | crates/strangecoin-net/src/rate_limiter.rs (token bucket + per-message caps, S2-P16) + peer/scoring.rs (ban state machine, S2-P17); tests/net_rate_limit_integration.rs |
| 18 | Event log / Receipt | Без изменений (regression) | emit_log + Receipt (Stage 1.5 FBP07 receipts_root extension) |
| 19 | State root match | Без изменений (regression) | root_after + receipts_root (Stage 1.5 FBP07) |
| 20 | Fee invariant | Deferred (Stage 5) | — (fixed fee proxy, no change in Stage 2) |
| 21 | Consensus versioning | Без изменений (regression); MIN_PROTO_VERSION=2 для Stage 2 (S2-P10) | consensus::consensus_version (Stage 1) + protocol/hello.rs (S2-P10) |
| 22 | Reproducible builds | Расширено (v2.0.0-stage2 release pipeline) | release.yml run for v2.0.0-stage2 (S2-P24) |

**Итог Stage 2:** 22/22 invariants enforced (расширение coverage для #16 Noise encrypted framing, #17 rate limiting + peer scoring integration, #22 v2.0.0-stage2 release pipeline; 1 still deferred до Stage 5: #20 fee invariant). Regression: S2-P23 (THREAT_MODEL.md + INVARIANTS_ENFORCED.md updated).

---

## 6. Примечания по использованию

1. **Порядок выполнения:** строго по карте зависимостей (§1). Гейт №1 — подтверждение тега `v1.5.0-stage1.5` и сверки `bugfixes-stage1.5.md` v1.2 (см. §0.1 — 11/11 fixed, tag на origin confirmed). Гейт №2 — ADR-0020..0023 (S2-P01..S2-P04) — без них код Stage 2 proper не начинается (S2-P05 не запускается без ADRs). Гейт №3 — S2-P05..S2-P06 (crate scaffolding + migration) — без них остальные промпты не могут писать в `crates/strangecoin-net/`. Гейт №4 — тег `v2.0.0-stage2` (S2-P24); без него Stage 3/4 не начинаются (мягкая зависимость для Stage 3, жёсткая для Stage 4).

2. **Размер промпта:** каждый промпт — ≤200 строк markdown, реализация помещается в контекст 32k-token агента. Если промпт кажется большим — делить на a/b (например, S2-P12a: peer_store + peer_manager; S2-P12b: connection_pool + integration с peer scoring).

3. **Контекст:** вставлять блок «Общий контекст» (§0) в начало каждого промпта при отправке агенту.

4. **Артефакты и измеримость:** прогресс = число созданных файлов / общее число ожидаемых. КГ обязательны: нельзя закрывать промпт без прохождения всех пунктов; если КГ не проходит — итерировать тот же промпт, не переходить к следующему.

5. **Процессные страховки из retro §8.1** (обязательны к применению, прецедент bugfixes-stage1.5.md):
   - 1 промпт = 1 атомарный коммит с ID промпта в сообщении, формат `[S2-PNN] <area>: <short description>`;
   - Changelog/README/AGENTS.md обновляются только коммитом, в котором артефакты уже существуют (урок FBP10 — Changelog до коммита = violation);
   - "done" объявления — только через DoD-промпт S2-P24 (тег), никогда — через ощущение завершённости;
   - факты, которые агент не может проверить (CI-run, fuzz-статистика, differential testing, release pipeline, cosign verify), фиксируются как "не верифицировано + причина", а не молча опускаются (прецедент FBP08/FBP11);
   - все отклонения от первичной спеки фиксируются явно в разделе «Решение» каждого промпта (прецедент FBP03/FBP04/FBP05/FBP07).

6. **Связь с чек-листом ARCHITECT3 §17:** после S2-P24 прогнать чек-лист §17 и убедиться, что все применимые к Stage 2 пункты отмечены:
   - `Protocol messages: GetHeaders/Headers, deprecated legacy удалён` (S2-P18)
   - `Peer store/manager/connection_pool разделены` (S2-P12)
   - `Sync engine: разрыв цикла network↔blockchain` (S2-P06 via trait BlockchainView)
   - `Tie-breaking: work → earliest timestamp → lowest hash` (regression from Stage 1)
   - `Mempool RBF policy implemented` (regression from Stage 1)
   - `Storage schema versioned + migrations` (regression — Stage 3 scope)
   - `Wallet: secp256k1/BLS, public API only, path to separate crate documented` (regression — Stage 3 scope)
   - `Economics: tail emission formula, fee invariant enforced` (regression — fee invariant deferred to Stage 5)
   - `Governance: SCIP + activation height для изменений консенсуса` (regression — Stage 1)
   - `Security: TLA+ spec актуализирован, fuzzing targets добавлены` (S2-P21 + S2-P23)
   - `Anti-goals respected (см. §15)` (см. §7 ниже)
   - `License: MIT/Apache-2.0, LICENSE файл в корне` (regression — Stage 0)
   - `Reproducible builds: CI gate, cosign signature для release` (S2-P24)
   - `Strangler pattern: перенос подсистемы в отдельный крейт` (S2-P05 + S2-P06)
   - `Tests: unit + property (proptest) + integration (two_clients, reorg, double_spend)` (S2-P22 + regression from Stage 0/1)
   - `CI: clippy -D warnings, all platforms (Linux/macOS/Windows)` (regression)
   - `Documentation: ADR для архитектурных решений, SCIP для изменений протокола` (S2-P01..S2-P04 + regression from Stage 1.5)

7. **Детерминизм — критический инвариант:** каждая новая network-функция (включая Noise handshake из S2-P07, Erlay reconciliation из S2-P15, peer scoring из S2-P17) обязана быть детерминированной (no time, no random, no I/O). Noise ephemeral keys генерируются на старте узла из long-term seed (keystore), не из system RNG в момент handshake. Erlay sketches — deterministic PinSketch encoding. Ревью каждой новой функции на детерминизм — обязательный пункт КГ (threat V-21 extension для network layer).

8. **snow version pinning:** `snow = "=x.y.z"` (pin to specific version, не "^", не patch) — Noise API может меняться между majors; pin в Cargo.toml (S2-P07); обновление только через SCIP после mainnet freeze. Аналогично `ed25519-dalek = "=2.0"` (S2-P09), `minisketch-rs = "=x.y.z"` (S2-P15), `siphasher = "x.y"` (S2-P14), `hickory-resolver = "x.y"` (S2-P11). Все network-crate dependencies pinned.

9. **Consensus-breaking changes в S2-P18** (deprecated messages removal) — mainnet не запущен (Stage 1.5 tag v1.5.0-stage1.5, mainnet freeze gate = BUG-S1-013/015 ops gates), прямая правка допустима до тега v2.0.0-stage2; после — только через SCIP + activation height. Каждый breaking change фиксируется в Changelog 2.0.0 (S2-P24).

10. **2 ops-gate из bugfixes-stage1** (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) — НЕ блокируют Stage 2 (regtest/devnet scope); становятся обязательными gates только для mainnet freeze. Зафиксировать в STAGE2_SUMMARY.md (S2-P24) как residual ops gates (так же, как в STAGE1_5_SUMMARY.md, FBP09).

11. **SCIP-0001 (genesis key replacement)** — отдельный трек, ops + code, не входит в Stage 2 scope (так же, как не входил в bugfixes-stage1.5.md scope). После S2-P24 SCIP-0001 остаётся единственным блокером mainnet; его выполнение — вне Stage 2 scope, ops + code track.

12. **Anti-goals Stage 2 (явно отложено — §7):** RocksDB migration (Stage 3), stateless validation Verkle proofs (Stage 3), state pruning EIP-4444 analog (Stage 3), Block-STM parallel execution (Stage 3+), snapshot sync (Stage 3), Account Abstraction ERC-4337 (Stage 5), EIP-1559 dynamic base fee (Stage 5), threshold encryption mempool impl (Stage 5), WIT/Wasm Component Model ABI (Stage 4+), K-framework formal verification WASM subset (Stage 6), ZK-VM integration RISC Zero/SP1 (Stage 6+ research), state rent (Stage 3+ optional via activation height), BLS12-381 dependency + bls_aggregate precompile impl (Stage 7), SDK/devnet/indexer/explorer/JSON-RPC eth_*/faucet (Stage 4), PoS migration Casper FFG/BLS consensus signatures (Stage 7), external audit formal verification (Stage 6), multidimensional fees EIP-7706 analog (Stage 6+ research), constructor fee burning (Stage 5 fee market), receipts prune (Stage 3+ SCIP), contract storage prune (Stage 3+ SCIP), TLA+ formal verification of network protocol (Stage 6, currently only consensus.tla from Stage 1.5 FBP07 — Stage 2 не расширяет TLA+, residual risk зафиксирован в S2-P23 RESIDUAL_RISKS_STAGE3_PLUS.md).

13. **Cross-cutting tracks (ROADMAP3 «Сквозные треки»):**
    - **Security track:** Stage 2+ — fuzzing (S2-P21), threat model update (S2-P23); Stage 6 — external audit, formal verification of network protocol.
    - **Governance track:** Stage 1+ — SCIP process + activation height; Stage 2 не вводит новых SCIPs (network-only changes, не consensus rules). Если S2-P18 (deprecated messages removal) касается protocol-version-gated features — формально через SCIP, но mainnet не запущен, прямая правка допустима до тега v2.0.0-stage2.
    - **Documentation track:** Stage 2+ — ADR-0020..0023 (S2-P01..S2-P04), `docs/stage2/{STAGE2_SUMMARY,INVARIANTS_ENFORCED,THREAT_MODEL_STAGE2,RESIDUAL_RISKS_STAGE3_PLUS}.md` (S2-P23/S2-P24); Stage 4 — API documentation, SDK documentation (out of Stage 2 scope).
    - **Operations track:** Stage 0 — CI/CD (regression), tracing (regression); Stage 2 не добавляет metrics/Docker/systemd (это Stage 4+ scope).

---

## 7. Что НЕ входит в Stage 2 (явно отложено)

Следующие задачи появляются на более поздних стадиях; если промпт случайно затрагивает их — остановить и переориентировать на Stage 2 network scope:

- **RocksDB миграция (LevelDB → RocksDB)** — Stage 3 (ARCHITECT3 §3.7, §10.5: «Stage 3: Перенос storage, wallet; LevelDB → RocksDB миграция»; сейчас rusty_leveldb).
- **Stateless validation witnesses (Verkle proofs)** — Stage 3 (ARCHITECT3 §3.3: «StateWitness для stateless validation»; сейчас SMT only, без Verkle proofs).
- **State pruning / expiry (EIP-4444 analog)** — Stage 3 (ARCHITECT3 §3.7, §7.3: «pruning исторического state старше N блоков»).
- **Snapshot sync** — Stage 3 (ARCHITECT3 §3.7: «snapshot sync для быстрой синхронизации новых узлов»).
- **Block-STM (parallel execution)** — Stage 3+ (ARCHITECT3 §15: «сначала корректная последовательная валидация (Stage 1.5), потом Block-STM (Stage 3+)»).
- **Account Abstraction (social recovery, multisig, session keys, paymaster)** — Stage 5 (ARCHITECT3 §15: «AA на Stage 5 после VM и dev-experience»).
- **EIP-1559 dynamic base fee** — Stage 5 (ARCHITECT3 §15: «фиксированный gas limit для MVP; EIP-1559 на Stage 5»).
- **Multidimensional fees (EIP-7706 analog)** — Stage 6+ research.
- **Threshold encryption mempool impl (EncryptedMempool)** — Stage 5 (placeholder допустим с Stage 1; full impl на Stage 5).
- **WIT/Wasm Component Model для ABI** — Stage 4+ (сейчас только `format_version: "wit-v2"` placeholder, S1.5-FBP02).
- **K-framework формальная верификация WASM subset** — Stage 6 (ARCHITECT3 §3.9: «K-framework спецификация WASM subset»).
- **ZK-VM integration (RISC Zero / SP1)** — Stage 6+ research.
- **State rent** — Stage 3+ опционально через activation height (ARCHITECT3 §15).
- **BLS12-381 dependency + `bls_aggregate` precompile impl** — Stage 7 (PoS-фаза; сейчас precompile возвращает `NotImplemented`, S1.5-P11).
- **SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet** — Stage 4 (требует Stage 2 завершённым, S2-P24).
- **PoS миграция, Casper FFG, BLS consensus signatures** — Stage 7.
- **External audit, formal verification of network protocol** — Stage 6 (security track; сейчас только fuzzing harness, S2-P21).
- **Перенос legacy-threads майнинга в async** — частично Stage 2 (S2-P19 завершает для network; mining workers остаются Stage 1.5 baseline; full mining async migration — Stage 6+ research, out of Stage 2 scope).
- **TLA+ формальная верификация network protocol** — Stage 6 (сейчас только consensus.tla + receipts_root_match invariant из Stage 1.5 FBP07 — Stage 2 не расширяет TLA+ для network; residual risk зафиксирован в S2-P23 RESIDUAL_RISKS_STAGE3_PLUS.md).
- **SCIP-0001 (genesis key replacement)** — отдельный трек, ops + code, не входит в Stage 2 scope (так же, как не входил в bugfixes-stage1.5.md scope).
- **BUG-S1-013 (PGP-key offline keygen)** — ops gate, не код; mainnet freeze gate.
- **BUG-S1-015 (offline genesis key air-gapped keygen)** — ops gate, не код; mainnet freeze gate. Tooling закрыт (`examples/generate_offline_genesis_key.rs`), ops остаётся.
- **Constructor fee burning** — Stage 5 fee market (сейчас gas = cap, fees = 0; не трогается в Stage 2).
- **Receipts prune (старые receipts можно удалить из sub-trie)** — Stage 3+ через SCIP.
- **Contract storage prune (удаление контракта с storage)** — Stage 3+ через SCIP.
- **Multidimensional gas markets (calldata/blob/storage separate markets)** — Stage 6+ research.
- **PBS (Proposer-Builder Separation)** — Stage 6+ research.
- **Batch auctions** — Stage 5+ опционально.
- **Inclusion lists (PoS-фаза)** — Stage 7+.
- **Validator set rotation, slashing, staking rewards** — Stage 7 (PoS migration).
- **JSON-RPC eth_* compatibility** — Stage 4 (требует Stage 2 завершённым для dev-experience).
- **GraphQL API** — Stage 4 (через indexer).
- **HD wallets (BIP-39/44)** — Stage 4 (wallet feature).
- **Hardware wallet support (Ledger)** — Stage 4+ (wallet feature).
- **Metrics (Prometheus/OpenTelemetry)** — Stage 4 (operations track).
- **Docker images, systemd unit files** — Stage 4 (operations track).
- **Validator operations guide** — Stage 7 (PoS migration).

---

## 8. Связь с другими документами

- `ARCHITECT3.md` — целевая архитектура (этот документ ведёт к ней через Stage 2). См. §3.6 network/p2p, §4.2 sync, §5 инварианты #16/#17, §6 угрози V-1/V-2/V-14/V-15/V-16/V-17/V-24, §10.4 strangler Stage 2, §13 headless, §14 network_id, §15 anti-goals, §17 PR-checklist.
- `ROADMAP3.md` — этапный план, **de-facto Stage 2 backlog** (на месте отсутствующего `BACKLOG3.md`). См. «Этап 2 — Сетевая зрелость (gossip, Noise, Erlay, tokio)», «Критические развилки → Stage 2» (ADRs), «Сквозные треки».
- `prompt-stage1.5.md` — шаблон/формат для `prompt-stage2.md` (§0 контекст-блок, §1 карта зависимостей, §2 список промптов с разделами Цель/Контекст/Задачи/Артефакты/КГ/Коммит, §6 процессные страховки из retro §8.1, §7 anti-goals).
- `bugfixes-stage1.5.md` v1.2 — verification of Stage 1.5 closure (11/11 fixed), precondition for Stage 2 (см. §0.1 в этом документе для табличной сверки).
- `Changelog.md` (секции 1.5.0..1.5.6) — Stage 1.5 closure; Stage 2 добавит секцию 2.0.0 (S2-P24).
- `docs/ADR/0007-tokio-on-stage-1.md` — tokio baseline (Stage 1).
- `docs/ADR/0011-async-migration.md` — async migration baseline (Stage 1, BUG-S0-021); S2-P19 continuation.
- `docs/ADR/0016-receipts-root-in-block-header.md` — **занятый ADR-0016 слот** (Stage 1.5 FBP07, commit `768fc99`); Stage 2 ADRs переименованы на 0020..0023 (см. §2.1 в этом документе).
- `docs/spec/consensus.tla` — TLA+ baseline (35,207 states PASS, 10 invariants including ReceiptsRootMatch); Stage 2 не расширяет TLA+ для network (residual risk зафиксирован в S2-P23).
- `docs/security/THREAT_MODEL.md` — STRIDE analysis (V-1..V-42); Stage 2 обновит V-1/V-2/V-14/V-15/V-17/V-24 mitigations (S2-P23).
- `docs/stage1.5/STAGE1_5_SUMMARY.md` — Stage 1.5 closure summary (14 DoD ✅); baseline для Stage 2.
- `docs/stage1.5/INVARIANTS_ENFORCED.md` — 22/22/22 enforced (Stage 0/1/1.5); Stage 2 добавит 4th column (S2-P23).
- `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` — V-18/V-19/V-21 mitigations (Stage 1.5); Stage 2 extends с V-1/V-2/V-14/V-15/V-17/V-24 (S2-P23).
- `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` — Stage 2+ residual risks (constructor gas burning Stage 5, receipts/contract prune Stage 3+ SCIP); Stage 2 обновит в `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (S2-P23).
- `docs/SCIP/scip-0000-process.md` — SCIP process (Stage 1).
- `docs/SCIP/scip-0001-genesis-key-replacement.md` — Draft, activation_height = TBD, mainnet freeze gate; НЕ Stage 2 scope.
- `docs/SCIP/scip-0002-state-root-enforcement.md` — Draft, mainnet/testnet reject zero state_root, regtest opt-in; regression в Stage 2.

**Замечание про отсутствующий `BACKLOG3.md`:** файл `analytics/BACKLOG3.md` не существует в репозитории на коммите HEAD `1b4dbcc` (поиск `find . -iname "*BACKLOG*"` пуст). ROADMAP3.md Этап 2 (раздел «Задачи» + «Definition of Done» + «Риски» + «ADRs required») используется как de-facto Stage 2 бэклог — он содержит полный список tasks с checkboxes и явный DoD, что функционально эквивалентно бэклогу. Все ссылки в этом документе на «бэклог Stage 2» означают `ROADMAP3.md` §«Этап 2 — Сетевая зрелость». Если позже появится `BACKLOG3.md`, промпты S2-P01..S2-P24 нужно переразметить по нему 1:1 (см. §0.1 «Замечание про `BACKLOG3.md`» в шапке этого документа).




