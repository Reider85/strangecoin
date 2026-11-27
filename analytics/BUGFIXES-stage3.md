# BUGFIXES-stage3.md — Промпты на устранение пробелов реализации Stage 3

**Версия:** 1.0
**Дата:** 2026-11-28
**Источник:** аудит реализации `analytics/prompt-stage3.md` (S3-P01..S3-P15) против фактического кода репозитория на HEAD `e2e8449` (2026-11-28), тег `v3.0.0-stage3` (перепривязан на `23f4641`), `docs/stage3/STAGE3_SUMMARY.md`, `docs/release-retag-v3.0.0-stage3.md`, `crates/strangecoin-storage/` (src + tests), `crates/strangecoin-core/src/state/witness.rs`, `tests/stateless_light_client.rs`, `tests/pruning.rs`, `tests/snapshot_sync.rs`, `tests/indices.rs`, `tests/leveldb_migration.rs`, `tests/per_block_writes.rs`.

---

## 0. Сводка аудита (что уже реализовано и реализовано хорошо)

Stage 3 формально закрыт: DoD 11/11, инварианты 26/26 (`docs/stage3/INVARIANTS_ENFORCED.md`), тег `v3.0.0-stage3` существует, регрессия 101 suite / 912 passed / 0 failed (S3-P15). Качество большинства артефактов **высокое**:

| Промпт | Статус | Комментарий по качеству |
|--------|--------|--------------------------|
| S3-P01 ADR-0024 (schema + trait) | ✅ | ADR создан, CF-схема + StorageEngine trait зафиксированы |
| S3-P02 ADR-0025 (pruning) | ✅ | height-window + archival + SCIP-path |
| S3-P03 ADR-0026 (snapshot format) | ✅ | 4-step commitment-верификация |
| S3-P04 crate scaffolding | ✅ | workspace member, rocksdb `=0.25.0` pinned |
| S3-P05 schema.rs | ✅ | BE-encoding, proptests на ordering/injectivity |
| S3-P06 RocksDbEngine | ✅ | 7 CF, schema-guard fail-closed, атомарный WriteBatch, prefix-iter |
| S3-P07 миграция монолита | ✅ (deviation) | per-block writes, mempool CF + delete, config wiring; **но**: accounts кодируются fixed-order `serde_json`, не canonical binary (см. BF-S3-02) |
| S3-P08 LevelDB→RocksDB миграция | ⚠️ (КГ нарушен) | авто-миграция fail-closed + `.leveldb.bak` есть; **но**: `rusty-leveldb` остался **runtime-**зависимостью (feature `legacy-migrate`, default-on) — КГ требовал «runtime-edge пуст / только dev-deps» (см. BF-S3-01) |
| S3-P09 индексы | ✅ (deviation documented) | txid→(height,index), hash→height, storage-root кэш, reorg-консистентность, backfill |
| S3-P10 perf + crash | ✅ | bench 719.7x (≫10x), kill-sweep + stale-LOCK, LRU `CachedEngine` |
| S3-P11 stateless | ⚠️ (частично) | wire-кодекс + light-client e2e качественные; **но**: верификация покрыта только transfer-блоками, `build_witness_for_block` — O(N) full replay (см. BF-S3-03, BF-S3-04) |
| S3-P12 pruning | ✅ | образцовая реализация: markers lifecycle, archival no-op, детерминизм, исчерпывающие тесты |
| S3-P13 snapshot | ✅ | export/import CLI, forged/corrupt/wrong-chain-id negative-тесты, e2e catch-up |
| S3-P14 docs | ✅ | INVARIANTS 26/26, THREAT_MODEL v3.3, RESIDUAL_RISKS_STAGE4_PLUS |
| S3-P15 DoD + tag | ⚠️ (process-gap) | DoD-таблица и регрессия есть; **но**: §7.1 «Verification follow-up» остался пустым плейсхолдером, release-пайплайн молча падал 3 недели, тег перепривязан на коммит со Stage 4-кодом (см. BF-S3-05, BF-S3-06) |

Плюс сквозное нарушение Pin Policy (BF-S3-07).

**Итого: 10/15 промптов реализованы полностью и хорошо; S3-P08 — с нарушением КГ; S3-P11 — функционально, но с ограниченным покрытием; S3-P15 — с незакрытыми process-обязательствами.** Ниже — 7 промптов на устранение (BF-S3-P01..BF-S3-P07), в формате `prompt-stage3.md` (§0 контекст-блок вставлять при отправке агенту отдельно).

---

## 1. Карта зависимостей промптов-исправлений

```
BF-S3-P01 (leveldb runtime-dep removal) ──────────────┐
BF-S3-P02 (canonical Account codec, schema v2) ───────┤── независимы друг от друга,
BF-S3-P07 (pin policy storage deps) ──────────────────┘   каждый = 1 атомарный коммит
                                                          (P02 → потом P04, т.к.
                                                          codec меняет snapshot body)
BF-S3-P03 (stateless VM-coverage) ──▶ BF-S3-P04 (witness perf) — последовательны
                                       (оптимизация должна сохранить покрытие)

BF-S3-P05 (release verification §7.1 + release.yml) ──┐
BF-S3-P06 (tag hygiene / gate-фиксация) ──────────────┘── независимы от кодовых,
                                                          но P05 до P06 (верификация
                                                          прогона нужна для решения
                                                          по тегу)
```

**Порядок исполнения:** BF-S3-P01 → BF-S3-P07 → BF-S3-P02 → BF-S3-P03 → BF-S3-P04 → BF-S3-P05 → BF-S3-P06. Каждый промпт закрывается отдельным коммитом формата `[BF-S3-PNN] <area>: <short description>`; после каждого — `cargo check --workspace`, `cargo test --workspace` (чанками), `cargo clippy --all-targets -- -D warnings` green.

---

## 2. Промпты-исправления

### BF-S3-P01. Устранение runtime-зависимости rusty-leveldb (закрытие КГ-отклонения S3-P08)

**Цель:** довести DoD-критерий №1 («RocksDB миграция завершена, LevelDB удалён») до буквального исполнения: убрать `rusty-leveldb` из **runtime**-графа зависимостей root-крейта. Сейчас (deviation, зафиксированный в STAGE3_SUMMARY §4 поз.10) `rusty-leveldb = "=4.0.0"` остаётся optional default-on feature `legacy-migrate` — read-only legacy reader одной авто-миграции; `cargo tree -i rusty-leveldb` = **1 runtime-edge**. КГ S3-P08 требовал: «`cargo tree -i rusty-leveldb` пуст (**или только dev-deps** — с пометкой)». Отклонение было допустимо как временная мера на момент тега, но теперь подлежит закрытию.

**Контекст:** legacy-миграция (S3-P08) уже отработала у существующих нод; новые ноды создают RocksDB-базу сразу и legacy- reader им не нужен. Чтение legacy-базы нужно только (a) в тесте `tests/leveldb_migration.rs` (фикстуры) и (b) теоретически — оператору старой ноды, ещё не запускали v3.0.0. Путь решения: перенести legacy-reader в **dev-dependencies** (тесты) + опциональный **отдельный tooling-бинарь** (не default-feature основного крейта), а авто-миграцию при первом открытии выполнять через этот reader, вкомпилированный только при явном feature, **выключенном** по умолчанию.

**Задачи:**

1. Перенести legacy LevelDB-читатель (`src/storage/legacy.rs` или эквивалент) за feature `legacy-migrate` со сменой default: `default = []` (feature **выключен** по умолчанию). Подтвердить: `cargo tree -i rusty-leveldb` на default-фичах → пусто (или ошибка «package not found»).
2. `tests/leveldb_migration.rs` — перевести на `#[cfg(feature = "legacy-migrate")]`-тесты либо CI-job, включающий фичу; fixture-сеяние legacy-базы оставить в dev-deps (`rusty-leveldb` как dev-dependency — допустимо по КГ).
3. CLI: `sc storage migrate-leveldb` (если есть) — пометить, что требует сборки с `--features legacy-migrate`; при отсутствии фичи — понятная ошибка компиляции/рантайма с инструкцией.
4. Обновить документацию отклонения: `docs/stage3/STAGE3_SUMMARY.md` §4 (поз.10 «Выпиливание legacy-migrate» — закрыть), Changelog (секция 3.0.x: «rusty-leveldb удалён из runtime-зависимостей; миграция — через `--features legacy-migrate`»), `AGENTS.md` Pin Policy (убрать строку rusty-leveldb из runtime, оставить в dev-deps).
5. Регрессия: авто-миграция при первом открытии с существующей legacy-директорией по-прежнему зелёная в тесте с включённой фичей; `cargo test --workspace` без фичи — зелёный.

**Артефакты:**
- root `Cargo.toml` (feature `legacy-migrate` выключен по умолчанию; rusty-leveldb → dev-deps)
- `src/storage/legacy.rs` / миграционный glue (перенос за feature)
- `tests/leveldb_migration.rs` (feature-gated)
- `docs/stage3/STAGE3_SUMMARY.md`, `Changelog.md`, `AGENTS.md` (правки)

**КГ:**
- [ ] `cargo tree -i rusty-leveldb` на default-фичах пуст (evidence в коммите)
- [ ] `cargo build` без фичей не тянет rusty-leveldb (проверено `cargo tree` / `cargo metadata`)
- [ ] Авто-миграция зелёная в тесте с `--features legacy-migrate`
- [ ] Deviation-записи в STAGE3_SUMMARY/AGENTS закрыты/обновлены
- [ ] `cargo test --workspace` green; clippy -D warnings green
- [ ] Коммит `[BF-S3-P01] migrate: remove rusty-leveldb from runtime deps (legacy-migrate opt-in)`

---

### BF-S3-P02. Канонический бинарный кодек Account (закрытие отклонения S3-P07: fixed-order serde_json)

**Цель:** реализовать canonical binary encode/decode для `Account`/`AccountState` в `strangecoin-core` и перевести хранение (`state` CF, `contract_storage` CF) и snapshot body (S3-P13) с fixed-order `serde_json` на canonical binary serialization — как требует ADR-0024 §Decision «Values» («canonical bytes») и ADR-0026 §Decision («canonical binary serialization core, не serde_json для body»).

**Контекст:** фактическое состояние (verified, комментарий в `crates/strangecoin-storage/Cargo.toml`): «S3-P07 deviation: core has no binary Account codec; fixed-order serde_json» — значения `state` CF и snapshot body кодируются `serde_json` с фиксированным порядком полей. Это детерминировано, но: (a) нарушает букву двух ADR; (b) раздувает объём (JSON-оверhead на каждый аккаунт); (c) фиксирует хрупкий контракт «порядок полей в JSON = формат хранения» без версии формата на уровне значения. Bump schema_version 1 → 2 с миграцией «на лету» при чтении (legacy JSON-значения декодируются, новые пишутся binary) — либо one-shot rewrite при первом открытии (прецедент S3-P08 auto-migration). Решение фиксировать в аменде ADR-0024.

**Задачи:**

1. `crates/strangecoin-core/src/state/` (или `serialize.rs`): `encode_account(acc: &Account) -> Vec<u8>` / `decode_account(&[u8]) -> Result<Account, _>` — canonical binary: tag EOA/Contract (1 байт), balance u64 BE, nonce u64 BE, для контракта code_hash (32) + storage_root (32). Pure function, round-trip + proptest (любой Account → encode → decode == исходный), golden vector.
2. Аменда `docs/ADR/0024-storage-schema-and-engine.md`: раздел «Value encoding» — binary Account codec, schema_version 1 → 2, стратегия обратной совместимости.
3. `src/storage/codec.rs`: писать `state`/`contract_storage` значения новым кодеком; при чтении — дизамбигуация legacy (первый байт `{` = JSON legacy → декодировать старым путём; иначе binary) либо флаг schema_version=2 с ленивой конвертацией при первом rewrite значения. Выбрать один вариант и задокументировать (рекомендуется: read-both/write-new + фоновый rewrite при следующем сохранении аккаунта — без отдельного migration-пасса).
4. `crates/strangecoin-storage/src/snapshot.rs`: body-аккаунты перевести на binary кодек (header `format_version` bump; verify-шаг (c) — recompute state_root из декодированных аккаунтов — сохранить семантику). Snapshot'ы старого формата: явно reject с сообщением «regenerate snapshot» (файл — транспорт, не персистентное хранилище; back-compat не требуется, зафиксировать).
5. Тесты: round-trip/proptest/golden; e2e — база, созданная на schema v1 (JSON-значения), открывается на v2, балансы эквивалентны; snapshot forge-тесты (`tests/snapshot_sync.rs`) зелёные на новом формате.

**Артефакты:**
- `crates/strangecoin-core/src/serialize.rs` или `state/` (codec + тесты)
- `docs/ADR/0024-storage-schema-and-engine.md` (аменда)
- `src/storage/codec.rs`, `crates/strangecoin-storage/src/snapshot.rs` (правки)
- `tests/` (новый `account_codec.rs` или дополнение существующих)

**КГ:**
- [ ] Binary Account codec в core: pure, round-trip + proptest + golden — зелёные
- [ ] `state`/`contract_storage` CF пишутся binary; legacy JSON читается (read-both/write-new) — e2e-тест зелёный
- [ ] schema_version 2; стратегия совместимости задокументирована в аменде ADR-0024
- [ ] Snapshot body — binary; старые `.scsnap` явно reject'ятся с внятной ошибкой
- [ ] `serde_json` в snapshot/state-path удалён (проверено grep'ом; остаётся только для optional manifest)
- [ ] `cargo test --workspace` green; clippy green
- [ ] Коммит `[BF-S3-P02] schema: canonical binary Account codec (schema v2, read-both/write-new)`

---

### BF-S3-P03. Stateless-покрытие VM/контрактных блоков (расширение S3-P11)

**Цель:** закрыть главный пробел качества S3-P11: `verify_block_stateless` и e2e-тесты покрывают **только transfer/coinbase-блоки** (STAGE3_SUMMARY §6.1: «stateless baseline — transfer-only verify, documented»). Light client должен уметь верифицировать блок с WASM contract-call/созданием контракта: touched-адреса включают контракт и его storage, post-state recompute должен учитывать изменения `storage_root` контракта и gas-семантику.

**Контекст:** `witness.rs::collect_touched_addresses` собирает sender/receiver всех tx — для contract-call это включает адрес контракта, но `AccountProof` контракта несёт только `code_hash` + `storage_root`, а `apply_block` на реконструированном partial-state должен корректно исполнить WASM-вызов (VM в core-пути? В baseline VM-крейт отдельно — проверить, что stateless-путь либо исполняет VM, либо явно reject'ит контрактные блоки с типизированной ошибкой). Текущее поведение для контрактных блоков не проверено ни одним тестом — риск: тихая неверифицируемость или ложный Ok.

**Задачи:**

1. Определить и зафиксировать семантику (в doc-comment `witness.rs` + аменда-заметка в STAGE3_SUMMARY): вариант A — stateless-верификация **исполняет** VM на reconstructed partial state (требует VM-доступ в verify-пути и корректной обработки отсутствующих storage-слотов, не затронутых блоком); вариант B — stateless-путь **reject'ит** контрактные блоки с явной ошибкой `ContractBlockUnsupported` (light client применяет transfer-блоки, контрактные — через полный replay). Выбрать минимально-корректный вариант для Stage 3 (рекомендуется B как fail-closed, A — residual Stage 4+), НО текущее поведение должно быть явным, а не неопределённым.
2. Если вариант B: добавить в `verify_block_stateless` проверку «блок содержит контрактную tx» (по code_hash из witness-пруфа контракта или по типу tx) → типизированная ошибка; light client получает понятный сигнал «этот блок требует full-ноды».
3. Если вариант A: расширить witness на touched storage-слоты (добавить `storage_proofs: Vec<(slot_key, pre_value, sibling_proof)>` в `AccountProof` при `WITNESS_FORMAT_VERSION` bump до 2), recompute contract storage_root через SMT-after-updates — по образцу уже существующего `root_after_updates`.
4. Тесты (`tests/stateless_light_client.rs` + core-тесты): блок с contract-call — (вариант B) reject с точной ошибкой; (вариант A) Ok + все негативные сценарии (corrupt slot proof, stripped slot) повторены для storage-пруфов.
5. Документировать границу в `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` (если вариант B: «stateless VM-верификация — Stage 4+»).

**Артефакты:**
- `crates/strangecoin-core/src/state/witness.rs` (семантика + тесты)
- `tests/stateless_light_client.rs` (VM-блок сценарии)
- `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` или STAGE3_SUMMARY (зафиксированная граница)

**КГ:**
- [ ] Семантика stateless-пути для контрактных блоков явная (исполняет или reject'ит) — задокументирована в doc-comment + docs
- [ ] Тест: блок с WASM contract-call проходит через `verify_block_stateless` с предсказуемым результатом (не «неопределённое поведение»)
- [ ] Негативные witness-тесты расширены на контрактный сценарий
- [ ] Wire-format bump (если вариант A) — `WITNESS_FORMAT_VERSION=2` + golden vectors обновлены
- [ ] `cargo test --workspace` green; clippy green
- [ ] Коммит `[BF-S3-P03] stateless: explicit contract-block semantics (+ VM coverage / typed reject)`

---

### BF-S3-P04. Оптимизация build_witness_for_block (устранение O(N) full-replay)

**Цель:** убрать документированную стоимость «pre-state comes from replay — rebuilds chain[..height]» в `BlockchainFacade::build_witness_for_block`: на высоте 100k+ блоков построение witness'а требует полного replay всей цепочки — light-client-фича на практике непригодна для глубоких блоков. Минимальная цель — windowed reconstruction: восстановить pre-state на height-1 из `state_history` CF (touched-дельты в retention-окне) + checkpoint (snapshot), вместо replay с нуля.

**Контекст:** module-docs `witness.rs` честно фиксируют: «The `state_history` CF (touched deltas only) cannot rebuild the leaf set and is therefore not an optimization path» — SMT-пруф охватывает все листья, значит нужен полный pre-state. Полный pre-state = current state + применение обратно всех state_history-дельт после height-1 (для адресов в окне) — это дешёвле replay в `retention_blocks` раз (default 5000), но не O(1). Альтернативы: (1) windowed reconstruct из state_history (требует archival-режим для глубины > retention); (2) incremental witness-cache на ноде (хранить последний pre-state SMT — нода и так держит current state; witness для свежих блоков = current state, для старых — из history); (3) оставить replay как documented fallback. Решение: (2) для «свежих» блоков (height ≥ tip − retention) — O(1) через current state + дельты; (1) для глубокой истории — только на archival; replay — последний fallback. Все три пути задокументировать.

**Задачи:**

1. `src/blockchain/blockchain_facade.rs`: `build_witness_for_block(height)` — каскад: (a) height == tip → witness из current state; (b) tip − retention ≤ height < tip → reconstruct из `state_history`-дельт (обратное применение к current state для touched-адресов) + current state для остальных адресов... **аккуратно**: witness-пруф требует полного leaf-set — использовать current state как базу leaf-set'а и «откатить» затронутые в (height..tip] адресах значения из state_history; адреса, созданные после height (нет в pre-state), исключить (доказывается: их нет в state_history на height); (c) иначе → полный replay (fallback, документировать стоимость). Каждый путь — unit-тест.
2. Инвариант-тест: witness, построенный всеми тремя путями для одного height, **байт-в-байт идентичен** (детерминизм; canonical codec это позволяет) — сравнение с replay-эталоном на цепочке из 300 блоков, retention 100.
3. Обновить module-docs `witness.rs` и facade: убрать «not an optimization path», описать каскад и границы (глубина > retention только archival).
4. Опционально (если trivial): `get_witness(height)` кэш в LRU `CachedEngine`-слое.

**Артефакты:**
- `src/blockchain/blockchain_facade.rs` (каскад + тесты)
- `crates/strangecoin-core/src/state/witness.rs` (doc-обновление)
- `tests/` (witness-path-equivalence тест)

**КГ:**
- [ ] Fresh-block witness — O(изменённых адресов), не O(N) replay (micro-assert через counting-mock — прецедент per_block_writes)
- [ ] Byte-identical equivalence трёх путей — тест зелёный
- [ ] Границы (архival-only глубокая история) задокументированы
- [ ] Существующие e2e light-client тесты зелёные без изменений
- [ ] `cargo test --workspace` green; clippy green
- [ ] Коммит `[BF-S3-P04] stateless: witness build cascade (fresh/windowed/replay fallback)`

---

### BF-S3-P05. Закрытие release-верификации: STAGE3_SUMMARY §7.1 + починка release.yml

**Цель:** закрыть незакрытое обязательство S3-P15: §7.1 «Verification follow-up» в `docs/stage3/STAGE3_SUMMARY.md` — **пустой плейсхолдер**: тег перепривязан, два release-прогона (#12, #13) молча упали на macOS (objc2 0.3.0-beta.3), это обнаружилось через 3 недели («silent failure» — признано в `docs/release-retag-v3.0.0-stage3.md`). Заполнить §7.1 фактическими данными последнего прогона и устранить первопричины хрупкости `release.yml`.

**Контекст:** «done» без verified release-прогона противоречит процессным правилам (не-локально-верифицируемое должно быть помечено и затем закрыто follow-up-коммитом — прецедент S2-P24 `db116f9`). Известные проблемы: (a) macOS-сборка падала из-за beta-зависимости objc2; (b) падение не создало GitHub Release и не сигнализировало; (c) matrix без allow_failure на падающей платформе блокировала весь релиз.

**Задачи:**

1. Верифицировать последний release-прогон тега `v3.0.0-stage3` через GitHub REST API (actions runs for `release.yml`, ref tag): run id, conclusion, артефакты (6 бинарей + SHA256SUMS.txt + cosign + SLSA provenance), наличие GitHub Release. Результат записать в §7.1 STAGE3_SUMMARY (фактический follow-up-коммит — прецедент S2-P24). Если прогон **не** зелёный — чинить release.yml до зелёного (пункт 2), не фиксировать красный как «done».
2. `release.yml` (`GITHUB-P24` из next-steps retag-дока): (a) убрать/зафиксировать beta-версию `objc2` (виновник macOS-падения — origin: UI-крейт eframe-стек; pin или перейти на stable); (b) `continue-on-error: true` для экспериментальных matrix-ячеек (macOS) с явной пометкой в release notes, либо полный матричный гейт; (c) failure-notification (workflow-шаг/issue-comment при падении) — устранить «silent failure»; (d) правило «не перетегивать существующий stage-тег» — внести в job-guard (fail, если tag уже существует с другим target).
3. `docs/release-retag-v3.0.0-stage3.md` — переложить в `docs/stage3/` (или сохранить, но добавить cross-link из STAGE3_SUMMARY §7.1): retag — документированное отступление, его причина и последствия должны быть видны из summary.
4. Обновить `Changelog.md`/`AGENTS.md`: примечание «v3.0.0-stage3 re-tagged 2026-11-19 → 23f4641 (release pipeline fix), оригинальный target 89dd756».

**Артефакты:**
- `docs/stage3/STAGE3_SUMMARY.md` §7.1 (заполнен фактическим прогоном)
- `.github/workflows/release.yml` (фиксы)
- `docs/release-retag-v3.0.0-stage3.md` (cross-link) / `Changelog.md` / `AGENTS.md`

**КГ:**
- [ ] §7.1 заполнен: run id, conclusion, список артефактов, дата — с evidence (URL прогона)
- [ ] GitHub Release существует и помечен stable; SHA256SUMS + cosign + SLSA на месте (или явная пометка «не верифицировано + причина»)
- [ ] release.yml: beta-dep objc2 зафиксирован/заменён; macOS-ячейка не молчит при падении; guard против повторного retag
- [ ] Retag-отступление cross-linked из STAGE3_SUMMARY
- [ ] Коммит `[BF-S3-P05] release: §7.1 verification follow-up + release.yml hardening`

---

### BF-S3-P06. Гигиена тега v3.0.0-stage3 (соответствие gate-семантике §3 prompt-stage3)

**Цель:** привести тег `v3.0.0-stage3` в соответствие с его семантикой жёсткого гейта: тег должен указывать на коммит, реализующий **только** Stage 3 (S3-P01..S3-P15). Фактически (verified `docs/release-retag-v3.0.0-stage3.md`): тег удалён и перепривязан на master HEAD `23f4641` (2026-11-19), который содержит коммиты **Stage 4 extension** (UI-P02-C — fixes 18 Nov, crates/strangecoin-ui, strangecoin-mcp, ADR-0028..0031). Это нарушает: (a) §3 prompt-stage3 гейт 7 («S3-P15 = Gate Stage 4/5» — Stage 4 код не должен быть «до» тега); (b) §8.3 hard gate #1 расширения («ни один AI-P##/UI-P## не начинается до тега v3.0.0-stage3» — код UI-P02-C фактически попал в тег Stage 3); (c) воспроизводимость: «Stage 3 complete» больше не соответствует замкнутому набору коммитов.

**Контекст:** перетегивание было вынужденным (release-pipeline падал на старом теге), но решение выбрано в пользу «двигать тег вперёд», а не «чинить сборку на исходном коммите». Теперь есть два корректных варианта — выбрать один и зафиксировать. Вариант A (строгий): переместить тег обратно на подлинный S3-P15-коммит, починив сборку на нём (objc2/eframe-фиксы cherry-pick'ом как release-only hotfix, без Stage 4-фич в дереве тега). Вариант B (прагматичный): оставить тег на `23f4641`, но формально закрыть Stage 4-утечки — задокументировать в STAGE3_SUMMARY §1/§7 и AGENTS.md, что тег включает Stage 4 extension-коммиты (UI-P02-C), и амнистировать нарушение §8.3 #1 задним числом (Stage 4 extension code был design-обоснован и не ломает Stage 3 инварианты 26/26 — regression это подтверждает).

**Задачи:**

1. Определить состав: `git log` между S3-P15-коммитом и `23f4641` — перечислить все Stage 4-коммиты, попавшие в тег (ожидаемо: UI-P02 + mcp_stdio + dependabot-bump wasmtime 50.0.1). Зафиксировать список в документе решения.
2. Выбрать вариант A или B (рекомендация: B — если Stage 4-коммиты уже используются и перемотка тега сломает внешние ссылки на Release/assets; A — если Release ещё не публиковался широко). Решение зафиксировать в `docs/stage3/TAG_RESOLUTION.md` (или в retag-доке).
3. Для варианта B: обновить STAGE3_SUMMARY §1 (сноска у тега), §7, AGENTS.md §Stage-3-complete; в §8.3-расширении — пометка «gate #1 амнистирован для UI-P02-C, прецедент: retag 2026-11-19».
4. Для варианта A: пересобрать/перепроверить release-пайплайн на исходном S3-P15-коммите (после BF-S3-P05), перетегировать, подтверждение `git ls-remote --tags origin`, заполнить §7.1 данными нового прогона.
5. В обоих вариантах: правило «stage-тег — иммутабелен; hotfix только через новый тег v3.0.1» — внести в AGENTS.md Common Gotchas (предохранитель на будущее; ретроспективно ретаг уже случился).

**Артефакты:**
- `docs/stage3/TAG_RESOLUTION.md` (или расширение retag-дока)
- `docs/stage3/STAGE3_SUMMARY.md`, `AGENTS.md` (правки)
- (вариант A) новые тег + release-прогон

**КГ:**
- [ ] Список Stage 4-коммитов, попавших в тег, зафиксирован с хешами
- [ ] Вариант A/B выбран и обоснован в документе решения
- [ ] STAGE3_SUMMARY + AGENTS.md согласованы с фактическим target тега
- [ ] Правило иммутабельности stage-тегов внесено в AGENTS.md
- [ ] Коммит `[BF-S3-P06] docs: stage3 tag hygiene (retag resolution, gate-амнистия/исправление)`

---

### BF-S3-P07. Pin Policy: зафиксировать `=` для всех deps storage-крейта

**Цель:** закрыть нарушение процессного правила §5.8 prompt-stage3: «Прочие новые deps (lru, criterion dev) — pinned тоже, если входят в storage-крейт». Фактически в `crates/strangecoin-storage/Cargo.toml` pinned через `=` только `rocksdb =0.25.0`, `lru =0.19.0`, `tempfile =3.27.0`, `proptest =1.11.0`, а **не** pinned: `thiserror = "2"`, `tracing = "0.1"`, `serde = "1.0"`, `blake3 = "1"`, `hex = "0.4"`, `serde_json = "1.0"`. blake3 и serde используются в snapshot-пути (checksum + manifest) — data-layout-чувствительные зависимости по духу Pin Policy Stage 3.

**Задачи:**

1. `crates/strangecoin-storage/Cargo.toml`: все 6 unpinned deps → `=x.y.z` по фактической резолюции `Cargo.lock`; каждая строка — комментарий «resolved YYYY-MM-DD → x.y.z; bumps через SCIP после mainnet freeze» (прецедент строк rocksdb/lru).
2. Проверить, что `Cargo.lock` не меняет версии (pin по lock, не upgrade): `cargo update --dry-run` — никаких непрошенных апгрейдов storage-депсов.
3. `AGENTS.md` §Stage 3 Pin Policy — дополнить полный список; sanity: `cargo tree -p strangecoin-storage` соответствует lock.
4. Deviation-тест в CI (опционально): скрипт/grep-check, что все `[dependencies]` storage-крейта имеют `=` (предохранитель на будущие deps).

**Артефакты:**
- `crates/strangecoin-storage/Cargo.toml`
- `AGENTS.md` (Pin Policy секция)

**КГ:**
- [ ] Все deps storage-крейта pinned `=`; версии соответствуют существующему Cargo.lock (dry-run без изменений)
- [ ] AGENTS.md Pin Policy обновлён полным списком
- [ ] `cargo test -p strangecoin-storage` green; clippy green
- [ ] Коммит `[BF-S3-P07] deps: pin policy compliance for strangecoin-storage`

---

## 3. Чего НЕ трогать (anti-goals исправлений)

- **Не расширять scope Stage 3**: Verkle, state expiry (удаление), receipts/contract-storage prune, Block-STM, wire snapshot/witness, SDK/MCP/ACP/UI — остаются Stage 4+/SCIP residuals (BF-S3-P03 только делает семантику явной, не реализует VM-верификацию, если выбран вариант B).
- **Не менять консенсус-правила**: все исправления — implementation-detail (storage/witness/release-инфраструктура); witness codec bump (BF-S3-P02/P03 если вариант A) — pre-mainnet breaking, фиксировать в Changelog 3.0.x.
- **Не переписывать то, что уже качественно реализовано**: pruning.rs (S3-P12) и snapshot verify (S3-P13) — образцовые; изменения только там, где указано в задачах.
- **Не ронять регрессию**: после каждого промпта — 101 suite / 912 passed baseline не ухудшается; новые тесты только добавляются.
- **Не перетегивать stage-тег без документа решения** (BF-S3-P06 — последний допустимый ретаг-прецедент).

---

## 4. Связь с другими документами

- `analytics/prompt-stage3.md` — первоисточник промптов S3-P01..S3-P15 и процессных правил (§5), на которые ссылаются КГ выше.
- `docs/stage3/STAGE3_SUMMARY.md` — DoD-таблица и deviations; BF-S3-P01/P05/P06 закрывают её открытые пункты.
- `docs/release-retag-v3.0.0-stage3.md` — документ ретега; исходник для BF-S3-P05/P06.
- `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` — residuals; BF-S3-P03 добавляет/уточняет пункт о stateless VM-покрытии.
- `AGENTS.md` — Pin Policy, Common Gotchas (BF-S3-P01/P06/P07).
