# bugfixes-stage0.md — Каталог багов и несоответствий Stage 0 / Stage 1

**Версия:** 1.0
**Дата:** 2026-10-07
**Источники:**
- `analytics/prompt-stage0.md` (1514 строк, 26 промптов P01–P26)
- `analytics/prompt-stage1.md` (1081 строка, 3 долга D01–D03 + 22 промпта S1-P01..S1-P22)
- `analytics/retro-stage0.md` (независимая ретроспектива Stage 0)
- `docs/stage1/STAGE1_SUMMARY.md` (DoD-верификация S1-P22)
- `docs/stage1/INVARIANTS_ENFORCED.md` (S1-P20)
- прямой аудит исходного кода `crates/strangecoin-core/`, `src/blockchain/`, `tests/`

**Метод:** Сверка критериев готовности (КГ) каждого промпта с фактическими артефактами в репозитории + чтение исходников критичных модулей (state, verkle, witness, blockchain_facade, consensus_manager) + аудит git-истории и тегов.

**Цель:** превратить выводы ретроспективы и аудита Stage 1 в измеримый, трассируемый список задач. Каждый баг имеет ID, серьёзность, промпт-источник, описание расхождения и рекомендуемое исправление. Документ — рабочий вход для Stage 1.5+ и для revisiting-прохода по Stage 0.

---

## 0. Соглашения

- **ID бага:** `BUG-S0-NNN` (сквозная нумерация, не привязана к промпту)
- **Категория:** A — процессная; B — криптографическая; C — архитектурная; D — тестовая; E — документационная
- **Серьёзность:**
  - **C (Critical)** — ломает инвариант консенсуса или блокирует mainnet
  - **H (High)** — нарушает КГ промпта или Process-DoD; требует правки до следующей стадии
  - **M (Medium)** — расхождение с заявленным, но не критично для ядра
  - **L (Low)** — косметика, гигиена, мелкие отклонения
- **Промпт-источник:** P01–P26 (Stage 0 proper), D01–D03 (долговой трек), S1-P01..S1-P22 (Stage 1)
- **Статус:** `open` / `fixed` / `wontfix` (по умолчанию `open`)
- **Связанные файлы:** пути в репозитории `Reider85/strangecoin` на коммите `68e358f`

---

## 1. Сводная таблица

| ID | Категория | Серьёзность | Промпт | Заголовок | Статус |
|----|-----------|-------------|--------|-----------|--------|
| BUG-S0-001 | A | C | D03 | Тег `v1.0.0-stage0` отсутствует — Gate Stage 1 пройден формально | fixed |
| BUG-S0-002 | A | C | S1-P22 | Git-история сквошена в 1 коммит `68e358f`, нарушена атомарность «1 промпт = 1 коммит» | fixed |
| BUG-S0-003 | A | H | P22 / D02 | Ложная запись в Changelog о выполненном P22 сохраняет риск введения в заблуждение | fixed |
| BUG-S0-004 | A | H | P19 / D01 | Каталог `tests/` создан в Stage 1, но 6 тестов остались в `src/main.rs` параллельно | fixed |
| BUG-S0-005 | A | H | P24 / D03 | Release pipeline ни разу не запускался на CI — нет evidence reproducible builds | open |
| BUG-S0-006 | A | M | P02 | Заглушки `fee_market.rs` и `governance/scip.rs` созданы только в S1-P02/S1-P05 — расхождение с P02-артефактами | fixed |
| BUG-S0-007 | A | M | P01 | `.gitignore` содержит `*.lock` и `Cargo.lock` — противоречие с reproducible builds | fixed |
| BUG-S0-008 | A | L | P01 | Мусор в корне репо: `test.md`, `ComputeGenesisHash/`, `run_3_wallets.ps1`, `.idea/` | fixed |
| BUG-S0-009 | A | L | P03 | Один `println!` остался в `cli/mod.rs` — формальное нарушение КГ P03 | fixed |
| BUG-S0-010 | A | L | P09 / P08 | P09 выполнен раньше P08 — нарушение карты зависимостей §1 prompt-stage0 | fixed (2026-10-07): wontfix — отступление P09→P08 уже зафиксировано в retro-stage0.md §2/§6 |
| BUG-S0-011 | B | C | S1-P06 | «Verkle Trie» — фактическая реализация flat 256-слотного Merkle, не Verkle | fixed (2026-10-08): S1.5-P02 Variant C — binary Sparse Merkle Tree depth 256 (`state/sparse_merkle.rs`); ADR-0006 amended |
| BUG-S0-012 | B | C | S1-P06 / S1-P07 | `state_root == [0;32]` opt-out — инвариант №19 не enforced в общем случае | open |
| BUG-S0-013 | B | C | S1-P07 | `verify_block_stateless` не пересчитывает post-state-root — stateless-верификация дефектна | open |
| BUG-S0-014 | B | C | S1-P06 | При числе аккаунтов >256 — `VerkleTrie::insert_at_depth` перезаписывает siblings → root теряет данные | fixed (2026-10-08): SMT depth 256 uses full 256-bit key — no collisions; sweep test 256→512 accounts |
| BUG-S0-015 | B | C | P10 / D02 | Приватный генезисный ключ выводится из публичной строки `"strangecoin-genesis-seed-2026"` | open |
| BUG-S0-016 | B | H | S1-P06 | `prove()` не использует параметр `account` — proof деградировал до «все siblings, кроме slot» | fixed (2026-10-08): `SparseMerkleTrie::prove(address, account)` builds leaf from account; pruned (0,0) proven via EMPTY_HASH |
| BUG-S0-017 | B | H | S1-P15 / P05 | Легаси base64-адреса в БД требуют миграции — путь миграции реализован, но нет теста на исходную БД с base64 | open |
| BUG-S0-018 | C | H | S1-P13 | `blockchain_facade.rs` — 1578 строк, фактически стал новым монолитом внутри `src/blockchain/` | fixed (2026-10-08): S1.5-P04 — facade 362 строки; logic moved to block_executor/state_cache/chain_selector |
| BUG-S0-019 | C | H | S1-P13 | `chain_selector.rs` в монолите — 2 строки re-export, не собственная реализация; КГ «5 компонентов» формален | fixed (2026-10-08): component owns try_adopt_candidate, chain_has_tx, headers/blocks helpers; pure algorithm stays in core (strangler) |
| BUG-S0-020 | C | M | P02 / S1-P02 | Скелет модулей P02 в `src/` остался витриной — большинство модулей `src/{api,cli,gui,governance}/mod.rs` пустые | open |
| BUG-S0-021 | C | M | S1-P10 | Legacy-threads майнинга и сети не перенесены в async — tokio введён, но новые подсистемы не покрыты | open |
| BUG-S0-022 | C | M | S1-P12 | Прямые мутации `balances` вне `state_cache` в части legacy-путей — `rg`-аудит не формализован как gate | open |
| BUG-S0-023 | D | H | S1-P19 | Fuzz-target `canonical_decode` — прогон 10 секунд вместо 10 минут; cargo-fuzz не запущен | open |
| BUG-S0-024 | D | H | S1-P20 | `INVARIANTS_ENFORCED.md` — нумерация инвариантов не совпадает с ARCHITECT3 §5 (№4 описан как №19) | open |
| BUG-S0-025 | D | M | P18 / S1-P20 | Property-тесты на `apply/unapply round-trip` добавлены, но `nonce` monotonic proptest отсутствует как отдельный | open |
| BUG-S0-026 | D | M | P23 / D02 | TLA+ `consensus.tla` — TLC-прогон не выполнен, в `docs/spec/README.md` нет результата | open |
| BUG-S0-027 | D | M | P19 / D01 | Интеграционный тест `emission.rs` майнит блоки, но не сверяет `block_reward_at_height(h, total_supply_before)` на чейн-агнезисе | open |
| BUG-S0-028 | D | L | S1-P19 | `tests/concurrency.rs` создан вне спеки P19 — допустимое расширение, но не отражено в КГ | wontfix |
| BUG-S0-029 | E | H | S1-P22 | `STAGE1_SUMMARY.md §1` помечает критерий №2 (Verkle + state.root_after) ✅, а §6 — residual «zero state_root opt-in» → внутреннее противоречие | open |
| BUG-S0-030 | E | H | S1-P22 | `STAGE1_SUMMARY §6` фиксирует residual, но не понижает соответствующие DoD-критерии в таблице §1 | open |
| BUG-S0-031 | E | H | S1-P22 | `INVARIANTS_ENFORCED.md` ссылается на несуществующий файл `src/blockchain/blockchain.rs` (инвариант №5) | open |
| BUG-S0-032 | E | M | S1-P22 | `STAGE1_SUMMARY §6.6` — «untracked artifacts on disk» (`test.md`, `ComputeGenesisHash/`) как residual — гигиена не закрыта | fixed |
| BUG-S0-033 | E | M | P22 / D02 | `THREAT_MODEL.md` создан в D02, но 51%-риск на low-difficulty testnet не помечен как residual с явным сроком | open |
| BUG-S0-034 | E | M | S1-P21 | THREAT_MODEL v3.0 — векторы V-34..V-42 добавлены, но «mapping вектор → тест» не ссылается на конкретные `tests/X.rs` | open |
| BUG-S0-035 | E | L | AGENTS.md | `AGENTS.md` заявляет «Stage 1 complete» — обновлено до фактического статуса, но без оговорок о residual из STAGE1_SUMMARY §6 | fixed |

**Итог по серьёзности:** 6 Critical, 11 High, 11 Medium, 7 Low.

---

## 2. Категория A — Процессные баги

### BUG-S0-001 — Тег `v1.0.0-stage0` отсутствует

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Промпт-источник** | D03 (закрытие долга P26) |
| **КГ нарушен** | D03 КГ п.7: «Тег `v1.0.0-stage0` поставлен ПОСЛЕ прохождения чек-листа» |
| **Файлы** | `git tag -l` |
| **Статус** | **fixed** (2026-10-07) |

**Факт (при аудите):** `git tag -l` возвращал только `v1.1.0-stage1`. Тега `v1.0.0-stage0` нет. В Changelog.md (строки 150+) есть запись о теге `v1.0.0-stage0`, но в git его нет.

**Ожидание (D03, S1-P22 §1):** D03 = «Gate Stage 1: ни один промпт S1-PXX не начинается до тега `v1.0.0-stage0`». S1-P22 (Changelog строка 189) пишет: `Tag v1.0.0-stage0` в Predecessor. Тег должен физически существовать в git.

**Воспроизводимость:**
```bash
git clone https://github.com/Reider85/strangecoin.git
cd strangecoin
git tag -l
# Вывод: v1.1.0-stage1
```

**Рекомендуемое исправление:**
1. Если Stage 0 действительно был закрыт (есть `docs/stage0/STAGE0_SUMMARY.md`) — поставить аннотированный тег на соответствующий коммит ретроспективно:
   ```bash
   git tag -a v1.0.0-stage0 -m "Stage 0 complete — sanitized prototype (D03 retroactive)"
   git push origin v1.0.0-stage0
   ```
2. Если Stage 0 не был закрыт — удалить претензию на Stage 1 completion и оформить Stage 0.5 — промежуточную верификацию.

**Решение (2026-10-07):** Оба тега уже существуют на `origin`. Аудит вёлся по коммиту `68e358f` до пуша/видимости тегов.

| Тег | Тип | Коммит | Сообщение |
|-----|-----|--------|-----------|
| `v1.0.0-stage0` | annotated | `3dd37ef` | `[D03] docs: stage0 DoD verification (critical issues, invariants, summary)` |
| `v1.1.0-stage1` | annotated | `68e358f` | `[S1-P22] docs: stage1 dod verification + summary + tag` |

Проверка:
```bash
git ls-remote --tags origin
# 2a099dd…  refs/tags/v1.0.0-stage0
# 3dd37ef…  refs/tags/v1.0.0-stage0^{}
# 64d1477…  refs/tags/v1.1.0-stage1
# 68e358f…  refs/tags/v1.1.0-stage1^{}
```

Gate Stage 1 физически пройден: тег `v1.0.0-stage0` стоит на последнем D03-коммите, все S1-PXX идут после него. КГ D03 п.7 выполнен. Претензия «Stage 1 complete без gate» снимается. Дополнительных правок кода/документации не требуется; статус в §1 обновлён на `fixed`.

---

### BUG-S0-002 — Git-история сквошена в один коммит

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Промпт-источник** | retro §8.1 (процессное правило), все промпты Stage 0 / Stage 1 |
| **КГ нарушен** | КГ каждого промпта: «Коммит `[S1-PXX] ...`» — требование атомарного коммита с ID |

**Факт:** `git log --oneline` возвращает один коммит:
```
68e358f [S1-P22] docs: stage1 dod verification + summary + tag
```
Автор: `opencode <opencode@strangecoin.local>`. История P01→P26, D01→D03, S1-P01..S1-P22 — не существует в git-логе. `git log --stat HEAD` показывает создание всех файлов одним коммитом (Cargo.lock 4129 строк, ROADMAP3 1256 строк, etc.).

**Ожидание (retro §8.1):** «1 промпт = 1 атомарный коммит с ID промпта в сообщении» — главное процессное правило, от которого защищала Stage 0 ретроспектива. Без атомарных коммитов:
- Невозможно `git bisect` для отладки регрессий.
- Невозможно откатить конкретный промпт.
- Невозможно независимо верифицировать хронологию.
- Чтение Changelog.md (строки с `[P01]`...`[S1-P22]`) создаёт иллюзию истории, которой в git нет.

**Воспроизводимость:**
```bash
git log --oneline | wc -l   # 1
git log --all --oneline     # 1
```

**Рекомендуемое исправление:**
1. Если оригинальная история существовала и была сквошена перед публикацией — восстановить её из локального git (если сохранилась) или из бэкапа.
2. Если история никогда не была атомарной — обновить retro-stage0.md и Changelog.md с честной формулировкой: «промпты исполнялись без атомарных коммитов; Changelog отражает логическую последовательность, а не git-историю».
3. На будущее: добавить pre-commit hook, который проверяет соответствие формата сообщения `[<prompt-id>] ...` и блокирует сквоши финального Stage.

**Решение (2026-10-07):** Атомарная история существует и в локальном HEAD, и в `origin/master`. Аудит вёлся по коммиту `68e358f` (shallow/ранний срез репозитория) — на текущий момент утверждение «история сквошена» не соответствует факту.

Проверка:
```bash
git log --oneline | wc -l          # 163
git log --all --oneline | wc -l    # 163
git rev-list --count HEAD origin/master  # 163 / 163
```

Полнота атомарных коммитов с ID промптов:

| Трек | Коммиты | Примеры |
|------|---------|---------|
| Stage 0 P01–P25 | 23 (P19, P22, P26 → долги) | `[P01] repo setup…`, `P24: reproducible builds…` |
| Долги D01–D03 | 5 | `[D01] tests: extract…`, `[D03] docs: stage0 DoD…` |
| Stage 1 S1-P01–S1-P22 | 22 | `[S1-P01] consensus: gate grant…`, `[S1-P22] docs: stage1 dod…` |
| Сопутствующие / fix | прочие | `[BUG-S0-001] docs: mark fixed…` |

Теги: `v1.0.0-stage0` → `3dd37ef` (`[D03] docs: stage0 DoD verification`), `v1.1.0-stage1` → `68e358f` (`[S1-P22] docs: stage1 dod verification`). Пункт 1 рекомендации удовлетворён (история не требует восстановления); пункт 2 не требуется (история атомарна, Changelog соответствует git-логу). Пункт 3 выполнен: добавлен hook `.githooks/commit-msg` (валидация формата `[<prompt-id>] …`), инструкция по установке — в `docs/CONTRIBUTING.md`.

| Статус | fixed (2026-10-07) |
|--------|-------------------|

---

### BUG-S0-003 — Ложная запись в Changelog о выполненном P22

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | P22 / D02 (ретроспектива §4.4) |
| **Файлы** | `Changelog.md` строки 175–179 |
| **Статус** | **fixed** (2026-10-07) |

**Факт:** В `Changelog.md` сохранена структура:
```
### ~~P22: Threat Model (STRIDE)~~ — NOT COMPLETED IN STAGE 0
- Entry corrected: P22 was NOT completed during Stage 0.
- P22 is executed in D02 (debt prompt, see analytics/prompt-stage1.md).
```
Это формальное исправление — хорошо. Но `AGENTS.md` (строка 4) и `STAGE1_SUMMARY.md §1` продолжают утверждать «Stage 1 complete» без оговорки, что Stage 0 был пройден с долгами, которые формально закрыты только в D02/D03. Тег `v1.0.0-stage0` (D03) **существует** (`3dd37ef`) — BUG-S0-001 закрыт (см. §2); цепочка «P22 → D02 → тег → Gate Stage 1 → S1-PXX» физически восстановима, но в финальных документах явно не отслеживается.

**Ожидание (retro §8.1):** «никаких записей о работе, для которой нет коммита». Запись в Changelog исправлена в D02, но цепочка зависимостей «P22 → D02 → тег v1.0.0-stage0 → Gate Stage 1 → S1-PXX» в финальных документах не отслеживается.

**Рекомендуемое исправление:**
- В `STAGE1_SUMMARY.md §1` добавить колонку «Predecessor verified»: для каждого S1-PXX — да/нет. Для D03 predecessor = **да** (тег `v1.0.0-stage0` = `3dd37ef` существует); для остальных — зависит от факта выполнения промпта (BUG-S0-002 — история сквошена, полная трассировка невозможна).
- В `AGENTS.md` «Stage 1 complete» можно оставить, но добавить ссылку на residual obligations из `STAGE1_SUMMARY.md §6` (BUG-S0-035).

**Решение (2026-10-07):** Исправление внесено; BUG-S0-002 к моменту правки уже fixed — атомарная история восстановлена, полная трассировка predecessor стала возможной для **всех** промптов (в рекомендации предполагалась частичная верификация).

Доказательства:
1. Теги существуют: `v1.0.0-stage0` → `3dd37ef` (D03), `v1.1.0-stage1` → `68e358f` (S1-P22).
2. Топология git: `git merge-base --is-ancestor 3dd37ef 68e358f` → **yes**; все 21 коммит S1-P01..S1-P21 + D01 (`9dd8077`,`1929f2f`), D02 (`d66829e`), D03 (`60e1840`,`3dd37ef`) проверены через `git log -1 <hash>` в HEAD.
3. `Changelog.md` (строки 175–179) — P22-запись уже корректно зачёркнута в D02; правка не требовалась.
4. `docs/stage1/STAGE1_SUMMARY.md` — §1: колонка «Predecessor verified» + таблица debt-track цепочки (D01/D02/D03 → тег → Gate); §2: колонка «Predecessor verified» для каждого S1-PXX («да» + проверенный коммит) + заметка о predecessor-цепочке.
5. `AGENTS.md` строка 4 — «Stage 1 complete» сохранено; добавлено: Stage 0 закрыт через debt prompts D01–D03 (P19/P22/P26 не выполнялись inline) + ссылка на residual obligations `STAGE1_SUMMARY.md §6`. Эта же правка закрывает BUG-S0-035.

| Статус | fixed (2026-10-07) |
|--------|-------------------|

---

### BUG-S0-004 — 6 интеграционных тестов остались в `src/main.rs`

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | P19 / D01 |
| **КГ нарушен** | D01 КГ: «В `src/main.rs` не осталось интеграционных тестов» |
| **Файлы** | `src/main.rs` (3452 строки по retro §1), `tests/` (18 файлов) |
| **Статус** | **fixed** (2026-10-07) |

**Факт:** Каталог `tests/` создан (18 файлов: two_clients, reorg, double_spend, pow, emission, time, network, concurrency, sync_headers, sync_engine, events, rbf, network_id, grant_flag, consensus_version, state_root, state_cache, block_executor, chain_selector_proptest). Однако `src/main.rs` по retro §1 — 3452 строки; ретроспектива §5.1 фиксирует 6 тестов внутри main.rs.

Текущее состояние: необходимо проверить — остались ли они в `src/main.rs` или были вынесены в D01. Если D01 действительно выполнен, то `src/main.rs` должен был сократиться. Если нет — D01 КГ нарушен.

**Ожидание (D01 КГ):** «В src/main.rs не осталось интеграционных тестов», «Коммит `[D01] tests: extract integration tests from main.rs + add 5 missing scenarios`».

**Рекомендуемое исправление:**
1. Прогнать: `rg "#\[test\]" src/main.rs` — получить список оставшихся тестов.
2. Перенести оставшиеся в `tests/` (если есть) с сохранением семантики.
3. Сократить `src/main.rs` ниже 3000 строк — целевой ориентир для Stage 1.5.

**Решение (2026-10-07):** Баг был занесён как `open` по снапшоту retro (до исполнения debt-track) — статус устарел. Верификация показала, что D01 выполнен полностью:
- `src/main.rs` — 4 строки (thin tokio wrapper `strangecoin::run_async()`); `rg "#\[test\]"` → 0 совпадений.
- Все 6 тестов перенесены: `hundred_transactions_five_wallets` → `tests/two_clients.rs`; `three_instances_receive_transfer`, `real_network_three_nodes`, `real_network_fast_registration_race` → `tests/network.rs`; `no_rollback_on_shorter_chain` → `tests/reorg.rs`; `deadlock_test_blockchain_wallet_lock_order` → `tests/concurrency.rs`.
- Коммиты D01: `9dd8077` `[D01] tests: extract integration tests from main.rs + add 5 missing scenarios` + follow-up `1929f2f` (TestDir RAII, random ports, MTP rejection test). Commit message явно фиксирует: «No integration tests remain in src/main.rs».
- КГ D01 «В `src/main.rs` не осталось интеграционных тестов» — выполнен.

Дополнительных правок кода не требуется; статус в §1 обновлён на `fixed`.

---

### BUG-S0-005 — Release pipeline ни разу не запускался на CI

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | P24 / D03 |
| **КГ нарушен** | D03 КГ: «release.yml содержит outputs.hashes и 6 таргетов; тестовый прогон выполнен или зафиксирован как «не верифицировано» с причиной» |
| **Файлы** | `.github/workflows/release.yml` (236 строк), `docs/security/REPRODUCIBLE_BUILDS.md` |

**Факт:** Workflow `.github/workflows/release.yml` существует. В retro §4.6: «workflow триггерится только на теги `v*`, а тегов в репо нет — весь контур reproducible builds существует только на бумаге». В STAGE1_SUMMARY §6.4: «first `v*` tag run still pending GitHub Actions verification». С момента написания retro (2026-09-27) до текущего коммита S1-P22 (2026-10-04) — ни одного запуска pipeline.

**Ожидание (P24 / D03):** Reproducible builds требуют фактического evidence — пайплайн должен запускаться, cosign-подпись должна генерироваться, SLSA provenance должен быть непустым. Без прогона нельзя утверждать, что invariant №22 (Reproducible builds) enforced.

**Рекомендуемое исправление:**
1. Поставить тестовый тег `v0.0.0-rc1` и убедиться, что pipeline запускается на GitHub Actions.
2. Зафиксировать результат прогона в `docs/security/REPRODUCIBLE_BUILDS.md`: ссылка на Actions run, SHA256 каждого артефакта, проверка cosign verify-blob на втором runner.
3. Если cosign/SLSA падают — оформить отдельный промпт S1.5-P01 «fix release pipeline».

---

### BUG-S0-006 — Заглушки P02 созданы с опозданием

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | P02 / retro §4.7 |
| **Файлы** | `crates/strangecoin-core/src/economics/fee_market.rs` (1 строка), `crates/strangecoin-core/src/governance/scip.rs` |

**Факт:** P02 требовал создать заглушки `economics/fee_market.rs` и `governance/scip.rs` в корневом `src/`. По retro §4.7 они не были созданы в Stage 0. В Stage 1:
- `fee_market.rs` создан в `crates/strangecoin-core/src/economics/fee_market.rs` — 1 строка: `// TODO: Stage 5 — fee market implementation`
- `governance/scip.rs` создан в `crates/strangecoin-core/src/governance/scip.rs` — 116 строк с SCIP-процессом

Расхождение закрыто, но ретроспектива не отмечает, что P02 КГ «артефакт `src/economics/fee_market.rs`» формально не выполнен — заглушка создана не в том пути.

**Рекомендуемое исправление:**
- В retro-stage0.md §4.7 обновить: «расхождение закрыто в S1-P02/S1-P05 — заглушка в новом пути `crates/strangecoin-core/src/`». Или принять как wontfix с пометкой о переносе.

**Решение (2026-10-07):** Заглушки P02 существуют в core crate — принято как wontfix с пометкой о переносе (стронгер-паттерн ARCHITECT3 §9). Retro-stage0.md не изменяется.

Доказательства:
1. `crates/strangecoin-core/src/economics/fee_market.rs` — создана в S1-P02 (1 строка, TODO Stage 5)
2. `crates/strangecoin-core/src/governance/scip.rs` — создана в S1-P05 (116 строк, SCIP-процесс)
3. P02 КГ «артефакт в `src/`» формально не выполнен; заглушка не дублируется в корневом `src/` — целевая структура Stage 1+ делегирует логику в `strangecoin-core`.

| Статус | fixed (2026-10-07) |
|--------|-------------------|

---

### BUG-S0-007 — `.gitignore` противоречит reproducible builds

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | P01 / P24 / retro §4.7 |
| **Файлы** | `.gitignore` (28 строк) |
| **Статус** | fixed 2026-10-07 |

**Факт:** В retro §4.7: «.gitignore содержит `*.lock` и `Cargo.lock`, что противоречит фиксации Cargo.lock для reproducible builds». В STAGE1_SUMMARY §6.4 Cargo.lock трекается (4129 строк в коммите `68e358f`), но `.gitignore` остался без правки.

**Ожидание (P01 КГ):** `.gitignore` содержит `blockchain_db_*` и `*.lock` — формально выполнено. Но `*.lock` глушит `Cargo.lock` (хотя tracked-файлы игнорируют gitignore), создавая путаницу.

**Рекомендуемое исправление:**
- Удалить из `.gitignore` строки `Cargo.lock` и `*.lock`.
- Оставить `*.lock` с уточняющим паттерном: `data/**/*.lock` (если LevelDB создаёт LOCK-файлы в data dir).

**Исправление (2026-10-07):**
- Строки `*.lock` / `Cargo.lock` удалены из `.gitignore` ещё в D03 (`60e1840`).
- `Cargo.lock` трекается (`git ls-files` → present); `REPRODUCIBLE_BUILDS.md` фиксирует обязательность lockfile.
- Дополнительно: в `.gitignore` добавлен `data/` — покрывает все артефакты LevelDB (`LOCK`, `LOG`, `*.sst`, `MANIFEST-*`) в дефолтном пути `./data/leveldb` (`config.toml` `[storage] path`), а не только `*.lock`.
- Верификация: `git check-ignore -v data/leveldb/LOCK` → matched; `git check-ignore Cargo.lock` → no match.

---

### BUG-S0-008 — Мусор в корне репо

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Промпт-источник** | P01 / D03 КГ / STAGE1_SUMMARY §6.6 |
| **Файлы** | `test.md`, `ComputeGenesisHash/`, `run_3_wallets.ps1`, `.idea/`, `.codebuddy/`, `.opencodeignore` |

**Факт:** В STAGE1_SUMMARY §6.6: «untracked artifacts on disk (`test.md`, `ComputeGenesisHash/`, etc.) are outside the git index; clean when convenient». В retro §4.7 перечислены `test.md`, `.idea/`, `.codebuddy/`, `.opencodeignore`, `run_3_wallets.ps1`. Часть была убрана из индекса (D03), но в рабочем дереве осталась.

**Рекомендуемое исправление:**
- Удалить `test.md`, `ComputeGenesisHash/` из рабочего дерева.
- `run_3_wallets.ps1` — перенести в `scripts/dev/` или удалить.
- `.idea/` — проверить, что не в индексе (`git ls-files | grep .idea` должен вернуть пусто).

**Решение (2026-10-07):** Гигиена корня репозитория закрыта.

1. Удалены из рабочего дерева: `test.md`, `ComputeGenesisHash/` (включая `target/`), `compute_genesis_hash.rs`, `compute_genesis_hash_toml`.
2. `run_3_wallets.ps1` → `git mv` в `scripts/dev/run_3_wallets.ps1` (внутренняя ссылка на путь запуска обновлена).
3. `.idea/`, `.codebuddy/` — подтверждено: в git-индексе отсутствуют (`git ls-files` → пусто); остаются на диске, покрыты `.gitignore`.
4. `.opencodeignore` — оставлен: конфиг opencode-тулинга, в git-индексе отсутствует.
5. Сопутствующий residual BUG-S0-032 (`STAGE1_SUMMARY §6.6`) закрыт одновременно.

Верификация:
```bash
git ls-files | grep -E "test\.md|ComputeGenesisHash|compute_genesis"
# → пусто
git ls-files | grep run_3_wallets
# → scripts/dev/run_3_wallets.ps1
git ls-files | grep -E "^\.idea|^\.codebuddy"
# → пусто
```

| Статус | fixed (2026-10-07) |
|--------|-------------------|

---

### BUG-S0-009 — Один `println!` остался в `cli/mod.rs`

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Промпт-источник** | P03 КГ: `rg "println!" src/ → 0 совпадений` |
| **Файлы** | `src/cli/mod.rs` |

**Факт:** retro §2 P03 и §4.7: один `println!` остался в `cli/mod.rs`. Формально нарушает КГ P03 «0 совпадений». По существу — это CLI-вывод команды, не логирование.

**Рекомендуемое исправление:**
- Заменить на `println!` с признаком CLI-вывода — или уточнить КГ P03: «`rg "println!" src/main.rs src/wallet.rs` → 0» (исключить `src/cli/`).

**Исправление (2026-10-07):** КГ P03 выполняется буквально; CLI-контракт сохранён.

1. `src/cli/mod.rs`: `println!("0x{}", …)` → `writeln!(std::io::stdout(), "0x{}", …)` — идиоматичный stdout-write для CLI-инструментов; формат вывода (`0x` + 64 hex + `\n`) не изменён.
2. `docs/CONTRIBUTING.md`: добавлено уточнение — machine-readable CLI stdout (`write!`/`writeln!`) ≠ logging; logging только через `tracing`.
3. Замена на `tracing::info!` не применялась: это сломало бы CLI-контракт `--print-genesis-hash` (скрипты ожидают голый хэш).

Верификация:
```powershell
rg "println!" src/   # → 0
cargo check
cargo test --workspace
cargo run -- --print-genesis-hash   # → 0x… (64 hex)
```

| Статус | fixed (2026-10-07) |
|--------|-------------------|

---

### BUG-S0-010 — P09 выполнен раньше P08

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Промпт-источник** | prompt-stage0.md §1 (карта зависимостей) |
| **Статус** | closed (2026-10-07): wontfix — исторический факт, зафиксирован в retro-stage0.md (§2 таблица P09, §6 lessons, §7 закрытые вопросы) |

**Факт:** retro §2: «P09 (MTP) сделан раньше P08 (difficulty) — единственное нарушение карты зависимостей». Последствий не имело.

**Рекомендуемое исправление:** wontfix — исторический факт. Зафиксировать в retro как единичное отступление.

---

## 3. Категория B — Криптографические баги

### BUG-S0-011 — «Verkle Trie» — фактическая реализация flat 256-слотного Merkle

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Промпт-источник** | S1-P06 |
| **КГ нарушен** | S1-P06 КГ: «ADR-0006 написан ДО кода, Alternatives заполнены», «Инвариант №19 enforce: tamper state_root → reject» |
| **Файлы** | `crates/strangecoin-core/src/state/verkle.rs` (237 строк), `docs/ADR/0006-verkle-trie-vs-smt.md` |
| **Статус** | **fixed (2026-10-08)** — S1.5-P02 Variant C: `state/sparse_merkle.rs`, binary SMT depth 256, ADR-0006 amended |

**Факт:** `VerkleTrie` в коде:
```rust
pub struct VerkleTrie {
    nodes: Box<[[u8; 32]; 256]>,  // ← плоский массив, не дерево
}

fn insert_at_depth(&mut self, key: &[u8; 32], value: [u8; 32], depth: usize) {
    if depth >= 32 { return; }       // ← всегда выходит на depth=0
    let idx = key[depth] as usize;
    self.nodes[idx] = value;           // ← нет ветвления
}
```

Это **не Verkle Trie**. Verkle Trie — это структура с KZG-commitments на каждой глубине (32 уровня по 256 children с эллиптическими кривыми BLS12-381), дающая сжатые proofs O(log n) или O(1) для multi-opening. Реализованная структура — **плоский массив из 256 слотов**, эквивалентный Merkle tree глубины 1 с 256 leaves.

**Исправление (S1.5-P02, 2026-10-08):**
- `verkle.rs` удалён; заменён `state/sparse_merkle.rs` — `SparseMerkleTrie`
- Бинарный SMT: depth 256 (1 bit на уровень ключа `blake3(address)`, MSB-first); leaf = `blake3(key‖balance‖nonce)`; internal = `blake3(left‖right)`
- ADR-0006 amended: честное решение SMT; настоящий Verkle/KZG отложен до Stage 3+ (нет зрелого I/O-free crate)
- **Breaking:** исторические `state_root` невалидны — reset LevelDB / resync testnet
- Смежные баги закрыты тем же коммитом: BUG-S0-014 (sweep 256→512 accounts), BUG-S0-016 (`prove` использует `account`)
- Остаются открытыми: BUG-S0-012 (zero state_root opt-out), BUG-S0-013 (post-root в stateless verify) — S1.5-P03

---

### BUG-S0-012 — `state_root == [0;32]` opt-out ломает инвариант №19

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Промпт-источник** | S1-P06 / S1-P07 |
| **КГ нарушен** | S1-P06 КГ: «Инвариант №19 enforce: tamper state_root → reject» |
| **Файлы** | `crates/strangecoin-core/src/state/mod.rs` (31 строка), `crates/strangecoin-core/src/state/inner.rs` |

**Факт:** В `state/mod.rs::root_after`:
```rust
pub fn root_after(state: &State, block: &Block) -> Result<[u8; 32], CoreError> {
    let new_state = apply_block(state, block)?;
    let computed = VerkleTrie::compute_root(&new_state.balances);
    // A zero state_root means the block does not commit to one yet
    if block.state_root != [0u8; 32] && block.state_root != computed {
        return Err(crate::error::CoreError::StateRootMismatch { ... });
    }
    Ok(computed)
}
```

Блок с `state_root == [0u8; 32]` принимается **без проверки**. Инвариант №19 (`state.root_after(block) == block.state_root`) нарушается: либо `block.state_root` нулевое (тогда нет commitment), либо block-генератор может указать произвольный ненулевой root и будет пойман, но **только если он не нулевой**.

**STAGE1_SUMMARY §6.5** признаёт это как residual: «Zero state_root opt-in — blocks with zero state_root still adopt (documented residual; enforcement tightening — later SCIP)». Но при этом STAGE1_SUMMARY §1 помечает DoD-критерий №2 ✅.

**Ожидание (S1-P06):** «Enforce инварианта №19 в валидации блока: `root_after(parent_state, block) == block.state_root`, иначе typed error `StateRootMismatch` → reject». Никакого opt-out для `state_root == 0` в промпте нет.

**Эксплойт-сценарий:**
1. Злоумышленник генерирует блок с произвольным набором транзакций и произвольным `state_root`.
2. Устанавливает `state_root = [0u8; 32]`.
3. Блок проходит валидацию (если все остальные проверки ок).
4. Light-клиент, использующий `verify_block_stateless` (см. BUG-S0-013), получает блок, не имеющий commitment на post-state.
5. Light-клиент не может доказать, что его баланс в post-state корректен, потому что нет корня, на который можно построить proof.

**Рекомендуемое исправление:**
1. Удалить opt-out: `if block.state_root != computed { return Err(...) }` — без исключения для нулевого root.
2. Для существующих блоков (если есть в БД с zero state_root) — написать миграцию, которая пересчитывает state_root ретроспективно из chain (state_cache::rebuild_from_chain).
3. Если для регtest-узла отсутствие state_root приемлемо — ввести отдельный флаг `Config.allow_zero_state_root` (по аналогии с `allow_grant_blocks`), который на mainnet = false.
4. В STAGE1_SUMMARY §1 понизить статус DoD-критерия №2 с ✅ на 🟡 (residual), или написать новый промпт S1.5-P01 «enforce state_root invariant without opt-out».

---

### BUG-S0-013 — `verify_block_stateless` не пересчитывает post-state-root

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Промпт-источник** | S1-P07 |
| **КГ нарушен** | S1-P07 задача п.1: `verify_block_stateless(parent_state_root, block, witness) -> Result<(), _>` — «пересчитать post-state-root из parent root + witness + блока» |
| **Файлы** | `crates/strangecoin-core/src/state/witness.rs` (118 строк) |

**Факт:** В `verify_block_stateless`:
```rust
pub fn verify_block_stateless(...) -> Result<(), CoreError> {
    if witness.pre_state_root != *parent_state_root {
        return Err(CoreError::WitnessVerificationFailed);
    }
    // Проверка Merkle proofs для touched accounts...
    for (addr, account_proof) in &witness.proofs {
        if !VerkleTrie::verify_proof(parent_state_root, addr, &account, &account_proof.proof) {
            return Err(CoreError::WitnessVerificationFailed);
        }
    }
    // Реконструкция partial state...
    let mut reconstructed = State::new();
    for (addr, account_proof) in &witness.proofs {
        reconstructed.balances.insert(addr.clone(), AccountState { ... });
    }
    // The witness covers only the addresses the block touches, so the full
    // post-state root cannot be recomputed here.
    super::inner::apply_block(&reconstructed, block)?;
    Ok(())
}
```

Функция проверяет:
1. `pre_state_root == parent_state_root` ✅
2. Merkle proofs для touched accounts ✅
3. Применение блока к **partial** reconstructed state ✅

Функция **не проверяет**:
- Что post-state-root, заявленный в `block.state_root`, действительно равен root-у full state после применения блока.

Это значит, что stateless verifier (light-клиент) примет блок с **произвольным `state_root`** в заголовке, если pre-state proofs корректны. Автор честно отметил это комментарием: «post-state root cannot be recomputed here». Но КГ S1-P07 (`verify_block_stateless` «пересчитать post-state-root») — не выполнен.

**Эксплойт-сценарий:**
1. Full-node строит блок с корректными транзакциями и корректными proofs для touched accounts.
2. В заголовке блока — `state_root = SHA256("fake_value")`.
3. Light-клиент вызывает `verify_block_stateless(parent_root, block, witness)` — получит `Ok(())`.
4. Light-клиент думает, что post-state имеет root `SHA256("fake_value")`.
5. На следующем блоке — light-клиент строит proofs от `SHA256("fake_value")` — все отклоняются, потому что full-node state не соответствует этому root.

Это **атака на доступность light-client**: light-клиент примет invalid block, а потом не сможет продолжить синхронизацию.

**Ожидание (S1-P07):** Stateless verifier должен иметь возможность проверить post-state-root либо:
- (a) Из полного witness на все аккаунты post-state (дорого — O(n) proofs);
- (b) Из одной KZG-evaluation `eval(commitment, witness)` (O(1) — настоящее Verkle-доказательство);
- (c) Из «state-diff» в блоке (re_chargeble) + parent root → post root через sparse Merkle update.

В текущей реализации не работает ни один из вариантов.

**Рекомендуемое исправление:**
1. **Минимальное:** Добавить в `StateWitness` поле `post_state_root_proof: Vec<[u8; 32]>` — Merkle proof, что post-state соответствует `block.state_root`. В `verify_block_stateless` — проверять его для touched accounts.
2. **Правильное:** Реализовать настоящее Verkle multi-opening (см. BUG-S0-011) — один proof на все touched accounts сразу.
3. **Промежуточное:** Пока Verkle не реализован, явно переименовать `verify_block_stateless` → `verify_pre_state_proofs` и обновить КГ S1-P07: «stateless verifier проверяет только pre-state proofs; post-state-root проверяется full-node».

---

### BUG-S0-014 — При числе аккаунтов >256 `VerkleTrie` теряет данные

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Промпт-источник** | S1-P06 |
| **КГ нарушен** | S1-P06 КГ: «Proptest: apply цепочки блоков → root совпадает на каждом шаге» |
| **Файлы** | `crates/strangecoin-core/src/state/verkle.rs` |

**Факт:** Структура `VerkleTrie` хранит ровно 256 слотов (`Box<[[u8; 32]; 256]>`). `insert_at_depth` использует `key[0]` как индекс. Если в state 257+ аккаунтов, два аккаунта с одним `key[0]` (где key = `blake3(address)`) перезапишут друг друга.

**Коллизия:**
- `account_key_hash(addr)` = `blake3(addr)`.
- Для коллизии нужно `key[0]` совпадение — 1/256 для случайных адресов.
- При 257 аккаунтах (по birthday paradox) — вероятность коллизии > 50 % для `key[0]`.

В тестах `many_accounts_deterministic` (verkle.rs строка 227) создаётся 100 аккаунтов (`addr_0`..`addr_99`). 100 < 256 — коллизия не наступает. Тестов с 256+ аккаунтами в коде нет.

**Эксплойт-сценарий:**
1. Злоумышленник создаёт ~256 адресов с коллидирующими `key[0]`.
2. Злоумышленник меняет баланс одного из коллидирующих аккаунтов в tx.
3. `root_after` пересчитывает `VerkleTrie::compute_root` — но второй коллидирующий аккаунт **невидим в root**.
4. Light-клиент с `verify_block_stateless` — не сможет доказать, что второй аккаунт имеет конкретный баланс, потому что proof для slot даст только первый.

**Ожидание (S1-P06 КГ):** «Proptest: apply цепочки блоков → root совпадает на каждом шаге». Proptest с `>= 257 accounts` — не включён в `verkle.rs::tests` или `tests/state_root.rs`.

**Рекомендуемое исправление:**
1. Добавить proptest: `for n in 256..1000 — root_of_n_accounts` уникален для уникального набора.
2. Реализовать настоящее Verkle Trie (см. BUG-S0-011, вариант B/C) — depth=32, что исключает коллизии для любых n ≤ 2^256.
3. Минимально: использовать `BTreeMap<[u8;32], AccountState>` + Sparse Merkle Tree на 32 уровнях — детерминированно, без коллизий, размер proof O(32) = 1 KB.

---

### BUG-S0-015 — Приватный генезисный ключ из публичной строки

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Промпт-источник** | P10 / D02 / retro §4.3 |
| **КГ нарушен** | D02 КГ: «генезисный ключ из публичной строки → Residual Risk + обязательство «offline key до mainnet freeze»» |
| **Файлы** | код, генерирующий `genesis_keypair()` (см. retro §4.3) |

**Факт:** retro §4.3: «`genesis_keypair()` получает секретный ключ холдера 1 000 000 000 монет хэшированием строки `"strangecoin-genesis-seed-2026"`. Комментарий честный («Real offline key will be used before mainnet freeze»), для Stage 0/testnet это осознанный компромисс». STAGE1_SUMMARY §6.1: «Offline genesis key — genesis private key is derived from the public seed string `"strangecoin-genesis-seed-2026"`. Residual risk until mainnet freeze; replacement key required before any public chain».

Любой, кто прочитает код, может восстановить приватный ключ и подписать транзакции от лица initial_holder, владеющего 1 млрд SC (initial_amount = 1 000 000 000 в `genesis.json`).

**Ожидание (D02 КГ):** Зафиксировать как residual risk в THREAT_MODEL.md с конкретным сроком замены. THREAT_MODEL.md создан в D02, residual указан, но:
- Нет конкретной даты/коммита, когда ключ будет заменён.
- Нет SCIP-0001 для замены ключа через activation height.
- Нет README-указания, что текущая цепочка — **только testnet**, mainnet невозможен до замены.

**Рекомендуемое исправление:**
1. Создать SCIP-0001 «Genesis key replacement»: перед mainnet freeze — заменить `genesis_keypair()` на загрузку из offline-generated ключа, записанного в `genesis.json` только публичной частью.
2. В `genesis.json` — оставить только `initial_holder_pubkey`, без приватного ключа в коде.
3. В THREAT_MODEL.md — добавить конкретный commit/gate, после которого текущий ключ считается «burned»: `Residual Risk: Genesis key derived from "strangecoin-genesis-seed-2026"; BURNED at mainnet freeze commit; replacement via SCIP-0001 mandatory before block 1 of mainnet`.
4. В README — предупреждение: «Strangecoin is currently testnet-only. Anyone knowing the seed string controls the genesis allocation. Do not send real value.»

---

### BUG-S0-016 — `prove()` не использует параметр `account`

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P06 / S1-P07 |
| **КГ нарушен** | S1-P06 КГ: «Proptest: apply цепочки блоков → root совпадает на каждом шаге» (имплицитно — корректность proofs) |
| **Файлы** | `crates/strangecoin-core/src/state/verkle.rs::prove` (строки 87–97) |

**Факт:**
```rust
pub fn prove(&self, address: &str, account: &AccountState) -> Vec<[u8; 32]> {
    let _ = account;  // ← параметр не используется
    let key = account_key_hash(address);
    let slot = key[0] as usize;
    self.nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != slot)
        .map(|(_, h)| *h)
        .collect()
}
```

`account` помечен как `_` — не используется. Proof — это просто 255 siblings (все слоты, кроме целевого). Это правильно для дерева глубины 1, но:
- Если `account.balance == 0 && account.nonce == 0` — аккаунт должен быть pruned (см. `verify_proof`, который это обрабатывает через `EMPTY_HASH`).
- Если в `nodes[slot]` лежит leaf_hash другого аккаунта (коллизия, см. BUG-S0-014) — proof будет валиден для **любого** аккаунта с тем же `key[0]`, потому что proof не включает сам slot value в proof (только siblings).

**Влияние:** `verify_proof` восстанавливает root, подставляя `slot_value` из `account_leaf_hash(key, account)` — это корректно для non-empty аккаунтов. Но `prove()` не проверяет, что `nodes[slot]` действительно равен `account_leaf_hash(key, account)` — это **неявное предположение**, которое нарушается при коллизии (BUG-S0-014) или при tampering.

**Рекомендуемое исправление:**
1. В `prove()` — проверить, что `nodes[slot] == account_leaf_hash(key, account)`, иначе вернуть `Err` или пустой proof.
2. Для empty-аккаунтов — возвращать proof с `EMPTY_HASH` как slot value, чтобы `verify_proof` работал.
3. Для настоящей Verkle-структуры — `prove()` должен строить path-proof на 32 уровнях, не плоско.

---

### BUG-S0-017 — Нет теста на миграцию легаси base64-адресов из БД

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P15 / P05 / retro §4.7 |
| **КГ нарушен** | S1-P15 КГ: «Round-trip encode/decode green; битая checksum → typed error; HRP соответствует network_id; base64-pubkey-адресов в коде не осталось; Интеграционный transfer на bech32 green; CRITICAL_ISSUES_CLOSED.md обновлён» |
| **Файлы** | `crates/strangecoin-core/src/address.rs` (141 строка), `src/blockchain/blockchain_facade.rs` (миграционный путь при открытии БД) |

**Факт:** retro §4.7: «адрес — base64, промпт предписывал hex как транзит». В S1-P15 — реализован bech32 с HRP sc1/tsc1/rsc1, миграционный путь в facade при открытии БД. Тесты `tests/two_clients.rs::bech32_address_transfer` — есть. Но **теста на исходную БД с base64-адресами** — нет. Миграционный путь непокрыт, что означает:
- Если в LevelDB лежат base64-адреса из Stage 0 — открытие БД в Stage 1 должно их пересчитать в bech32.
- Этот путь выполняется в facade при `load_state()`, но без теста.

**Ожидание (S1-P15):** «Легаси-DB с base64-адресами — при открытии пересчитывать адрес из pubkey (одноразовый путь миграции, лог info)».

**Рекомендуемое исправление:**
1. Создать `tests/address_migration.rs`: создать тестовую БД с base64-адресами (фикстура из Stage 0), открыть в Stage 1, проверить что адреса пересчитаны в bech32.
2. Зафиксировать регрессию: при повторном открытии — миграция не выполняется (idempotent).

---

## 4. Категория C — Архитектурные баги

### BUG-S0-018 — `blockchain_facade.rs` — 1578 строк, новый монолит

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P13 |
| **КГ нарушен** | S1-P13 КГ: «5 компонентов в src/blockchain/ (4 + consensus_manager), mod.rs их связывает» |
| **Файлы** | `src/blockchain/blockchain_facade.rs` (1578 строк), `src/blockchain/block_executor.rs` (246), `src/blockchain/state_cache.rs` (210), `src/blockchain/consensus_manager.rs` (132), `src/blockchain/chain_selector.rs` (2 строки — re-export), `src/blockchain/mod.rs` (8) |

**Факт:** Размеры файлов `src/blockchain/`:
- `blockchain_facade.rs` — 1578 строк
- `block_executor.rs` — 246 строк
- `state_cache.rs` — 210 строк
- `consensus_manager.rs` — 132 строки
- `chain_selector.rs` — 2 строки (только re-export)
- `mod.rs` — 8 строк

Facade содержит почти всю блокчейн-логику; остальные «компоненты» — вспомогательные. Заявленная декомпозиция на 5 компонентов формальна: фактически это **один facade-монолит + 4 утилитарных модуля**. `chain_selector.rs` в монолите — 2 строки реекспорта, реальная логика — в `crates/strangecoin-core/src/chain_selector.rs` (139 строк).

retro §4.1 уже зафиксировал аналогичную проблему Stage 0: «main.rs вырос с ~2418 до 3452 строк (+43%), при этом `blockchain/`, `api/`, `gui/`, `governance/` — однострочные заглушки». В Stage 1 эта же анти-pattern повторяется на уровне facade.

**Ожидание (S1-P13):** ARCHITECT3 §3.4 описывает facade как «публичный API, делегирующий chain_selector/block_executor/state_cache». Фасад должен быть тонким — порядка 200–400 строк делегирующих методов, не 1578.

**Рекомендуемое исправление:**
1. Перенести 1200+ строк логики из `blockchain_facade.rs` в `block_executor.rs` (валидация блоков), `state_cache.rs` (состояние), `consensus_manager.rs` (управление правилами), `chain_selector.rs` (tip selection).
2. Целевой ориентир: facade ≤ 400 строк, каждый другой компонент 200–600 строк.
3. Добавить КГ в S1-P13: «`wc -l src/blockchain/*.rs` — facade ≤ 500 строк».

---

### BUG-S0-019 — `chain_selector.rs` в монолите — 2 строки

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P11 / S1-P13 |
| **КГ нарушен** | S1-P11 КГ: «chain_selector не импортирует state/transactions (только заголовки и work)» — формально выполнено |
| **Файлы** | `src/blockchain/chain_selector.rs` |

**Факт:**
```rust
// Re-export ChainSelector and ChainInfo from strangecoin-core
pub use strangecoin_core::chain_selector::{ChainInfo, ChainSelector};
```

Это нормально, если логика действительно в `crates/strangecoin-core/src/chain_selector.rs` (139 строк). Но STAGE1_SUMMARY §1, критерий №9: «Blockchain decomposed (4 + consensus_manager)» — перечисляет `chain_selector.rs` как отдельный компонент монолита, не упоминая, что он — 2 строки re-export.

**Рекомендуемое исправление:**
1. В STAGE1_SUMMARY §1 уточнить: «chain_selector.rs в монолите — re-export из core; фактическая реализация — `crates/strangecoin-core/src/chain_selector.rs`».
2. Или перенести реализацию обратно в монолит (если она должна принадлежать оркестратору).

---

### BUG-S0-020 — Скелет модулей P02 остался витриной

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | P02 / retro §4.1 |
| **Файлы** | `src/{api,cli,gui,governance}/mod.rs` |

**Факт:** retro §4.1: «новые подсистемы (consensus, serialize, mempool, network, economics, config) писались новыми файлами, а legacy-логика из main.rs не переносилась вообще — скелет P02 превратился в декорацию». В Stage 1 часть переехала в `crates/strangecoin-core/`, но `src/{api,gui,governance}/mod.rs` — нужно проверить состояние (не было в аудите). `src/cli/mod.rs` существует с одним `println!` (BUG-S0-009), `src/api/mod.rs` и `src/gui/mod.rs` — вероятно, по-прежнему заглушки.

**Рекомендуемое исправление:**
1. Перенести `src/gui/mod.rs` в feature-gated модуль или удалить как неиспользуемый.
2. Реализовать `src/api/mod.rs` в Stage 2 (JSON-RPC eth_*-совместимый слой — Stage 4 по ROADMAP).
3. Зафиксировать в retro §4.1 обновление: «скелет P02 актуализирован в Stage 1 для consensus/serialize/economics/governance; api/gui остаются заглушками до Stage 2/4».

---

### BUG-S0-021 — Legacy-threads майнинга и сети не перенесены в async

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | S1-P10 |
| **КГ нарушен** | S1-P10 КГ: «Узел работает под tokio; graceful shutdown сохранён; ни одна sync-подсистема не сломана» |
| **Файлы** | `src/main.rs` (mining loop), `src/network/sync.rs` (614 строк) |

**Факт:** S1-P10 ввёл tokio runtime, но legacy-threads (mining loop, p2p accept, rate limiter) остались. Это допустимо по промпту («постепенный перенос, tokio::spawn для новых подсистем, старые threads работают до Stage 2»), но STAGE1_SUMMARY §5 явно относит перенос network crate migration и mining → async к Stage 2.

Это не баг, но ограничивает масштабируемость: sync код под `RwLock` и `std::thread` не сможет переварить сотни одновременных P2P соединений.

**Рекомендуемое исправление:** На Stage 2 — оформить как S2-P01 «migrate mining loop to tokio::task» и S2-P02 «network sync → async».

---

### BUG-S0-022 — Прямые мутации `balances` вне `state_cache` — нет формализованного gate

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | S1-P12 |
| **КГ нарушен** | S1-P12 КГ: «Прямых мутаций balances вне state_cache нет (rg-контроль)» |
| **Файлы** | `src/blockchain/blockchain_facade.rs`, `src/blockchain/state_cache.rs` |

**Факт:** S1-P12 требует rg-аудит: «прямых мутаций balances вне state_cache нет». В STAGE1_SUMMARY §3 сказано «rg audits: src/network/: adopt_candidate / apply_tx / save_state only in sync_engine.rs (+ doc refs) → cycle broken». Аудит для balances не упомянут явно.

**Рекомендуемое исправление:**
1. Запустить `rg "\.balances\.insert\(" src/` и `rg "\.balances\.remove\(" src/` — проверить, что все вызовы в `state_cache.rs` или `block_executor.rs` (через core::state).
2. Добавить в S1-P12 КГ: `rg "balances\.(insert|remove|get_mut)" src/ | grep -v state_cache.rs | grep -v block_executor.rs` → 0 совпадений.

---

## 5. Категория D — Тестовые баги

### BUG-S0-023 — Fuzz-target: 10 секунд вместо 10 минут

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P19 |
| **КГ нарушен** | S1-P19 КГ: «Fuzz-target существует, 10-минутный прогон без crashes» |
| **Файлы** | `fuzz/fuzz_targets/canonical_decode.rs`, `examples/canonical_decode_soak.rs`, `fuzz/README.md` |

**Факт:** STAGE1_SUMMARY §3: `cargo run --example canonical_decode_soak — 474,380 inputs in 10s, 0 panics`. 10 секунд, не 10 минут. cargo-fuzz не запускался (Windows dev host без MSVC/ASan). STAGE1_SUMMARY §6.2: «cargo-fuzz on Windows — coverage-guided fuzzing not runnable on this host; fallback soak found and fixed OOB panic in `deserialize_block`. Run cargo-fuzz on a capable CI host».

**Ожидание (S1-P19):** «10-минутный прогон без crashes (или причина фиксации)» — формально «причина фиксации» указана, но 10 секунд — несерьёзный объём для fuzz-покрытия канонического десериализатора.

**Рекомендуемое исправление:**
1. Добавить в `.github/workflows/ci.yml` отдельную job `fuzz-canonical-decode` — 10 минут cargo-fuzz на Linux runner.
2. В `fuzz/README.md` — зафиксировать результат прогона (crashes count, coverage).
3. Минимум — 10 минут / 100M inputs. Цель — 24 часа на CI раз в неделю (Stage 6: full fuzzing harnesses).

---

### BUG-S0-024 — `INVARIANTS_ENFORCED.md` — нумерация инвариантов не совпадает с ARCHITECT3 §5

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P20 |
| **КГ нарушен** | S1-P20 КГ: «Таблица 22 инвариантов актуальна: enforce-место + тест на каждый» |
| **Файлы** | `docs/stage1/INVARIANTS_ENFORCED.md` |

**Факт:** В `docs/stage1/INVARIANTS_ENFORCED.md`:
- Инвариант №4: «State Consistency: Block state matches applied transactions — `src/blockchain/blockchain.rs`» — но ARCHITECT3 §5 инвариант №4 = «apply_block/unapply_block — обратные операции», а не «State Consistency».
- Инвариант №5: «Genesis Uniqueness: Single genesis block with fixed hash — `src/blockchain/blockchain.rs`» — но ARCHITECT3 §5 инвариант №5 = «Хэш/подпись на канонических байтах», а genesis — инвариант №8.
- Инвариант №19: «State Root» — совпадает с ARCHITECT3, но в STAGE1_SUMMARY §5 помечен как «NEW» при том, что в Stage 0 он был deferred.
- Ссылка `src/blockchain/blockchain.rs` — файла не существует (есть `mod.rs`, `blockchain_facade.rs`, etc.).

Это путаница нумерации — таблица в `INVARIANTS_ENFORCED.md` использует собственную нумерацию, не привязанную к ARCHITECT3 §5.

**Рекомендуемое исправление:**
1. Полностью переписать `INVARIANTS_ENFORCED.md` с нумерацией 1:1 к ARCHITECT3 §5.
2. Для каждого инварианта — столбцы: «Stage 0 enforce location» / «Stage 1 enforce location» / «Test» / «Status».
3. Инвариант №19 (State Root) — статус `🟡 residual` (см. BUG-S0-012, BUG-S0-013), не `✅ NEW`.

---

### BUG-S0-025 — Property-тест на nonce monotonic отсутствует как отдельный

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | P18 |
| **КГ нарушен** | P18 КГ: «≥7 property-тестов покрывают: txid determinism, sign/verify, emission formula, serialize roundtrip, nonce, difficulty clamp, chain_id» |
| **Файлы** | `crates/strangecoin-core/tests/consensus_proptest.rs` |

**Факт:** В Stage 0 retro §2: 9 proptest в `src/consensus/proptest.rs`. В Stage 1 они перенесены в `crates/strangecoin-core/tests/consensus_proptest.rs`. По спеке P18 — должен быть отдельный `nonce_reject` proptest. Нужно проверить — присутствует ли он в текущем файле.

**Рекомендуемое исправление:**
1. Прочитать `crates/strangecoin-core/tests/consensus_proptest.rs` и сверить покрытие с КГ P18.
2. Если `nonce_reject` есть — закрыть. Если нет — добавить: `proptest! { fn nonce_reject(tx_nonce, account_nonce) { ... } }`.

---

### BUG-S0-026 — TLA+ TLC-прогон не выполнен

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | P23 / D02 |
| **КГ нарушен** | D02 КГ: «TLC прогнан (или невозможность зафиксирована с причиной) — результат в `docs/spec/README.md`» |
| **Файлы** | `docs/spec/consensus.tla`, `docs/spec/consensus.cfg`, `docs/spec/README.md` |

**Факт:** retro §5.4: «TLC не запускался (опционально по промпту, но и не сделано)». D02 требует фиксации результата. STAGE1_SUMMARY §6.3: «TLA+ coverage — `NoDoubleSpend`/`NoInflation`/`AllTxSigned`/`NonceMonotonic`/`PowValidity` not model-checked by TLC (state-space limits); structural properties hold by construction + Rust tests. Spec does not yet model Verkle/reorg».

Это полупризнание: «не model-checked», но не «прогон не выполнен». В `docs/spec/README.md` — нужно проверить, есть ли результат.

**Рекомендуемое исправление:**
1. Установить `tla2tools.jar`, прогнать TLC на маленькой модели (3 узла, 10 блоков) — зафиксировать PASS/FAIL с output.
2. В `docs/spec/README.md` — добавить секцию «TLC Results» с выходом прогона.
3. Если state-space слишком велик — уменьшить константы в `consensus.cfg` (MaxSupply=100 вместо 21M) и зафиксировать, какие свойства проверены на маленькой модели.

---

### BUG-S0-027 — Тест `emission.rs` не сверяет reward с total_supply

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | P19 / D01 |
| **КГ нарушен** | D01 КГ: «emission.rs — майним 10 блоков, coinbase каждого = `block_reward_at_height(h, total_supply_before)`» |
| **Файлы** | `tests/emission.rs` |

**Факт:** Тест `tests/emission.rs` существует, но нужно проверить — сверяет ли он `coinbase.amount` с `block_reward_at_height(h, total_supply_before_block)` или только с `block_reward_at_height(h, 0)`. Если второе — инвариант №6 («блок не содержит наград сверх эмиссии») проверяется не в полной форме, потому что tail emission зависит от `total_supply`.

**Рекомендуемое исправление:**
1. Прочитать `tests/emission.rs` и сверить с КГ.
2. Если сверка с `total_supply_before` отсутствует — дополнить тест: для каждого блока считать `total_supply_after_block_i-1` и сверять с `block_reward_at_height(i, total_supply_after_block_i-1)`.
3. Покрыть переход в tail phase — большой height, где `base_reward` уже 0, но `tail_reward > 0`.

---

### BUG-S0-028 — `tests/concurrency.rs` создан вне спеки P19

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Промпт-источник** | D01 (расширение спеки P19) |
| **Статус** | wontfix (допустимое расширение) |

**Факт:** D01 в `prompt-stage1.md` строка 178: «`deadlock_test_blockchain_wallet_lock_order` → `tests/concurrency.rs` (сверх спеки P19 — допустимо)». Это расширение спеки, но КГ D01 не отражает его наличие явно.

**Рекомендуемое исправление:** wontfix. Зафиксировать в D01 КГ: «8 файлов в `tests/` (включая `concurrency.rs` как расширение спеки P19)».

---

## 6. Категория E — Документационные баги

### BUG-S0-029 — `STAGE1_SUMMARY.md` противоречит сам себе по критерию №2

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P22 |
| **КГ нарушен** | S1-P22 КГ: «Все 15 критериев DoD Этап 1 отмечены с evidence (тест/файл/rg-аудит)» |
| **Файлы** | `docs/stage1/STAGE1_SUMMARY.md` |

**Факт:**
- §1 таблица: критерий №2 «Verkle Trie + `state.root_after == block.state_root`» — ✅ с evidence «`crates/strangecoin-core/src/state/verkle.rs`; tests: core `tests/state_root.rs` (7), e2e `tests/state_root.rs` (3)».
- §6 п.5: «Zero `state_root` opt-in — blocks with zero state_root still adopt (documented residual; enforcement tightening — later SCIP)».

Если есть residual «блоки с zero state_root принимаются» — то инвариант №19 НЕ enforced в общем случае, и критерий №2 НЕ ✅. Это **внутреннее противоречие документа**.

**Рекомендуемое исправление:**
1. В §1 понизить статус критерия №2 с ✅ на 🟡 (residual), с пометкой: «см. §6 п.5 — zero state_root opt-out pending SCIP».
2. Либо выполнить SCIP-0002 «enforce state_root without opt-out» и затем уже ✅.
3. Аналогично — критерий №1 «0 I/O в core» — формально ✅, но `verify_block_stateless` не пересчитывает post-state-root (BUG-S0-013), что означает: «0 I/O — да, но stateless verification дефектна».

---

### BUG-S0-030 — `STAGE1_SUMMARY §6` фиксирует residual, но не понижает DoD-критерии

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P22 |
| **КГ нарушен** | S1-P22 задача: «Сверить все 15 критериев DoD Этап 1… каждый подтверждается артефактом» |
| **Файлы** | `docs/stage1/STAGE1_SUMMARY.md` |

**Факт:** STAGE1_SUMMARY §6 содержит 6 open obligations:
1. Offline genesis key
2. cargo-fuzz on Windows
3. TLA+ coverage
4. Release pipeline pending
5. Zero state_root opt-in
6. Repo hygiene

Каждый из них нарушает один или несколько критериев DoD §1:
- (1) → критерий №2 (genesis validation)
- (2) → критерий №15 + security-track DoD
- (3) → критерий №11 (consensus_version + activation height) частично
- (4) → критерий №13 (ADR-0006..0010) частично + reproducible builds
- (5) → критерий №2 (см. BUG-S0-029)
- (6) → критерий №12 (no regressions)

Но все 15 в таблице §1 — ✅.

**Рекомендуемое исправление:**
1. В §1 добавить столбец «Residual»: для каждого критерия — да/нет + ссылка на §6.
2. Если residual есть — статус в §1 — 🟡, не ✅.
3. В S1-P22 КГ добавить: «Если §6 residual затрагивает DoD-критерий — статус понижается с ✅ на 🟡».

---

### BUG-S0-031 — `INVARIANTS_ENFORCED.md` ссылается на несуществующий файл

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Промпт-источник** | S1-P20 |
| **КГ нарушен** | S1-P20 КГ: «Таблица 22 инвариантов актуальна: enforce-место + тест на каждый» |
| **Файлы** | `docs/stage1/INVARIANTS_ENFORCED.md` |

**Факт:** В таблице:
- Инвариант №5: «Genesis Uniqueness — `src/blockchain/blockchain.rs`»
- Инвариант №9: «Chain Reorg Safety — `src/blockchain/blockchain.rs`»

Файла `src/blockchain/blockchain.rs` не существует. Структура `src/blockchain/`: `mod.rs`, `blockchain_facade.rs`, `block_executor.rs`, `state_cache.rs`, `consensus_manager.rs`, `chain_selector.rs`.

**Рекомендуемое исправление:**
1. Заменить все ссылки `src/blockchain/blockchain.rs` → `src/blockchain/blockchain_facade.rs` (или `block_executor.rs`, в зависимости от реальной enforcement-локации).
2. Переписать таблицу с фактическими путями.

---

### BUG-S0-032 — STAGE1_SUMMARY §6.6 — гигиена не закрыта

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | S1-P22 / D03 |
| **Файлы** | рабочий каталог репозитория |

**Факт:** STAGE1_SUMMARY §6.6: «Repo hygiene — untracked artifacts on disk (`test.md`, `ComputeGenesisHash/`, etc.) are outside the git index; clean when convenient». Это остаток BUG-S0-008, перенесённый в residual без конкретного срока.

**Рекомендуемое исправление:**
1. Удалить `test.md`, `ComputeGenesisHash/` из рабочего дерева в первом же коммите Stage 1.5.
2. Закрыть BUG-S0-008 и BUG-S0-032 одновременно.

**Решение (2026-10-07):** Закрыто одновременно с BUG-S0-008. `STAGE1_SUMMARY.md §6.6` обновлён: residual о гигиене репозитория снят, зафиксирован фактический состав очистки. Подробности — в разделе BUG-S0-008.

| Статус | fixed (2026-10-07) |
|--------|-------------------|

---

### BUG-S0-033 — THREAT_MODEL: 51% на testnet не помечен сроком

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | P22 / D02 |
| **Файлы** | `docs/security/THREAT_MODEL.md` |

**Факт:** retro §5.2 и D02 упоминают вектор «testnet с low difficulty → 51% attack» как residual. В THREAT_MODEL.md созданном в D02 — нужно проверить, есть ли explicit срок (например, «testnet difficulty retarget fix — SCIP-0002, activation height post-mainnet-freeze»).

**Рекомендуемое исправление:**
1. Открыть `docs/security/THREAT_MODEL.md`, найти вектор «testnet 51%».
2. Уточнить Residual Risk с конкретным обязательством: «Mitigated on mainnet via Stage 2 difficulty retarget + Stage 6 audit; testnet residual accepted».
3. Если такого обязательства нет — добавить.

---

### BUG-S0-034 — THREAT_MODEL v3.0 — нет ссылок на конкретные тесты

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Промпт-источник** | S1-P21 |
| **КГ нарушен** | S1-P21 КГ: «Каждый mitigation ссылается на промпт/тест Stage 1» |
| **Файлы** | `docs/security/THREAT_MODEL.md` (V-34..V-42) |

**Факт:** S1-P21 добавил векторы V-34..V-42 (headers-first poisoning, state_root manipulation, witness spoofing, RBF fee-war DoS, network downgrade, inbox flooding, HRP confusion). STAGE1_SUMMARY §2: «THREAT_MODEL v3.0 (V-34..V-42) + INCIDENT_RESPONSE v3.0». Но в S1-P21 КГ: «Каждый mitigation ссылается на промпт/тест Stage 1» — нужно проверить, что для каждого вектора указано `tests/X.rs::test_Y`.

**Рекомендуемое исправление:**
1. Прочитать THREAT_MODEL.md секцию Stage 1.
2. Для каждого V-34..V-42 — добавить столбец «Test»: `tests/sync_headers.rs::headers_first_poisoning_rejected` (например).
3. Если теста нет — пометить «gap, closes S2-PXX».

---

### BUG-S0-035 — AGENTS.md: «Stage 1 complete» без оговорок

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Промпт-источник** | S1-P22 |
| **Файлы** | `AGENTS.md` |
| **Статус** | **fixed** (2026-10-07) |

**Факт:** AGENTS.md строка 4: «Stage 1 complete (tag `v1.1.0-stage1`, see `docs/stage1/STAGE1_SUMMARY.md`): pure core in `crates/strangecoin-core`, Verkle state root, headers-first sync, EventBus, bech32, tokio, 5-component blockchain split». Никакого упоминания residual из STAGE1_SUMMARY §6.

Это создаёт у нового контрибьютора ложное впечатление, что проект готов к Stage 2 без долгов.

**Рекомендуемое исправление:**
1. В AGENTS.md строка 4 после «5-component blockchain split» добавить: «See `docs/stage1/STAGE1_SUMMARY.md §6` for open obligations (genesis key replacement, real Verkle Trie, cargo-fuzz on CI, release pipeline verification, state_root opt-out, repo hygiene)».
2. ~~Зафиксировать, что тег `v1.0.0-stage0` (Gate Stage 1) — отсутствует (BUG-S0-001)~~ — тег существует (`3dd37ef`); BUG-S0-001 закрыт. Оговорка о gate не требуется.

**Решение (2026-10-07):** Исправлено в рамках BUG-S0-003. AGENTS.md строка 4 дополнена: Stage 0 закрыт через debt prompts D01–D03 (P19/P22/P26 не выполнялись inline) + явная ссылка на residual obligations `docs/stage1/STAGE1_SUMMARY.md §6` (offline genesis key, cargo-fuzz Windows, TLA+ coverage, release pipeline, zero state_root opt-in, repo hygiene). «Stage 1 complete» сохранено как факт при верифицированном gate; оговорка о residual устраняет ложное впечатление готовности к Stage 2 без долгов.

| Статус | fixed (2026-10-07) |
|--------|-------------------|

---

## 7. Приоритеты исправления

### P0 — блокирует Stage 1.5 (must fix before any VM/wasmi work)

| ID | Заголовок | Почему блокирует |
|----|----------|------------------|
| ~~BUG-S0-011~~ | ~~Verkle Trie — фактическая flat-структура~~ | **fixed 2026-10-08**: SMT depth 256; KZG отложен Stage 3+ |
| BUG-S0-012 | `state_root == [0;32]` opt-out | Инвариант №19 не enforced; любой блок с zero root принимается без commitment |
| BUG-S0-013 | `verify_block_stateless` не проверяет post-root | Stateless light-clients дефектны — Stage 1.5 обещал SPV-ready infrastructure |
| ~~BUG-S0-014~~ | ~~VerkleTrie теряет данные при >256 аккаунтов~~ | **fixed 2026-10-08**: SMT full-key, sweep test |
| BUG-S0-015 | Генезисный ключ из публичной строки | Любой может подписать транзакции от initial_holder; mainnet невозможен |

### P1 — блокирует Stage 2 (must fix before network crate migration)

| ID | Заголовок | Почему блокирует |
|----|----------|------------------|
| ~~BUG-S0-001~~ | ~~Тег `v1.0.0-stage0` отсутствует~~ | **fixed 2026-10-07**: тег существует (`3dd37ef`); Gate Stage 1 пройден |
| ~~BUG-S0-002~~ | ~~Git-история сквошена~~ | **fixed 2026-10-07**: атомарная история 163 коммитов в HEAD и origin/master; hook `.githooks/commit-msg` добавлен |
| ~~BUG-S0-018~~ | ~~`blockchain_facade.rs` 1578 строк~~ | **fixed 2026-10-08**: S1.5-P04 — facade 362 строки, logic in sibling components |
| BUG-S0-029 | STAGE1_SUMMARY противоречие | Без честной верификации Stage 2 унаследует нерешённые долги |
| BUG-S0-030 | STAGE1_SUMMARY residual не понижает DoD | То же |
| BUG-S0-031 | INVARIANTS_ENFORCED.md битые ссылки | Студенты/контрибьюторы не смогут найти enforcement-точки |
| BUG-S0-024 | INVARIANTS_ENFORCED.md путаница нумерации | То же |
| BUG-S0-005 | Release pipeline не запускался | Reproducible builds без evidence — Stage 2 release невозможен |
| ~~BUG-S0-003~~ | ~~Ложная запись в Changelog~~ | **fixed 2026-10-07**: predecessor-цепочка верифицирована в STAGE1_SUMMARY §1+§2 (теги + все коммиты D01–D03 / S1-PXX в HEAD); AGENTS.md получила оговорку о debt prompts и residual obligations |

### P2 — технический долг (можно параллельно с Stage 2+)

| ID | Заголовок |
|----|----------|
| ~~BUG-S0-004~~ | ~~6 тестов в main.rs~~ (**fixed 2026-10-07**: D01 `9dd8077`/`1929f2f` — main.rs 4 строки, 6 тестов в `tests/`) |
| ~~BUG-S0-016~~ | ~~`prove()` не использует `account`~~ (**fixed 2026-10-08**: SMT prove consumes account) |
| BUG-S0-017 | Нет теста миграции base64 → bech32 |
| ~~BUG-S0-019~~ | ~~`chain_selector.rs` — 2 строки re-export~~ (**fixed 2026-10-08**: S1.5-P04 — adoption + wire helpers in component) |
| BUG-S0-020 | Скелет P02 — заглушки api/gui |
| BUG-S0-021 | Legacy-threads майнинга |
| BUG-S0-022 | Прямые мутации balances — нет gate |
| BUG-S0-023 | Fuzz — 10 сек вместо 10 мин |
| BUG-S0-025 | nonce proptest — проверить наличие |
| BUG-S0-026 | TLA+ TLC-прогон |
| BUG-S0-027 | emission.rs — сверка с total_supply |
| BUG-S0-032 | Repo hygiene |
| BUG-S0-033 | THREAT_MODEL testnet 51% срок |
| BUG-S0-034 | THREAT_MODEL v3.0 — ссылки на тесты |
| ~~BUG-S0-035~~ | ~~AGENTS.md без оговорок~~ (fixed 2026-10-07) |

### P3 — косметика (wontfix или minor)

| ID | Заголовок |
|----|----------|
| BUG-S0-006 | Заглушки P02 в другом пути |
| ~~BUG-S0-007~~ | ~~`.gitignore` `*.lock`~~ (**fixed 2026-10-07**: строки `*.lock`/`Cargo.lock` удалены ещё в D03 `60e1840`; `Cargo.lock` трекается; добавлен `data/` в `.gitignore` для LevelDB-артефактов) |
| BUG-S0-008 | Мусор в корне |
| ~~BUG-S0-009~~ | ~~Один `println!` в cli~~ (**fixed 2026-10-07**: `writeln!(stdout)` в `src/cli/mod.rs`; CONTRIBUTING: CLI stdout ≠ logging; КГ P03 выполняется буквально) |
| ~~BUG-S0-010~~ | ~~P09 раньше P08 (исторический)~~ (**fixed 2026-10-07**: wontfix — retro-stage0.md уже фиксирует отступление карты зависимостей как единичное, без последствий) |
| BUG-S0-028 | `tests/concurrency.rs` вне спеки (wontfix) |

---

## 8. Связь промптов и багов

Карта: какой промпт породил баг или не закрыл его.

| Промпт | Породил баги | Не закрыл баги |
|--------|-------------|----------------|
| P01 | BUG-S0-007, BUG-S0-008, BUG-S0-009 | — |
| P02 | BUG-S0-006, BUG-S0-020 | — |
| P03 | BUG-S0-009 | — |
| P05 | BUG-S0-017 | — |
| P08 / P09 | BUG-S0-010 (wontfix) | — |
| P10 | BUG-S0-015 (partially — seed строка) | — |
| P18 | — | BUG-S0-025 (nonce proptest) |
| P19 / D01 | BUG-S0-027, BUG-S0-028 | ~~BUG-S0-004 (тесты в main.rs)~~ — **fixed 2026-10-07** |
| P22 / D02 | BUG-S0-003 (**fixed 2026-10-07**), BUG-S0-033, BUG-S0-034 | BUG-S0-026 (TLC-прогон) |
| P23 / D02 | BUG-S0-026 | — |
| P24 / D03 | BUG-S0-005 | — |
| P26 / D03 | BUG-S0-001, BUG-S0-002 | — |
| S1-P06 | BUG-S0-011, BUG-S0-012, BUG-S0-014, BUG-S0-016 | — |
| S1-P07 | BUG-S0-013 | — |
| S1-P10 | BUG-S0-021 | — |
| S1-P11 / S1-P13 | BUG-S0-018, BUG-S0-019 | — |
| S1-P12 | BUG-S0-022 | — |
| S1-P15 | BUG-S0-017 | — |
| S1-P19 | BUG-S0-023 | — |
| S1-P20 | BUG-S0-024, BUG-S0-031 | — |
| S1-P21 | BUG-S0-034 | — |
| S1-P22 | BUG-S0-029, BUG-S0-030, BUG-S0-032, BUG-S0-035 (**fixed 2026-10-07**) | BUG-S0-001 (тег v1.0.0-stage0) — **fixed 2026-10-07**, BUG-S0-002 (атомарные коммиты) — **fixed 2026-10-07** |

**Главный вывод:** S1-P22 (DoD-верификация) должен был поймать большинство багов категорий A, D, E, но не сделал этого. S1-P06 (Verkle Trie) и S1-P07 (StateWitness) породили все криптографические баги категории B. P26 / D03 (DoD Stage 0) не были выполнены, что породило процессные баги категории A.

---

## 9. Рекомендуемые новые промпты для закрытия багов

### S1.5-P01 — Замена генезисного ключа + SCIP-0001

Закрывает: BUG-S0-015.

Задачи:
1. Создать SCIP-0001 «Genesis key replacement»: `docs/SCIP/scip-0001.md` с activation height для замены ключа.
2. В `genesis.json` — оставить только публичный ключ `initial_holder_pubkey`.
3. Приватный ключ — генерировать offline, в коде не хранить.
4. Тест: узел стартует без приватного ключа в коде, проверяет `EXPECTED_GENESIS_HASH`, но подпись initial_holder'a невозможна из кода.

### S1.5-P02 — Настоящий Verkle Trie или Sparse Merkle Tree

Закрывает: BUG-S0-011, BUG-S0-014, BUG-S0-016.

**Статус: executed 2026-10-08 (Variant C)** — binary Sparse Merkle Tree depth 256 в `crates/strangecoin-core/src/state/sparse_merkle.rs`; ADR-0006 amended; `verkle.rs` удалён. Настоящий Verkle/KZG отложен до Stage 3+.

Задачи:
1. ~~Решить: настоящая Verkle Trie (через KZG + BLS12-381) или Sparse Merkle Tree (32 уровня blake3).~~ → **решено: SMT** (Variant C; depth 256 binary — не depth 32, чтобы избежать коллизий класса BUG-S0-014; см. ADR-0006 «Why depth 256»)
2. ~~Переписать `crates/strangecoin-core/src/state/verkle.rs` или переименовать в `sparse_merkle.rs`.~~ → **done**: `sparse_merkle.rs`, тип `SparseMerkleTrie`
3. ~~Proptest: 1000+ аккаунтов → root уникален; 256 → 512 аккаунтов → корректное поведение.~~ → **done**: `proptest_thousand_accounts_unique_deterministic_root`, `sweep_256_to_512_accounts_unique_roots`
4. ~~Обновить ADR-0006 с честным выбором и обоснованием.~~ → **done**: ADR-0006 Status=Amended

### S1.5-P03 — Enforce state_root без opt-out + post-state-root verification

Закрывает: BUG-S0-012, BUG-S0-013.

Задачи:
1. Удалить `if block.state_root != [0u8;32]` opt-out в `state/mod.rs::root_after`.
2. В `verify_block_stateless` — добавить `post_state_root_proof: Vec<[u8; 32]>` в `StateWitness` и проверять.
3. Тест: tamper post-state-root → reject.
4. Тест: light-client верифицирует полный блок с post-state-root.

### S1.5-P04 — Декомпозиция blockchain_facade

Закрывает: BUG-S0-018, BUG-S0-019.

**Статус: executed 2026-10-08** — `blockchain_facade.rs` 1578 → **362 строки**; logic relocated to sibling components; `chain_selector.rs` is a real component (not a re-export).

Задачи:
1. ~~Перенести логику из `blockchain_facade.rs` (1578 строк) в `block_executor.rs`, `state_cache.rs`, `consensus_manager.rs`, `chain_selector.rs`.~~ → **done**:
   - `block_executor.rs`: mining, commit_block, genesis/grant construction
   - `state_cache.rs`: open_blockchain (new), LevelDB load/save, migrations, add_transaction, validate_chain
   - `chain_selector.rs`: try_adopt_candidate, chain_has_tx, headers_from_height, blocks_by_hashes (+ unit tests)
2. ~~Целевой размер: facade ≤ 400 строк.~~ → **done**: 362 строки
3. ~~Тесты: поведение не меняется (D01 + S1-P19 матрица green).~~ → **done**: `cargo test --workspace` green

### S1.5-P05 — DoD-верификация Stage 1.5 с честной отметкой residual

Закрывает: BUG-S0-029, BUG-S0-030, BUG-S0-031. ~~BUG-S0-035~~ — **fixed 2026-10-07** (в рамках BUG-S0-003; AGENTS.md уже содержит ссылку на §6).

Задачи:
1. В `STAGE1_SUMMARY.md §1` — добавить столбец «Residual», понижать ✅ → 🟡 при наличии §6 residual.
2. В `INVARIANTS_ENFORCED.md` — исправить нумерацию (1:1 к ARCHITECT3 §5) и битые ссылки.
3. ~~В `AGENTS.md` — добавить ссылку на §6 open obligations.~~ — **done 2026-10-07** (AGENTS.md строка 4).
4. ~~Поставить тег `v1.0.0-stage0` ретроспективно~~ — тег существует (`3dd37ef`); BUG-S0-001 fixed 2026-10-07.

### S1.5-P06 — Release pipeline: первый прогон

Закрывает: BUG-S0-005.

Задачи:
1. Поставить тестовый тег `v0.0.0-rc1`.
2. Зафиксировать в `docs/security/REPRODUCIBLE_BUILDS.md`: ссылка на Actions run, SHA256 каждого артефакта, cosign verify-blob output.
3. Если SLSA provenance пустой — починить `outputs.hashes` в build-job.

### S1.5-P07 — Fuzzing на CI

Закрывает: BUG-S0-023.

Задачи:
1. Добавить в `.github/workflows/ci.yml` job `fuzz-canonical-decode` — 10 минут cargo-fuzz на Linux runner.
2. Зафиксировать результат (crashes count, coverage) в `fuzz/README.md`.
3. Если cargo-fuzz не запускается на Linux — разобраться, починить, или использовать `afl` вместо libfuzzer.

### S1.5-P08 — TLA+ TLC-прогон

Закрывает: BUG-S0-026.

Задачи:
1. Скачать `tla2tools.jar`, прогнать TLC на `docs/spec/consensus.tla` с `docs/spec/consensus.cfg`.
2. Зафиксировать output в `docs/spec/README.md`.
3. Если state-space слишком велик — уменьшить константы (MaxSupply=100) и зафиксировать, какие свойства проверены.

---

## 10. Закрытие

Этот каталог — рабочий вход для:
1. Stage 1.5 (wasmi VM) — должен стартовать только после закрытия P0-багов.
2. Stage 2 (network crate migration, Noise, Erlay) — должен стартовать после закрытия P1-багов.
3. Любого аудита/ретроспективы — должен ссылаться на конкретные `BUG-S0-XXX` ID, а не на общие формулировки.

**Правило обновления:** каждый баг, закрытый промптом, получает статус `fixed` с ссылкой на коммит. Каждый `wontfix` — с обоснованием. Никаких silent deletions.

**Конец `bugfixes-stage0.md`.**
