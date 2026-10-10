# bugfixes-stage1.md — Каталог открытых багов и несоответствий Stage 1.5+

**Версия:** 1.0
**Дата:** 2026-10-10
**Источники:**
- `analytics/bugfixes-stage0.md` (35 багов BUG-S0-001..BUG-S0-035, 2026-10-07)
- `download/strangecoin_audit_report.md` (независимый аудит по коду, 2026-10-10)
- прямой аудит исходного кода `crates/strangecoin-core/`, `src/blockchain/`, `src/network/`, `src/mempool/`, `src/wallet.rs`, `tests/`, `docs/`
- `git log --oneline` (190 коммитов), `git tag -l` (`v1.0.0-stage0`, `v1.1.0-stage1`)

**Метод:** Для каждого открытого пункта — сверка с фактическим кодом (file path + line numbers), git-историей и документами. Все факты ниже подтверждены `Read`/`Grep`/`Bash` по коду.

**Цель:** превратить остаточные расхождения Stage 0/1 (после закрытия 30 из 35 багов `bugfixes-stage0.md`) и новые находки независимого аудита в измеримый, трассируемый список задач для Stage 1.5+.

---

## 0. Контекст: что уже закрыто

`bugfixes-stage0.md` содержал 35 багов. По состоянию на 2026-10-10 (HEAD `53930ea`, tag `v1.1.0-stage1`):

| Категория | Кол-во | ID |
|---|---|---|
| **fixed** | 29 | BUG-S0-001, 002, 003, 004, 007, 008, 009, 011, 014, 015, 016, 017, 018, 019, 020, 021, 022, 023, 024, 025, 026, 027, 029, 030, 031, 032, 033, 034, 035 |
| **wontfix** (задокументировано) | 3 | BUG-S0-006 (P02 stubs — Strangler pattern), BUG-S0-010 (P09 before P08 — исторический факт), BUG-S0-028 (concurrency.rs — расширение спеки) |
| **partial** | 1 | BUG-S0-005 (release pipeline написан, но ни разу не запускался на CI) |
| **open** | 2 | BUG-S0-012 (state_root opt-out), BUG-S0-013 (verify_block_stateless post-root) |

### Почему в `strangecoin-audit-report.md` остались «нереализованные пункты»

Первоначальный отчёт `strangecoin_audit_report.md` строился по **shallow clone** (`git clone --depth 1`), что скрыло теги и атомарную историю коммитов. Это привело к **двум ложным срабатываниям**:

1. **«Нет git-тегов»** — фактически теги `v1.0.0-stage0` и `v1.1.0-stage1` **существуют** на origin (`git ls-remote --tags origin` показывает их; `git fetch --tags` вытягивает локально). `BUG-S0-001` правильно закрыт.
2. **«История squashed в 1 коммит»** — фактически репо содержит **190 атомарных коммитов** с ID `[P0X]`/`[D0X]`/`[S1-PXX]`/`[BUG-S0-XXX]` после `git fetch --unshallow`. `BUG-S0-002` правильно закрыт.

Кроме того, независимый аудит по 51 пункту промптов (`prompt-stage0.md` P01–P26 + `prompt-stage1.md` D01–D03 + S1-P01..S1-P22) нашёл **12 проблем**, которые **не вошли** в `bugfixes-stage0.md` — потому что `bugfixes-stage0.md` фокусировался на расхождениях с КГ-чеклистами (детали реализации), а не на архитектурных дефектах консенсуса/сети, которые независимый аудит посчитал критичными. Эти 12 проблем переносятся в `bugfixes-stage1.md` как `BUG-S1-NNN`.

---

## 1. Соглашения

- **ID бага:** `BUG-S1-NNN` (сквозная нумерация для Stage 1.5+, продолжение после BUG-S0-035)
- **Категория:** A — процессная; B — криптографическая; C — архитектурная; D — тестовая; E — документационная; F — сетевая; G — операционная
- **Серьёзность:**
  - **C (Critical)** — ломает инвариант консенсуса или блокирует mainnet
  - **H (High)** — нарушает КГ промпта или Process-DoD; требует правки до следующей стадии
  - **M (Medium)** — расхождение с заявленным, но не критично для ядра
  - **L (Low)** — косметика, гигиена, мелкие отклонения
- **Источник:** `BUG-S0-NNN` (перенос из Stage 0), `P0X/S1-PXX` (промпт), `audit` (независимый аудит 2026-10-10)
- **Статус:** `open` (по умолчанию) / `partial` / `fixed` (после исправления — внести запись в `Решение`)
- **Stage target:** Stage 1.5 / Stage 2 / Stage 3+ / Stage 5 / Stage 6 / Stage 7 / ops

---

## 2. Перенос из `bugfixes-stage0.md` (остались открытыми или частично закрытыми)

---

### BUG-S1-001 — Release pipeline ни разу не запускался на CI

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Источник** | BUG-S0-005 (перенос); инвариант #22; D03 КГ п.6; P24 КГ |
| **Категория** | G — операционная |
| **Статус** | **partial** |
| **Stage target** | Stage 1.5-P06 / ops gate |

**Факт (по коду, 2026-10-10):**
- `.github/workflows/release.yml` существует (7067 байт), triggers on `push: tags: ['v*']`, 6 matrix targets, SHA256, cosign keyless OIDC, SLSA3 provenance.
- Теги `v1.0.0-stage0` (commit `3dd37ef`) и `v1.1.0-stage1` (commit `68e358f`) **существуют** в git (подтверждено `git ls-remote --tags origin`).
- `docs/security/REPRODUCIBLE_BUILDS.md` (113 строк) описывает verify-флоу, но **содержит 0 ссылок на Actions run URL**, 0 опубликованных SHA256, 0 `cosign verify-blob`/`slsa-verifier` output.
- `docs/stage1/STAGE1_SUMMARY.md:28` и `:141` явно признают: «first `v*` tag run still pending GitHub Actions verification».
- `docs/stage1/INVARIANTS_ENFORCED.md` #22 = 🟡 residual.

**Ожидание:**
- После push тега `v*` должен сработать `release.yml` → собрать 6 артефактов → опубликовать Release с SHA256 + cosign signature + SLSA provenance.
- В `REPRODUCIBLE_BUILDS.md` должны появиться: Actions run URL, artifact SHA256s, `cosign verify-blob`/`slsa-verifier` outputs для каждого артефакта.
- `INVARIANTS_ENFORCED.md` #22 должен стать ✅.

**Воспроизводимость:**
```bash
git clone https://github.com/Reider85/strangecoin.git && cd strangecoin
grep -E "github.com/.*actions/runs|cosign verify-blob" docs/security/REPRODUCIBLE_BUILDS.md
# Вывод: пусто (нет evidence)
```

**Рекомендуемое исправление:**
1. Push тег `v0.0.1-rc1` (если v1.0.0/v1.1.0 уже использованы для аудита) → триггер `release.yml`.
2. Дождаться зелёного CI на всех 6 targets.
3. Скопировать Actions run URL в `REPRODUCIBLE_BUILDS.md`.
4. Скачать artifacts, посчитать `sha256sum`, добавить в `REPRODUCIBLE_BUILDS.md`.
5. Запустить `cosign verify-blob --certificate-identity=<workflow> --certificate-oidc-issuer=https://token.actions.githubusercontent.com --signature sig artifact`, добавить вывод.
6. Обновить `INVARIANTS_ENFORCED.md` #22 с 🟡 на ✅.

---

### BUG-S1-002 — `state_root == [0;32]` opt-out ломает инвариант №19

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Источник** | BUG-S0-012 (перенос); инвариант #19; S1-P06 КГ; S1-P07 КГ |
| **Категория** | B — криптографическая |
| **Статус** | **open** |
| **Stage target** | S1.5-P03 |

**Факт (по коду, 2026-10-10):**
- `crates/strangecoin-core/src/state/mod.rs:23`:
  ```rust
  if block.state_root != [0u8; 32] && block.state_root != computed {
      return Err(crate::error::CoreError::StateRootMismatch { ... });
  }
  ```
  Opt-out для zero state_root **всё ещё присутствует**.
- `src/blockchain/block_executor.rs:136`:
  ```rust
  if block.state_root != [0u8; 32] && block.state_root != compute_state_root(&new_state.balances)
  ```
  Тот же opt-out.
- Любой блок с `state_root == [0;32]` принимается без проверки commitment → light-client не может доверять `block.state_root`.
- `docs/stage1/INVARIANTS_ENFORCED.md` #19 = 🟡 residual.
- `STAGE1_SUMMARY.md §6.5` явно признаёт residual.

**Ожидание:**
- Инвариант #19: «`state_root` обязательна для каждого блока (кроме genesis)».
- Блок с `state_root == [0;32]` должен **отвергаться** на mainnet/testnet.
- На regtest допускается флаг `Config.allow_zero_state_root` (аналог `allow_grant_blocks`).

**Воспроизводимость:**
```bash
rg "state_root != \[0u8; 32\]" crates/strangecoin-core/src/ src/blockchain/
# crates/strangecoin-core/src/state/mod.rs:23
# src/blockchain/block_executor.rs:136
```

**Рекомендуемое исправление:**
1. Создать `Config.allow_zero_state_root: bool` (default `false` на mainnet/testnet, `true` на regtest).
2. В `state/mod.rs:23` и `block_executor.rs:136` убрать безусловный opt-out; заменить на `if !view.allow_zero_state_root || block.state_root != [0u8;32] { /* обязательная проверка */ }`.
3. Создать миграцию chain state для regtest-блоков с zero state_root → пересчёт через `state_cache::rebuild_from_chain`.
4. Написать тест: mainnet-блок с `state_root == [0;32]` → `Err(StateRootMismatch)`.
5. Написать тест: regtest-блок с `state_root == [0;32]` и `allow_zero_state_root=true` → принимается.
6. Обновить `INVARIANTS_ENFORCED.md` #19 с 🟡 на ✅.
7. Активировать через SCIP с activation height (по правилу retro §8.1 — consensus-меняющие изменения через SCIP).

---

### BUG-S1-003 — `verify_block_stateless` не пересчитывает post-state-root

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Источник** | BUG-S0-013 (перенос); S1-P07 КГ; инвариант #19 |
| **Категория** | B — криптографическая |
| **Статус** | **open** |
| **Stage target** | S1.5-P03 |

**Факт (по коду, 2026-10-10):**
- `crates/strangecoin-core/src/state/witness.rs:51-89` — `verify_block_stateless(parent_state_root, block, witness)`:
  - Проверяет pre-state proofs (что `witness.proofs` соответствуют `parent_state_root`).
  - Реконструирует partial state из witness.
  - Применяет `apply_block(&reconstructed, block)`.
  - **НЕ** пересчитывает post-state root из результата.
  - **НЕ** сравнивает post-root с `block.state_root`.
- Коммент в `witness.rs:81-85` явно признаёт: «the full post-state root cannot be recomputed here. Checking it is the job of a full node (`root_after` / `validate_chain`)».
- Light-client, который вызывает `verify_block_stateless`, не может проверить, что `block.state_root` корректен → атакующий может подсунуть любое значение `state_root` и prover должен лишь собрать consistent pre-state proofs.
- `STAGE1_SUMMARY.md §6.5` подтверждает residual.
- `docs/security/THREAT_MODEL.md V-36` документирует риск.

**Ожидание:**
- `verify_block_stateless` должна:
  1. Проверить pre-state proofs (как сейчас).
  2. Применить block к partial state.
  3. Пересчитать post-state root из результата.
  4. Сравнить с `block.state_root` → `Err(PostStateRootMismatch)` если не совпадает.

**Воспроизводимость:**
```bash
sed -n '51,90p' crates/strangecoin-core/src/state/witness.rs
# Видно: нет вызова root_after(...); нет сравнения с block.state_root.
```

**Рекомендуемое исправление (одна из 3 опций):**

**Опция A (близкая к Stage 1.5):** Расширить `StateWitness` с `post_state_root_proof: Vec<[u8;32]>` — Merkle path от корня до изменившихся листьев после apply. Тогда `verify_block_stateless` проверит и pre-state proofs, и post-state proof.

**Опция B (отложенная на Stage 3+):** Реальный Verkle Trie с KZG commitments и multi-opening proofs — one proof покрывает и pre, и post state. Требует `blst` или `ark-bls12-381` crate.

**Опция C (минимум, честная):** Переименовать `verify_block_stateless` → `verify_pre_state_proofs` (честное имя, не обещает statelessness); обновить S1-P07 КГ; документировать light-client promise как «pre-state-only verification, full state commitment requires full node».

**Рекомендация:** Опция A (S1.5-P03). Опция C как interim, если Stage 1.5-P03 не успевает.

---

## 3. Новые баги из независимого аудита (не вошедшие в `bugfixes-stage0.md`)

---

### BUG-S1-004 — `current_chain_id()` захардкожен в `CHAIN_ID_REGTEST`

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Источник** | audit (2026-10-10); P10 КГ (детерминированный генезис); P05 КГ (chain_id) |
| **Категория** | C — архитектурная |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P05 (или раньше) |

**Факт (по коду, 2026-10-10):**
- `crates/strangecoin-core/src/consensus.rs:171-173`:
  ```rust
  pub fn current_chain_id() -> u32 {
      CHAIN_ID_REGTEST
  }
  ```
- Хардкод. Никакого условия, никакой конфигурации.
- На дефолтном старте `is_regtest(current_chain_id())` всегда `true`.
- Mainnet-ветка `EXPECTED_GENESIS_HASH` валидации (`block_executor.rs:517-518`) — **мёртвый код**.
- Все тесты (23 файла) выполняются на regtest; mainnet/testnet структурно **не протестированы**.
- `genesis.json:3` declares `"network_id": 1` (mainnet), но код игнорирует это.
- SCIP-0001 (genesis key) упоминает mainnet-freeze, но без переключения `current_chain_id()` это невозможно.

**Ожидание (P10, P05, S1-P14):**
- `current_chain_id()` должен возвращать `Config.network_id` (полученный из `config.toml`).
- Mainnet-узел с `network_id=1` должен валидировать `EXPECTED_GENESIS_HASH`.
- Testnet-узел с `network_id=2` — отдельный genesis.
- Regtest-узел с `network_id=3` — собственный genesis (как сейчас).

**Воспроизводимость:**
```bash
sed -n '170,175p' crates/strangecoin-core/src/consensus.rs
# pub fn current_chain_id() -> u32 {
#     CHAIN_ID_REGTEST
# }
```

**Рекомендуемое исправление:**
1. Удалить `pub fn current_chain_id() -> u32` из core (core должен быть stateless относительно chain).
2. В `Config` добавить поле `network_id: u32` (уже есть, `config.rs:15`).
3. Все вызовы `current_chain_id()` в `src/` заменить на `view.chain_id()` (через `BlockView` или новый `ChainContext`).
4. В `BlockView`/`ChainContext` добавить `chain_id: u32` — пробрасывается из `Config` через `BlockchainFacade::new(port, chain_id)`.
5. Mainnet startup path (`block_executor.rs:517-518`) активировать через `view.chain_id() == CHAIN_ID_MAINNET` → сравнение с `EXPECTED_GENESIS_HASH`.
6. Тест `tests/genesis_key.rs` расширить: mainnet → `EXPECTED_GENESIS_HASH`, testnet → отдельный hash, regtest → own genesis.
7. Тест на `chain_id=2` (testnet) tx отвергается на mainnet-узле — отдельный integration test.

---

### BUG-S1-005 — Пер-сообщенный rate-limit не enforced

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Источник** | audit; P13 КГ; THREAT_MODEL V-16/V-17 |
| **Категория** | F — сетевая |
| **Статус** | **open** |
| **Stage target** | Stage 2-P03 (network crate migration) |

**Факт (по коду, 2026-10-10):**
- `src/network/rate_limiter.rs:55-104` — `RateLimiter::check(peer)` корректно работает (4 теста pass).
- `src/lib.rs:571` — `handle_connection` вызывает `rate_limiter.check(peer_addr)` **один раз** на вход TCP-коннекта.
- Внутри request-loop (`lib.rs:620-732`) — **нет** повторного вызова `rate_limiter.check()` на каждое сообщение.
- Long-lived peer с одним TCP-коннектом может стримить **неограниченное количество** сообщений без per-message throttling.
- Реальная защита от спама — bounded `SyncEngine` inbox (`HEADERS_INBOX_CAP=64`, `BLOCKS_INBOX_CAP=256`) + `rate_limiter.ban(addr)` на inbox overflow.
- Это дефолт-фикс, но **не** per-message token bucket, который требовал P13.

**Ожидание (P13):**
- «Peer sends 200 msgs in 1 sec → ban after 100th» — это per-message rate-limit, не per-connection.
- Token bucket / leaky bucket per peer, пополняемый по времени, проверяемый на каждое входящее сообщение.

**Воспроизводимость:**
```bash
rg "rate_limiter\.check" src/
# src/lib.rs:348  (sync_blockchain, outbound peer)
# src/lib.rs:571  (handle_connection, once per connection)
# src/network/sync_engine.rs:144-150  (inbox overflow → ban)
# — нигде в request-loop lib.rs:620-732
```

**Рекомендуемое исправление:**
1. В `handle_connection` request-loop (`lib.rs:620-732`) добавить `rate_limiter.check(peer_addr)?` перед каждой `read_length_prefixed` операцией.
2. Если `Err(PeerBanned)` → лог + `break` (закрыть соединение).
3. Добавить тест: открыть 1 TCP-коннект, отправить 200 messages за < 1s → узел должен бан после 100-го.
4. Дополнительно: рассмотреть **per-message-type** rate-limit (GET_HEADERS/HEADERS/BLOCKS — разные bucket'ы).
5. После Stage 2-P03 (network crate migration) — вынести rate_limiter в `strangecoin-net` crate.

---

### BUG-S1-006 — Single-block size check отсутствует в `validate_and_apply`

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Источник** | audit; P12 КГ (лимиты размеров) |
| **Категория** | F — сетевая / C — архитектурная |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P07 |

**Факт (по коду, 2026-10-10):**
- `src/network/protocol.rs:10` — `MAX_BLOCK_SIZE = 4 * 1024 * 1024` (4 MB).
- `src/network/protocol.rs:11` — `MAX_TX_SIZE = 256 * 1024` (256 KB).
- `parse_blocks` (`protocol.rs:215`) проверяет `len > MAX_BLOCK_SIZE` per item — wire-level ✓.
- `validate_chain` (`state_cache.rs:393-402`) проверяет размер каждого блока — whole-chain scan ✓.
- `validate_and_apply` (`block_executor.rs:85-149`) — **НЕ** проверяет размер блока.
- `commit_block` (`block_executor.rs:366-381`) — **НЕ** проверяет размер блока.
- Блок, приходящий через `adopt_candidate` из `ChainSnapshot` (deserialized from network) — обходит размерный инвариант на validation-уровне. Только wire-уровень ловит.

**Ожидание (P12):**
- «В `validate_block` — проверка `block.size() <= MAX_BLOCK_SIZE` ДО любых других проверок».

**Воспроизводимость:**
```bash
rg "MAX_BLOCK_SIZE" src/
# src/network/protocol.rs:10  (const)
# src/network/protocol.rs:188, 215  (wire level)
# src/blockchain/state_cache.rs:393-402  (validate_chain whole scan)
# src/blockchain/block_executor.rs:0 hits — нет проверки в validate_and_apply/commit_block
```

**Рекомендуемое исправление:**
1. В `validate_and_apply` (`block_executor.rs:85-149`) первой проверкой добавить:
   ```rust
   let block_size = serialize::serialize_block(block).len();
   if block_size > MAX_BLOCK_SIZE { return Err(SizeLimitExceeded("block")); }
   ```
2. Дополнительно: проверить каждый tx в блоке против `MAX_TX_SIZE`.
3. Тест: блок 5 MB (или с tx 300 KB) → `validate_and_apply` возвращает `SizeLimitExceeded`.
4. Тест: блок 3 MB → принимается.

---

### BUG-S1-007 — Golden-векторы блоков таутологичны

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P06 КГ (golden vectors); BUG-S0-024 (родственная проблема) |
| **Категория** | D — тестовая |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P08 |

**Факт (по коду, 2026-10-10):**
- `crates/strangecoin-core/tests/serialize_golden.rs` — golden vectors для 3 transactions корректны (сравнение с literal hex).
- `serialize_golden.rs:201-204` — golden для **блока 1** сравнивает `actual_hex == actual_hex.clone()` — **таутология** (всегда true).
- `serialize_golden.rs:243-247` — то же для блока 2.
- `serialize_golden.rs:329-333` — то же для блока 3.
- Любое изменение `serialize_block`/`serialize_block_header` **не** сломает эти тесты.
- `serialize_block_header` имеет тест `block_hash_deterministic` (отдельный), но **сериализация блока как таковая не pinned**.

**Ожидание (P06):**
- «Golden-вектор: сериализация test-vector-1 = зафиксированные байты (hex)» — для tx AND block.
- Любое изменение кодировки = сломает тест.

**Воспроизводимость:**
```bash
sed -n '195,210p' crates/strangecoin-core/tests/serialize_golden.rs
# let actual_hex = hex::encode(serialize_block(&block));
# assert_eq!(actual_hex, actual_hex.clone());  // ← всегда true
```

**Рекомендуемое исправление:**
1. Заменить `actual_hex.clone()` на literal hex string (вычисленный один раз через `cargo run --example print_serialized_block` или ручной расчёт).
2. Зафиксировать как golden vector — каноническая сериализация 3 эталонных блоков.
3. Добавить CI-проверку: если `serialize_block`/`serialize_block_header` меняется → golden vector test падает → разработчик обязан либо обновить вектор с пометкой `FORMAT_VERSION` bump + ADR, либо откатить изменение.
4. Дополнительно: для tx golden vectors — сделать `serialize_deserialize_roundtrip` (есть в proptest), но golden hex — более сильный тест (ловит и order, и padding, и т.д.).

---

### BUG-S1-008 — Мёртвая зависимость `sha2` в core

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Источник** | audit; P06 (PoW hash) |
| **Категория** | E — гигиена |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P09 |

**Факт (по коду, 2026-10-10):**
- `crates/strangecoin-core/Cargo.toml:13` — `sha2 = "0.10"`.
- `rg "sha2::|Sha256" crates/strangecoin-core/src/` → **0 совпадений**.
- PoW hash использует `blake3` (см. `serialize.rs:145-147`, `consensus.rs:253`).
- `sha2` — orphan dependency, тянется в build, но не используется.

**Воспроизводимость:**
```bash
rg "sha2" crates/strangecoin-core/Cargo.toml
# sha2 = "0.10"
rg "sha2::|Sha256|sha2::Sha256" crates/strangecoin-core/src/
# (empty)
```

**Рекомендуемое исправление:**
1. Удалить `sha2 = "0.10"` из `crates/strangecoin-core/Cargo.toml:13`.
2. `cargo build --workspace` должен пройти.
3. Если `sha2` нужен для будущих crypto additions (HMAC, RNG) — добавить через feature flag, не как безусловную dep.

---

### BUG-S1-009 — Нет Wallet round-trip теста (`new → save → load → sign/verify`)

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P04 КГ |
| **Категория** | D — тестовая |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P10 |

**Факт (по коду, 2026-10-10):**
- `src/wallet.rs` полностью переписан под secp256k1 (P04 закрыт).
- `tests/genesis_key.rs` тестирует genesis validation, но не `Wallet` API.
- `crates/strangecoin-core/tests/consensus_proptest.rs:133` `signature_verification_roundtrip` тестирует только secp256k1 sign/verify, не `Wallet::new`/`Wallet::load`/keystore.
- **Нет** теста: создать Wallet с паролем → сохранить keystore в temp dir → загрузить → sign message → verify signature → zeroize.
- Краши в `Wallet::load` (PBKDF2 params, AES-GCM nonce, corrupted keystore) — непокрыты.

**Ожидание (P04 КГ):**
- «Тест: создали кошелёк → сохранили keystore → загрузили → sign/verify round-trip работает».
- «Тест: чужой публичный ключ не верифицирует подпись».

**Рекомендуемое исправление:**
1. Создать `tests/wallet.rs` с тестами:
   - `wallet_new_save_load_sign_verify_roundtrip` — полный цикл через temp dir.
   - `wallet_load_rejects_wrong_password` — неверный пароль → `Err`.
   - `wallet_load_rejects_corrupted_keystore` — повреждённый keystore → `Err`.
   - `wallet_load_rejects_tampered_ciphertext` — изменённый ciphertext → AES-GCM auth error.
   - `wallet_sign_verify_with_wrong_pubkey_rejected` — подпись не верифицируется чужим pubkey.
2. Использовать `TestDir` из `tests/common/mod.rs`.
3. Добавить в CI как отдельный `cargo test --test wallet`.

---

### BUG-S1-010 — Нет теста `InvalidChainId` rejection

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P05 КГ |
| **Категория** | D — тестовая |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P10 |

**Факт (по коду, 2026-10-10):**
- `crates/strangecoin-core/tests/consensus_proptest.rs:197` `chain_id_validation` — proptest, который **таутологичен**: `prop_assert_eq!(is_valid, tx.chain_id == current_chain_id())` — проверяет `x == x`, не вызывает реальной валидации.
- Mempool `mempool/mod.rs:92-97` отвергает `tx.chain_id != current_chain_id()` → `InvalidChainId`.
- **Нет** явного unit-теста, который бы:
  1. Создал tx с `chain_id=2` (testnet).
  2. Попробовал вставить в mainnet mempool.
  3. Asserted `Err(InvalidChainId { expected: 1, got: 2 })`.

**Ожидание (P05 КГ):**
- «Тест: tx с `chain_id=2` отвергается на mainnet-узле».

**Рекомендуемое исправление:**
1. В `tests/` создать `tests/chain_id.rs` с тестами:
   - `cross_chain_tx_rejected_in_mempool` — mainnet mempool rejects testnet tx.
   - `cross_chain_tx_rejected_in_validate_and_apply` — block executor rejects.
   - `correct_chain_tx_accepted` — control test.
2. Использовать `view.chain_id()` после исправления BUG-S1-004 (chain_id from Config).
3. Заменить таутологический `chain_id_validation` proptest на реальный вызов `mempool::insert`.

---

### BUG-S1-011 — Нет теста retarget на реальной высоте 2016

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P08 КГ |
| **Категория** | D — тестовая |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P10 |

**Факт (по коду, 2026-10-10):**
- `crates/strangecoin-core/src/consensus.rs:202-232` — `compute_target` корректно реализует sliding window retarget.
- `tests/block_executor.rs:268` `accepts_the_target_computed_at_a_retarget_height` — тест, но он использует height=1 (наследует parent target), **не** height=2016.
- `consensus_proptest.rs:180` `difficulty_target_clamp` — proptest clamp-фактора, но **переcчитывает формулу сам**, не вызывает `compute_target`.
- **Нет** теста, который:
  1. Строит chain из 2016 блоков с realistic timestamps (например, 600s per block).
  2. На height 2016 вызывает `compute_target(chain)`.
  3. Сравнивает с эталоном (вычисленным в тесте по той же формуле).

**Ожидание (P08 КГ):**
- «Тест: на retarget height `target` пересчитан корректно (сравнение с эталоном)».

**Рекомендуемое исправление:**
1. Создать `tests/retarget.rs` с тестом `compute_target_at_height_2016`:
   - Построить 2016 блоков с `timestamps = [600, 1200, 1800, ...]`.
   - Вызвать `compute_target(&chain)`.
   - Сравнить с эталоном (тот же sliding window расчёт в тесте).
2. Дополнительно: тест с разными фактическими times (300s, 1200s, 6000s) → проверка clamp [prev/4, prev*4].
3. Запускать через `cargo test --test retarget` — должен быть < 5s.

---

### BUG-S1-012 — `#[allow(dead_code)]` бандажи без TODO

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Источник** | audit; P21 КГ (устранение warnings) |
| **Категория** | D — тестовая / E — гигиена |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P11 |

**Факт (по коду, 2026-10-10):**
- 6 `#[allow(dead_code)]` маркеров:
  - `tests/common/mod.rs:5` (`wait_for_event`)
  - `tests/common/mod.rs:29` (`wait_for_block_applied`)
  - `tests/common/mod.rs:40` (`walk_state`)
  - `tests/common/mod.rs:51` (`coinbase_tx`)
  - `tests/common/mod.rs:68` (`craft_child`)
  - `crates/strangecoin-core/tests/state_roundtrip.rs:6` (`arbitrary_address`)
- 6 `#[allow(clippy::await_holding_lock)]` маркеров:
  - `tests/network_id.rs:28`, `tests/sync_headers.rs:24, 86, 133`, `tests/network.rs:69, 169`, `tests/events.rs:18`
- **Ни один** не имеет TODO-комментария с обоснованием «для Stage X+» (P21 явно требовал: «пометить `#[allow(dead_code)]` с TODO (если планируется Stage 1+)»).

**Ожидание (P21):**
- Либо устранить dead code, либо пометить с TODO с обоснованием стадии.

**Рекомендуемое исправление:**
1. Для каждого `#[allow(dead_code)]`:
   - Если helper **используется** в других тестах — найти и убрать атрибут (он не dead).
   - Если helper **не используется** — удалить helper altogether.
   - Если helper **планируется** на Stage 2+ — оставить `#[allow(dead_code)]` + TODO: «// TODO(S2-P0X): used in <test name> when <feature> lands».
2. Для `#[allow(clippy::await_holding_lock)]` — изучить, реально ли удерживается lock across `.await`. Если да — refactor (переместить lock acquisition после `.await` или в `spawn_blocking`). Если нет — убрать атрибут.
3. Добавить CI-проверку: `rg "#\[allow\(dead_code\)\]" tests/ src/ --count` → должно быть 0 или только с TODO.

---

### BUG-S1-013 — PGP-ключ — placeholder, файла `pgp_key.asc` нет

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Источник** | audit; P25 КГ (SECURITY.md с PGP key) |
| **Категория** | G — операционная |
| **Статус** | **open** |
| **Stage target** | ops gate (до mainnet freeze) |

**Факт (по коду, 2026-10-10):**
- `docs/security/SECURITY.md:25` содержит текст: `[PLACEHOLDER: Maintainer must generate real PGP key]`.
- `ls docs/security/pgp_key.asc` → **нет файла**.
- `docs/security/BOUNTY.md:93-100` §5.1: «This bounty program is prepared locally. Full Immunefi integration requires: [4 human actions]».
- `docs/security/BOUNTY.md:45-50` — 4 tier'а ($500/$1k/$10k/$100k), но промпт P25 просил 3 ($1k/$10k/$100k).
- `Changelog.md:215` утверждает «3 reward tiers ($1k/$10k/$100k)» — **противоречит** BOUNTY.md.

**Ожидание (P25 КГ):**
- «SECURITY.md содержит PGP public key (сгенерировать, добавить в repo)».
- «Bug bounty program активна на Immunefi» (P26 DoD #9).

**Рекомендуемое исправление:**
1. Сгенерировать PGP key: `gpg --gen-key` (offline machine).
2. Экспортировать pubkey: `gpg --armor --export <keyid> > docs/security/pgp_key.asc`.
3. В `SECURITY.md:25` заменить placeholder на fingerprint + ссылку на `pgp_key.asc`.
4. Загрузить pubkey на ключ-сервер (keys.openpgp.org, keyserver.ubuntu.com).
5. В `BOUNTY.md` — или привести к 3 tier'ам (удалить $500), или обновить Changelog с правильным количеством.
6. Зарегистрировать bounty program на Immunefi (или явно удалить претензию из P26 DoD).
7. После (5) — обновить `STAGE0_SUMMARY.md` и `STAGE1_SUMMARY.md` DoD-критерии #9.

---

### BUG-S1-014 — `INVARIANTS_ENFORCED.md` Stage 0 имеет арифметические расхождения

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Источник** | audit; P26 КГ; BUG-S0-024 (родственная, но для Stage 1) |
| **Категория** | E — документационная |
| **Статус** | **open** |
| **Stage target** | Stage 1.5-P12 |

**Факт (по коду, 2026-10-10):**
- `docs/stage0/INVARIANTS_ENFORCED.md` (66 строк) — Stage 0 snapshot, не был переписан в BUG-S0-024 (только Stage 1 был переписан).
- Header (lines 11-13): «17 Enforced, 4 Deferred, 1 N/A».
- Фактически по таблице: 5 Deferred (15, 18, 19, 20, 21), 0 N/A.
- Арифметика: 17 + 4 + 1 = 22 (правильно), но 17 + 5 + 0 = 22 — фактическое распределение.
- `docs/stage0/CRITICAL_ISSUES_CLOSED.md` (42 строки) — header «10 closed, 3 partially closed, 0 deferred», но таблица считает 11+1 closed, 3 partial.
- `docs/stage0/STAGE0_SUMMARY.md:19` — «Critical issues closed | 9/13 fully, 3 partially, 1 deferred» — третье расхождение.
- Три документа дают **три разные** сводки одного и того же.

**Ожидание:**
- Все три Stage 0 summary документа должны давать согласованную арифметику.

**Рекомендуемое исправление:**
1. Переписать `docs/stage0/INVARIANTS_ENFORCED.md` header: «17 Enforced, 5 Deferred, 0 N/A».
2. Переписать `docs/stage0/CRITICAL_ISSUES_CLOSED.md` header с реальной арифметикой (пересчитать closed/partial/deferred).
3. Переписать `docs/stage0/STAGE0_SUMMARY.md:19` с согласованной арифметикой.
4. Добавить cross-reference между тремя документами (либо один-источник, остальные ссылки).
5. Скрипт-проверка в CI: `python3 scripts/audit/check_stage0_arith.py` — парсит 3 документа, сверяет суммы.

---

### BUG-S1-015 — Offline genesis key не сгенерирован (ops gate для mainnet)

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) для mainnet |
| **Источник** | SCIP-0001; BUG-S0-015 (код закрыт, ops — нет); audit |
| **Категория** | G — операционная / B — криптографическая |
| **Статус** | **open** (ops gate) |
| **Stage target** | ops gate (до mainnet freeze / block 1) |

**Факт (по коду, 2026-10-10):**
- BUG-S0-015 закрыт в коде: `genesis_keypair()` и seed `"strangecoin-genesis-seed-2026"` удалены (`src/consensus/mod.rs`).
- `genesis.json:6` — `"initial_holder_pubkey": "0x026f7d841405..."` — но это **тестnet-key**, BURNED.
- `docs/SCIP/scip-0001-genesis-key-replacement.md` — фиксирует обязательство: сгенерировать offline key перед mainnet freeze.
- `STAGE1_SUMMARY.md §6.1` — 🟡 residual: «before mainnet freeze/block 1, generate an offline key, write only the pubkey, update `EXPECTED_GENESIS_HASH`. Current testnet genesis key is **burned**».
- **Никаких действий по генерации не сделано** — это человеческая операция, не кодовая.

**Ожидание:**
- Offline machine (air-gapped) → `secp256k1::SecretKey::new(&mut OsRng)` → сохранить privkey в physical vault.
- Экспортировать pubkey → `genesis.json:initial_holder_pubkey`.
- Пересчитать `EXPECTED_GENESIS_HASH` от нового genesis.
- Обновить `tests/genesis_key.rs` с новым hash.
- Уничтожить testnet privkey (BURNED).

**Рекомендуемое исправление:**
1. Подготовить air-gapped machine (Tails OS или аналог).
2. `cargo run --release --example generate_offline_genesis_key > genesis_pubkey.txt`.
3. Записать privkey в physical vault (бумага + steel plate).
4. Скопировать pubkey в `genesis.json:initial_holder_pubkey`.
5. `cargo run -- --print-genesis-hash` → новый hash → `crates/strangecoin-core/src/consensus.rs:EXPECTED_GENESIS_HASH`.
6. `cargo test --test genesis_key` — должен пройти с новым hash.
7. Commit + tag `v1.0.0-mainnet-freeze` (или `v1.0.0-rc1`).
8. Публично объявить BURN тестnet key.
9. После (8) — обновить `STAGE1_SUMMARY.md §6.1` статус с 🟡 на ✅.

---

## 4. Сводная таблица

| BUG ID | Title | Severity | Status | Stage |
|---|---|---|---|---|
| BUG-S1-001 | Release pipeline не запускался на CI | H | partial | S1.5-P06 / ops |
| BUG-S1-002 | `state_root == [0;32]` opt-out ломает #19 | C | **open** | S1.5-P03 |
| BUG-S1-003 | `verify_block_stateless` не пересчитывает post-root | C | **open** | S1.5-P03 |
| BUG-S1-004 | `current_chain_id()` захардкожен в REGTEST | C | **open** | S1.5-P05 |
| BUG-S1-005 | Per-message rate-limit не enforced | H | **open** | Stage 2-P03 |
| BUG-S1-006 | Single-block size check отсутствует | H | **open** | S1.5-P07 |
| BUG-S1-007 | Golden-векторы блоков таутологичны | M | **open** | S1.5-P08 |
| BUG-S1-008 | Мёртвая зависимость `sha2` | L | **open** | S1.5-P09 |
| BUG-S1-009 | Нет Wallet round-trip теста | M | **open** | S1.5-P10 |
| BUG-S1-010 | Нет теста `InvalidChainId` rejection | M | **open** | S1.5-P10 |
| BUG-S1-011 | Нет теста retarget на высоте 2016 | M | **open** | S1.5-P10 |
| BUG-S1-012 | `#[allow(dead_code)]` бандажи без TODO | L | **open** | S1.5-P11 |
| BUG-S1-013 | PGP-ключ — placeholder | H | **open** | ops gate |
| BUG-S1-014 | `INVARIANTS_ENFORCED.md` Stage 0 арифметика | L | **open** | S1.5-P12 |
| BUG-S1-015 | Offline genesis key не сгенерирован | C (mainnet) | **open** (ops) | ops gate |

**Итого:** 15 открытых пунктов: 4 Critical, 5 High, 4 Medium, 2 Low + 1 partial.
- 3 Critical blocker для mainnet (BUG-S1-002, 003, 015).
- 1 Critical blocker архитектурный (BUG-S1-004 — mainnet-ветка мёртвая).
- 2 High сетевые/операционные (BUG-S1-005, 013).
- Остальные 7 — качество/тесты/гигиена.

---

## 5. Карта зависимостей

```
BUG-S1-015 (offline genesis key) ──── зависит от ──── BUG-S1-004 (chain_id from Config)
                                              │
                                              ▼
                                   BUG-S1-002 (state_root opt-out)
                                              │
                                              ▼
                                   BUG-S1-003 (post-root в witness)
                                              │
                                              ▼
                                   BUG-S1-001 (release pipeline) ──► первый v* tag push
                                              │
                                              ▼
                                   BUG-S1-013 (PGP key)
                                              │
                                              ▼
                                   mainnet freeze (block 1)

BUG-S1-006 (size check) ──► independent ──► можно закрывать параллельно
BUG-S1-005 (per-message RL) ──► independent ──► Stage 2-P03 (network crate)
BUG-S1-007..012 ──► test/quality track ──► параллельно
BUG-S1-014 ──► doc-only ──► можно закрыть сразу
BUG-S1-008 ──► remove `sha2` dep ──► можно закрыть сразу
```

---

## 6. Приоритеты

**Must-fix до mainnet freeze (Critical):**
1. BUG-S1-004 — `current_chain_id()` from Config (архитектурный prerequisite)
2. BUG-S1-015 — Offline genesis key (ops gate, блокирует mainnet)
3. BUG-S1-002 — `state_root` opt-out (инвариант #19)
4. BUG-S1-003 — `verify_block_stateless` post-root (инвариант #19)
5. BUG-S1-001 — Release pipeline first run (инвариант #22)

**Must-fix до public testnet (High):**
6. BUG-S1-005 — Per-message rate-limit
7. BUG-S1-006 — Single-block size check
8. BUG-S1-013 — PGP key + Immunefi

**Quality track (можно параллельно):**
9. BUG-S1-007 — Golden vectors
10. BUG-S1-009 — Wallet round-trip
11. BUG-S1-010 — `InvalidChainId` test
12. BUG-S1-011 — Retarget на высоте 2016

**Hygiene track:**
13. BUG-S1-008 — Remove `sha2`
14. BUG-S1-012 — `#[allow]` bandaids
15. BUG-S1-014 — Doc arithmetic

---

## 7. Связь с `bugfixes-stage0.md`

| BUG-S0 (source) | BUG-S1 (continuation) | Comment |
|---|---|---|
| BUG-S0-005 (partial) | BUG-S1-001 | Перенос: pipeline написан, но не запускался |
| BUG-S0-012 (open) | BUG-S1-002 | Перенос: state_root opt-out |
| BUG-S0-013 (open) | BUG-S1-003 | Перенос: post-root в witness |
| BUG-S0-015 (fixed in code) | BUG-S1-015 | Ops gate остался |
| (none) | BUG-S1-004 | Новый: chain_id hardcoded (audit) |
| (none) | BUG-S1-005 | Новый: per-message rate-limit (audit) |
| (none) | BUG-S1-006 | Новый: single-block size check (audit) |
| (none) | BUG-S1-007 | Новый: golden vectors tautology (audit) |
| (none) | BUG-S1-008 | Новый: sha2 dead dep (audit) |
| (none) | BUG-S1-009 | Новый: wallet round-trip test (audit) |
| (none) | BUG-S1-010 | Новый: InvalidChainId test (audit) |
| (none) | BUG-S1-011 | Новый: retarget на 2016 test (audit) |
| (none) | BUG-S1-012 | Новый: `#[allow]` без TODO (audit) |
| (none) | BUG-S1-013 | Новый: PGP placeholder (audit) |
| (none) | BUG-S1-014 | Новый: doc arithmetic (audit) |

---

## 8. Источники для перепроверки

```bash
# Теги и история
git fetch --tags origin
git fetch --unshallow origin  # если клон shallow
git tag -l
git log --oneline | wc -l

# BUG-S1-002
rg "state_root != \[0u8; 32\]" crates/strangecoin-core/src/ src/blockchain/

# BUG-S1-003
sed -n '51,90p' crates/strangecoin-core/src/state/witness.rs

# BUG-S1-004
sed -n '170,175p' crates/strangecoin-core/src/consensus.rs

# BUG-S1-005
rg "rate_limiter\.check" src/

# BUG-S1-006
rg "MAX_BLOCK_SIZE" src/

# BUG-S1-007
sed -n '195,210p' crates/strangecoin-core/tests/serialize_golden.rs

# BUG-S1-008
rg "sha2" crates/strangecoin-core/Cargo.toml
rg "sha2::|Sha256" crates/strangecoin-core/src/

# BUG-S1-013
grep "PLACEHOLDER" docs/security/SECURITY.md
ls docs/security/pgp_key.asc 2>/dev/null

# BUG-S1-015
grep "initial_holder_pubkey" genesis.json
cat docs/SCIP/scip-0001-genesis-key-replacement.md
```

---

## 9. Процесс закрытия

Каждый баг должен быть закрыт:
1. **Атомарным коммитом** с сообщением `[BUG-S1-NNN] <short description>` (правило retro §8.1).
2. В коммите: код + тесты + (если нужно) doc update.
3. После коммита — обновить `bugfixes-stage1.md` поле `Статус` с `open` на `fixed`, добавить секцию `Решение (date): ...` с описанием что сделано.
4. Если баг закрывается как `wontfix` — `Решение` должно объяснить rationale + пометить `wontfix — <reason>`.
5. Для Critical багов (BUG-S1-002, 003, 004, 015) — после фикса запустить `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`, `cargo fuzz run canonical_decode -- -max_total_time=600` и приложить выводы.
6. Для ops gate багов (BUG-S1-013, 015) — описать offline-процедуру и записать в `STAGE1_SUMMARY.md §6` статус.

---

## 10. Замечание о первоначальном аудите `strangecoin_audit_report.md`

Первоначальный отчёт (2026-10-10) был основан на shallow clone (`git clone --depth 1`) и неверно пометил как «❌ Critical»:
- «Нет git-тегов» → фактически есть (`v1.0.0-stage0`, `v1.1.0-stage1`) — `BUG-S0-001` fixed.
- «История squashed в 1 коммит» → фактически 190 атомарных коммитов — `BUG-S0-002` fixed.

Эти два пункта в `strangecoin_audit_report.md` §3 «Критические дефекты» (п.1) — **ошибочны** и должны быть исправлены. Остальные 8 пунктов в §3 критических дефектов подтверждены и перенесены в `bugfixes-stage1.md` как BUG-S1-002..014.

**Действие:** обновить `download/strangecoin_audit_report.md` — удалить из §3 п.1 (отсутствие тегов + squashed history), заменить примечанием: «см. `bugfixes-stage1.md` §0 — первоначальный аудит по shallow clone неверно зафиксировал эти два пункта».
