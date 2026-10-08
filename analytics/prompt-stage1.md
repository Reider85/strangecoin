# prompt-stage1.md — Промпты для реализации Stage 1 (Криптоядро + Verkle Trie)

**Версия:** 1.0
**Дата:** 2026-09-27
**Источник:** `ROADMAP3.md` (Этап 1), `ARCHITECT3.md` (§3, §5, §8, §9, §10.2, §11, §14), `retro-stage0.md` (§4, §5, §8)
**Предусловие:** ретроспектива Stage 0 (`retro-stage0.md`) выявила три невыполненных промпта — P19, P22, P26. Они оформлены как долговые промпты **D01–D03** и выполняются **до** Stage 1 proper, согласно правилу `ROADMAP3.md`: «Не переходить к Stage 1 до завершения Stage 0 Definition of Done».
**Цель:** перенести чистое ядро в крейт `strangecoin-core` (strangler), ввести Verkle Trie + state root, events bus, headers-first sync, bech32, tokio, декомпозировать `blockchain` на 5 компонентов — и закрыть при этом все три долга Stage 0. Каждый промпт — самостоятельное задание для ИИ-агента, завершающееся конкретными артефактами и чек-листом КГ.

---

## Связь с ретроспективой Stage 0 (обязательная reading)

`retro-stage0.md` зафиксировал итог: 22/26 промптов выполнено, DoD не достигнут, проект преждевременно объявлен завершённым. Три выявленных долга и их покрытие промптами этого документа:

| Долг (retro §5) | Суть | Промпт в этом документе |
|---|---|---|
| **P19** — интеграционные тесты | Каталога `tests/` нет; 6 тестов заперты в `main.rs`; отсутствуют double_spend, pow, time, emission, полный reorg | **D01** |
| **P22** — threat model + incident response | `THREAT_MODEL.md`/`INCIDENT_RESPONSE.md` не созданы; в Changelog — ложная запись о выполненном P22; генезис-ключ из публичной строки не зафиксирован как residual risk | **D02** |
| **P26** — DoD-верификация Stage 0 | Нет `docs/stage0/*`, тега, синхронности версий (0.8.6 vs «v1.0.0 complete»); release.yml дефектен и не запускался | **D03** |

Остальные долги из `retro-stage0.md` §8.2 распределены так: №3 (grant-механизм в консенсусном пути) → **S1-P01**; №4,5,6,7,8 (release.yml, версии, .gitignore, чистка репо) → **D03**; №9 (прогон consensus.tla через TLC) → **D02**.

Главный процессный урок retro §8.1, встроенный во все промпты ниже: **«done» объявляется только артефактами DoD-промпта (документы сверки + тег), а не ощущением завершённости последних коммитов. Запрещено обновлять Changelog/AGENTS.md/README о работе, для которой ещё нет коммита с файлами.**

---

## 0. Общий контекст (вставлять в начало каждого промпта)

```
Проект: Strangecoin, Rust, edition 2021, версия 0.8.6 → целевая 1.0.0 (синхронизация — D03).
Монолит: src/main.rs (~3452 строк) + модули src/{serialize,consensus,economics,mempool,
  network/{mod,protocol,rate_limiter},storage,config,cli,wallet,address,error}.rs.
  Скелеты-заглушки: src/{blockchain,api,gui,governance}/mod.rs.
Криптоядро (сделано в Stage 0):
  - secp256k1 recoverable ECDSA; verify_transaction принуждает sender == address(pubkey).
  - Каноническая сериализация blake3 (src/serialize.rs, 586 строк), txid = commitment
    от подписанных канонических байтов; golden-векторы.
  - Детерминированный генезис: genesis.json + EXPECTED_GENESIS_HASH + panic при mismatch;
    regtest — свой генезис (chain_id=3).
Консенсус (сделано в Stage 0): PoW U256 (hash <= target), retarget каждые 2016 блоков
  (clamp x4), median-time-past (окно 11) + запрет будущего (+2h), tail emission
  (src/economics/emission.rs), реконструкция балансов из цепочки в validate_chain.
Защита (сделано в Stage 0): MAX_MESSAGE/BLOCK/TX_SIZE + safe framing (проверка до
  аллокации), rate limiter per peer (100 msg/10s, ban), mempool с валидацией на insert
  (MAX_PENDING_TXS=10000, MempoolFull), RwLock<BlockchainInner> (порядок блокировок —
  шапка src/error.rs + deadlock-тест), graceful shutdown (AtomicBool + Drop + ctrlc).
Тесты: 48 #[test] внутри src/ (serialize 19, emission 10, proptest 9, rate_limiter 4,
  main.rs 6); каталога tests/ НЕТ — закрывается долговым промптом D01.
Concurrency: threads + std::sync::mpsc (tokio ВВОДИТСЯ на Stage 1, промпт S1-P10).
Известные долги (retro-stage0.md): P19 → D01, P22 → D02, P26 → D03; grant-механизм в
  консенсусном пути → S1-P01.
Целевая архитектура Stage 1 (ARCHITECT3.md §9, §10.2, ROADMAP3 Этап 1):
  - crates/strangecoin-core/ — чистое ядро (serialize, consensus, state, economics,
    governance), 0 I/O, тесты переносятся в crates/strangecoin-core/tests/.
  - Verkle Trie + block.state_root (инвариант #19: state.root_after(block) == block.state_root).
  - Merkle root транзакций в заголовке (SPV-ready).
  - Events bus (crossbeam, multi-subscriber): BlockApplied, BlockReorged, TxAccepted,
    TxRejected, MiningStarted/Finished, PeerScoreChanged, StatePersisted.
  - Headers-first sync: GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS, выбор вершины по cumulative work.
  - Mempool RBF: feerate, find_replaceable, Replaced(Vec<TxId>).
  - Tie-breaking: больше work → раньше timestamp → меньше hash; детерминировано.
  - tokio (постепенно: новые подсистемы async, старые threads живут до Stage 2).
  - network_id в генезисе и HELLO: mainnet=1, testnet=2, regtest=3; чужие пиры отбрасываются.
  - bech32-адреса: HRP sc1.../tsc1.../rsc1... (закрывает критическую проблему №6 ARCHITECT2).
  - Декомпозиция blockchain: chain_selector, block_executor, state_cache, blockchain_facade,
    consensus_manager (5 компонентов).
  - SyncEngine разрывает цикл network<->blockchain (inbox через mpsc).
  - governance: SCIP skeleton + consensus_version + activation height (инвариант #21).
Принципы:
  - Strangler pattern: монолит остаётся рабочим; подсистемы переносятся в крейт по одной,
    каждый перенос = отдельный промпт с полным прогоном тестов.
  - Чистое ядро: consensus/state/serialize/economics/governance в core — 0 I/O.
  - ADR пишется ДО того, как решение затронет код (ADR-0006..0010 на Stage 1).
  - Любое изменение консенсусных правил — через SCIP + activation height. Исключение для
    Stage 1: mainnet не запущен, цепочка локальная — прямые правки допустимы до тега
    v1.1.0-stage1, но каждая правка фиксируется в Changelog; если к моменту промпта
    существует синхронизированная внешняя цепочка — остановиться и оформить SCIP.
  - Валидация на входе (размеры, подписи, время, генезис, replay, rate) не ослабляется НИ
    одним промптом (DoD: «All Stage 0 invariants still enforced — no regressions»).
Anti-goal на Stage 1: WASM VM (Stage 1.5), Noise Protocol / Erlay / gossip-оптимизации
  (Stage 2), RocksDB (Stage 3 — только ADR-план), EIP-1559 (Stage 5), Account Abstraction
  (Stage 5), PoS (Stage 7), EncryptedMempool реализация (Stage 5, только placeholder),
  полный light client (позже — сейчас только API witness).
Стиль кода:
  - Не добавлять комментарии без необходимости.
  - После каждого промпта: cargo check и cargo test --workspace обязаны проходить.
  - Ничего не коммитить без явной просьбы; 1 промпт = 1 атомарный коммит, ID промпта в
    сообщении коммита, формат: «[S1-P05] governance: SCIP skeleton + consensus_version».
  - Логирование — только через tracing (structured fields); println! запрещён.
  - Секреты — только в keystore/env; в config не писать.
  - Запрещено обновлять Changelog/README/AGENTS.md о работе, для которой ещё нет коммита
    с файлами (урок retro §8.1).
Сокращения: КГ — критерии готовности; Долг — пункт из retro-stage0.md.
```

---

## 1. Карта зависимостей промптов

```
ДОЛГОВЫЙ ТРЕК (закрытие Stage 0, до любых промптов Stage 1 proper):

D01 (P19: tests/) ─────────────┐
                               ├──▶ D03 (P26: DoD-верификация + версия 1.0.0 + тег v1.0.0-stage0)
D02 (P22: threat model) ───────┘
D01 и D02 параллельны; D03 — последним: он тегирует Stage 0 и есть Gate для Stage 1.

STAGE 1 PROPER (старт только после тега v1.0.0-stage0):

S1-P01 (grant за regtest-флаг) ──▶ S1-P02 (workspace + serialize в core)
                                       │
                                       ├──▶ S1-P03 (consensus + economics в core) ──▶ S1-P04 (state.rs)
                                       │                                                    │
                                       ├──▶ S1-P05 (governance: SCIP + consensus_version) ──┤
                                       │                                                    │
                                       └──▶ S1-P06 (ADR-0006 + Verkle Trie + state_root) ───┤
                                                                                 │
                                                                                 ▼
                                                            S1-P07 (StateWitness)   S1-P08 (merkle root)

S1-P03 ──▶ S1-P09 (ADR-0009 + EventBus) ──▶ S1-P10 (ADR-0007 + tokio, постепенно)

S1-P04 + S1-P09 ──▶ S1-P11 (chain_selector + tie-breaking) ──▶ S1-P12 (block_executor + state_cache)
                                                                       │
S1-P05 ────────────────────────────────────────────────────────────────┤
                                                                       ▼
                                                     S1-P13 (facade + consensus_manager)

S1-P03 ──▶ S1-P14 (network_id) ──▶ S1-P15 (bech32)
S1-P13 + S1-P14 ──▶ S1-P16 (headers-first sync) ──▶ S1-P17 (mempool RBF)
S1-P10 + S1-P16 ──▶ S1-P18 (ADR-0010 + SyncEngine)

S1-P12..S1-P18 ──▶ S1-P19 (интеграционные тесты Stage 1 + первый fuzz-target)
                       │
                       ▼
                  S1-P20 (регрессия 22 инвариантов) ──▶ S1-P21 (threat model актуализация)
                                                            │
                                                            ▼
                                      S1-P22 (DoD-верификация Stage 1 + тег v1.1.0-stage1)
```

Параллельные треки (независимые линии):
- **Network-трек:** S1-P14 → S1-P15 может идти параллельно с декомпозицией blockchain (S1-P11..S1-P13) — оба требуют только S1-P03.
- **State-root трек:** S1-P06 → S1-P07 → S1-P08 параллелен с events/tokio (S1-P09, S1-P10).
- Долговый трек D01 ∥ D02 — параллелен между собой, но НЕ параллелен Stage 1 proper.

Жёсткие гейты:
1. **D03 = Gate Stage 1:** ни один промпт S1-PXX не начинается до тега `v1.0.0-stage0`.
2. **S1-P01 до S1-P04:** grant-санация обязательна до переноса state/consensus в core, иначе крейт унаследует обходной путь инварианта №6.
3. **ADR до кода:** S1-P06 без ADR-0006 не начинается; S1-P09 без ADR-0009; S1-P10 без ADR-0007; S1-P18 без ADR-0010.
4. **S1-P22 = Gate Stage 1.5:** WASM не начинается до тега `v1.1.0-stage1`.

---

## 2. Список промптов

### 2.0. Долговой трек — закрытие Stage 0 (D01–D03)

---

### D01. Закрытие долга P19: каталог tests/ — 7 сценариев + перенос 6 тестов из main.rs

**Цель:** выполнить P19 из `prompt-stage0.md` в полном объёме, который был пропущен в Stage 0 (retro §5.1). Смешанные в бинарнике интеграционные тесты вынести в изолированные файлы `tests/`, добавить пять отсутствующих сценариев. Это precondition для всего Stage 1: strangler-переносы в S1-P02..S1-P13 будут проверяться регрессией именно этих тестов.

**Контекст:** Каталога `tests/` не существует. 6 интеграционных тестов живут в `#[cfg(test)]` внутри `src/main.rs` (~3452 строки): `hundred_transactions_five_wallets`, `three_instances_receive_transfer`, `no_rollback_on_shorter_chain`, `real_network_three_nodes`, `real_network_fast_registration_race`, `deadlock_test_blockchain_wallet_lock_order`. Их нельзя запустить изолированно (`cargo test --test two_clients` не существует). Не покрыто: double_spend (второй tx с тем же nonce), pow (end-to-end майнинг на regtest), time (MTP/+2h через Node, а не только через validate_chain), emission (интеграционный сценарий «майним N блоков → сверяем coinbase»), полный reorg (unapply/apply более длинной цепочки — есть только частный случай «короткая не откатывается»).

**Задачи:**

1. Создать тестовую инфраструктуру `tests/common/mod.rs`:
   - хелперы: спавн узла на regtest (chain_id=3) во временной директории, random ports, детерминированныйgenesis для тестов;
   - ожидание события через polling баланса/height с таймаутом (никаких `sleep` > 1 сек);
   - гарантированный cleanup temp-dir и портов (Drop-гвардия).
2. Перенести 6 существующих тестов из `main.rs` с сохранением семантики:
   - `hundred_transactions_five_wallets` → `tests/two_clients.rs` (сценарий нагрузки);
   - `three_instances_receive_transfer` → `tests/network.rs`;
   - `no_rollback_on_shorter_chain` → `tests/reorg.rs` (как частный случай);
   - `real_network_three_nodes`, `real_network_fast_registration_race` → `tests/network.rs`;
   - `deadlock_test_blockchain_wallet_lock_order` → `tests/concurrency.rs` (сверх спеки P19 — допустимо).
3. Добавить отсутствующие 5 сценариев по спеке P19:
   - `tests/reorg.rs` — дополнить полным случаем: две цепочки A (height 10) и B (height 11), узел переключается на B, корректно unapply блоки A и apply B, балансы после reorg совпадают с реконструкцией из цепочки;
   - `tests/double_spend.rs` — отправитель шлёт две tx с одним nonce и разными receiver → вторая отклонена, баланс отправителя списан один раз;
   - `tests/pow.rs` — майнинг на regtest с low difficulty (target = 0xFF..FF), блок найден < 1 сек, проходит validate_chain на втором узле;
   - `tests/emission.rs` — майним 10 блоков, coinbase каждого = `block_reward_at_height(h, total_supply_before)`;
   - `tests/time.rs` — блок с `timestamp > now + 2h` → reject; блок с `timestamp <= mtp` → reject; валидный → accept.
4. Для каждого теста: regtest (chain_id=3), temp dir с cleanup, random ports, без `sleep` > 1 сек.
5. Убрать перенесённые тесты из `src/main.rs` (в монолите не остаётся интеграционных `#[cfg(test)]`-тестов — только юнит-тесты модулей).

**Артефакты:**
- `tests/common/mod.rs`
- `tests/{two_clients,reorg,double_spend,pow,emission,time,network,concurrency}.rs`
- `src/main.rs` (минус 6 перенесённых тестов)

**КГ (чек-лист):**
- [ ] Все 8 файлов в `tests/` существуют и проходят
- [ ] `cargo test --test two_clients` (и каждый прочий) запускается изолированно
- [ ] `cargo test --workspace` (или `cargo test`) проходит полностью, суммарно < 120 сек
- [ ] В `src/main.rs` не осталось интеграционных тестов
- [ ] Каждый тест не оставляет orphan-файлов и listening ports (проверка cleanup)
- [ ] В тестах нет `sleep` > 1 сек
- [ ] Коммит `[D01] tests: extract integration tests from main.rs + add 5 missing scenarios`

---

### D02. Закрытие долга P22: THREAT_MODEL.md + INCIDENT_RESPONSE.md + правда в Changelog

**Цель:** выполнить P22 из `prompt-stage0.md`, пропущенный в Stage 0 (retro §5.2), и заодно устранить самую неприятную находку ретро — ложную запись о выполненном P22 в Changelog (§4.4). Threat model превращает «код вроде работает» в прослеживаемую карту «вектор → промпт → код → тест».

**Контекст:** `docs/security/` содержит только BOUNTY.md, SECURITY.md, REPRODUCIBLE_BUILDS.md — THREAT_MODEL.md и INCIDENT_RESPONSE.md отсутствуют. `Changelog.md` содержит запись «P22: Threat Model (STRIDE) — Created docs/security/THREAT_MODEL.md…» — файл не существует, коммита P22 нет (запись внесена «наперёд»). DoD Stage 0 требует «25 векторов митигированы с Stage 0» — но без документа трассировки нет. Дополнительно (retro §4.3): приватный ключ генезиса выводится хэшированием публичной строки `"strangecoin-genesis-seed-2026"` — осознанный компромисс, зафиксированный только комментарием в коде. Требование retro §8.2-9: прогнать `docs/spec/consensus.tla` через TLC и зафиксировать результат.

**Задачи:**

1. **Исправить ложь в Changelog** (делается ПЕРВЫМ делом, до создания документов): запись о выполненном P22 заменить на честную — либо удалить, либо пометить «запись внесена ошибочно; P22 выполняется в D02 (см. prompt-stage1.md)». Правило retro §8.1: никаких записей о работе, для которой нет коммита.
2. Создать `docs/security/THREAT_MODEL.md`:
   - Введение: scope, assumptions, trust boundaries;
   - STRIDE-категории; таблица всех 25 векторов из `ARCHITECT3.md §6` (перенести + расширить): ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk, Monitoring;
   - Дополнить векторами, специфичными для фактического состояния Stage 0:
     - «генезисный ключ выводится из публичной строки» → Residual Risk + обязательство «offline key до mainnet freeze»;
     - «grant-блоки в консенсусном пути (`create_grant_block`, исключение `block.index != 1`, magic-строки sender)» → Residual Risk, закрытие запланировано в S1-P01;
     - «release pipeline ни разу не запускался — артефакты не воспроизводимы» → Residual Risk, закрытие в D03;
     - «testnet с low difficulty → 51% attack» (из спеки P22);
   - Mitigations map: вектор → промпт Stage 0 (P0X) → модуль/функция → тест. Для закрытых P19-сценариев указать тесты из D01; если теста нет — честно пометить «gap, закрывается D01»;
   - Residual risks: что НЕ закрыто (MEV — Stage 5, полный light client, offline genesis key).
3. Создать `docs/security/INCIDENT_RESPONSE.md` (~100–200 строк): источник alerts, роль responder'а, timeline disclosure (согласовать с 90-дневным disclosure из BOUNTY.md и 48h SLA из SECURITY.md).
4. Прогнать `docs/spec/consensus.tla` через TLC (tla2tools), зафиксировать результат в `docs/spec/README.md`: «свойства NoDoubleSpend/NoInflation/AllTxSigned/NonceMonotonic/ChainContinuity проверены: PASS/FAIL + дата». Если toolchain недоступен — зафиксировать причину и добавить в residual risks. Найденные контрпримеры включить в THREAT_MODEL.
5. Все mitigation-ссылки — на конкретные промпты (P04, P07, P12…) или ADR.

**Артефакты:**
- `docs/security/THREAT_MODEL.md` (25+ векторов)
- `docs/security/INCIDENT_RESPONSE.md`
- `Changelog.md` (ложная запись исправлена)
- `docs/spec/README.md` (результат прогона TLC)

**КГ (чек-лист):**
- [ ] Документ покрывает все 25 векторов из `ARCHITECT3.md §6` + ≥4 вектора из retro
- [ ] Каждый вектор имеет: ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk, Monitoring
- [ ] Mitigations ссылаются на конкретные промпты/ADR; отсутствующие тесты помечены как gap
- [ ] `Changelog.md` не содержит утверждений о несуществующих артефактах
- [ ] TLC прогнан (или невозможность зафиксирована с причиной) — результат в `docs/spec/README.md`
- [ ] Incident response содержит: alerts source, responder role, disclosure timeline
- [ ] Коммит `[D02] security: THREAT_MODEL.md + INCIDENT_RESPONSE.md + changelog correction`

---

### D03. Закрытие долга P26: DoD-верификация Stage 0 + синхронизация версий + тег v1.0.0-stage0

**Цель:** выполнить P26 из `prompt-stage0.md` — независимую сверку 12 критериев DoD — и устранить всё, что мешает честно поставить тег `v1.0.0-stage0` (retro §5.3, §4.5, §4.6, §8.2-4..8). Это **Gate Stage 1**: после этого промпта начинается S1-P01.

**Контекст:** `docs/stage0/` не существует, тега нет. Три источника противоречат друг другу: Cargo.toml = 0.8.6, Changelog содержит одновременно секции 1.0.0 и 0.8.6, AGENTS.md заявляет «v1.0.0, Stage 0 is complete» (обновлено за 15 часов до окончания фактической работы — retro §4.5). `release.yml` дефектен: build-job не объявляет `outputs.hashes` (SLSA-джоб получит пустой subject), 5 таргетов вместо 6 (нет windows-aarch64), workflow ни разу не запускался (нет тегов) — retro §4.6. `.gitignore` содержит `*.lock` и `Cargo.lock`, что противоречит reproducible builds. В корне мусор: test.md, .idea/ (в индексе, хотя в .gitignore), .codebuddy/, .opencodeignore. CI-бейджа в README нет — критерии DoD 7–8 остаются заявлениями (retro §8.2-6).

**Задачи:**

1. **Починить release.yml:**
   - build-job объявляет `outputs: hashes: ${{ steps.hash.outputs.hashes }}` (или чек-суммы собираются в отдельной джобе);
   - добавить 6-й таргет `aarch64-pc-windows-msvc` (или явно изменить спеку с фиксацией в REPRODUCIBLE_BUILDS.md);
   - прогнать тестовый тег `v0.0.0-rc1`: убедиться, что pipeline запускается, cosign/SLSA работают, subject не пуст. Если доступ к GitHub Actions отсутствует — зафиксировать «не верифицировано» в STAGE0_SUMMARY (честная фиксация вместо молчания).
2. **Гигиена репозитория:** убрать `*.lock`/`Cargo.lock` из `.gitignore`; удалить из индекса test.md, .idea/, .codebuddy/, .opencodeignore (или дополнить .gitignore); добавить бейдж CI в README.
3. **Синхронизировать версии:** Cargo.toml 0.8.6 → 1.0.0; Changelog — единая секция 1.0.0 «sanitized prototype»; AGENTS.md — убрать «Stage 0 is complete» до фактического прохождения этого промпта (вернуть формулировку только после тега).
4. **Выполнить сверку P26** — создать:
   - `docs/stage0/CRITICAL_ISSUES_CLOSED.md` — таблица: 13 проблем из `ARCHITECT2.md §1.1` → промпт → статус (по retro §7: 9 закрыто, 3 частично, 1 не закрыто — отразить честно, включая «адреса без checksum → Stage 1, S1-P15»);
   - `docs/stage0/INVARIANTS_ENFORCED.md` — таблица: 22 инварианта из `ARCHITECT3.md §5` → где enforce → какой тест проверяет (инварианты 15/18/19/20/21 пометить как deferred на Stage 1);
   - `docs/stage0/STAGE0_SUMMARY.md` — что сделано, что перешло в Stage 1, явные обязательства: offline genesis key до mainnet freeze (из D02), bech32 (S1-P15), grant-санация (S1-P01), невыполненные пункты P24 (частично).
5. **Прогнать верификацию:** `cargo test` (включая tests/ из D01) и `cargo clippy -- -D warnings` — если toolchain недоступен локально, evidence = зелёный прогон CI (ссылка на run); без evidence критерии 7–8 не закрываются.
6. **Поставить аннотированный тег `v1.0.0-stage0`** — строго ПОСЛЕ прохождения всех пунктов КГ этого промпта. Тег — последний артефакт, не первый.

**Артефакты:**
- `docs/stage0/{CRITICAL_ISSUES_CLOSED,INVARIANTS_ENFORCED,STAGE0_SUMMARY}.md`
- `.github/workflows/release.yml` (исправлен)
- `.gitignore`, `README.md` (бейдж)
- `Cargo.toml`, `Changelog.md`, `AGENTS.md` (версии синхронизированы)
- Тег `v1.0.0-stage0`

**КГ (чек-лист):**
- [ ] release.yml содержит outputs.hashes и 6 таргетов; тестовый прогон выполнен или зафиксирован как «не верифицировано» с причиной
- [ ] Версия в Cargo.toml = версии в Changelog = версии в AGENTS.md = 1.0.0
- [ ] Все 13 критических проблем отмечены в CRITICAL_ISSUES_CLOSED.md со ссылками и честными статусами
- [ ] Все 22 инварианта отмечены в INVARIANTS_ENFORCED.md (модуль + тест); deferred — явно
- [ ] STAGE0_SUMMARY.md перечисляет обязательства Stage 1 (genesis key, bech32, grant)
- [ ] cargo test green (или evidence от CI)
- [ ] В репозитории нет test.md/.idea/.codebuddy в индексе; Cargo.lock трекается
- [ ] Тег `v1.0.0-stage0` поставлен ПОСЛЕ прохождения чек-листа (порядок коммитов подтверждает)
- [ ] Коммиты `[D03] ...` (допускается 2–3 атомарных коммита: fix release.yml / hygiene / dod-verification)

---

### 2.1. Подготовка ядра (S1-P01–S1-P05)

---

### S1-P01. Консенсусная санация: grant-механизм за regtest-флаг

**Цель:** устранить консенсусный долг retro §4.2 ДО переноса ядра в `strangecoin-core`. Крейт `strangecoin-core` не должен унаследовать обходной путь инварианта №6 («блок не содержит наград сверх эмиссии») — иначе чистое ядро будет чистым только по имени.

**Контекст:** Легаси-механизм `create_grant_block` (первичная эмиссия: 10000 с «initial_wallet_address» первому кошельку) — это `is_coinbase: true` без подписи, прямая мутация `balances` в обход mempool и эмиссионных правил; вызывается из production-кода GUI (main.rs:2126 на момент ретро) и миграций легаси-баз. Чтобы цепочка с grant-блоком проходила валидацию, в `validate_chain` захардкожено исключение `block.index != 1` (блок №1 выведен из проверки coinbase против эмиссии), а magic-строки `"genesis"`/`"coinbase"` в поле sender пропускают проверку баланса. Retro §8.2-3: «удалить или спрятать за regtest-only флаг; легаси-миграции БД — отдельно от правил консенсуса».

**Важно:** mainnet не запущен, цепочка локальная — прямая правка консенсусных правил допустима до тега `v1.1.0-stage1`. Если к моменту выполнения существует синхронизированная внешняя цепочка — ОСТАНОВИТЬСЯ и оформить SCIP-0001 (механизм появится в S1-P05).

**Задачи:**

1. Ввести в `Config` флаг `allow_grant_blocks` (bool, default = false; true допустим только для regtest и утилит легаси-миграции БД).
2. В `validate_chain`: исключение `block.index != 1` и пропуск magic-строк `"genesis"`/`"coinbase"` в sender — выполняются ТОЛЬКО при `allow_grant_blocks = true`. При false — блок №1 валидируется по общим правилам (coinbase ≤ block_reward_at_height), баланс отправителя проверяется всегда.
3. Генезисный блок продолжит валидироваться по `EXPECTED_GENESIS_HASH` (якорь из P10) — это не зависит от флага и не меняется.
4. `create_grant_block` перевести за флаг: вызов из GUI/CLI при `allow_grant_blocks = false` → typed error (новый вариант `GrantBlocksDisabled` в error.rs) + warn-лог. Легаси-миграции БД — отдельный путь (утилита/feature), не консенсусный.
5. Убрать magic-строки из консенсусного пути: генезисные транзакции идентифицируются структурно (индекс блока + якорь генезиса), а не сравнением строк в sender.
6. Обновить/добавить тесты: блок с наградой сверх эмиссии на высоте 1 → reject при флаге false; grant-блок → accept при флаге true (regtest); magic-строка в sender обычного блока → reject.
7. Прогнать все тесты из D01 — регрессий нет.

**Артефакты:**
- `src/main.rs` (validate_chain, create_grant_block за флагом)
- `src/config.rs` (+ `allow_grant_blocks`)
- `src/error.rs` (+ вариант GrantBlocksDisabled)
- Тесты: `tests/emission.rs` / `tests/two_clients.rs` (дополнение)

**КГ (чек-лист):**
- [ ] При `allow_grant_blocks = false` ни один путь валидации не содержит исключений для grant/magic-строк
- [ ] `rg '"genesis"|"coinbase"' src/` — совпадения только внутри ветки флага (или 0)
- [ ] GUI/CLI вызов grant при флаге false возвращает typed error, не паникует
- [ ] Все тесты D01 green; добавлены 2–3 новых теста на флаг
- [ ] `cargo test` green; коммит `[S1-P01] consensus: gate grant blocks behind regtest-only flag`

---

### S1-P02. Cargo workspace + crates/strangecoin-core: перенос serialize (+ ADR-0008)

**Цель:** начать strangler-миграцию (ARCHITECT3 §10.2, ROADMAP3 Этап 1): создать workspace и крейт `strangecoin-core`, перенести в него самый чистый модуль — `serialize.rs`. Это первый перенос; каждый следующий — отдельный промпт с полным прогоном тестов.

**Контекст:** Сейчас всё живёт в бинарном крейте: src/serialize.rs (586 строк, каноническая кодировка blake3, txid = commitment, format_version, 19 тестов с golden-векторами). Целевая структура — ARCHITECT3 §9: `crates/strangecoin-core/` — 0 I/O, внешние зависимости только криптопримитивы. Также retro §4.7: не созданы заглушки `fee_market.rs` и `governance/scip.rs`, упомянутые в P02, — расхождение закрывается здесь и в S1-P05. ADR-0008 (RocksDB planning вместо redb) фиксируется сейчас, хотя миграция — Stage 3 (правило «ADR до кода»).

**Задачи:**

1. Корневой `Cargo.toml` → workspace: `members = [".", "crates/strangecoin-core"]`, включить `resolver = "2"`. Бинарный крейт корня остаётся основным продуктом.
2. Создать `crates/strangecoin-core/`:
   - `Cargo.toml`: зависимости — blake3, secp256k1 (только типы), thiserror; БЕЗ tokio, БЕЗ сетевых/файловых крейтов;
   - `src/lib.rs` — корень крейта.
3. Перенести `src/serialize.rs` → `crates/strangecoin-core/src/serialize.rs`:
   - 19 unit-тестов переносятся вместе с модулем;
   - golden-векторы — в `crates/strangecoin-core/tests/serialize_golden.rs` (интеграционный тест крейта);
   - `format_version` и все публичные типы — реэкспорт из `strangecoin_core::serialize`.
4. `src/main.rs` и все потребители — `use strangecoin_core::serialize::...`; старый `src/serialize.rs` удалить.
5. Создать заглушки в core: `src/economics/fee_market.rs` (пустой, TODO Stage 5) — закрывает расхождение P02 из retro §4.7.
6. Написать `docs/ADR/0008-rocksdb-plan.md` (planning): Context (rusty-leveldb — временный, ограничения), Decision (RocksDB на Stage 3, миграция через migrations.rs), Alternatives (redb — отвергнут: battle-tested-преимущество RocksDB в Bitcoin Core/reth; sled — отвергнут: статус проекта), Consequences (план миграции на Stage 3).
7. Убедиться, что CI (ci.yml) собирает workspace целиком (`cargo test --all` уже в спеке P20 — проверить, что workspace подхватился).

**Артефакты:**
- Корневой `Cargo.toml` (workspace)
- `crates/strangecoin-core/{Cargo.toml,src/lib.rs,src/serialize.rs,src/economics/fee_market.rs}`
- `crates/strangecoin-core/tests/serialize_golden.rs`
- `docs/ADR/0008-rocksdb-plan.md`
- `src/main.rs` (использует крейт), удалённый `src/serialize.rs`

**КГ (чек-лист):**
- [ ] `cargo build --workspace` и `cargo test --workspace` green
- [ ] Golden-векторы проходят из крейта (`cargo test -p strangecoin-core`)
- [ ] В корневом src/ нет serialize-кода (дубляжа нет)
- [ ] `crates/strangecoin-core` не тянет tokio/сеть/файлы: `rg "tokio|std::net|std::fs" crates/strangecoin-core/src` → 0
- [ ] ADR-0008 существует с Alternatives-секцией
- [ ] Коммит `[S1-P02] core: create strangecoin-core crate, move serialize (+ADR-0008)`

---

### S1-P03. Перенос consensus + economics в core

**Цель:** продолжить strangler (ROADMAP3 Этап 1, задача «Перенести consensus.rs, economics/»): правила валидности и эмиссия — чистые функции без I/O — переезжают в `strangecoin-core`. Монолит остаётся оркестратором.

**Контекст:** Сейчас правила размазаны по `src/main.rs` (validate_difficulty, U256-математика, retarget, MTP-11/+2h, проверки nonce/chain_id, validate_chain) и `src/economics/emission.rs` (block_reward_at_height, tail emission, 10 тестов). Anti-goal ARCHITECT3 §15: консенсусные константы — `pub const` в одном месте, никакого lazy_static/once_cell; тесты подменяют параметры через dependency injection, не через глобальное состояние. После S1-P01 grant-исключения уже за флагом — переносится чистая логика.

**Задачи:**

1. Создать `crates/strangecoin-core/src/consensus.rs`:
   - константы: RETARGET_INTERVAL, CLAMP_FACTOR, MTP_WINDOW, MAX_FUTURE_TIME, CHAIN_ID-константы сетей (mainnet=1, testnet=2, regtest=3) — единый источник (пригодится S1-P14);
   - чистые функции: `validate_difficulty`, `calculate_next_target` (retarget), `validate_block_time` (MTP + future), правила nonce/chain_id;
   - `validate_chain` НЕ переносится целиком — она зависит от state/storage; в core переезжают чистые проверки-компоненты (монолит соберёт их обратно до S1-P12).
2. Перенести `src/economics/emission.rs` → `crates/strangecoin-core/src/economics/emission.rs` (10 тестов вместе с модулем); `src/economics/mod.rs` корня — реэкспорт.
3. Перенести `src/consensus/proptest.rs` (9 property-тестов) → `crates/strangecoin-core/tests/consensus_proptest.rs`.
4. main.rs/emission-потребители — `use strangecoin_core::...`; старые файлы удалить.
5. Проверить чистоту: в core нет std::fs/std::net/tokio/rusty-leveldb; рандом в тестах — только через proptest.

**Артефакты:**
- `crates/strangecoin-core/src/{consensus.rs,economics/emission.rs}`
- `crates/strangecoin-core/tests/consensus_proptest.rs`
- Обновлённые `src/main.rs`, `src/economics/mod.rs`

**КГ (чек-лист):**
- [ ] `cargo test --workspace` green (48+ тестов D01 не потеряны)
- [ ] `rg "std::fs|std::net|tokio|leveldb" crates/strangecoin-core/src` → 0 (0 I/O)
- [ ] Консенсусные константы — `pub const` в одном модуле, без lazy_static
- [ ] В корневом src/ нет дублей перенесённых функций
- [ ] Коммит `[S1-P03] core: move consensus rules + economics into strangecoin-core`

---

### S1-P04. state.rs: чистое исполнение apply_block/unapply_block в core

**Цель:** выделить детерминированное исполнение state (ARCHITECT3 §3.3) в `strangecoin-core/src/state.rs`: `apply_block`/`unapply_block` как чистые функции. Это precondition для Verkle Trie (S1-P06) и декомпозиции blockchain (S1-P11..S1-P12).

**Контекст:** Сейчас применение блоков и мутация `balances` живут внутри `Blockchain` в main.rs, перемешаны с I/O (LevelDB), блокировками и сетью. ARCHITECT3 §3.3: «чистые apply_block/unapply_block; balances — кэш, пересчитываемый из цепочки; результат исполнения готов стать Verkle root (S1-P06)». Инвариант №4: apply и unapply — обратные операции.

**Задачи:**

1. Создать `crates/strangecoin-core/src/state.rs`:
   - `State` — чистая структура (balances: HashMap<Address, u64>, nonces: HashMap<Address, u64>);
   - `apply_block(state: &State, block: &Block) -> Result<State, StrangecoinError>` — без мутаций входа (или явный `apply_block_mut`, выбрать один стиль и зафиксировать);
   - `unapply_block(state: &State, block: &Block) -> Result<State, _>` — точная обратная операция (восстановление балансов/nonce из пре-стейта блока);
   - coinbase/эмиссия применяются через `strangecoin_core::economics` (не дублируются).
2. Перенести чистую логику применения транзакций из main.rs в state.rs; всё, что касается LevelDB/блокировок/сети, остаётся в монолите.
3. Property-тест (добавить в core/tests): для произвольной последовательности валидных блоков `unapply(apply(s, b), b) == s` (инвариант №4).
4. Монолит переключён на `strangecoin_core::state` там, где применимо, без смены внешнего поведения; тесты D01 green.

**Артефакты:**
- `crates/strangecoin-core/src/state.rs`
- `crates/strangecoin-core/tests/state_roundtrip.rs`
- Обновлённый `src/main.rs`

**КГ (чек-лист):**
- [ ] apply_block/unapply_block в core, 0 I/O, без обращения к RwLock/DB
- [ ] Property-тест round-trip green (инвариант №4 enforce)
- [ ] `cargo test --workspace` green, регрессий D01 нет
- [ ] Коммит `[S1-P04] core: extract pure state apply/unapply`

---

### S1-P05. governance: SCIP skeleton + consensus_version + activation height

**Цель:** заложить механизм управляемых изменений консенсуса (инвариант №21, ROADMAP3 Governance track: «Stage 1: SCIP process skeleton; Stage 1: consensus_version + activation height mechanism»). После этого промпта любое изменение правил получает формальный контейнер.

**Контекст:** ARCHITECT3 §8: SCIP-процесс (Idea → Draft → Review → Activation → Finalized), формат документа (scip, title, status, consensus_version, activation_height, author, discussions, created); §8.2: `consensus_version` в заголовке блока, `activation_height` для каждого SCIP в consensus_rules, backward compatibility: узлы со старой версией принимают блоки до activation height, после — отвергают (hard fork). Retro §4.7: заглушка `governance/scip.rs` так и не была создана — закрыть здесь.

**Задачи:**

1. Создать `crates/strangecoin-core/src/governance/scip.rs`:
   - типы `ScipDocument` (поля формата §8.1), `ConsensusRules { consensus_version: u32, activations: BTreeMap<Height, ScipId>, ... }`;
   - `current_consensus_rules(height) -> ConsensusRules` — детерминированный выбор правил по высоте;
   - тесты: до activation_height — старые правила, после — новые, на границе — новый блок по новым правилам.
2. Добавить `consensus_version: u32` в заголовок блока:
   - обновить каноническую сериализацию (S1-P02 уже в core): **bump `format_version`**, обновить golden-векторы (это ломающее изменение кодировки — допустимо, mainnet не запущен; отметить в Changelog);
   - `validate_chain` проверяет `block.consensus_version` против `current_consensus_rules(height)` — версия ниже активной → reject.
3. Создать `docs/SCIP/README.md` + `docs/SCIP/scip-0000-process.md` — skeleton процесса (стадии, формат, кто ревьюит). Никаких содержательных SCIP пока не активировать — только механизм.
4. В `consensus.rs` — единый источник версии: `pub const CURRENT_CONSENSUS_VERSION: u32 = 1;`.
5. Тест на dummy-правило: регистрируем тестовый SCIP (изменение несущественного параметра), проверяем enforce по высоте (реальный fork — не сейчас).

**Артефакты:**
- `crates/strangecoin-core/src/governance/scip.rs`
- `crates/strangecoin-core/src/consensus.rs` (+ CURRENT_CONSENSUS_VERSION)
- Обновлённая сериализация + golden-векторы (format_version bump)
- `docs/SCIP/{README.md,scip-0000-process.md}`

**КГ (чек-лист):**
- [ ] `governance/scip.rs` существует (заглушка из retro §4.7 закрыта), 0 I/O
- [ ] Заголовок блока содержит consensus_version; сериализация обновлена, golden-векторы проходят
- [ ] Тест: блок с устаревшей consensus_version → reject
- [ ] Тест activation-height на dummy-правиле green
- [ ] `docs/SCIP/` создан; коммит `[S1-P05] core: governance skeleton — SCIP + consensus_version + activation height`

---

### 2.2. State root и Merkle (S1-P06–S1-P08)

---

### S1-P06. ADR-0006 + Verkle Trie + state_root в заголовке

**Цель:** реализовать Verkle Trie и ввести `block.state_root` — исполнение инварианта №19 (`state.root_after(block) == block.state_root`), ключевой новый элемент Stage 1 (ROADMAP3: «State root (Verkle Trie)»).

**Контекст:** ROADMAP3 Этап 1: «Реализовать Verkle Trie (или взять готовый crate, например verkle-trie); state.root_after(block) == block.state_root (инвариант #19); state.apply_block обновляет Verkle root; block.state_root поле в заголовке блока». Риск (ROADMAP3): «Verkle Trie implementation сложна — mitigation: взять готовый crate, иначе минимальная версия с property-based tests». Правило «ADR до кода»: ADR-0006 пишется первым пунктом этого промпта. Заметим: инвариант №19 в prompt-stage0 §5 был явно отложен «до Stage 1» — сейчас наступает его время.

**Задачи:**

1. Написать `docs/ADR/0006-verkle-trie-vs-smt.md`: Context (нужен state root для stateless validation и light-клиентов), Decision (Verkle Trie), Alternatives (SMT — отвергнут: reasoning; MPT — отвергнут: reasoning; готовый crate vs своя реализация — оценка), Consequences (witness-пробы, размер proof, производительность).
2. Выбрать путь: готовый crate (предпочтительно, если зрелый) или минимальная собственная реализация в `crates/strangecoin-core/src/state/verkle.rs` с property-based тестами. Критерии выбора зафиксировать в ADR.
3. Интегрировать в state.rs (S1-P04):
   - `State` получает Verkle-представление ключей (адрес → значение);
   - `root_after(state, block) -> [u8; 32]` — чистая функция: корень после применения блока;
   - `apply_block` возвращает state, root которого детерминирован.
4. Добавить `state_root: [u8; 32]` в заголовок блока: bump `format_version`, обновить golden-векторы, отметить в Changelog (ломающее изменение, mainnet не запущен).
5. Enforce инварианта №19 в валидации блока: `root_after(parent_state, block) == block.state_root`, иначе typed error `StateRootMismatch` → reject.
6. Тесты: proptest «последовательность применённых блоков → корни совпадают»; tamper-тест «блок с неверным state_root → reject»; тест детерминизма (порядок применения tx внутри блока не влияет на root — блочный root считается по каноническому порядку tx).

**Артефакты:**
- `docs/ADR/0006-verkle-trie-vs-smt.md`
- `crates/strangecoin-core/src/state/verkle.rs` (или выбор crate в Cargo.toml)
- Обновлённые `state.rs`, сериализация, golden-векторы
- `crates/strangecoin-core/tests/state_root.rs`

**КГ (чек-лист):**
- [ ] ADR-0006 написан ДО кода, Alternatives заполнены
- [ ] `block.state_root` в заголовке; format_version bump; golden-векторы обновлены и green
- [ ] Инвариант №19 enforce: tamper state_root → reject (тест)
- [ ] Proptest: apply цепочки блоков → root совпадает на каждом шаге
- [ ] `cargo test --workspace` green; коммит `[S1-P06] core: Verkle Trie + block.state_root (ADR-0006)`

---

### S1-P07. StateWitness + API stateless validation

**Цель:** подготовить контур stateless-валидации (ARCHITECT3 §3.3): `StateWitness` позволяет верифицировать блок без полного state. Полный light client — не в этом промпте; фиксируются типы и API.

**Контекст:** ROADMAP3 Этап 1: «StateWitness для stateless validation (light-клиенты верифицируют блоки без полного state)». ARCHITECT3 §12: light client проверяет баланс через proof, а не через копию state. Witness строится на Verkle-пробах (S1-P06). Scope-контроль: только генерация/проверка witness на уровне ядра; сетевые сообщения и light-клиент — поздние стадии.

**Задачи:**

1. Типы в `crates/strangecoin-core/src/state/witness.rs`:
   - `StateWitness` — пробы Verkle для всех адресов, затронутых блоком (sender/receiver/coinbase);
   - `verify_block_stateless(parent_state_root, block, witness) -> Result<(), _>` — чистая функция: пересчитать post-state-root из parent root + witness + блока.
2. Генератор: `build_witness(full_state, block) -> StateWitness` (используется full-нодой при раздаче блоков).
3. Тесты: round-trip «build_witness → verify_block_stateless → Ok» на случайных блоках (proptest); tamper-тест «подменённый баланс в witness → Err»; тест «witness не содержит лишних адресов» (минимальность — не жёсткий инвариант, но регрессионный критерий).
4. Ничего не менять в сетевом протоколе и валидации full-ноды (та продолжает по полному state).

**Артефакты:**
- `crates/strangecoin-core/src/state/witness.rs`
- `crates/strangecoin-core/tests/witness.rs`

**КГ (чек-лист):**
- [ ] Типы + API в core, 0 I/O
- [ ] Proptest round-trip witness green; tamper → Err
- [ ] Full-node валидация не ослаблена (все тесты green)
- [ ] Коммит `[S1-P07] core: StateWitness + stateless verification API`

---

### S1-P08. Merkle root транзакций

**Цель:** добавить `tx_root` в заголовок блока — SPV-готовность (ROADMAP3 Этап 1: «Merkle root транзакций в каждом блоке (для SPV); merkle_root(transactions) -> [u8;32]»).

**Контекст:** Заголовок уже расширен consensus_version (S1-P05) и state_root (S1-P06) — tx_root встраивается тем же способом: bump format_version, обновление golden-векторов. Правило детерминизма: merkle строится по каноническому порядку транзакций блока (тот же порядок, что в сериализации); при нечётном числе узлов последний дублируется — правило фиксируется в спецификации и тестах (аналогично Bitcoin, но на blake3).

**Задачи:**

1. `crates/strangecoin-core/src/serialize.rs` (или consensus.rs): `merkle_root(txids: &[[u8; 32]]) -> [u8; 32]` — blake3-пары, дублирование последнего при нечётности, детерминированно.
2. Добавить `tx_root: [u8; 32]` в заголовок; bump format_version; golden-векторы; Changelog.
3. Enforce в валидации: `merkle_root(tx блока) == block.tx_root`, иначе `TxRootMismatch` → reject.
4. Тесты: векторы (1 tx, 2, 3, 7 tx — чёт/нечёт), tamper-тест (список tx подменён → reject), property-тест (перестановка tx → другой root, но оба валидны при пересчёте).

**Артефакты:**
- `crates/strangecoin-core/src/serialize.rs` (merkle_root) / `consensus.rs` (проверка)
- Обновлённая сериализация + golden-векторы
- `crates/strangecoin-core/tests/merkle.rs`

**КГ (чек-лист):**
- [ ] merkle_root детерминирован, векторы чёт/нечёт green
- [ ] tamper tx → reject (тест)
- [ ] format_version bump, golden-векторы обновлены
- [ ] `cargo test --workspace` green; коммит `[S1-P08] core: merkle tx root in block header`

---

### 2.3. События и async-рантайм (S1-P09–S1-P10)

---

### S1-P09. ADR-0009 + EventBus

**Цель:** ввести событийную шину (ARCHITECT3 §11) — замену «GUI сам читает Mutex» и разбросанным mpsc. Это инфраструктура детерминированных тестов Stage 1 (ожидание событий вместо sleep) и подписки GUI/метрик.

**Контекст:** ROADMAP3 Этап 1: «EventBus (crossbeam channel, multi-subscriber broadcast); события: BlockApplied, BlockReorged, TxAccepted, TxRejected, MiningStarted/Finished, PeerScoreChanged, StatePersisted; подписчики: GUI (перерисовка), метрики, тесты; никаких std::sync::mpsc для broadcast». ARCHITECT3 §11: публикация неблокирующая (try_send + drop slow subscriber или unbounded). Правило «ADR до кода»: ADR-0009 (crossbeam vs flume vs tokio::broadcast) — первым пунктом; выбор crossbeam позволяет ввести шину до tokio (S1-P10).

**Задачи:**

1. Написать `docs/ADR/0009-events-bus.md`: Context (разрозненные mpsc, GUI читает Mutex), Decision (crossbeam multi-subscriber), Alternatives (flume, tokio::broadcast — до введения tokio; после S1-P10 шина остаётся crossbeam — обосновать), Consequences.
2. Создать `src/events.rs` (монолит — шина живёт на стороне node, не в core):
   - `NodeEvent` enum: BlockApplied { height, hash }, BlockReorged { old_tip, new_tip }, TxAccepted { txid }, TxRejected { txid, reason }, MiningStarted, MiningFinished, PeerScoreChanged { peer, score }, StatePersisted { height };
   - `EventBus { subscribe() -> Receiver<NodeEvent>, publish(event) }` — неблокирующий publish.
3. Пробросить publish в ключевых точках main.rs: применение блока, reorg, приём/отклонение tx, майнинг, персист. Дублирующие mpsc-уведомления GUI — заменить подпиской на шину.
4. Подписчики: GUI (перерисовка по BlockApplied), тестовый хелпер в tests/common (ожидание конкретного события с таймаутом).
5. Тесты (DoD Этап 1): 3 подписчика получают все события; publish не блокируется при медленном подписчике (тест с каналом, который не читают); reorg генерирует BlockReorged.

**Артефакты:**
- `docs/ADR/0009-events-bus.md`
- `src/events.rs`
- Обновлённые `src/main.rs`, GUI-модуль, `tests/common/mod.rs`

**КГ (чек-лист):**
- [ ] ADR-0009 до кода; выбран crossbeam, обоснование зафиксировано
- [ ] Тест: 3 subscribers получают события (DoD-критерий Этап 1)
- [ ] Тест: медленный подписчик не блокирует publish
- [ ] `std::sync::mpsc` для broadcast не используется (rg по broadcast-путям)
- [ ] Тесты D01 переведены на event-wait где было polling (минимум network.rs)
- [ ] `cargo test --workspace` green; коммит `[S1-P09] node: EventBus (crossbeam) + ADR-0009`

---

### S1-P10. ADR-0007 + tokio (постепенная миграция)

**Цель:** ввести async-рантайм tokio (ROADMAP3 Этап 1: «Ввести tokio (заменяет threads + mpsc). Подготовка к Stage 2 (gossip, Noise, Erlay)») — но постепенно, по стратегии mitigations из ROADMAP3: новые подсистемы async, старые threads живут до Stage 2.

**Контекст:** ADR-0007 «Why tokio on Stage 1» числится в ROADMAP3 как required ADR. Anti-goal Stage 0 «tokio» на Stage 1 снимается официально. Правило «ADR до кода». Риск (ROADMAP3): «tokio миграция ломает существующие threads — mitigation: постепенный перенос, tokio::spawn для новых подсистем, старые threads работают до Stage 2».

**Задачи:**

1. Написать `docs/ADR/0007-tokio-on-stage-1.md`: Context (threads + mpcs не масштабируются на Stage 2: gossip, Noise, Erlay), Decision (tokio, постепенная миграция), Alternatives (async-std — отвергнут: экосистема; оставаться на threads — отвергнут: Stage 2), Consequences (block_on границы, spawn_blocking для sync-кода).
2. Добавить `tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "net"] }`.
3. `main()` → `#[tokio::main]`; runtime инициализируется до subscriber'а tracing.
4. Новые/переписываемые подсистемы — async (SyncEngine в S1-P18 будет первым полноценным); EventBus-подписчики GUI — на выделенном thread через `tokio::task::spawn_blocking` или блокирующий bridge (зафиксировать выбранный паттерн).
5. Существующие threads (mining loop, p2p accept, rate limiter) — НЕ трогать в этом промпте (кроме минимальных адаптеров на границе). Их перенос — Stage 2.
6. Graceful shutdown (P17) не должен деградировать: ctrlc → graceful stop под tokio; deadlock-тест и тесты D01 green.
7. Тест: узел стартует под tokio, обрабатывает блок, корректно гасится (расширить tests/two_clients.rs).

**Артефакты:**
- `docs/ADR/0007-tokio-on-stage-1.md`
- `Cargo.toml` (+tokio), `src/main.rs` (runtime)

**КГ (чек-лист):**
- [ ] ADR-0007 до кода
- [ ] Узел работает под tokio; graceful shutdown сохранён (тесты P17/D01 green)
- [ ] Ни одна sync-подсистема не сломана (полный прогон --workspace)
- [ ] Коммит `[S1-P10] node: introduce tokio runtime (ADR-0007), legacy threads intact`

---

### 2.4. Декомпозиция blockchain (S1-P11–S1-P13)

---

### S1-P11. chain_selector.rs: tip selection + tie-breaking rule

**Цель:** выделить из монолита выбор вершины и fork choice (первый компонент декомпозиции blockchain, ARCHITECT3 §3.4) и сделать tie-breaking детерминированным (ROADMAP3 Этап 1: «select_best(chains): больше work → раньше timestamp → меньше hash; детерминировано, нет гонок»).

**Контекст:** Сейчас tip selection/reorg-логика растворены в main.rs. ARCHITECT3 §3.4: chain_selector владеет `tip_height`, `tip_hash`, `total_work`; «chain_selector не знает про state/transactions — только заголовки и work». Сейчас откат возможен только по высоте (тест no_rollback_on_shorter_chain), а правило сравнения равновысотных цепочек не зафиксировано — это источник недетерминизма между узлами.

**Задачи:**

1. Создать `src/blockchain/chain_selector.rs` (заглушка P02 наконец наполняется):
   - `ChainSelector { tip_height, tip_hash, total_work }`;
   - `select_best(chains) -> ChainId` — лексикографическое правило: (1) больше cumulative work → (2) раньше timestamp родителя-форка (MTP-совместимое сравнение) → (3) меньше hash байтово. Правило зафиксировать в doc-комментарии и в `docs/spec/fork_choice.md` (краткая спека, ~50 строк).
   - reorg-логика: переход на лучшую цепочку = unapply/apply через block_executor (S1-P12) и state_cache; до его готовности — через существующие вызовы монолита.
2. Cumulative work: сумма работы по заголовкам (трудность PoW → work), считать при приёме заголовков/блоков, хранить на высоту.
3. Property-тест: перестановка порядка подачи цепочек в select_best не меняет результат; ties по work разрешаются детерминированно (DoD Этап 1: «Tie-breaking rule детерминирован (property-based test)»).
4. Интеграционный тест: две равновысотные цепочки → все узлы сходятся на одной и той же (детерминизм между узлами).
5. Обновить `tests/reorg.rs`: сценарий переключения по work (не только по высоте).

**Артефакты:**
- `src/blockchain/chain_selector.rs`
- `docs/spec/fork_choice.md`
- `crates/.../tests` — нет: компонент живёт в монолите; тесты в `tests/reorg.rs` + proptest

**КГ (чек-лист):**
- [ ] chain_selector не импортирует state/transactions (только заголовки и work)
- [ ] Property-тест детерминизма select_best green
- [ ] Равновысотные форки разрешаются одинаково на всех узлах (интеграционный тест)
- [ ] tests/reorg.rs расширен; весь прогон green
- [ ] Коммит `[S1-P11] blockchain: chain_selector + deterministic tie-breaking`

---

### S1-P12. block_executor.rs + state_cache.rs

**Цель:** выделить исполнение блоков и кэш состояния (компоненты 2–3 декомпозиции, ARCHITECT3 §3.4). block_executor — «validate + apply», state_cache — единственное место чтения балансов.

**Контекст:** ARCHITECT3 §3.4: block_executor — валидация (consensus.validate), исполнение (state.apply_block), coinbase reward; чистый, не знает про выбор вершины. state_cache — balances/nonces кэш + invalidation + пересчёт из цепочки при расхождении; «единственное место, где читаются балансы». Сейчас обе ответственности внутри Blockchain (RwLock<BlockchainInner>) в main.rs. Перенос опирается на core::state (S1-P04) и core::consensus (S1-P03).

**Задачи:**

1. Создать `src/blockchain/block_executor.rs`:
   - `validate_and_apply(parent_state, block) -> Result<NewState, _>` — собирает чистые проверки из core (difficulty, time, emission, consensus_version, state_root, tx_root) и применяет core::state::apply_block;
   - НЕ выбирает tip, НЕ пишет в DB (I/O остаётся на слое выше).
2. Создать `src/blockchain/state_cache.rs`:
   - кэш balances/nonces поверх storage; invalidation при reorg (unapply);
   - «пересчёт из цепочки при расхождении» — функция rebuild_from_chain (механизм уже есть в validate_chain Stage 0 — вынести и переиспользовать);
   - инвариант №1 («вся валидность — из цепочки») закрепить тестом: расхождение кэша и реконструкции → реконструкция выигрывает.
3. Переключить validate_chain/apply-путь монолита на block_executor; прямые мутации balances вне state_cache — удалить (контроль: rg мутаций).
4. Тесты: executor на валидных/невалидных блоках (матрица из D01-сценариев на уровне компонента); state_cache: invalidation при reorg; rebuild при tamper.

**Артефакты:**
- `src/blockchain/block_executor.rs`
- `src/blockchain/state_cache.rs`
- Обновлённый `src/main.rs` (Blockchain делегирует)

**КГ (чек-лист):**
- [ ] block_executor не знает про tip selection; state_cache — единственный читатель балансов
- [ ] Тест: rebuild-from-chain исправляет расхождение кэша (инвариант №1)
- [ ] Прямых мутаций balances вне state_cache нет (rg-контроль; gate: `tests/balances_gate.rs` + CI job `source-gates` — `rg '\.balances\.(insert|remove|get_mut)' src/ --glob '!state_cache.rs' --glob '!block_executor.rs'` и `rg '\.balances\s*=[^=]' src/ --glob '!state_cache.rs' --glob '!chain_selector.rs'` → 0; formalized 2026-10-09, BUG-S0-022)
- [ ] `cargo test --workspace` green; коммит `[S1-P12] blockchain: block_executor + state_cache`

---

### S1-P13. blockchain_facade.rs + consensus_manager.rs

**Цель:** завершить декомпозицию «4 + 1» (ARCHITECT3 §3.4, ROADMAP3 DoD: «Blockchain декомпозирован на 5 компонентов»): facade — единственная точка входа; consensus_manager — единственный, кто решает, какие правила применяются на данной высоте.

**Контекст:** ARCHITECT3 §3.4: blockchain_facade — публичный API (`add_block`, `apply_tx`, `get_balance`, `get_tip`), делегирует; владеет `RwLock<BlockchainInner>`; «facade — единственная точка входа для network/mempool/api/gui». consensus_manager — «единственный, кто решает, какие правила применяются на данной высоте», expose `current_consensus_rules(height)`. Механизм правил уже есть в core (S1-P05) — здесь он встраивается в исполнение.

**Задачи:**

1. Создать `src/blockchain/blockchain_facade.rs`: публичный API, делегирующий chain_selector/block_executor/state_cache; RwLock<BlockchainInner> переносится сюда; все вызовы network/mempool/api/gui — через facade.
2. Создать `src/blockchain/consensus_manager.rs`:
   - обёртка над `core::governance::current_consensus_rules(height)`;
   - block_executor получает правила ТОЛЬКО через consensus_manager (прямой вызов core-функции из executor — запретить);
   - подготовка к PoS-фазе: enum ConsensusPhase { Pow, Pos } на высоте (Pos — unreachable до Stage 7, но тип заведён).
3. Контроль декомпозиции: rg-аудит — нет прямых обращений network/mempool/api/gui к внутренностям blockchain вне facade; `src/blockchain/` содержит 5 компонентов + mod.rs.
4. Обновить интеграционные тесты на facade API (поведение не меняется).

**Артефакты:**
- `src/blockchain/{blockchain_facade.rs,consensus_manager.rs}`
- Обновлённые потребители (network/mempool/api/gui)

**КГ (чек-лист):**
- [ ] 5 компонентов в src/blockchain/ (4 + consensus_manager), mod.rs их связывает
- [ ] rg: ни один внешний модуль не трогает внутренности blockchain мимо facade
- [ ] consensus_manager — единственный источник правил для executor
- [ ] `wc -l src/blockchain/*.rs` — facade ≤ 400 строк; остальные компоненты 200–600 (consensus_manager — тонкая обёртка правил, допустимо <200)
- [ ] DoD Этап 1 «декомпозиция на 5 компонентов» — выполнен
- [ ] `cargo test --workspace` green; коммит `[S1-P13] blockchain: facade + consensus_manager (5-component split done)`

---

### 2.5. Сеть (S1-P14–S1-P18)

---

### S1-P14. network_id в генезисе и HELLO

**Цель:** изолировать сети друг от друга на уровне протокола (ARCHITECT3 §14, ROADMAP3 Этап 1: «network_id в генезисе и в HELLO: mainnet=1, testnet=2, regtest=3; пиры с чужим network_id отбрасываются»).

**Контекст:** Сейчас regtest отличается только chain_id=3 в генезисе (P10); HELLO не несёт идентификатора сети — узел regtest теоретически может соединиться с узлом другой сети и получить «валидный» для себя мусор. ARCHITECT3 §14: «пиры с чужим network_id отбрасываются до любых данных; все тестовые сценарии — на regtest с fake clock; chain_id в каждой транзакции для replay protection (инвариант #10)». Константы сетей уже сведены в core::consensus (S1-P03) — единый источник.

**Задачи:**

1. Добавить `network_id` в genesis.json (mainnet=1, testnet=2, regtest=3) и в структуру генезиса; пересчитать EXPECTED_GENESIS_HASH для каждой сети (genesis.json regtest обновляется; ломающее изменение — допустимо, mainnet не запущен).
2. `HELLO`-сообщение (network/protocol.rs) несёт `network_id`; узел при handshake сверяет со своей сетью: mismatch → disconnect + бан + warn-лог, ДО обработки любых других сообщений.
3. Согласовать chain_id транзакций и network_id (инвариант #10): цепочка regtest = network_id 3 = chain_id 3; проверку связности добавить в тесты.
4. Тест: узел A (regtest) и узел B с подменённым network_id в HELLO → B отброшен, синхронизации нет, ни один блок не обработан.

**Артефакты:**
- `genesis.json`, `src/main.rs` (генезис-структура)
- `src/network/protocol.rs` (HELLO + проверка)
- `tests/network.rs` (дополнение)

**КГ (чек-лист):**
- [ ] genesis.json содержит network_id; хэш пересчитан, panic-guard работает
- [ ] HELLO-проверка отбрасывает чужие сети до данных (тест)
- [ ] chain_id == network_id согласованы из единого источника констант
- [ ] `cargo test --workspace` green; коммит `[S1-P14] network: network_id in genesis + HELLO, drop foreign peers`

---

### S1-P15. bech32-адреса (HRP sc1/tsc1/rsc1)

**Цель:** закрыть критическую проблему №6 ARCHITECT2 (адреса без контрольной суммы), отложенную с Stage 0, — перевести `address_from_public_key` на bech32 (ROADMAP3 Этап 1: «address_from_public_key(pubkey) -> Address (bech32 с контрольной суммой); HRP: sc1.../tsc1.../rsc1...»).

**Контекст:** Stage 0 отступил от спеки P05: адрес — base64(pubkey) без checksum (retro §4.7, ARCHITECT2 §1.1 №6 «не закрыто»). Битая контрольная сумма = потеря средств при ручном вводе и вечный вектор спуфинга. Сеть не запущена — миграция форматов бесплатна: keystore хранит pubkey, адрес — производная. HRP выбирается по network_id (S1-P14): sc1/tsc1/rsc1.

**Задачи:**

1. В core (`crates/strangecoin-core/src/address.rs` — перенос и расширение `src/address.rs`): `encode_address(pubkey, network_id) -> String` (bech32, HRP по сети) и `decode_address(s) -> (Pubkey, NetworkId)`; checksum-ошибка → typed error.
2. Все точки создания адреса (wallet, GUI, CLI, genesis) — на новую функцию; base64-адреса вывести из кода (rg-контроль: base64-кодирования pubkey больше нет).
3. Миграция: легаси-DB с base64-адресами — при открытии пересчитывать адрес из pubkey (одноразовый путь миграции, лог info); genesis.json обновить на bech32 (hash пересчитать).
4. Тесты: round-trip encode/decode (DoD Этап 1: «bech32 addresses работают (test: round-trip)»); checksum-битый адрес → Err; чужой HRP → Err; интеграционный тест: transfer на bech32-адрес между двумя узлами.
5. Отметить закрытие проблемы №6 в `docs/stage0/CRITICAL_ISSUES_CLOSED.md` (обновить статус «отложено → закрыто на Stage 1, S1-P15»).

**Артефакты:**
- `crates/strangecoin-core/src/address.rs`
- Обновлённые wallet.rs, gui, cli, genesis.json
- `tests/two_clients.rs` (bech32-transfer)

**КГ (чек-лист):**
- [ ] Round-trip encode/decode green; битая checksum → typed error
- [ ] HRP соответствует network_id (rsc1... в regtest — тест)
- [ ] base64-pubkey-адресов в коде не осталось
- [ ] Интеграционный transfer на bech32 green
- [ ] CRITICAL_ISSUES_CLOSED.md обновлён; коммит `[S1-P15] core: bech32 addresses with network HRP`

---

### S1-P16. Headers-first sync

**Цель:** перевести синхронизацию на протокол «сначала заголовки» (ROADMAP3 Этап 1: «GET_HEADERS(from_height) → HEADERS(Vec<BlockHeader>); GET_BLOCKS(Vec<BlockHash>) → BLOCKS(Vec<Block>); выбор вершины по cumulative work»).

**Контекст:** Сейчас узел синхронизируется полными блоками (heavy): вершина выбирается по высоте, PoW заголовка проверяется только вместе с блоком. Headers-first позволяет: (1) дёшево узнать форму форков ДО загрузки тел, (2) валидировать PoW заголовков на лету, (3) готовит почву для SPV (tx_root из S1-P08 уже в заголовке). Выбор вершины — через chain_selector (S1-P11) по cumulative work. Безопасность: заголовок без PoW-проверки не продвигает счётчики пира (rate limiter из P13 продолжает работать).

**Задачи:**

1. Расширить `src/network/protocol.rs`: сообщения `GET_HEADERS { from_height }`, `HEADERS(Vec<BlockHeader>)`, `GET_BLOCKS(Vec<[u8;32]>)`, `BLOCKS(Vec<Block>)`; лимит батча (например, 2000 заголовков) + проверка размеров до аллокации (инвариант №7/16 — не ослаблять).
2. Логика на приёме: PoW-проверка каждого заголовка (core::consensus::validate_difficulty по заголовку), связность prev_hash; валидные заголовки — в header-cache; вершина по cumulative work через chain_selector.
3. Download-цикл: для лучшей ветки запрашивать блоки пачками (GET_BLOCKS), ограничение in-flight запросов на пира (защита от DoS), таймауты.
4. Тест (DoD Этап 1: «Headers-first sync работает (test: new node sync за разумное время)»): `tests/sync_headers.rs` — 3 узла, один майнит 20 блоков, новый узел синхронизируется через headers-first; балансы совпадают; форк из теста reorg разрешается через заголовки.
5. Инварианты: полный validate_chain для тел блоков — не ослаблен (заголовки только прокладывают маршрут).

**Артефакты:**
- `src/network/protocol.rs` (4 сообщения)
- `src/network/sync.rs` (download-цикл) или расширение Node
- `tests/sync_headers.rs`

**КГ (чек-лист):**
- [ ] GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS реализованы, батчи ограничены, safe framing сохранён
- [ ] Заголовок с плохим PoW → отброшен и не влияет на выбор вершины (тест)
- [ ] Новый узел синхронизируется headers-first (интеграционный тест)
- [ ] validate_chain не ослаблен (все тесты D01 green)
- [ ] `cargo test --workspace` green; коммит `[S1-P16] network: headers-first sync`

---

### S1-P17. Mempool RBF (Replace-By-Fee)

**Цель:** добавить замену транзакций в mempool (ROADMAP3 Этап 1: «feerate = fee / tx_weight (пока fee=0 — по размеру); find_replaceable, Replaced(Vec<TxId>) для анонса TxRejected»).

**Контекст:** Mempool из P14 валидирует insert (подпись/dup/nonce/chain_id/balance, MAX_PENDING_TXS=10000, MempoolFull), но не умеет заменять: застрявшая tx блокирует nonce до ручного вмешательства. ARCHITECT3 §3.5: «RBF: find_replaceable, Replaced(Vec<TxId>) для анонса; эвристика приоритизации по feerate (EIP-1559 на Stage 5, до этого fixed gas)». Fee-модели ещё нет (fee=0) — feerate считается по размеру сериализованной tx. Правила замены — упрощённый BIP-125: замена допустима только если новый feerate ≥ старый × (1 + Δ), Δ фиксируется константой; исходная tx и её зависимости удаляются, событие Replaced анонсируется.

**Задачи:**

1. В `src/mempool/mod.rs`: `feerate(tx) = fee / weight`, пока fee = 0 → feerate = 1/serialized_len (детерминированная прокси-метрика).
2. `find_replaceable(tx) -> Vec<TxId>` — конфликтующие tx (тот же sender + nonce) и их зависимости; правила замены: new feerate ≥ old × (1 + RBF_MIN_DELTA), константа RBF_MIN_DELTA в consensus-константах; число последовательных замен одной «цепочки» ограничено (анти-DoS, например MAX_RBF_REPLACEMENTS = 10).
3. Замена: удалить старые, вставить новую (все проверки insert — обязательны), событие `TxRejected { txid, reason: Replaced }` для каждой удалённой через EventBus (S1-P09) + анонс пира́м.
4. Тесты: замена с большим feerate → ok, TxRejected(Replaced) получен подписчиком; замена с меньшим feerate → отклонена; лимит замен; после замены майнится новая версия, старая исчезает из блоков; double_spend-тест из D01 не деградировал (RBF не открывает двойную трату: nonce-правило приоритетнее).

**Артефакты:**
- `src/mempool/mod.rs` (RBF)
- `tests/rbf.rs`

**КГ (чек-лист):**
- [ ] Правила замены детерминированы (feerate, delta, лимит — константы)
- [ ] Тест замены + TxRejected через EventBus green
- [ ] Анти-DoS: лимит последовательных замен работает (тест)
- [ ] double_spend/reorg-тесты green (регрессий нет)
- [ ] Коммит `[S1-P17] mempool: replace-by-fee with deterministic rules`

---

### S1-P18. ADR-0010 + SyncEngine: разрыв цикла network↔blockchain

**Цель:** построить SyncEngine — единственного потребителя входящих блоков (ROADMAP3 Этап 1: «SyncEngine владеет ссылками на BlockchainFacade и NetworkService; NetworkService не вызывает blockchain напрямую — только кладёт входящие блоки в SyncEngine.inbox (mpsc); SyncEngine — единственный, кто делает validate → apply → announce»).

**Контекст:** Сейчас network-обработчики вызывают blockchain напрямую — цикл зависимости network↔blockchain, гонки при одновременном приходе блоков из gossip и sync. ARCHITECT3 §3.6/§10.2. Правило «ADR до кода»: ADR-0010 (Sync engine architecture: inbox pattern, порядок обработки, backpressure). После S1-P10 (tokio) inbox — tokio mpsc; обработка — на facade (S1-P13).

**Задачи:**

1. Написать `docs/ADR/0010-sync-engine.md`: Context (прямой вызов blockchain из network, гонки), Decision (inbox pattern, единственный validate→apply→announce), Alternatives (прямые вызовы с локами — отвергнуто; отдельный worker на пира — отвергнуто: упорядочивание), Consequences (backpressure, приоритет HEADERS).
2. Создать `src/network/sync_engine.rs` (или `src/sync/mod.rs`):
   - `SyncEngine { inbox: tokio::sync::mpsc::Receiver<Incoming>, facade: Arc<BlockchainFacade>, bus: Arc<EventBus> }`;
   - `Incoming = NewBlock | NewHeaders | NewTx`; цикл: validate (через executor/consensus) → apply (facade) → announce (publish + gossip peers);
   - упорядочивание: обработка строго последовательная (single consumer) — детерминизм.
3. NetworkService: обработчики сообщений только кладут в inbox (send), НИКАКИХ вызовов facade/blockchain; rg-контроль: в network/ нет импорта внутренних типов blockchain (только facade-API и inbox).
4. Backpressure: bounded inbox; при переполнении — приоритет: дропать дубликаты, банить спамеров через rate_limiter; HEADERS обрабатываются до BLOCKS того же пира.
5. Тест: гонка — два пира одновременно шлют один блок и разные форки; финальное состояние узлов консистентно, событий BlockApplied/BlockReorged корректное число; `real_network_fast_registration_race` из D01 green.

**Артефакты:**
- `docs/ADR/0010-sync-engine.md`
- `src/network/sync_engine.rs`
- Обновлённый `src/network/mod.rs` (только inbox), `src/main.rs`

**КГ (чек-лист):**
- [ ] ADR-0010 до кода
- [ ] NetworkService не импортирует blockchain внутренности (rg-аудит — 0)
- [ ] SyncEngine — единственный validate→apply→announce; inbox bounded
- [ ] Гонки: интеграционный тест на параллельный приход green
- [ ] DoD Этап 1 «Sync engine разрывает цикл network↔blockchain» — выполнен
- [ ] `cargo test --workspace` green; коммит `[S1-P18] node: SyncEngine inbox (ADR-0010), network decoupled`

---

### 2.6. Верификация и закрытие Stage 1 (S1-P19–S1-P22)

---

### S1-P19. Интеграционные тесты Stage 1 + первый fuzz-target

**Цель:** покрыть новые подсистемы Stage 1 end-to-end (DoD Этап 1: tests на headers-first, events bus 3 subscribers, bech32 round-trip, tie-breaking proptest — часть уже создана в своих промптах; здесь — сводная матрица и недостающие сценарии) и запустить security-track требование «Stage 1+: Fuzzing (cargo-fuzz)» с одного фаззера на самом критичном входе.

**Контекст:** Тесты D01 проверяют поведение Stage 0 — они обязаны остаться зелёными (DoD: «All Stage 0 invariants still enforced — no regressions»). Новое поведение: state_root/tx_root/consensus_version в заголовке, headers-first, RBF, network_id, bech32, facade-декомпозиция, tokio. Сквозной security track (ROADMAP3): «Stage 1+: Fuzzing (cargo-fuzz)» — первый target: канонический десериализатор (safe framing уже покрыт юнит-тестами, но фаззер на parse — самое ценное вложение).

**Задачи:**

1. Матрица новых интеграционных тестов (что ещё не создано в S1-P06..S1-P18 — создать здесь):
   - `tests/state_root.rs` — end-to-end: цепочка с state_root проходит на втором узле, tamper → reject (если не создан в S1-P06);
   - `tests/events.rs` — 3 подписчика EventBus на живом узле (если не создан в S1-P09);
   - `tests/rbf.rs` — RBF-сценарии (если не создан в S1-P17);
   - `tests/network_id.rs` — изоляция сетей;
   - обновить `tests/two_clients.rs`/`reorg.rs` под facade API + bech32-адреса.
2. Сводный чек-лист соответствия DoD Этап 1 (тестовая часть): headers-first test, 3 subscribers, tie-breaking proptest, bech32 round-trip — отметить в таблице S1-P22.
3. Первый fuzz-target: `fuzz/fuzz_targets/canonical_decode.rs` (cargo-fuzz): произвольные байты → deserialize block/tx → не паникует, не аллоцирует сверх лимитов; прогон 10 минут, 0 crashes — зафиксировать в `docs/security/THREAT_MODEL.md` (обновление придёт в S1-P21, здесь — только результат в issue/заметке).
4. Прогнать ВЕСЬ набор: `cargo test --workspace` + каждый `--test X` изолированно; суммарное время < 180 сек.

**Артефакты:**
- `tests/{state_root,events,rbf,network_id}.rs` (недостающие), обновления existing
- `fuzz/fuzz_targets/canonical_decode.rs` + `fuzz/Cargo.toml`

**КГ (чек-лист):**
- [ ] Все тесты D01 green (регрессий нет)
- [ ] Каждый новый сценарий изолированно запускаем; суммарно < 180 сек
- [ ] Fuzz-target существует, 10-минутный прогон без crashes (или причина фиксации)
- [ ] `cargo test --workspace` green; коммит `[S1-P19] tests: stage1 integration matrix + first fuzz target`

---

### S1-P20. Регрессия инвариантов Stage 0 + регистрация новых

**Цель:** доказать, что strangler-миграция ничего не сломала (DoD Этап 1: «All Stage 0 invariants still enforced — no regressions») и зарегистрировать впервые enforce-нутые инварианты №19 (state root) и №21 (consensus versioning).

**Контекст:** `docs/stage0/INVARIANTS_ENFORCED.md` (создан в D03) фиксирует состояние на теге v1.0.0-stage0: инварианты 1–14, 16, 17, 22 — enforce; 15, 18, 20 — N/A по плану; 19, 21 — deferred до Stage 1. За Stage 1 заголовок блока изменился трижды (consensus_version, state_root, tx_root), balances переехали в state_cache, валидация — в block_executor: каждая из этих перестановок — потенциальный регресс. Общий DoD-критерий №2: «Все invariants enforced, включая новые».

**Задачи:**

1. Прогнать все 22 инварианта из `ARCHITECT3.md §5` по актуальному коду:
   - для каждого — где enforce теперь (модуль/функция могли переехать: balances → state_cache, правила → core::consensus) и какой тест проверяет;
   - инвариант №4 (apply/unapply обратны) — теперь state_roundtrip.rs; №1 (валидность из цепочки) — state_cache rebuild; №19, №21 — enforce впервые.
2. Обновить `docs/stage0/INVARIANTS_ENFORCED.md` или создать `docs/stage1/INVARIANTS_ENFORCED.md` (выбрать одно место истины — предпочтительна актуализация stage0-файла со столбцом «Stage 1 статус»; это living-документ, а не историческая справка).
3. Все 48+ тестов Stage 0 + тесты D01 + Stage 1 — green (`cargo test --workspace`).
4. rg-аудит: в core не появилось I/O; в network не появился прямой доступ к blockchain; magic-строки и grant-исключения не вернулись.

**Артефакты:**
- `docs/stage1/INVARIANTS_ENFORCED.md` (или обновлённый stage0-файл)
- Отчёт прогона (в STAGE1_SUMMARY — S1-P22)

**КГ (чек-лист):**
- [ ] Таблица 22 инвариантов актуальна: enforce-место + тест на каждый
- [ ] Инварианты №19, №21 переведены из deferred в enforced со ссылками на тесты
- [ ] Полный прогон green; rg-аудиты чистые
- [ ] Коммит `[S1-P20] docs: invariants re-audit after strangler migration`

---

### S1-P21. Threat model актуализация Stage 1

**Цель:** дополнить THREAT_MODEL.md векторами, привнесёнными Stage 1 (общий DoD-критерий №9: «threat model актуализирован, если Stage добавляет новые attack vectors»).

**Контекст:** THREAT_MODEL.md создан в D02 и отражает Stage 0. Stage 1 добавил поверхности атаки: заголовки принимаются до тел (headers-first poisoning), witness-пробы (spoofing), state_root/tx_root в заголовке (манипуляция), RBF (DoS через бесконечные замены), network_id (confusion/ downgrade), bech32 (HRP confusion), consensus_version (downgrade до activation height), SyncEngine inbox (backpressure abuse).

**Задачи:**

1. Дополнить `docs/security/THREAT_MODEL.md` (новые векторы с ID, продолжающие нумерацию):
   - headers-first poisoning: поток фальшивых заголовков с валидным PoW-«островом» → mitigated: PoW-проверка каждого заголовка + cumulative work + rate limiter (S1-P16);
   - state_root manipulation: блок с невалидным корнем → mitigated: инвариант №19 enforce (S1-P06);
   - witness spoofing: подделка проб → mitigated: verify_block_stateless tamper-тесты (S1-P07);
   - RBF fee-war DoS: бесконечные замены → mitigated: RBF_MIN_DELTA + MAX_RBF_REPLACEMENTS (S1-P17);
   - network downgrade/confusion: чужая сеть / старая consensus_version → mitigated: HELLO-check (S1-P14) + version enforce (S1-P05);
   - inbox flooding: переполнение SyncEngine.inbox → mitigated: bounded + приоритеты + rate limiter (S1-P18);
   - HRP confusion: перевод средств между сетями по человеческому фактору → mitigated: HRP в адресе (S1-P15).
2. Для каждого нового вектора: Mitigation → промпт S1-PXX → код → тест; Residual Risk и Monitoring заполнить.
3. Обновить Mitigations map (вектор → промпт): добавить секцию Stage 1.
4. Сверить с fuzz-результатом из S1-P19 (canonical_decode): добавить в Monitoring.

**Артефакты:**
- `docs/security/THREAT_MODEL.md` (секция Stage 1)
- `docs/security/INCIDENT_RESPONSE.md` (если новые alert-источники)

**КГ (чек-лист):**
- [ ] Все 7+ новых векторов задокументированы с Mitigation/Residual/Monitoring
- [ ] Каждый mitigation ссылается на промпт/тест Stage 1
- [ ] Документ self-review по чек-листу ARCHITECT3 §17 пройден
- [ ] Коммит `[S1-P21] security: threat model update for stage 1 surfaces`

---

### S1-P22. DoD-верификация Stage 1 + тег v1.1.0-stage1

**Цель:** финальная сверка Stage 1 по Definition of Done из ROADMAP3 (Этап 1) и честное закрытие стадии. Промпт не пишет функциональность — только верифицирует и тегирует. Урок retro §8.1 встроен в порядок действий: сначала артефакты, потом заявление.

**Контекст:** DoD Этап 1 (ROADMAP3) — 15 критериев. Ни один критерий не считается выполненным «по ощущению»: каждый подтверждается артефактом (файл/тест/ADR/тег). Это тот промпт, чьё отсутствие в Stage 0 привело к ложному «Stage 0 complete» (retro §4.5, §5.3).

**Задачи:**

1. Сверить все 15 критериев DoD Этап 1 (см. таблицу §4 этого документа):
   - strangecoin-core создан, чистые функции перенесены (evidence: rg-аудит 0 I/O + структура крейта);
   - Verkle Trie + state.root_after == block.state_root (тест);
   - headers-first sync (тест sync_headers.rs);
   - events bus 3 subscribers (тест);
   - tie-breaking детерминирован (proptest);
   - tokio введён (ADR-0007 + сборка);
   - network_id в HELLO (тест);
   - bech32 round-trip (тест);
   - декомпозиция 5 компонентов (rg-аудит facade);
   - sync engine разрывает цикл (rg-аудит network);
   - consensus_version + activation height (тесты S1-P05);
   - no regressions (S1-P20);
   - ADR-0006..0010 существуют;
   - Changelog: «1.1.0 — core extracted + Verkle + headers-first»;
   - STAGE1_SUMMARY.md создан.
2. Создать `docs/stage1/STAGE1_SUMMARY.md`: что сделано (по промптам S1-P01..S1-P21), что перешло дальше (Stage 1.5: VM trait — заготовка VmExecutor trait; Stage 2: перенос network в крейт), открытые обязательства (offline genesis key, fuzzing-расширение).
3. Заготовка для Stage 1.5: создать `crates/strangecoin-core/src/vm/traits.rs` с `trait VmExecutor` (без имплементации) — по ROADMAP3 Этап 1.5 «core зависит от trait, не от рантайма»; только объявление, никакой логики.
4. Обновить `Changelog.md` (после фактических артефактов, не до): секция 1.1.0; синхронизировать версию в Cargo.toml workspace.
5. Прогнать финальную проверку: `cargo test --workspace`, `cargo clippy -- -D warnings`, fuzz-target smoke.
6. Поставить аннотированный тег `v1.1.0-stage1` — ПОСЛЕДНИМ действием, после прохождения всех пунктов КГ.

**Артефакты:**
- `docs/stage1/STAGE1_SUMMARY.md`
- `crates/strangecoin-core/src/vm/traits.rs`
- `Changelog.md`, `Cargo.toml` (1.1.0)
- Тег `v1.1.0-stage1`

**КГ (чек-лист):**
- [ ] Все 15 критериев DoD Этап 1 отмечены с evidence (тест/файл/rg-аудит)
- [ ] STAGE1_SUMMARY.md описывает готово/отложено/обязательства
- [ ] VmExecutor trait объявлен (без реализации)
- [ ] Версия 1.1.0 синхронна во всех файлах
- [ ] cargo test / clippy / fuzz smoke — green
- [ ] Тег `v1.1.0-stage1` поставлен последним коммит-действием
- [ ] Коммит `[S1-P22] docs: stage1 dod verification + summary + tag`

---

## 3. Сводная таблица промптов

| ID | Заголовок | Ключевые артефакты | Зависимости |
|----|-----------|--------------------|-------------|
| D01 | Закрытие P19: tests/ (7 сценариев + перенос 6) | tests/*.rs (8 файлов), main.rs (минус тесты) | — |
| D02 | Закрытие P22: THREAT_MODEL + INCIDENT_RESPONSE + TLC | docs/security/*.md, Changelog fix, docs/spec/README.md | — (∥ D01) |
| D03 | Закрытие P26: DoD-верификация + версии + тег v1.0.0-stage0 | docs/stage0/*.md, release.yml fix, тег | D01, D02 |
| S1-P01 | Grant-механизм за regtest-флаг | main.rs, config.rs, error.rs | D03 |
| S1-P02 | Workspace + strangecoin-core: serialize + ADR-0008 | crates/strangecoin-core/*, ADR-0008 | S1-P01 |
| S1-P03 | consensus + economics в core | core/{consensus.rs,economics/} | S1-P02 |
| S1-P04 | state.rs: apply/unapply | core/state.rs, state_roundtrip.rs | S1-P03 |
| S1-P05 | governance: SCIP + consensus_version + activation height | core/governance/scip.rs, docs/SCIP/ | S1-P03 |
| S1-P06 | ADR-0006 + Verkle Trie + state_root | ADR-0006, core/state/verkle.rs | S1-P04 |
| S1-P07 | StateWitness + stateless API | core/state/witness.rs | S1-P06 |
| S1-P08 | Merkle root транзакций | core/serialize.rs (merkle), заголовок | S1-P06 |
| S1-P09 | ADR-0009 + EventBus | ADR-0009, src/events.rs | S1-P03 |
| S1-P10 | ADR-0007 + tokio (постепенно) | ADR-0007, Cargo.toml, main.rs | S1-P09 |
| S1-P11 | chain_selector + tie-breaking | src/blockchain/chain_selector.rs, fork_choice.md | S1-P04, S1-P09 |
| S1-P12 | block_executor + state_cache | src/blockchain/{block_executor,state_cache}.rs | S1-P11 |
| S1-P13 | facade + consensus_manager | src/blockchain/{facade,consensus_manager}.rs | S1-P05, S1-P12 |
| S1-P14 | network_id в генезисе и HELLO | genesis.json, protocol.rs | S1-P03 |
| S1-P15 | bech32-адреса | core/address.rs, wallet/cli/gui | S1-P14 |
| S1-P16 | Headers-first sync | protocol.rs (4 msg), sync.rs, тест | S1-P13, S1-P14 |
| S1-P17 | Mempool RBF | src/mempool/mod.rs, tests/rbf.rs | S1-P09 (события), S1-P16 |
| S1-P18 | ADR-0010 + SyncEngine | ADR-0010, src/network/sync_engine.rs | S1-P10, S1-P13, S1-P16 |
| S1-P19 | Тесты Stage 1 + первый fuzz-target | tests/*.rs, fuzz/ | S1-P12..S1-P18 |
| S1-P20 | Регрессия 22 инвариантов | docs/stage1/INVARIANTS_ENFORCED.md | S1-P19 |
| S1-P21 | Threat model актуализация | docs/security/THREAT_MODEL.md | S1-P19 |
| S1-P22 | DoD-верификация Stage 1 + тег v1.1.0-stage1 | docs/stage1/STAGE1_SUMMARY.md, vm/traits.rs, тег | S1-P20, S1-P21 |

Параллельные треки: (a) state-root линия S1-P06→P07→P08 ∥ (b) events/tokio S1-P09→P10 ∥ (c) network S1-P14→P15 — после S1-P03; декомпозиция S1-P11→P12→P13 и сеть S1-P16..P18 сходятся в S1-P19.

---

## 4. Покрытие Definition of Done Этап 1 (ROADMAP3)

| # | Критерий DoD Этап 1 | Промпт | Evidence |
|---|---------------------|--------|----------|
| 1 | strangecoin-core создан, чистые функции перенесены | S1-P02..P05 | структура крейта, rg 0 I/O |
| 2 | Verkle Trie, state.root_after == block.state_root | S1-P06 | tests/state_root.rs |
| 3 | Headers-first sync работает | S1-P16 | tests/sync_headers.rs |
| 4 | Events bus работает (3 subscribers) | S1-P09, S1-P19 | tests/events.rs |
| 5 | Tie-breaking детерминирован (proptest) | S1-P11 | proptest select_best |
| 6 | tokio введён, новые подсистемы async | S1-P10 | ADR-0007, сборка |
| 7 | Network ID в HELLO, чужие отбрасываются | S1-P14 | tests/network_id.rs |
| 8 | bech32 round-trip | S1-P15 | core/address.rs тесты |
| 9 | Blockchain декомпозирован (4 + consensus_manager) | S1-P11..P13 | rg-аудит facade |
| 10 | Sync engine разрывает network↔blockchain | S1-P18 | rg-аудит network |
| 11 | consensus_version + activation height | S1-P05, S1-P13 | тесты scip.rs |
| 12 | All Stage 0 invariants still enforced | S1-P20 | INVARIANTS_ENFORCED |
| 13 | ADR-0006..0010 написаны | S1-P02, P06, P09, P10, P18 | docs/ADR/ |
| 14 | Changelog: «1.1.0 — core extracted + Verkle + headers-first» | S1-P22 | Changelog.md |
| 15 | Тег v1.1.0-stage1 | S1-P22 | git tag |

Плюс общий DoD-критерий fuzzing (security track): первый target — S1-P19.

## 5. Покрытие инвариантов ARCHITECT3 §5 на Stage 1

| # | Инвариант | Статус на Stage 1 | Где enforce / тест |
|---|-----------|-------------------|--------------------|
| 1 | Валидность — из цепочки | Усилен | state_cache::rebuild_from_chain (S1-P12) |
| 2–14, 16, 17, 22 | Без изменений | Регрессия обязана быть green | S1-P20 |
| 19 | State root match | **Enforce впервые** | core/state/verkle.rs + validate (S1-P06) |
| 21 | Consensus versioning | **Enforce впервые** | core/governance/scip.rs (S1-P05) |
| 15 | Block gas limit | Deferred (Stage 1.5) | — |
| 18 | Event log / Receipt | Deferred (Stage 1.5) | — |
| 20 | Fee invariant | Deferred (Stage 5) | — (RBF-fee=0 — прокси, S1-P17) |

## 6. Примечания по использованию

1. **Порядок выполнения:** строго по карте зависимостей (§1). Гейт №1 — тег `v1.0.0-stage0` (D03); без него Stage 1 не начинается. Гейт №2 — тег `v1.1.0-stage1` (S1-P22); без него Stage 1.5 не начинается.
2. **Размер промпта:** каждый промпт — ≤200 строк markdown, реализация помещается в контекст 32k-token агента. Если промпт кажется большим — делить на a/b (например, S1-P16a: протокол; S1-P16b: download-цикл + тест).
3. **Контекст:** вставлять блок «Общий контекст» (§0) в начало каждого промпта при отправке агенту; для долговых промптов — дополнительно указать ссылку на `retro-stage0.md` §5.
4. **Артефакты и измеримость:** прогресс = число созданных файлов / общее число ожидаемых. КГ обязательны: нельзя закрывать промпт без прохождения всех пунктов; если КГ не проходит — итерировать тот же промпт, не переходить к следующему.
5. **Процессные страховки из retro §8.1** (обязательны к применению):
   - 1 промпт = 1 атомарный коммит с ID промпта в сообщении;
   - Changelog/README/AGENTS.md обновляются только коммитом, в котором артефакты уже существуют;
   - «done» объявления — только через DoD-промпты (D03, S1-P22), никогда — через ощущение завершённости;
   - факты, которые агент не может проверить (CI-run, TLC-прогон, fuzz-статистика), фиксируются как «не верифицировано + причина», а не молча опускаются.
6. **Связь с чек-листом ARCHITECT3 §17:** после S1-P22 прогнать чек-лист §17 и убедиться, что все применимые к Stage 1 пункты отмечены.

## 7. Что НЕ входит в Stage 1 (явно отложено)

Следующие задачи появляются на более поздних стадиях; если промпт случайно затрагивает их — остановить и переориентировать на Stage 1 scope:

- **WASM VM / wasmi / gas / precompiles** — Stage 1.5 (сейчас только объявление `VmExecutor` trait, S1-P22);
- **Noise Protocol Framework, Erlay, compact blocks, gossip-оптимизации** — Stage 2;
- **RocksDB миграция** — Stage 3 (сейчас только ADR-план, S1-P02);
- **EncryptedMempool реализация** — Stage 5 (placeholder допустим);
- **EIP-1559 fee market** — Stage 5 (fee_market.rs — заглушка);
- **Account Abstraction** — Stage 5;
- **Полный light client / SPV-верификация без тел** — после Stage 1 (headers-first только прокладывает маршрут);
- **PoS миграция, Casper FFG, BLS12-381** — Stage 7 (consensus_manager готовит enum, не более);
- **Перенос legacy-threads майнинга в async** — Stage 2 (S1-P10 намеренно их не трогает);
- **TLA+ полный formal verification** — Stage 6 (skeleton уже есть, актуализация свойств — по мере новых инвариантов).
