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
| **Статус** | **fixed** (2026-10-10) |
| **Stage target** | Stage 1.5-P06 / ops gate |

**Факт (по коду, 2026-10-10, до фикса):**
- `.github/workflows/release.yml` существует (7067 байт), triggers on `push: tags: ['v*']`, 6 matrix targets, SHA256, cosign keyless OIDC, SLSA3 provenance.
- Теги `v1.0.0-stage0` (commit `3dd37ef`) и `v1.1.0-stage1` (commit `68e358f`) **существуют** в git (подтверждено `git ls-remote --tags origin`).
- `docs/security/REPRODUCIBLE_BUILDS.md` (113 строк) описывает verify-флоу, но **содержит 0 ссылок на Actions run URL**, 0 опубликованных SHA256, 0 `cosign verify-blob`/`slsa-verifier` output.
- `docs/stage1/STAGE1_SUMMARY.md:28` и `:141` явно признают: «first `v*` tag run still pending GitHub Actions verification».
- `docs/stage1/INVARIANTS_ENFORCED.md` #22 = 🟡 residual.

**Ожидание:**
- После push тега `v*` должен сработать `release.yml` → собрать 6 артефактов → опубликовать Release с SHA256 + cosign signature + SLSA provenance.
- В `REPRODUCIBLE_BUILDS.md` должны появиться: Actions run URL, artifact SHA256s, `cosign verify-blob`/`slsa-verifier` outputs для каждого артефакта.
- `INVARIANTS_ENFORCED.md` #22 должен стать ✅.

**Воспроизводимость (до фикса):**
```bash
git clone https://github.com/Reider85/strangecoin.git && cd strangecoin
grep -E "github.com/.*actions/runs|cosign verify-blob" docs/security/REPRODUCIBLE_BUILDS.md
# Вывод: пусто (нет evidence)
```

**Решение (2026-10-10):**

Перед первым запуском в `release.yml` обнаружены и исправлены **4 блокирующих дефекта** (коммит `a830c51`), которые гарантированно уронили бы первый run:

1. **`aggregate-hashes` — коллизия `hash.txt`:** `download-artifact` с `merge-multiple: true` складывал все 6 `*-checksum/hash.txt` в одну папку (перезапись 5 из 6), а цикл `for f in checksums/*/hash.txt` не матчил плоскую структуру → 0 subjects → job `provenance` падал на пустом `base64-subjects`. Фикс: `merge-multiple` убран (подпапки на артефакт) + guard `test $(wc -l) -eq 6`.
2. **`base64-subjects` не был base64:** контракт `slsa-github-generator@v2.1.0` требует, чтобы вход декодировался в формат `sha256sum`; workflow передавал сырой текст. Фикс: `cat checksums/*/hash.txt | base64 -w0`.
3. **`RUSTFLAGS` — silent no-op:** `env:`-блок GitHub Actions **не делает shell-экспанд** `$GITHUB_WORKSPACE` → rustc получал literal-строку и `--remap-path-prefix` ничего не вычищал (ломало локальную сверку воспроизводимости). Фикс: `${{ github.workspace }}`.
4. **Ретированный runner `macos-13`** (retirement Dec 2025) → job x86_64-apple-darwin не получил бы рантайм. Фикс: `macos-latest` + target `x86_64-apple-darwin` (кросс-компиляция со ARM mac).

Гигиена: `if-no-files-found: error` + OS-gate на upload-шагах; `hash.txt` исключён из подписи и из assets; опубликован нормальный `SHA256SUMS.txt` (6 строк, один на артефакт).

**Прогон и evidence:**
- Тег `v0.0.1-rc1` → `a830c51` → push → run https://github.com/Reider85/strangecoin/actions/runs/38071514479 — **green все jobs** (6 builds, aggregate-hashes, sign, SLSA Provenance detect-env/generator/upload-assets/final, Create Release).
- Release: https://github.com/Reider85/strangecoin/releases/tag/v0.0.1-rc1 — 31 asset (6 бинарей, 6 `.sha256`, 6 `.cosign`, 6 `.cosign.pem`, `SHA256SUMS.txt`, `multiple.intoto.jsonl`).
- Независимая верификация (Windows x86_64 + Linux x86_64, 2026-10-10): локальный SHA256 = MATCH; `cosign verify-blob` (v3.1.3) = `Verified OK`; `slsa-verifier` (v2.7.1) = `PASSED` @ commit `a830c51`.
- Полное evidence — `docs/security/REPRODUCIBLE_BUILDS.md` §Verified Release Runs; попутно исправлены 3 битые ссылки `anomalyco/strangecoin` → `Reider85/strangecoin`.
- Закрыты residual-маркеры: `INVARIANTS_ENFORCED.md` #22 → ✅; `STAGE1_SUMMARY.md` §6.4 п.4 → closed, критерии #12/#13 обновлены; `THREAT_MODEL.md` V-33 → closed (Residual: Низкий).

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
| **Статус** | **fixed** (2026-10-11) |
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

**Решение (2026-10-11):**

Закрыт полностью (SCIP-0002 draft, activation_height 0, consensus_version без bump):

1. **Config:** `allow_zero_state_root: Option<bool>` (serde default `None`) + геттер `zero_state_root_allowed()` — network-aware default: `None` → `true` на regtest (network_id=3), `false` на mainnet/testnet. `Config::validate` отклоняет `Some(true)` вне regtest (mainnet/testnet физически не могут включить opt-out). Unit-тесты: default resolution ×3 сети, explicit override, mainnet/testnet reject, regtest accept.
2. **Core `root_after(state, block, allow_zero_state_root)`:** нулевой корень допустим только при `index == 0` (genesis, инвариант #19 «кроме genesis») или явном opt-in; иначе `Err(StateRootMismatch { expected: [0;32], got: computed })`. Ненулевой корень обязан совпадать (как раньше).
3. **`block_executor::validate_and_apply`:** та же логика через `BlockView.allow_zero_state_root` (builder `.with_allow_zero_state_root`, default `false` — secure by default); проброска `Blockchain.allow_zero_state_root` → `view_for` → `rebuild_from_chain` → `validate_chain`/`try_adopt_candidate` (паттерн `allow_grant_blocks`).
4. **Node-блоки теперь коммитят реальный корень** (сверх оригинального пункта плана — без этого майнинг ломался строгой проверкой): `mine_block_inner` вычисляет post-state root через `apply_block(parent_state, block)` до PoW-цикла (unapplicable mempool-tx → abort mining); `create_grant_block` — аналогично до вычисления hash. Genesis (index 0) остаётся с нулевым корнем (exempt); `EXPECTED_GENESIS_HASH`/`genesis.json` не тронуты.
5. **Deviation от рекомендации №3 (деструктивная миграция БД):** performed **не выполнялась** — перезапись `state_root` в сохранённых блоках меняет block hash → ломает цепь `previous_hash`. Вместо миграции: legacy regtest-БД валидны через network-aware default флага; mainnet/testnet legacy-цепей не существует (mainnet не запущен, testnet genesis key burned). Зафиксировано в SCIP-0002 §Migration + Changelog.
6. **Тесты:** core `tests/state_root.rs` — `zero_state_root_rejected_without_opt_in`, `genesis_zero_state_root_is_always_tolerated` (+ обновлены существующие вызовы); `tests/block_executor.rs` — `rejects_zero_state_root_when_commitment_required`, `accepts_zero_state_root_with_opt_in_flag` (helper `view()` получает opt-in для legacy-фикстур); e2e `tests/state_root.rs` — `zero_state_root_chain_rejected_without_the_opt_in_flag`, `grant_and_mined_blocks_commit_real_state_roots`, переименован `zero_state_root_is_tolerated_under_the_opt_in_flag`.
7. **Docs:** `INVARIANTS_ENFORCED.md` #19 → ✅ (residual count 2→0, #22 уже закрыт); `STAGE1_SUMMARY.md` §6.5 п.5 → closed, criteria #2/#12 обновлены; `THREAT_MODEL.md` V-35 → closed-вектор (3), residual обновлён; SCIP-0002 draft создан.
8. **Verification:** `cargo test --workspace` — full green (включая новые тесты); `cargo clippy --workspace --all-targets -- -D warnings` — clean (попутно зачищены pre-existing clippy-lint'ы в `address.rs`, `merkle.rs`, `consensus_proptest.rs` — не входили в этот баг, но блокировали clippy-gate).

---

### BUG-S1-003 — `verify_block_stateless` не пересчитывает post-state-root

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Источник** | BUG-S0-013 (перенос); S1-P07 КГ; инвариант #19 |
| **Категория** | B — криптографическая |
| **Статус** | **fixed** (2026-10-11) |
| **Stage target** | S1.5-P03 |

**Факт (по коду, 2026-10-10, до фикса):**
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

**Решение (2026-10-11) — вариант A′ (усиленный пересчёт, выбран вместо литературной Опции A):**

Вместо добавления post-proofs в witness реализован **полный пересчёт post-root из parent root + witness + блока** — дословное выполнение КГ S1-P07 («пересчитать post-state-root из parent root + witness + блока») и текста mitigation V-36:

1. **`SparseMerkleTrie::root_after_updates(pre_root, updates)`** (`state/sparse_merkle.rs`): mulproof-update для depth-256 SMT. Каждый `LeafUpdate` несёт pre-аккаунт + его 256-sibling inclusion proof + post-аккаунт. Алгоритм: (1) verify каждого pre-proof против `pre_root`; (2) seed node-map из proofs (frontier pre-дерева); (3) overwrite обновлённых leaves пост-хешами (`EMPTY_HASH` для pruned `(0,0)`); (4) bottom-up recompute ancestors каждого ключа — общий предок пересчитывается после детей, stale sibling из proof перезаписывается до использования. Незатронутые sibling-subtrees сохраняют pre-хеши, уже привязанные к `pre_root` → untouched-лист не может разойтись, не сломав pre-proof.
2. **`verify_block_stateless`** (`state/witness.rs`): после apply_block собирает `updates` из witness + post-state, вызывает `root_after_updates`, сравнивает с `block.state_root` → `Err(PostStateRootMismatch { expected, got })`. Genesis-exempt (`index == 0 && state_root == [0;32]`) — то же правило, что в `root_after`/SCIP-0002. Новый guard: witness обязан покрывать все touched-адреса (`collect_touched_addresses` ⊆ `witness.proofs`) — иначе `Err(WitnessVerificationFailed)` (partial apply дефолтил бы отсутствующий аккаунт в `(0,0)`).
3. **Новая ошибка** `CoreError::PostStateRootMismatch { expected, got }` + маппинг в `src/error.rs`.
4. **Формат witness не изменился** (в отличие от Опции A: без удвоения размера, без новых полей). Zero-root на non-genesis отвергается автоматически — recomputed root никогда не `[0;32]`.
5. **Тесты:** core unit (`sparse_merkle.rs`, 8 + proptest): single-key roundtrip; overlapping key paths (brute-forced пара с общим первым байтом ключа — stale-sibling overwrite); pruned post; absent→present; invalid pre-proof; wrong pre-account; empty updates; 64-аккаунтов scatter-update vs full recompute; proptest roundtrip. Integration (`tests/witness.rs`, 6 + proptest): `forged_state_root_rejected`; `untouched_account_tamper_in_post_commitment_rejected` (главное усиление: подделка untouched-аккаунта в post-commitment ловится, Опция A это оставила бы открытым); `zero_state_root_on_non_genesis_rejected`; `missing_touched_address_rejected`; `genesis_zero_state_root_is_tolerated` (+ forged genesis root rejected); `proptest_forged_state_root_rejected` (bit-flip любого байта корня → Err).
6. **Docs:** `INVARIANTS_ENFORCED.md` #19 residual BUG-S1-003 снят; `STAGE1_SUMMARY.md` §6.5 п.5 + criterion #1 caveat; `THREAT_MODEL.md` V-36 mitigation обновлён до фактического пересчёта; Changelog.
7. **Без нового SCIP:** full-node правило не менялось (уже SCIP-0002); фикс — только stateless API в core, wire-протокол witness не существует (Stage 2+).
8. **Verification:** `cargo test --workspace` — green (33 suites); `cargo clippy --workspace --all-targets -- -D warnings` — clean.

---

## 3. Новые баги из независимого аудита (не вошедшие в `bugfixes-stage0.md`)

---

### BUG-S1-004 — `current_chain_id()` захардкожен в `CHAIN_ID_REGTEST`

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) |
| **Источник** | audit (2026-10-10); P10 КГ (детерминированный генезис); P05 КГ (chain_id) |
| **Категория** | C — архитектурная |
| **Статус** | **fixed** (2026-10-11) |
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

**Решение (2026-10-11):** Исправлено полностью, объём шире описанного в баге — хардкод жил глубже, чем mempool:

1. **Core (`strangecoin-core`)**: `current_chain_id()` **удалён**; `block_reward_at_height()` (глобальная зависимость) и `address_from_public_key()` удалены — остались явные `block_reward_at_height_for_chain(.., chain_id)` / `encode_address(pk, network_id)`.
2. **State-machine**: `apply_block(state, block, chain_id)` / `root_after(.., chain_id)` / `verify_block_stateless(.., chain_id)` — **явный параметр** (не вывод из tx). Внутри: награда по расписанию сети + **новый state-level gate** — каждая `tx.chain_id == chain_id`, иначе `CoreError::InvalidChainId` (закрывает и witness-, и rebuild-пути). Это критично: прежний `apply_non_genesis` брал reward-cap из хардкода (regtest=0) и **отклонил бы mainnet-блоки с реальной наградой**.
3. **Node**: `Blockchain.chain_id` (из `Config.network_id` через `BlockchainFacade::new(port, chain_id)` / `with_event_bus(.., chain_id)`; `Node::new` прокидывает уже существовавший `network_id`); `BlockView.chain_id`; `Mempool.chain_id` (insert-reject); **новая per-tx проверка в `validate_and_apply`** (invariant #10 теперь и на block-пути); майнинг/grant/genesis-конструирование — `self.chain_id`; миграция bech32 — `self.chain_id`; `create_test_blockchain_for_network(db, network_id)` для тестов mainnet-режима.
4. **Genesis по сетям**: `validate_genesis(block, chain_id)` — regtest skip / mainnet `EXPECTED_GENESIS_HASH` / testnet **`EXPECTED_TESTNET_GENESIS_HASH` (новая константа)**; новый файл **`genesis-testnet.json`** (network_id=2; burned pubkey SCIP-0001 §2 допустим для testnet); `install_fresh_genesis` выбирает файл по сети и сверяет chain_id гезиса с конфигом; regtest-гезис вынесен в тестируемую `regtest_genesis_block()`.
5. **Тесты**: `tests/chain_id.rs` (5: mainnet mempool reject testnet tx + accept control; block с foreign-chain tx → reject; mainnet reward schedule в mining; coinbase-overpay reject); `tests/genesis_key.rs` расширен до 9 (mainnet/testnet/regtest hashes, cross-network reject, mismatched file fields); taутологичный proptest `chain_id_validation` заменён реальным apply_block-гейтом (закладывает фундамент BUG-S1-010).
6. **Verification (2026-10-11)**: `cargo test --workspace` — **green** (все suites, exit 0); `cargo clippy --workspace --all-targets -- -D warnings` — **clean**; `cargo run --example canonical_decode_soak` — 223,103 inputs / 10s / **0 panics** (cargo-fuzz — CI job `fuzz-canonical-decode`).
7. **Docs**: `INVARIANTS_ENFORCED.md` №6/№8/№10 обновлены; `THREAT_MODEL.md` V-11 Stage 1 mitigation; `STAGE1_SUMMARY.md` §6.1 — prerequisite BUG-S1-015 снят.
8. **Без нового SCIP**: полный переход на config-driven chain_id — не консенсус-изменение действующей regtest-цепи (та же сериализация/хеши), а снятие мёртвого mainnet-кода и prerequisite для mainnet freeze (SCIP-0001 ops-gate остаётся отдельным).

---

### BUG-S1-005 — Пер-сообщенный rate-limit не enforced

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Источник** | audit; P13 КГ; THREAT_MODEL V-16/V-17 |
| **Категория** | F — сетевая |
| **Статус** | **fixed** (2026-10-11) |
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

**Решение (2026-10-11): per-message + per-message-type rate-limit**

Выполнено базовое исправление (пункты 1–3) **и** per-message-type buckets (пункт 4 — реализован сейчас, не отложен):

1. **`MessageClass` + per-class buckets** (`src/network/rate_limiter.rs`): enum `MessageClass { GetHeaders, GetBlocks, ChainGossip, Other }`; `PeerCounter` получил `class_counts: [usize; NUM_CLASSES]` (сбрасываются вместе с окном); константы `CLASS_LIMITS` — GetHeaders=100, GetBlocks=100, ChainGossip=30, Other=30 (окно то же, 10 s). Новый метод **`check_class(addr, class)`**: проверяет и инкрементирует **total + class**; превружение любого из bucket'ов → тот же бан (5 min), что и `check()`. Старый `check(addr)` — только total (обратная совместимость: outbound sync, `ban`-пути, `network_id.rs`). Общий bookkeeping (ban/window) вынесен в приватный `with_checked_counter`.
2. **Классификация + per-message check** (`src/lib.rs`): новый `classify_request(&[u8]) -> MessageClass` (0x01→GetHeaders, 0x02→GetBlocks, префикс `UPDATE_BLOCKCHAIN:`→ChainGossip, остальное→Other). В `handle_connection` request-loop check стоит **после** успешного `read_length_prefixed_async` (не «before read», как предлагал баг): классификация требует прочитанные байты первого тега; попутно idle-timeout перестал сжигать токены. `Err` → warn + `break` (соединение закрывается). Accept-time `check()` (pre-HELLO) оставлен.
3. **Тесты:** 4 новых unit-теста `rate_limiter.rs` (per-class флуд → бан, виден через `check()`; изоляция классов — Other-флуд не ест бюджет GetHeaders; сброс окна чистит class-счётчики; бан общий между `check`/`check_class`); интеграционный `tests/rate_limit.rs::per_message_flood_bans_peer_on_single_connection` — 1 TCP-коннект, 200 junk `PING` за <1s → сервер закрывает соединение (EOF/RST) + бан через `rate_limiter.check(client_addr)` (паттерн `network_id.rs`).
4. **Docs:** инвариант #17 дополнен per-message/per-class enforcement; THREAT_MODEL V-01/V-17 + P13-mitigation обновлены; Changelog.
5. **Residual / Stage 2-P03:** bucket ключируется по полному `SocketAddr` (ephemeral port входящего коннекта ≠ per-IP лимит — отдельный design change); `CLASS_LIMITS` захардкожены (config-driven — Stage 2, network crate); при миграции в `strangecoin-net` (Stage 2-P03) rate_limiter переезжает как есть.
6. **Verification (2026-10-11):** `cargo test --workspace` — **green** (все suites, exit 0); `cargo clippy --workspace --all-targets -- -D warnings` — **clean**.
7. **Без нового SCIP:** network-layer DoS mitigation, wire-протокол не менялся (те же фреймы/теги); консенсус не затронут.

---

### BUG-S1-006 — Single-block size check отсутствует в `validate_and_apply`

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Источник** | audit; P12 КГ (лимиты размеров) |
| **Категория** | F — сетевая / C — архитектурная |
| **Статус** | **fixed** (2026-10-11) |
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

**Решение (2026-10-11): size checks в `validate_and_apply` — первыми проверками**

1. **Код** (`src/blockchain/block_executor.rs`): в начало `validate_and_apply` (до `validate_position` — по P12 «до любых других проверок») добавлены обе проверки:
   - **блок**: `strangecoin_core::serialize::serialize_block(block).len() > MAX_BLOCK_SIZE` → `SizeLimitExceeded("block")` (canonical wire-метрика — та же, что в `state_cache::validate_chain` и `parse_blocks`);
   - **каждый tx**: `serialize_transaction_signed(tx).len() > MAX_TX_SIZE` → `SizeLimitExceeded("transaction")` (безусловный цикл — grant-исключение касается только подписей, не размеров).
   Обе — с `warn!`-логом (observability THREAT_MODEL P12). Константы переиспользуются из `crate::network::protocol` (паттерн `state_cache.rs:374,399`), без дублирования. `commit_block` отдельной проверки не требует — он вызывает `validate_and_apply`. Doc-comment модуля дополнен пунктом 0.
2. **Тесты** (`tests/block_executor.rs`, 3 новых):
   - `rejects_block_larger_than_max_block_size` — coinbase с receiver ~4 MB → `SizeLimitExceeded("block")`;
   - `rejects_transaction_larger_than_max_tx_size` — подписанный transfer с receiver > 256 KB в блоке < 4 MB → `SizeLimitExceeded("transaction")` (грань: block-check проходит, tx-check срабатывает);
   - `accepts_block_under_both_size_limits` — позитивный: блок ~3.1 MB (coinbase + 13 подписанных transfer'ов с receiver по 250 KB; каждый tx < 256 KB) → применяется, `state.get_balance(alice) == 987`.
3. **Docs:** инвариант #7 дополнен `validate_and_apply` в locations; THREAT_MODEL P12-mitigation — size limits теперь и на validation-уровне; Changelog.
4. **Verification (2026-10-11):** `cargo test --workspace` — **green**; `cargo clippy --workspace --all-targets -- -D warnings` — **clean**.
5. **Без нового SCIP:** завершение enforcement уже объявленного инварианта #7 / P12 — лимиты на wire не меняются (любой блок, прошедший `parse_blocks`, уже ≤ 4 MB), расщепления цепи нет; закрыт только внутренний путь (`adopt_candidate` / `commit_block` / `rebuild_from_chain`).

---

### BUG-S1-007 — Golden-векторы блоков таутологичны

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P06 КГ (golden vectors); BUG-S0-024 (родственная проблема) |
| **Категория** | D — тестовая |
| **Статус** | **fixed** (2026-10-11) |
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

**Решение (2026-10-11): literal hex для всех трёх таутологий + pin block_hash + example для регенерации**

Уточнение к факту: строковые диапазоны в баге смещены — таутологии жили в `test_golden_block1` (serialize_block), `test_golden_block2_header` (serialize_block_header) и `test_golden_block2` (serialize_block); отдельного «блока 3» в файле не было. Tx-golden'ы (9 тестов) уже были корректны и не менялись.

1. **Код** (`crates/strangecoin-core/tests/serialize_golden.rs`):
   - 3 таутологии `expected_hex = actual_hex.clone()` заменены на сравнение с literal hex (FORMAT_VERSION = 4):
     - `test_golden_block1` → `serialize_block(block1)` = 322 hex chars (161 байт = `HEADER_WIRE_LEN` 157 + u32 tx_count);
     - `test_golden_block2_header` → `serialize_block_header(block2)` = 314 hex chars (157 байт);
     - `test_golden_block2` → `serialize_block(block2)` = 648 hex chars (header + 2 length-prefixed tx: 0x73 и 0x28).
   - Hash-тесты усилены: `test_golden_block1_hash` и `test_golden_block2_hash` теперь также пинят `hex::encode(block_hash(...))` (blake3 от header) — ловит и смену кодировки, и подмену хеш-функции.
   - В шапку файла добавлен anti-regression комментарий (процедура при изменении кодировки: FORMAT_VERSION bump + ADR + регенерация векторов).
2. **Example** (`crates/strangecoin-core/examples/print_serialized_block.rs`, новый): конструирует block1/block2 (те же структуры, что в тесте) и печатает все 5 golden hex — инструмент регенерации векторов при будущих FORMAT_VERSION bump.
3. **Verification (2026-10-11):** `cargo test -p strangecoin-core --test serialize_golden` — **14/14 green**; `cargo test --workspace` — **green**; `cargo clippy --all-targets -- -D warnings` — **clean**.
4. **CI-gate:** как отдельный CI job не требуется — тесты входят в стандартный `cargo test --workspace`; любое молчаливое изменение `serialize_block`/`serialize_block_header`/`block_hash` теперь ломает 5 golden-тестов. Ожидаемое действие разработчика задокументировано в шапке `serialize_golden.rs`.

---

### BUG-S1-008 — Мёртвая зависимость `sha2` в core

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Источник** | audit; P06 (PoW hash) |
| **Категория** | E — гигиена |
| **Статус** | **fixed** (2026-10-11) |
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

**Решение (2026-10-11): удаление direct-dep + drive-by фикс flake-проперти из BUG-S1-003**

1. **Код:** `sha2 = "0.10"` удалён из `crates/strangecoin-core/Cargo.toml`. Отсутствие использований подтверждено повторно: `sha2::`/`Sha256` → 0 совпадений в `crates/strangecoin-core/` (src, tests, examples), `src/`, `tests/`. PoW-hash — blake3 (`serialize.rs`, `consensus.rs`). Будущие crypto additions — только через feature flag (п.3 рекомендации принят).
2. **Lockfiles:** `Cargo.lock` — `sha2` убран из списка зависимостей `strangecoin-core`; сам пакет `sha2` остаётся в графе транзитивно через `pbkdf2` (wallet keystore — вне core). `fuzz/Cargo.lock` пересинхронизирован — `sha2` удалён полностью (fuzz-граф: strangecoin-core + libfuzzer-sys).
3. **Drive-by (не новый BUG-ID):** при верификации `cargo test --workspace` впервые воспроизведён ~6%-flake `proptest_root_after_updates_matches_full_recompute` (`sparse_merkle.rs`, minimal failing input: `alice_bal=1, send=2` → subtract overflow; regression-seed сохранён proptest). Flake — из фикса BUG-S1-003 (коммит `a60e61a`): `alice_bal` и `send` генерируются независимо, без ограничения `send <= alice_bal`. Fix (test-only): `let send = send.min(alice_bal);`. Устаревший untracked `proptest-regressions/` удалён.
4. **Verification (2026-10-11):** `cargo check` — green; `cargo test --workspace` — **green** (все suites, включая исправленный proptest); `cargo clippy --all-targets -- -D warnings` — **clean**. Cargo-fuzz по §9.5 не требуется (L-баг); fuzz-сборка на этом Windows-хосте падает на известном `getrandom`/`dlltool` (см. AGENTS.md), лок фикс не затрагивает.

---

### BUG-S1-009 — Нет Wallet round-trip теста (`new → save → load → sign/verify`)

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P04 КГ |
| **Категория** | D — тестовая |
| **Статус** | **fixed** (2026-10-12) |
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

**Решение (2026-10-12): `tests/wallet.rs` — round-trip + 4 негативных сценария (test-only)**

1. **Тесты** (`tests/wallet.rs`, новый файл, 5 тестов; production-код не менялся):
   - `wallet_new_save_load_sign_verify_roundtrip` — `TestDir` → `Wallet::new` → keystore сохранён (`list_keystores` = 1 файл) → `Wallet::load` тем же паролем → pubkey совпадает → `sign(blake3-digest)` → `Wallet::verify(compact[64], digest, loaded_pk)` = **true**;
   - `wallet_load_rejects_wrong_password` — неверный пароль → `Err(ConfigError)` (AES-GCM decrypt fail);
   - `wallet_load_rejects_corrupted_keystore` — не-JSON мусор вместо keystore → `Err(ConfigError)`;
   - `wallet_load_rejects_tampered_ciphertext` — flip 1 байта ciphertext через decode/re-encode base64 (base64 остаётся валидным → load доходит до AES-GCM decrypt и падает по auth-tag; nonce/salt не тронуты — `Nonce::from_slice` паникует на неверной длине) → `Err(ConfigError)`;
   - `wallet_sign_verify_with_wrong_pubkey_rejected` — подпись wallet A **не** верифицируется pubkey из `test_support::generate_keypair()`; sanity-проверка своим pubkey = true.
2. **CI (п.3 рекомендации — с оговоркой по прецеденту):** отдельный step `cargo test --test wallet` **не добавлен** — CI уже гоняет `cargo test --all`, который запускает все `tests/*.rs` включая новый `wallet.rs`; тот же rationale зафиксирован при закрытии BUG-S1-007 (п.4: «как отдельный CI job не требуется»).
3. **Changelog** не обновлялся — прецедент BUG-S1-007/008 (test/hygiene-фиксы категории D/E без Changelog-записей; Changelog пополняется только production-изменениями).
4. **Verification (2026-10-12):** `cargo test --test wallet` — **5/5 green**; `cargo test --workspace` — **green** (EXIT=0, все suites); `cargo clippy --all-targets -- -D warnings` — **clean**. Cargo-fuzz не требуется (категория D — тестовая; production-код и wire/consensus не затронуты).

---

### BUG-S1-010 — Нет теста `InvalidChainId` rejection

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P05 КГ |
| **Категория** | D — тестовая |
| **Статус** | **fixed** (2026-10-12) |
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

**Решение (2026-10-12):** Закрыт на основании коммита `b9bec04` (фикс BUG-S1-004, 2026-10-11) — рекомендованные тесты легли в тот же changeset; статус обновлён по факту верификации:

1. **Тесты** (`tests/chain_id.rs`, создан в `b9bec04`, 5 тестов):
   - `cross_chain_tx_rejected_in_mempool` — mainnet-mempool (`create_test_blockchain_for_network(dir, CHAIN_ID_MAINNET)`) отвергает testnet-tx (`chain_id=2`) → `Err(InvalidChainId { expected: CHAIN_ID_MAINNET, got: CHAIN_ID_TESTNET })` + mempool остаётся пустым;
   - `block_with_foreign_chain_id_tx_rejected` (≈ рекомендованный `cross_chain_tx_rejected_in_validate_and_apply`) — структурно валидный блок (position/hash/tx_root/target OK) с foreign-chain transfer → `add_block` → `Err(InvalidChainId { .. })` на state-machine гейте до любого state-transition;
   - `correct_chain_tx_accepted` — control: same-chain tx принят, `mempool_len == 1`;
   - +2 сверх рекомендации: mainnet reward schedule в mining; coinbase-overpay reject.
2. **Proptest** `chain_id_validation` (`consensus_proptest.rs:197-246`) — таутологичный `x == x` заменён реальным `apply_block(&State::new(), &block, chain_id)`-гейтом: own chain → не `InvalidChainId`; foreign chain → `Err(InvalidChainId { expected, got })`. Рекомендация «реальный вызов `mempool::insert`» выполнена интеграционно через `Blockchain::apply_tx` → mempool в `cross_chain_tx_rejected_in_mempool` (п.1); на proptest-уровне выбран state-machine гейт — он закрывает witness- и rebuild-пути (инвариант #10).
3. **chain_id из Config**: тесты используют `create_test_blockchain_for_network(db, network_id)` / `bc.chain_id()` — prerequisite п.2 рекомендации (BUG-S1-004) выполнен.
4. **CI/Changelog**: категория D — отдельный CI job не добавлялся (`cargo test --all` уже запускает `tests/chain_id.rs`); Changelog не обновлялся (precedent BUG-S1-007/008/009).
5. **Verification (2026-10-12)**: `cargo test --test chain_id` — **5/5 green**; `cargo test -p strangecoin-core --test consensus_proptest` — **9/9 green** (`chain_id_validation` ok); `cargo test --workspace` — **green** (EXIT=0, все suites); `cargo clippy --all-targets -- -D warnings` — **clean**. Cargo-fuzz не требуется (категория D — тестовая; production-код и wire/consensus не затронуты).

---

### BUG-S1-011 — Нет теста retarget на реальной высоте 2016

| Поле | Значение |
|------|----------|
| **Серьёзность** | M (Medium) |
| **Источник** | audit; P08 КГ |
| **Категория** | D — тестовая |
| **Статус** | **fixed** (2026-10-13) |
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

**Решение (2026-10-13): `tests/retarget.rs` + обнаружен и исправлен production consensus-баг BUG-S1-016**

1. **Тесты** (`tests/retarget.rs`, новый файл, 6 тестов; хелпер `chain_with_spacing` строит цепочку без PoW/хешей — `compute_target` читает только timestamp и target):
   - `compute_target_at_height_2016` — 2016 блоков, spacing 600s (= TARGET_BLOCK_TIME) → ratio 1.0 → target возвращается **byte-for-byte** (сравнение с literal hex, формула не дублируется);
   - `retarget_doubles_target_when_blocks_twice_as_slow` — 1200s → ×2 (точная арифметика степеней двойки);
   - `retarget_halves_target_when_blocks_twice_as_fast` — 300s → ÷2;
   - `retarget_clamps_at_max_factor` — 6000s (ratio 10) → clamp ×4;
   - `retarget_clamps_at_min_factor` — 75s (ratio ⅛) → clamp ÷4;
   - `no_retarget_below_interval` — 2015 блоков → inherit parent target.
2. **Production-баг, вскрытый тестами** — clamp-тесты упали: `compute_target` вернул raw ratio 10× без clamp. Корневая причина — **endian-инконсистентность U256-хелперов** (`consensus.rs`): конвертеры `u256_from_bytes`/`u256_to_bytes`/`u256_from_u64` хранят limbs в BE-порядке, а арифметика (`u256_mul`/`u256_div`/`u256_gt`/`u256_min`/`u256_max`) — в LE. Ratio-арифметика случайно корректна (факторы сокращаются в div), а clamp-границы и `validate_difficulty` сравнивают limb-reversed значения. Описан подробно в **BUG-S1-016** (новая запись ниже) — фикс применён совместно.
3. **Proptest усилен по рекомендации аудита** (п.1 «сравнение с эталоном» + критика «пересчитывает формулу сам»): `difficulty_target_clamp` (`consensus_proptest.rs`) переписан — теперь вызывает **реальный** `compute_target` на синтетической цепочке из 2016 блоков и проверяет clamp-инварианты против `u256_mul_sat`/`u256_div`-границ; диапазон timespan расширен до 10M (покрывает max-clamp ветку).
4. **CI/Changelog**: категория D — отдельный CI job не добавлялся (`cargo test --all` запускает `tests/retarget.rs`); Changelog не обновлялся (precedent BUG-S1-007/009/010).
5. **Verification (2026-10-13)**: `cargo test --test retarget` — **6/6 green** (< 0.05s); `cargo test -p strangecoin-core --test u256` — **7/7 green**; `cargo test -p strangecoin-core --test consensus_proptest` — **9/9 green**; `cargo test --workspace` — **green** (EXIT=0, все suites); `cargo clippy --all-targets -- -D warnings` — **clean**. Cargo-fuzz не требуется (категория D + consensus-фикс BUG-S1-016 покрыт детерминированными тестами).

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

**Решение (2026-10-13): удаление мёртвых helper'ов + обоснование оставшихся allow + CI-gate**

1. **Удалено (3 полностью мёртвых helper'а):**
   - `tests/common/mod.rs`: `wait_for_event` + `wait_for_block_applied` — использовались только друг другом, ни один тест не вызывал;
   - `crates/strangecoin-core/tests/state_roundtrip.rs`: `arbitrary_address` — не вызывался (в `consensus_proptest.rs` своя независимая копия, используемая там).
2. **Обоснованы (3 `#[allow(dead_code)]` в `tests/common/mod.rs`):** рекомендация «убрать allow у используемых в других тестах helpers» неприменима: 20 integration-бинарей через `mod common;` компилируют свою копию модуля; `coinbase_tx`/`craft_child` живы только в `events.rs`/`state_root.rs`, в остальных 18 бинарях lint срабатывает. Над каждым allow — `//`-комментарий с фактическим использованием (P21).
3. **`#[allow(clippy::await_holding_lock)]` (8 сайтов: network×2, network_id, sync_headers×3, events, rate_limit) — оставлены, рефакторинг отклонён:** анализ подтвердил осознанное удержание `NETWORK_TEST_LOCK` (std Mutex) через весь тест — cross-binary serialization (ADR-0011; зафиксировано ещё в BUG-S0-005 resolution); перенос lock acquisition после `.await` сломал бы инвариант сериализации сетевых тестов. 3 сайта (`network.rs:169`, `sync_headers.rs:86,133`) были без обоснования — добавлен стандартный комментарий, как у остальных 5.
4. **CI-gate** (`.github/workflows/ci.yml`, job `source-gates`): rg-проверка — каждый `#[allow(dead_code)]`/`#[allow(clippy::await_holding_lock)]` в `src/ tests/ crates/` обязан иметь `//`-комментарий на предыдущей строке; без Rust-сборки, как существующий balances-gate (BUG-S0-022 / S1-P12).
5. **Итог инвентаризации:** было 14 allow (6 dead_code + 8 await_holding_lock) → стало 11 (3 dead_code + 8 await_holding_lock), все с обоснованием; `src/` — 0 allow.
6. **Verification (2026-10-13):** `cargo check` — green; `cargo test --workspace` — **green** (EXIT=0, все suites, включая events/state_root с craft_child и state_roundtrip 11/11); `cargo clippy --all-targets -- -D warnings` — **clean**; симуляция CI-gate (Select-String, на хосте нет rg) — **11 hits / 0 violations**. `cargo fmt --check` — предсуществующие расхождения форматирования по всему репо не связаны с правками BUG-S1-012 (мои правки fmt-clean) — отдельный гигиен-вопрос, не в скоупе этого бага. Категория E — отдельный CI job не добавлялся (gate встроен в существующий `source-gates`); Changelog не обновлялся (precedent BUG-S1-007/009/010/011).

---

### BUG-S1-013 — PGP-ключ — placeholder, файла `pgp_key.asc` нет

| Поле | Значение |
|------|----------|
| **Серьёзность** | H (High) |
| **Источник** | audit; P25 КГ (SECURITY.md с PGP key) |
| **Категория** | G — операционная |
| **Статус** | **partially fixed (2026-10-13, doc-side) / open (ops gate)** |
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

**Решение (2026-10-13): doc-side закрыто; key generation + Immunefi — human ops gate**

1. **Tier-расхождение устранено в пользу фактического артефакта:** BOUNTY.md сохраняет 4 tier'а (Critical $100k / High $10k / Medium $1k / Low $500); `Changelog.md` P25-запись исправлена с «3 reward tiers ($1k/$10k/$100k)» на «4 reward tiers ($500/$1k/$10k/$100k)» с пометкой о коррекции (precedent — коррекция P22-записи в Changelog). Вариант «урезать BOUNTY.md до 3» отклонён: 4-уровневая таблица — нормативная программа, соответствует Immunefi-конвенциям.
2. **SECURITY.md §2 — placeholder убран, fake armor-блок удалён:** прежний `-----BEGIN PGP PUBLIC KEY BLOCK-----` с текстом `[PLACEHOLDER: ...]` был неимпортируем и вводил в заблуждение; §2 теперь содержит честный ops-gate статус (pending до mainnet launch), 4-пунктовый чек-лист, слот fingerprint `[TBD]` и pointer на `pgp_key.asc` (pending). Таблица контактов и футер обновлены; §2.1 переписан как pointer на процедуру. Версия документа 1.0 → 1.1.
3. **Процедура зафиксирована в `docs/security/PGP_SETUP.md`:** offline-генерация (RSA 4096 / ed25519, security@strangecoin.io, no expiration) → export pubkey → commit `pgp_key.asc` + fingerprint → keyservers (keys.openpgp.org, keyserver.ubuntu.com) → Immunefi dashboard → verification → §7 closure checklist (9 пунктов, включая обновление этого файла и `STAGE1_SUMMARY §6.8`). Явные non-actions: не генерировать на dev-станции/CI, не оборачивать fingerprint в armor-блок.
4. **STAGE1_SUMMARY.md §6.8 — новый residual:** PGP key + Immunefi = открытый ops gate со ссылкой на процедуру (требование §9.6 процесса закрытия — «для ops gate багов описать offline-процедуру и записать в §6 статус»).
5. **Immunefi: претензия из P26 DoD НЕ удалялась** — DoD #9 уже содержит escape-hatch «активна на Immunefi **(или локально подготовлена)**» (`prompt-stage0.md` P26 п.9); локально подготовленная программа документационно закрывает критерий. Регистрация на Immunefi остаётся human action (BOUNTY.md §5.1).
6. **Drive-by:** `INCIDENT_RESPONSE.md` §3.2 — фантомный URL `immunefi.com/strangecoin` заменён на «pending registration (BOUNTY.md §5.1)»; email `security@strangecoin.org` приведён к `.io` (несоответствие SECURITY.md/BOUNTY.md).
7. **Статус:** doc-side части закрыты (tier-счётчик, placeholder, процедура, §6.8); генерация ключа, commit `pgp_key.asc`, keyserver upload и Immunefi registration — human ops, баг остаётся **open (ops gate)** до выполнения чек-листа `PGP_SETUP.md §7`. Ограничение среды: gpg на dev-хосте отсутствует, приватный ключ не должен кустариться на сетевой рабочей станции — потому п.1–4 рекомендованного исправления (offline-машина) выполняет maintainer, не агент. Категория G — cargo-сборка не применялась (doc-only); верификация — grep-проверки (см. ниже).
8. **Verification (2026-10-13):** grep-аудит doc-side (вне исторических записей audit/bugfixes): `PLACEHOLDER` в SECURITY.md — 0; fake armor-блок (`BEGIN PGP PUBLIC KEY BLOCK`) — 0 в SECURITY.md; «3 reward tiers» в Changelog — 0; `security@strangecoin.org` — 0; `immunefi.com/strangecoin` — 0; `pgp_key.asc` упоминается только как pending/ops-gate или в процедурах; статус бага + секция `Решение` присутствуют. `cargo test` не запускался — изменения doc-only (precedent BUG-S1-012: doc-части не требуют workspace-сборки).

---

### BUG-S1-014 — `INVARIANTS_ENFORCED.md` Stage 0 имеет арифметические расхождения

| Поле | Значение |
|------|----------|
| **Серьёзность** | L (Low) |
| **Источник** | audit; P26 КГ; BUG-S0-024 (родственная, но для Stage 1) |
| **Категория** | E — документационная |
| **Статус** | **fixed** (2026-10-13) |
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

**Решение (2026-10-13): три header'а переписаны по фактическим таблицам + cross-refs + CI-gate**

1. **`docs/stage0/INVARIANTS_ENFORCED.md`:** summary-таблица → `Enforced 17 / Deferred 5 / N/A 0 / **Total** 22` (фактическое распределение таблиц: enforced #1–14,16,17,22; deferred #15,18,19,20,21; N/A — none); label `Deferred (planned Stage 1)` → `Deferred` (#20 — Stage 5, #15/18 — Stage 1.5, старый label был неверен и по составу); добавлена Revision-строка и source-of-truth ссылка (таблицы документа — авторитетный источник; STAGE0_SUMMARY ссылается; CI-gate `check_stage0_arith.py`).
2. **`docs/stage0/CRITICAL_ISSUES_CLOSED.md`:** header + summary-таблица → `12 closed, 3 partially closed, 0 deferred` (детальная таблица: 11 нумерованных closed + строка D02 «Changelog claims»; partial — #8, #13, строка D03 «Release pipeline»); Source-строка уточнена: 13 original + 2 rows (D02/D03) = 15 detail rows; добавлены Revision-строка и source-of-truth ссылка.
3. **`docs/stage0/STAGE0_SUMMARY.md:18-19`:** обе metric-строки переписаны с cross-refs: `12 closed, 3 partially closed, 0 deferred — see CRITICAL_ISSUES_CLOSED.md` и `17/22 enforced, 5 deferred, 0 N/A — see INVARIANTS_ENFORCED.md`. Историческое состояние tag-момента (9/3/1, где «1 deferred» = #6 bech32) сохранено в Notes-столбце детальной таблицы (#6 «Closed on Stage 1», #8 «closed by S1-P01»).
4. **Cross-reference модель:** детальные таблицы (INVARIANTS_ENFORCED, CRITICAL_ISSUES_CLOSED) — источники истины; STAGE0_SUMMARY — указатель; взаимная согласованность защищена CI-gate.
5. **CI-gate** (`scripts/audit/check_stage0_arith.py` + step в `.github/workflows/ci.yml` job `source-gates`): парсит 3 документа — summary-счётчики == row-счётчики секций; сумма инвариантов == 22 и полное покрытие #1..22 без дублей/пропусков; header `**Status:**` == summary-таблица == detail-таблица; metric-строки STAGE0_SUMMARY == другим двум документам. Python stdlib; exit 1 + отчёт при любом расхождении; без Rust-сборки (прецедент gates BUG-S0-022 / BUG-S1-012).
6. **Verification (2026-10-13):** doc-only + CI-YAML — cargo-сборка не применялась (precedent BUG-S1-012/013). Симуляция CI-gate на хосте (нет Python; PowerShell-реплика логики скрипта, Select-String/regex): INV sections 17/5/0 sum=22, coverage 1..22 без dupes/missing; CRITICAL header=summary=detail 12/3/0; STAGE0_SUMMARY metric-строки совпадают. Grep-аудит: `4 deferred|1 N/A|10 closed|9/13` — 0 hits вне исторических записей (`analytics/strangecoin_audit_report.md` — источник бага, «Факт»-секции BUG-S1-014, Revision-заметки «was …» в исправленных документах). Реальный запуск скрипта — на CI (ubuntu-latest, python3).

---

### BUG-S1-015 — Offline genesis key не сгенерирован (ops gate для mainnet)

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) для mainnet |
| **Источник** | SCIP-0001; BUG-S0-015 (код закрыт, ops — нет); audit |
| **Категория** | G — операционная / B — криптографическая |
| **Статус** | **partially fixed (2026-10-14, tooling) / open (ops)** |
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

**Решение (2026-10-14): tooling + docs закрыты; offline keygen остаётся human ops**

1. **`examples/generate_offline_genesis_key.rs` (новый):** генерирует свежий secp256k1-ключ через `SecretKey::new(&mut OsRng)`; stdout — только сжатый pubkey (33 bytes, `0x`-hex, одна строка, безопасно редиректить в `genesis_pubkey.txt`); privkey — hex-файл через позиционный аргумент с `create_new(true)` (никогда не перезаписывает) либо stderr с warning-баннером при отсутствии аргумента; privkey **никогда** не попадает в stdout; `--help`; doc-comment фиксирует air-gap контракт (SCIP-0001 §1/§4).
2. **`docs/SCIP/scip-0001-genesis-key-replacement.md`:** §4 — Rust-example объявлен primary path (команда с `-- genesis_privkey.hex > genesis_pubkey.txt`), openssl понижен до alternative; Current-state таблица + Reference Implementation дополнены example'ом.
3. **Документация:** `STAGE1_SUMMARY.md §6.1` и `THREAT_MODEL.md` V-31 дополнены ссылкой на tooling; `Changelog.md` — подсекция1.1.1.
4. **Не сделано (намеренно, human ops):** ключ не сгенерирован — SCIP-0001 §4 требует air-gapped машину, генерация на сетевом dev-хосте нарушила бы контракт; `genesis.json:initial_holder_pubkey` и `EXPECTED_GENESIS_HASH` не тронуты (testnet genesis стабилен, тесты зелёные).
5. **Остаток ops (шаги 3–9 «Рекомендуемого исправления» выше):** air-gapped прогон example → vault privkey → pubkey в `genesis.json` → `--print-genesis-hash` → новый `EXPECTED_GENESIS_HASH` → `cargo test --test genesis_key` → tag `v1.0.0-mainnet-freeze` → burn-announce → `STAGE1_SUMMARY §6.1` 🟡→✅.
6. **Verification (2026-10-14):** `cargo check`, `cargo build --examples`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` — green; прогон example: stdout = `0x` + 64 hex (02/03-префикс, 33 bytes), privkey-файл variant + no-overwrite refusal проверены, временные артефакты удалены; `git diff` подтверждает: `genesis.json`/`consensus.rs` (`EXPECTED_GENESIS_HASH`) не изменены.

**Residual / follow-up:**
- Human ops (шаги 3–9 выше) — закрывает BUG-S1-015 и снимает 🟡 в `STAGE1_SUMMARY §6.1`; блокирует mainnet freeze.

---

### BUG-S1-016 — U256-конвертеры в другом limb-порядке, чем арифметика (endian inconsistency)

| Поле | Значение |
|------|----------|
| **Серьёзность** | C (Critical) — consensus |
| **Источник** | discovered during BUG-S1-011 (retarget-тесты упали на clamp) |
| **Категория** | B — криптографическая / C — консенсус |
| **Статус** | **fixed** (2026-10-13) |
| **Stage target** | S1.5-P10 (вместе с BUG-S1-011) |

**Факт (по коду, 2026-10-13):**
- `crates/strangecoin-core/src/consensus.rs` — U256 = `[u64; 4]`, но две группы функций использовали **разные limb-порядки**:
  - Конвертеры `u256_from_bytes` / `u256_to_bytes` / `u256_from_u64` — **BE-limb** (`words[0]` = старшие 8 байт; `from_u64(v) = [0,0,0,v]`);
  - Арифметика `u256_mul` / `u256_div` / `u256_add` / `u256_gt` / `u256_le` / `u256_min` / `u256_max` / `u256_shl1` — **LE-limb** (`words[0]` = младший).
- **Следствие 1 (retarget clamp мёртв):** в `compute_target` ratio-арифметика случайно корректна (фактор `2^192` из `from_u64` сокращается в div), но clamp-границы `prev*4` / `prev/4` живут в `words[3]`, а результат ratio — в `words[0]` → `u256_min`/`u256_max` сравнивают «через limbs» и clamp **никогда не срабатывает**. Тест BUG-S1-011 `retarget_clamps_at_max_factor` это доказал: ratio 10× проходил без clamp (`0x0a00` в первом limb).
- **Следствие 2 (PoW-правило garbled):** `validate_difficulty` / `validate_header_pow` / mining-loop сравнивают `u256_from_bytes(hash)` с `u256_from_bytes(target)` через `u256_gt`/`u256_le` — численно это **limb-reversed сравнение**, не `hash <= target`. Для real-world target'ов с leading zeros PoW-правило фактически не ограничивает хеш (Mitigation V-05 / инвариант #3 формально выполнялся, семантически — нет).
- **Следствие 3 (fork choice):** `work_from_target` → `cumulative_work` → `chain_selector` — work-значения и их сравнения вычислялись в garbled-порядке; при разных target'ах в цепочке порядок cumulative work мог инвертироваться относительно истинного.
- **Следствие 4 (clamp overflow edge):** даже после endian-фикса `prev_target * 4` переполняет u256 для prev ≥ 2^254 (regtest-специфика: genesis target = `0xffff…ff`); wrap-нутая ceiling < floor → clamp возвращал target ниже минимума. Proptest `difficulty_target_clamp` это поймал (minimal input: prev_target top-byte 193).
- Wire-формат не затронут: `target` в блоках — hex BE-строка, менялся только внутренний limb-порядок после decode.

**Ожидание:**
- `u256_from_bytes`/`to_bytes`/`from_u64` обязаны отдавать/принимать значения в том же limb-порядке, в котором работают `mul`/`div`/`gt`/`min`/`max`.
- Clamp `[prev/4, prev*4]` в `compute_target` обязан срабатывать на всех допустимых target'ах, включая regtest max-target.
- `validate_difficulty` обязан реализовывать численное `hash <= target`.

**Решение (2026-10-13): LE-limb конвертеры + saturating ceiling + u64 short-circuit (вместе с BUG-S1-011)**

1. **Конвертеры → LE-limb** (`consensus.rs`):
   - `u256_from_bytes`: `words[3 - i] = u64::from_be_bytes(chunk_i)` (первые 8 BE-байт → `words[3]` = старший limb);
   - `u256_to_bytes`: обратная индексация `(3 - i) * 8`;
   - `u256_from_u64(v) = [v, 0, 0, 0]` (младший limb);
   - Doc-comment фиксирует конвенцию ссылкой на BUG-S1-016.
2. **`u256_mul_sat`** (новая pub-fn): saturating multiply — переполнение детектируется как `(a*b)/b != a`; ceiling clamp в `compute_target` теперь `u256_mul_sat(prev, 4)` → при prev ≥ 2^254 возвращает `U256::MAX` вместо wrap.
3. **`compute_target` u64 short-circuit**: clamp-границы decide'ятся по u64-отношению `actual_time` к `expected_time` **до** u256-умножения (`actual >= 4*expected` → ceiling; `4*actual <= expected` → floor) — устраняет переполнение numerator для near-max target'ов и делает clamp точным на границах.
4. **Regression-тесты конвертеров** (`crates/strangecoin-core/tests/u256.rs`, новый файл, 7 тестов): limb-placement, bytes-roundtrip, from_u64≡from_bytes, mul-без-overflow, численный ordering, add/div, `work_from_target` numeric difficulty.
5. **Proptest `difficulty_target_clamp` переписан** (по прямой рекомендации BUG-S1-011 «не пересчитывать формулу сам»): вызывает реальный `compute_target` на синтетической цепочке из 2016 блоков; timespan-диапазон расширен до 10M (покрытие max-clamp ветки).
6. **Chain-compat анализ:** mainnet/testnet genesis не используют U256-конвертеры в wire-путях, где старый и новый порядок различались бы на существующих данных; regtest-цепочки с `0xffff…ff`/`0x7fff…ff`/нулевым target'ом дают идентичные результаты до/после (symmetric/zero patterns). Cumulative work пересчитывается из chain при загрузке (`state_cache.rs:268`, `chain_selector.rs:227`) — stale garbled-значения не переживают рестарт.
7. **Verification (2026-10-13):** `cargo test --test retarget` — **6/6 green**; `cargo test -p strangecoin-core --test u256` — **7/7 green**; `cargo test -p strangecoin-core --test consensus_proptest` — **9/9 green** (включая переписанный `difficulty_target_clamp` + сохранённые proptest-regression seeds); `cargo test --workspace` — **green** (EXIT=0); `cargo clippy --all-targets -- -D warnings` — **clean**.

**Residual / follow-up:**
- V-04 mitigation «clamp factor 4x» (THREAT_MODEL) теперь реально enforced — обновить строку V-04 при следующем THREAT_MODEL review.
- Инвариант #3 (`hash <= target`, INVARIANTS_ENFORCED.md) семантически исправлен — строка таблицы не требует правки (локации те же), но формулировка «via U256 arithmetic» теперь соответствует действительности.

---

## 4. Сводная таблица

| BUG ID | Title | Severity | Status | Stage |
|---|---|---|---|---|
| BUG-S1-001 | Release pipeline не запускался на CI | H | **fixed** (2026-10-10) | S1.5-P06 / ops |
| BUG-S1-002 | `state_root == [0;32]` opt-out ломает #19 | C | **fixed** (2026-10-11) | S1.5-P03 (SCIP-0002) |
| BUG-S1-003 | `verify_block_stateless` не пересчитывает post-root | C | **fixed** (2026-10-11) | S1.5-P03 |
| BUG-S1-004 | `current_chain_id()` захардкожен в REGTEST | C | **fixed** (2026-10-11) | S1.5-P05 |
| BUG-S1-005 | Per-message rate-limit не enforced | H | **fixed** (2026-10-11) | Stage 2-P03 |
| BUG-S1-006 | Single-block size check отсутствует | H | **fixed** (2026-10-11) | S1.5-P07 |
| BUG-S1-007 | Golden-векторы блоков таутологичны | M | **fixed** (2026-10-11) | S1.5-P08 |
| BUG-S1-008 | Мёртвая зависимость `sha2` | L | **fixed** (2026-10-11) | S1.5-P09 |
| BUG-S1-009 | Нет Wallet round-trip теста | M | **fixed** (2026-10-12) | S1.5-P10 |
| BUG-S1-010 | Нет теста `InvalidChainId` rejection | M | **fixed** (2026-10-12) | S1.5-P10 |
| BUG-S1-011 | Нет теста retarget на высоте 2016 | M | **fixed** (2026-10-13) | S1.5-P10 |
| BUG-S1-012 | `#[allow(dead_code)]` бандажи без TODO | L | **fixed** (2026-10-13) | S1.5-P11 |
| BUG-S1-013 | PGP-ключ — placeholder | H | **partially fixed (2026-10-13, doc) / open (ops)** | ops gate |
| BUG-S1-014 | `INVARIANTS_ENFORCED.md` Stage 0 арифметика | L | **fixed** (2026-10-13) | S1.5-P12 |
| BUG-S1-015 | Offline genesis key не сгенерирован | C (mainnet) | **partially fixed (2026-10-14, tooling) / open (ops)** | ops gate |
| BUG-S1-016 | U256-конвертеры в другом limb-порядке (clamp мёртв, PoW garbled) | C | **fixed** (2026-10-13) | S1.5-P10 (с BUG-S1-011) |

**Итого:** 0 полностью открытых + 2 частично закрытых (BUG-S1-013: doc-side fixed, ops gate открыт; BUG-S1-015: tooling fixed 2026-10-14, offline keygen — human ops) + 14 fixed (BUG-S1-001, 2026-10-10; BUG-S1-002, 2026-10-11; BUG-S1-003, 2026-10-11; BUG-S1-004, 2026-10-11; BUG-S1-005, 2026-10-11; BUG-S1-006, 2026-10-11; BUG-S1-007, 2026-10-11; BUG-S1-008, 2026-10-11; BUG-S1-009, 2026-10-12; BUG-S1-010, 2026-10-12; BUG-S1-011, 2026-10-13; BUG-S1-012, 2026-10-13; BUG-S1-014, 2026-10-13; BUG-S1-016, 2026-10-13).
- 1 Critical ops-gate для mainnet (BUG-S1-015: tooling закрыт 2026-10-14 — `examples/generate_offline_genesis_key.rs`; human ops остаётся: air-gapped keygen → vault → `genesis.json` + `EXPECTED_GENESIS_HASH` → tag, см. §3 «Решение»).
- ~~1 Critical blocker архитектурный (BUG-S1-004 — mainnet-ветка мёртвая)~~ **закрыт 2026-10-11**.
- ~~1 High сетевой (BUG-S1-005 — per-message RL)~~ **закрыт 2026-10-11** (per-message + per-type buckets).
- ~~1 High сетевой/архитектурный (BUG-S1-006 — size check)~~ **закрыт 2026-10-11** (block + per-tx checks в `validate_and_apply`).
- 1 High: BUG-S1-013 (PGP, ops) — **doc-side закрыт 2026-10-13** (placeholder убран, процедура в `PGP_SETUP.md`, tier-счётчик выровнен); key generation + Immunefi — human ops gate, см. `STAGE1_SUMMARY §6.8`.
- ~~1 Medium тестовый (BUG-S1-007 — golden vectors tautology)~~ **закрыт 2026-10-11** (literal hex + hash pin + example).
- ~~1 Low гигиена (BUG-S1-008 — sha2 dead dep)~~ **закрыт 2026-10-11** (direct-dep удалён; drive-by: flake-фикс proptest из BUG-S1-003).
- ~~1 Medium тестовый (BUG-S1-009 — Wallet round-trip)~~ **закрыт 2026-10-12** (`tests/wallet.rs`: round-trip new→save→load→sign/verify + wrong password / corrupted keystore / tampered ciphertext / wrong pubkey).
- ~~1 Medium тестовый (BUG-S1-010 — InvalidChainId rejection)~~ **закрыт 2026-10-12** (тесты легли в коммит BUG-S1-004 `b9bec04`: `tests/chain_id.rs` — mempool reject testnet tx + block reject + control accept; таутологичный proptest заменён apply_block-гейтом).
- ~~1 Medium тестовый (BUG-S1-011 — retarget на 2016)~~ **закрыт 2026-10-13** (`tests/retarget.rs`: height-2016 equality + ×2/÷2 + clamp ×4/÷4 + below-interval inherit; literal-hex эталоны без дублирования формулы; proptest `difficulty_target_clamp` переписан на реальный `compute_target`).
- ~~1 Critical consensus (BUG-S1-016 — U256 limb-order)~~ **закрыт 2026-10-13** (discovered by BUG-S1-011 clamp-тесты: конвертеры BE-limb vs арифметика LE-limb → retarget clamp молча не срабатывал, `validate_difficulty` сравнивал limb-reversed; фикс: LE-limb конвертеры + `u256_mul_sat` + u64 clamp short-circuit + `tests/u256.rs` 7 regression-тестов).
- Остальные 2 — качество/гигиена (BUG-S1-012, BUG-S1-014); ~~BUG-S1-012~~ **закрыт 2026-10-13** (удалены 3 мёртвых helper'а, 3 allow обоснованы, 8 await_holding_lock — held-by-design, CI-gate в source-gates); ~~BUG-S1-014~~ **закрыт 2026-10-13** (3 Stage 0 header'а переписаны по фактическим таблицам: invariants 17/5/0, critical issues 12/3/0; cross-refs; CI-gate `check_stage0_arith.py` в source-gates).

---

## 5. Карта зависимостей

```
BUG-S1-015 (offline genesis key; tooling 2026-10-14, ops keygen остаётся) ──── зависит от ──── BUG-S1-004 (chain_id from Config)
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
                                    BUG-S1-013 (PGP key — doc-side closed 2026-10-13; ops key-gen остаётся)
                                              │
                                              ▼
                                   mainnet freeze (block 1)

BUG-S1-006 (size check) ──► independent ──► **закрыт 2026-10-11** (block + per-tx checks в validate_and_apply)
BUG-S1-005 (per-message RL) ──► independent ──► **закрыт 2026-10-11** (монолит; Stage 2-P03 перенесёт в network crate)
BUG-S1-007..012 ──► test/quality track ──► параллельно; BUG-S1-009 **закрыт 2026-10-12**; BUG-S1-010 **закрыт 2026-10-12**; BUG-S1-011 **закрыт 2026-10-13** (вместе с BUG-S1-016); BUG-S1-012 **закрыт 2026-10-13** (hygiene: allow-инвентаризация + CI-gate)
BUG-S1-016 (U256 limb-order) ──► discovered by BUG-S1-011 tests ──► **закрыт 2026-10-13** (LE-limb конвертеры + u256_mul_sat + u64 clamp short-circuit)
BUG-S1-014 ──► doc-only ──► **закрыт 2026-10-13** (3 header'а переписаны, cross-refs, CI-gate check_stage0_arith.py)
BUG-S1-008 ──► remove `sha2` dep ──► **закрыт 2026-10-11**
```

---

## 6. Приоритеты

**Must-fix до mainnet freeze (Critical):**
1. ~~BUG-S1-004 — `current_chain_id()` from Config (архитектурный prerequisite)~~ **закрыт 2026-10-11** (chain_id из Config через Blockchain/BlockView/Mempool; per-network genesis; state-machine gate; per-tx block reject)
2. BUG-S1-015 — Offline genesis key (ops gate, блокирует mainnet; prerequisite BUG-S1-004 снят; **tooling 2026-10-14**: `examples/generate_offline_genesis_key.rs` + SCIP-0001 §4 primary path — остался human ops: air-gapped keygen → vault → genesis rebuild)
3. ~~BUG-S1-002 — `state_root` opt-out (инвариант #19)~~ **закрыт 2026-10-11** (SCIP-0002 draft, mainnet/testnet reject + regtest opt-in)
4. ~~BUG-S1-003 — `verify_block_stateless` post-root (инвариант #19)~~ **закрыт 2026-10-11** (вариант A′: mulproof-update пересчёт post-root из parent root + witness + блока; `PostStateRootMismatch`; покрытие touched-адресов)
5. ~~BUG-S1-001 — Release pipeline first run (инвариант #22)~~ **закрыт 2026-10-10** (тег `v0.0.1-rc1`, run 38071514479 green)

**Must-fix до public testnet (High):**
6. ~~BUG-S1-005 — Per-message rate-limit~~ **закрыт 2026-10-11** (per-message `check_class` в request-loop + per-type buckets: GetHeaders/GetBlocks=100, ChainGossip/Other=30; интеграционный flood-тест)
7. ~~BUG-S1-006 — Single-block size check~~ **закрыт 2026-10-11** (`serialize_block`/`serialize_transaction_signed` против `MAX_BLOCK_SIZE`/`MAX_TX_SIZE` первыми проверками в `validate_and_apply`; reject 5 MB block / 300 KB tx + positive 3 MB block)
8. BUG-S1-013 — PGP key + Immunefi — **doc-side закрыт 2026-10-13** (SECURITY.md ops-gate status, `PGP_SETUP.md` процедура, tier-фикс Changelog); осталось human ops: offline keygen + commit `pgp_key.asc` + keyservers + Immunefi (чек-лист `PGP_SETUP.md §7`, `STAGE1_SUMMARY §6.8`)

**Quality track (можно параллельно):**
9. BUG-S1-007 — Golden vectors
10. ~~BUG-S1-009 — Wallet round-trip~~ **закрыт 2026-10-12** (`tests/wallet.rs`, 5 тестов)
11. ~~BUG-S1-010 — `InvalidChainId` test~~ **закрыт 2026-10-12** (тесты в `tests/chain_id.rs`, легли в коммит BUG-S1-004 `b9bec04`)
12. ~~BUG-S1-011 — Retarget на высоте 2016~~ **закрыт 2026-10-13** (`tests/retarget.rs`, 6 тестов; вскрыл production-баг BUG-S1-016 — U256 limb-order — исправлен вместе)

**Hygiene track:**
13. ~~BUG-S1-008 — Remove `sha2`~~ **закрыт 2026-10-11**
14. ~~BUG-S1-012 — `#[allow]` bandaids~~ **закрыт 2026-10-13** (3 мёртвых helper'а удалены; 11 оставшихся allow с обоснованием; CI-gate `source-gates` требует `//`-комментарий над каждым allow)
15. ~~BUG-S1-014 — Doc arithmetic~~ **закрыт 2026-10-13** (invariants 17/5/0 of 22, critical issues 12/3/0 — header'ы переписаны по таблицам; STAGE0_SUMMARY metric-строки с cross-refs; CI-gate `python3 scripts/audit/check_stage0_arith.py` в job `source-gates`)

---

## 7. Связь с `bugfixes-stage0.md`

| BUG-S0 (source) | BUG-S1 (continuation) | Comment |
|---|---|---|
| BUG-S0-005 (partial) | BUG-S1-001 | Перенос: pipeline написан, но не запускался |
| BUG-S0-012 (open) | BUG-S1-002 | Перенос: state_root opt-out |
| BUG-S0-013 (open) | BUG-S1-003 | Перенос: post-root в witness — **fixed 2026-10-11** (вариант A′) |
| BUG-S0-015 (fixed in code) | BUG-S1-015 | Ops gate остался |
| (none) | BUG-S1-004 | Новый: chain_id hardcoded (audit) |
| (none) | BUG-S1-005 | Новый: per-message rate-limit (audit) |
| (none) | BUG-S1-006 | Новый: single-block size check (audit) |
| (none) | BUG-S1-007 | Новый: golden vectors tautology (audit) |
| (none) | BUG-S1-008 | Новый: sha2 dead dep (audit) |
| (none) | BUG-S1-009 | Новый: wallet round-trip test (audit) |
| (none) | BUG-S1-010 | Новый: InvalidChainId test (audit) |
| (none) | BUG-S1-011 | Новый: retarget на 2016 test (audit) — **fixed 2026-10-13**, вскрыл BUG-S1-016 |
| (none) | BUG-S1-012 | Новый: `#[allow]` без TODO (audit) |
| (none) | BUG-S1-013 | Новый: PGP placeholder (audit) — **doc-side fixed 2026-10-13**, ops gate remains |
| (none) | BUG-S1-014 | Новый: doc arithmetic (audit) — **fixed 2026-10-13** (3 Stage 0 header'а + cross-refs + CI-gate) |
| (none) | BUG-S1-016 | Новый: U256 limb-order inconsistency (discovered during BUG-S1-011) |

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
# ожидается: src/network/protocol.rs (const + wire) + src/blockchain/block_executor.rs (validate_and_apply) + src/blockchain/state_cache.rs

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
