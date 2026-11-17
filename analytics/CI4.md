# Промпты для исправления релизного пайплайна в Reider85/strangecoin (CI4)

Готовые промпты для поочерёдной передачи агенту-помощнику (Claude / GPT / Copilot /
Cursor) или разработчику. Каждый промпт самодостаточен: контекст, точная причина,
требуемое решение, формат коммита, способ проверки. Применять строго в указанном
порядке — Промпт 1 чинит прямой регресс, из-за которого релизы и теги перестали
выпускаться на каждом пуше; Промпты 2-4 закрывают первопричины (blind-merge
dependabot-пачки, отсутствующий CI-gate, устаревший Node 16 action в Pages);
Промпты 5-7 устраняют оставшиеся хрупкости (невидимый skip prerelease,
тихие регрессии GitHub Actions major bumps, eframe/objc2 на macOS).

Репозиторий: https://github.com/Reider85/strangecoin
Ветка по умолчанию: `master`. Версия в `Cargo.toml`: `3.0.0`.
Промпты нумеруются как `[GITHUB-PNN]`, продолжая счётчик из CI2/CI3 (P07-P15 уже в
истории — см. коммиты `29c311a` [GITHUB-P12], `53b204c` [GITHUB-P15], `4c2101f`
[S3-P15 deps: revert secp256k1 0.33 bump]).

---

## 0. Сводка расследования: есть ли релизы и теги на каждом пуше?

### 0.1. Фактическое состояние на момент анализа (2026-11-17, до 11:00 UTC+4)

**Теги.** В `git ls-remote --tags origin` видны 12 тегов:

```
v3.0.0-stage3   (89dd756, 2026-11-16 21:50 +0400)
v3.0.0-dev.2    (89dd756, 2026-11-16 21:50 +0400) ← последний dev-тег
v3.0.0-dev.1    (117b2b8, 2026-11-16 20:55 +0400)
v2.0.0-stage2   (346e3c0, 2026-10-29)
v2.0.0-dev.4    (23a2488, 2026-11-16 12:26 +0400)
v2.0.0-dev.3    (d8153ef, 2026-11-16 03:05 +0400)
v2.0.0-dev.2    (43f3d34, 2026-11-15 23:34 +0400)
v2.0.0-dev.1    (29c311a, 2026-11-15 14:57 +0400) ← GITHUB-P12, после него pipeline ожил
v1.5.0-stage1.5 / v1.1.0-stage1 / v1.0.0-stage0 / v0.0.1-rc1
```

**Releases.** На странице `github.com/Reider85/strangecoin/releases` показаны 9
релизов: 6 dev pre-release (v2.0.0-dev.1…v3.0.0-dev.2) + 3 stage release.
Последний dev pre-release — **`v3.0.0-dev.2`** от 16 Nov 18:09 UTC, на коммит
`89dd756`.

**Пуши в master ПОСЛЕ последнего dev-тега** (все должны были породить
`v3.0.0-dev.3`, но ни один не породил):

```
2026-11-16 17:34 UTC  8531a00 chore(deps): bump actions/upload-pages-artifact 3→5
2026-11-16 17:36 UTC  b6e243b chore(deps): bump git-auto-commit-action 5→7
2026-11-16 17:36 UTC  a6b6360 chore(deps): bump codecov-action 4→7
2026-11-16 17:37 UTC  4da092a chore(deps): bump github-script 7→9
2026-11-16 17:37 UTC  a861767 chore(deps): bump toml 0.5.11→1.1.8+spec-1.1.0  ← MAJOR
2026-11-16 17:42 UTC  0e314ed chore(deps): bump actions/deploy-pages 4→5
2026-11-16 17:43 UTC  efbe658 chore(deps): bump ctrlc 3.4.7→3.5.2
2026-11-16 17:44 UTC  e464ba7 chore(deps): bump secp256k1 0.29.1→0.33.1      ← MAJOR, breaker
2026-11-16 17:49 UTC  1d40b89 chore(deps): bump serde_json 1.0.142→1.0.154
2026-11-16 17:56 UTC  d293e32 chore(deps): bump uuid 1.17.0→1.29.2
2026-11-17 02:20 UTC  99b6c7e chore(stars): hourly snapshot 25 (GITHUB_TOKEN push — НЕ триггерит CI, это нормально)
2026-11-16 21:14 UTC  71a89c9 [S4-UI-P01] ui-adr-0031: professional ui replacement + spec
2026-11-17 06:52 UTC  ba39472 [S3-P15] ci: pin release toolchain 1.98.1 + cross g++ (macos/linux-aarch64)
2026-11-17 06:52 UTC  4c2101f [S3-P15] deps: revert secp256k1 0.33 bump (blind merge, CI red)  ← revert breaker'а
```

**Соответствие CI runs → Pre-release runs** (взято со страницы Actions):

| Push commit | CI run | conclusion | Pre-release run | conclusion |
| --- | --- | --- | --- | --- |
| 0e314ed | CI #218 | cancelled | #41 | skipped |
| efbe658 | CI #219 | cancelled | #42 | skipped |
| e464ba7 (secp256k1) | CI #220 | **failed** | #43 | **skipped** |
| 1d40b89 | CI #221 | failed | #44 | skipped |
| d293e32 | CI #222 | failed | #45 | skipped |
| 71a89c9 | CI #223 | failed | #46 | skipped |
| 4c2101f (revert) | CI #224 | in_progress | #47 | skipped |

12+ пушей в master за ~13 часов НЕ породили ни одного нового dev-тега. Pipeline
«push → CI green → dev-tag → GitHub Pre-release», построенный в
`prerelease.yml` после GITHUB-P12, сломался ровно на одном уровне:

### 0.2. Точная причина остановки

**CI красный → `prerelease.yml` корректно скипает dev-tag.**

Шаг `create-dev-tag` в `prerelease.yml` имеет:

```yaml
if: ${{ github.event.workflow_run.conclusion == 'success' }}
```

Это **правильный** гейт (GITHUB-P12). Когда CI красный, prerelease обязан
пропускать создание dev-тега — иначе релизился бы нерабочий код. Поэтому
вопрос не «почему prerelease не создаёт тег», а «почему CI красный».

CI красный по одной конкретной причине — dependabot-коммит
`e464ba7 chore(deps): bump secp256k1 from 0.29.1 to 0.33.1` (16 Nov 17:44 UTC)
сделал semver-несовместимый скачок через 4 minor-релиза (0.30/0.31/0.32/0.33).
В secp256k1 0.30.0 (release notes см.
https://github.com/rust-bitcoin/rust-secp256k1/blob/master/CHANGELOG.md)
**удалили re-export `secp256k1::rand`** — теперь `rand` нужно тянуть
отдельно, а `secp256k1::rand::rngs::OsRng` больше не компилируется.

Локально подтверждено на свежем checkout (ДО revert-коммита `4c2101f`,
который вернул `secp256k1 = "0.29"`):

```
$ cargo check -p strangecoin-core --all-targets 2>&1 | tail -40
error[E0599]: no variant, associated function, or constant named `from_i32`
              found for enum `RecoveryId` in the current scope
   --> crates/strangecoin-core/src/consensus.rs:325:48
    |
325 |     let rec_id = secp256k1::ecdsa::RecoveryId::from_i32(signature[64] as i32)
    |                                                ^^^^^^^^^^^ variant not found
    |
help: consider using `RecoveryId::from_u8_masked` instead

error[E0277]: the trait bound `&Message: Into<Message>` is not satisfied
   --> crates/strangecoin-core/src/consensus.rs:330:24
    |
330 |         .recover_ecdsa(&msg, &sig)
    |          ------------- ^^^^ the trait `From<&Message>` is not implemented

warning: use of deprecated associated function
             `secp256k1::Message::from_digest_slice`: use from_digest instead
   --> crates/strangecoin-core/src/consensus.rs:321:35
```

**Дополнительно** (НЕ блокирует компиляцию, но независимо от secp256k1)
бump `toml 0.5.11 → 1.1.8+spec-1.1.0` (commit `a861767`) — major-скачок через
0.6/0.7/0.8. Локально на revert-коммите `4c2101f` (где secp256k1 уже 0.29, но
toml ещё 1.1.8) `cargo check -p strangecoin-core` проходит чисто, единственный
warning — `unused dependency base64` (давний, не связан). То есть **toml-бамп
сам по себе текущий код не ломает**, но это не гарантия для будущего —
`toml::from_str` в 1.x совместим с 0.5.x на уровне API, но более строгий
парсер может выявить спрятанные ошибки в тестовых TOML-файлах
(`tests/config_*.rs`).

### 0.3. Состояние revert'а (на момент анализа)

Мейнтейнер уже диагностировал проблему сам и за 8 минут до запроса
пользователя запушил revert `4c2101f [S3-P15] deps: revert secp256k1 0.33
bump (blind merge, CI red)` — он возвращает `secp256k1 = "0.29"` в
`Cargo.toml` и `secp256k1 v0.29.1` / `secp256k1-sys v0.10.1` в `Cargo.lock`.

CI #224 в момент анализа — in_progress на этом коммите. После зелёного CI
pipeline автоматически возобновится: workflow_run(CI completed, success) →
prerelease.yml → compute-tag (`v3.0.0-dev.3`) → build → gh release create →
commit-comment. **Таким образом, прямой блок уже снят revert'ом `4c2101f`.**

Ниже идут промпты на устранение **первопричин**, чтобы ситуация не
повторилась на следующей пачке dependabot-PR'ов.

### 0.4. Сводка багов по приоритету

| # | Баг | Workflow / файл | Промпт | Критичность |
| --- | --- | --- | --- | --- |
| 1 | secp256k1 0.33 bump ломает компиляцию (`RecoveryId::from_i32`, `recover_ecdsa(&msg, &sig)`, `Message::from_digest_slice`) — уже reverted в `4c2101f`, нужен форс-фикс без reopen'а | `Cargo.toml`, `crates/strangecoin-core/src/consensus.rs` | Промпт 1 | критично (повтор) |
| 2 | Dependabot пачкает master 12+ коммитами за 30 мин без CI-gate → следующий «blind merge» снова ломает pipeline | `.github/dependabot.yml`, `.github/workflows/` | Промпт 2 | критично |
| 3 | `peaceiris/actions-mdbook@v2.0.0` — Node 16 deprecated → Pages #29, #30 failed | `.github/workflows/pages.yml` | Промпт 3 | высоко |
| 4 | `prerelease.yml` «skipped» — невидимая тишина: мейнтейнер не видит, что dev-tag не создаётся | `.github/workflows/prerelease.yml` | Промпт 4 | высоко |
| 5 | `actions/github-script@v7→v9` (bump `4da092a`) без повторного аудита API | все workflows с github-script | Промпт 5 | средне |
| 6 | eframe 0.22 → objc2 0.3.0-beta.3 ломается на rustc ≥1.99; workaround = pin 1.98.1 в release.yml (commit `ba39472`) — не настоящий фикс | `Cargo.toml`, `crates/strangecoin-vm-wasm`, `src/gui` | Промпт 6 | средне |
| 7 | `actions/upload-artifact@v7` + `actions/download-artifact@v8` version-skew + deprecation `merge-multiple` в `release.yml` | `.github/workflows/release.yml` | Промпт 7 | низко |

---

## Промпт 1. [КРИТИЧНО] Зафиксировать revert secp256k1 0.33 + добавить pin-политику в AGENTS.md

```
Контекст. 16 Nov 2026 17:44 UTC dependabot смержил коммит e464ba7
"chore(deps): bump secp256k1 from 0.29.1 to 0.33.1" прямо в master без
локальной проверки ("blind merge"). secp256k1 0.30.0 — major release
(https://github.com/rust-bitcoin/rust-secp256k1/blob/master/CHANGE.md) —
сделал два несовместимых изменения:

  1. Удалил re-export `secp256k1::rand` (модуль `rand` больше не входит в
     feature `rand-std`; нужно тянуть `rand = "0.8"` или `"0.9"` напрямую
     и звать `OsRng` оттуда). Эффект: 4 файла перестали компилироваться:

       src/lib.rs:1387                       use secp256k1::rand::rngs::OsRng;
       src/wallet.rs:13                      use secp256k1::rand::rngs::OsRng;
       tests/address_migration.rs:57         let sk = SecretKey::new(
                                             &mut secp256k1::rand::rngs::OsRng);
       examples/generate_offline_genesis_key.rs:22  use secp256k1::rand::rngs::OsRng;

  2. Переименовал/удалил методы API ecdsa-recovery:
       RecoveryId::from_i32(i32) → RecoveryId::from_u8_masked(u8)
       Secp256k1::recover_ecdsa(&msg, &sig) → sig.recover_ecdsa(&msg)
       Message::from_digest_slice(slice) → Message::from_digest([u8;32])
                                  (устаревший, deprecated warning)

     Эффект: crates/strangecoin-core/src/consensus.rs:321, 325, 330 — три
     независимые ошибки компиляции:

       error[E0599]: no variant `from_i32` found for `RecoveryId`
       error[E0277]: `&Message: Into<Message>` not satisfied for
                     `.recover_ecdsa(&msg, &sig)`
       warning: use of deprecated `Message::from_digest_slice`

Поскольку `cargo check` падает на 4 крейтах, весь CI-воркфлоу `CI` помечается
failed. Дальше `prerelease.yml` корректно скипает dev-tag (правильный гейт
GITHUB-P12) → 12+ пушей подряд не породили ни одного нового тега. Это
зафиксировано в сводке раздела 0.

Мейнтейнер уже откатил бамп коммитом 4c2101f [S3-P15] deps: revert secp256k1
0.33 bump (blind merge, CI red) — Cargo.toml снова secp256k1 = "0.29",
Cargo.lock содержит secp256k1 v0.29.1 / secp256k1-sys v0.10.1. Локально
проверено: cargo check -p strangecoin-core проходит чисто, единственный
warning — давний `unused dependency base64` (не связан с этим фиксом).

Задача. Защититься от повторного blind-merge major-бампа криптозависимостей:

1. Привести AGENTS.md секцию "Stage 3 Pin Policy" в соответствие с реальным
   списком pinned-крейтов. На данный момент в таблице Pin Policy указано:

       consensus / data-layout / crypto dependencies are pinned with `=`
       and may only be bumped via SCIP + activation height after mainnet
       freeze.

   Но `secp256k1 = { version = "0.29", features = [...] }` — НЕ pinned с
   `=`, это caret-спецификация (caret = "^0.29" = ">=0.29, <0.30", что
   безопасно для 0.x). Однако dependabot всё равно открыл PR на 0.33
   (semver-minor, потому что `0.29` → `0.33` для 0.x не нарушает caret
   — 0.x.y совместим с 0.x.0; переход через minor-границу 0.29→0.30
   формально breaking для 0.x, но dependabot это не останавливает).
   Зафиксировать в таблице:
       secp256k1     = 0.29      (caret, можно минор внутри 0.29.x; >=0.30 — SCIP)
       secp256k1-sys = transitive (не pin явно, secp256k1 решает)
       rand          = 0.10      (косвенно через rand_core 0.10.1; bump — SCIP)
       rand_core     = 0.10.1    (caret; пин `=` при mainnet freeze)
       aes-gcm       = 0.10      (caret; bump — SCIP)
       pbkdf2        = 0.12.2    (caret; bump — SCIP)
       blake3        = 1         (caret; bump — SCIP)
       base64        = 0.22.1    (caret; bump — SCIP)

2. В .github/dependabot.yml добавить `ignore`-список для crypto/consensus
   крейтов, чтобы dependabot НЕ открывал PR'ы на major-бампы этих
   зависимостей. Пример:

       - package-ecosystem: cargo
         directory: "/"
         schedule: { interval: weekly }
         open-pull-requests-limit: 5
         labels: [dependencies]
         ignore:
           - dependency-name: "secp256k1"
             update-types: ["version-update:semver-major",
                             "version-update:semver-minor"]
           - dependency-name: "secp256k1-sys"
             update-types: ["version-update:semver-major",
                             "version-update:semver-minor"]
           - dependency-name: "rand"
             update-types: ["version-update:semver-major",
                             "version-update:semver-minor"]
           - dependency-name: "rand_core"
             update-types: ["version-update:semver-major"]
           - dependency-name: "aes-gcm"
             update-types: ["version-update:semver-major",
                             "version-update:semver-minor"]
           - dependency-name: "pbkdf2"
             update-types: ["version-update:semver-major",
                             "version-update:semver-minor"]
           - dependency-name: "blake3"
             update-types: ["version-update:semver-major"]
           - dependency-name: "wasmtime"
             update-types: ["version-update:semver-major"]
           - dependency-name: "wasmi"
             update-types: ["version-update:semver-major"]

   (Политика: minor-бампы crypto/consensus крейтов — SCIP, не dependabot.
    Patch-бампы внутри той же minor-линейки разрешены автоматически —
    они обратно-совместимы по semver.)

3. В .github/dependabot.yml включить grouped updates (фича Dependabot
   v2-config, GA с Nov 2024) для остальных крейтов — один PR "chore(deps):
   grouped cargo weekly bump" вместо 12 раздельных. Это уменьшит число
   слепых merge'ей и упростит review. Добавить в оба ecosystems:

       groups:
         cargo-minor:
             update-types: ["version-update:semver-minor",
                             "version-update:semver-patch"]
         github-actions-minor:
             update-types: ["version-update:semver-major",
                             "version-update:semver-minor"]

   (github-actions-group: actions/* принято бампать по major вместе —
    v4→v5 breaking changes обычно требуют общего аудита.)

4. НЕ переписывать Cargo.toml — secp256k1 уже на "0.29" (commit 4c2101f).
   НЕ удалять `Cargo.lock` — он уже корректен.

Требования к коммиту. Сообщение:
`[GITHUB-P16] ci: dependabot ignore crypto/consensus minor + grouped bumps + AGENTS.md pin policy update`

Проверка.

1. Локально:
   - `cargo check -p strangecoin-core --all-targets` exit 0, нет ошибок
     `RecoveryId::from_i32` / `recover_ecdsa` / `Message::from_digest_slice`.
     (Эти ошибки уже устранены revert'ом 4c2101f, проверка — регресс-тест.)
   - `cargo check -p strangecoin --no-default-features --features
     net,vm-wasm,legacy-migrate --lib` exit 0 (root-крейт без gui+storage).
   - `git diff Cargo.toml Cargo.lock` пустой (этот коммит НЕ должен
     трогать Cargo.toml/Cargo.lock — только .github/dependabot.yml и
     AGENTS.md).

2. После push в master:
   - CI lint → test-core → coverage → source-gates — все зелёные (CI #225
     или #224 в зависимости от очереди).
   - workflow_run(CI success) → prerelease.yml — НЕ skipped, доходит до
     шага "Compute next dev tag" и создаёт `v3.0.0-dev.3`. Проверить
     `git ls-remote --tags origin | grep v3.0.0-dev.3` через 10-15 мин
     после push.
   - Следующий dependabot run НЕ открывает PR на secp256k1 0.30+ —
     зависимость в ignore-списке.

3. На странице `github.com/Reider85/strangecoin/network/dependencies` —
   у secp256k1 должна появиться плашка "Ignored" для minor/major
   update-types (Dependabot отображает это в Insights → Dependency
   graph → Dependabot).

Не делай.
- НЕ форсируй bump secp256k1 обратно на 0.33 с миграцией кода — это
  отдельная задача (Stage 4+); текущий фикс = revert + pin policy.
- НЕ ставь `secp256k1 = "=0.29.1"` (точный pin) — caret "0.29" уже даёт
  нужную защиту (patch-бампы разрешены, minor — нет); точный pin
  заблокирует даже security-patch (RUSTSEC), что плохо.
- НЕ удаляй `secp256k1-sys` из Cargo.lock руками — он транзитивный и
  управляется secp256k1.
- НЕ переписывай `src/consensus.rs` / `src/wallet.rs` / `src/lib.rs` /
  `tests/address_migration.rs` / `examples/generate_offline_genesis_key.rs`
  под secp256k1 0.33 API — это будущая миграция, не часть этого промпта.
```

---

## Промпт 2. [КРИТИЧНО] CI-gate на dependabot PR'ах: запретить merge без зелёного CI

```
Контекст. 16 Nov 2026 17:34-17:56 UTC (22 минуты) dependabot смержил в master
12 коммитов "chore(deps): bump X" — все прямым squash-merge в master, без
локального cargo check. Из-за settings по умолчанию GitHub разрешает merge
PR без обязательных status checks. Один из бампомов (secp256k1 0.29→0.33)
оказался breaking — blind merge протащил его в master, CI покраснел,
prerelease-стeb перестал создавать dev-теги на ~13 часов (до прихода
revert-коммита 4c2101f).

Тот же паттерн повторился 9 Nov 2026 — CI #150 на dependabot PR #7 "Bump
eframe from 0.22.0 to 0.34.3" уже failed в момент открытия PR'а
(https://github.com/Reider85/strangecoin/actions/runs/145), но PR остался
открытым и потенциально мог быть смержен той же "blind" процедурой.

В .github/dependabot.yml стоит `open-pull-requests-limit: 5`, что позволяет
5 одновременных PR. Все 12 бампомов за 22 мин — это несколько волн по 5
PR каждая, мержились последовательно. Слияние без CI-gate = корень
проблемы, не dependabot сам по себе (он лишь предлагает).

Задача. Включить branch protection rules для ветки `master` (через
`gh api` CLI — это настройки репозитория, не файл):

1. Требовать обязательные status checks перед merge:
   - "CI" / lint (success)
   - "CI" / test-core (success)
   - "CI" / source-gates (success)
   - coverage — НЕ требовать (continue-on-error: true по спеке GITHUB-P2,
     coverage не должен блокировать merge).
2. Требовать approving review = 1 (для не-dependabot PR; dependabot
   exempt через `required_linear_history` + bypass для `dependabot[bot]`
   не нужен — dependabot PR'ы всё равно идут через тот же CI).
3. Запретить force-push в master.
4. Включить "Require branches to be up to date before merging" —
   это форсирует rebase перед merge, убирает "merge red PR" паттерн.

Конкретные команды для выполнения через `gh` CLI (это НЕ коммит в репо,
это settings — выполняется один раз администратором репозитория):

    # 1. Получить текущий commit SHA master для branch protection baseline
    MASTER_SHA=$(gh api repos/Reider85/strangecoin/branches/master \
      --jq '.commit.sha')

    # 2. Получить ID контекстов status checks, которые нужно требовать.
    #    Сначала сделать тестовый push или ждать первый CI-ран на
    #    текущем HEAD — нужно получить имена контекстов из
    #    /commits/{sha}/check-runs.
    gh api repos/Reider85/strangecoin/commits/${MASTER_SHA}/check-runs \
      --jq '.check_runs[] | {name, app: .app.slug}'

    # 3. Установить branch protection (замена всех существующих правил):
    gh api -X PUT repos/Reider85/strangecoin/branches/master/protection \
      -F required_status_checks[strict]=true \
      -F required_status_checks[contexts][]="CI / Lint (fmt + clippy)" \
      -F required_status_checks[contexts][]="CI / Test (ubuntu, stable)" \
      -F required_status_checks[contexts][]="CI / Source gates (rg audits)" \
      -F enforce_admins=true \
      -F required_pull_request_reviews[required_approving_review_count]=1 \
      -F required_pull_request_reviews[dismiss_stale_reviews]=true \
      -F restrictions= \
      -F required_linear_history=true \
      -F allow_force_pushes=false \
      -F allow_deletions=false

    # 4. Для Dependabot PRs — добавить bypass для бота через
    #    repository ruleset (требует Pro/Team; на free-аккаунте
    #    просто ждать CI как и для обычных PR).
    #    Альтернатива на free: добавить в CODEOWNERS /.github/CODEOWNERS:
    #        * @Reider85
    #    Это форсирует review request, но не блокирует auto-merge.

5. Опционально (но рекомендуется): включить GitHub Merge Queue
   (Settings → General → Pull Requests → "Allow merge queue"). Это
   форсирует, что все PR проходят CI на инкрементальном merge-commit
   (а не на исходной ветке PR), устраняя "merge green PR into red master"
   гонку. Free-аккаунт поддерживается.

Требования к коммиту. Это НЕ файловый коммит — это settings-изменение.
Задокументировать выполнение в `analytics/GITHUB.md` секцией "§10 Branch
protection & merge gate" (новая секция, продолжение нумерации после §9
GITHUB-P15 setup). В коммите в `analytics/GITHUB.md` перечислить:
- какие контексты добавлены в required_status_checks;
- кто может bypass (никто, если enforce_admins=true);
- дата включения merge queue (если включена).

Коммит-сообщение:
`[GITHUB-P17] docs: branch protection + required status checks + merge queue (settings-side fix for blind-merge dependabot)`

Проверка.

1. Открыть новый PR (например, тестовый dependabot-like PR с пробелом в
   README.md) → в UI GitHub должна появиться плашка "Required checks
   are pending" — без зелёного CI кнопка "Merge pull request" заблокирована.
2. `gh api repos/Reider85/strangecoin/branches/master/protection` —
   JSON-ответ содержит `required_status_checks.contexts` с тремя
   перечисленными контекстами.
3. На странице Actions следующий dependabot run НЕ должен попасть в
   master, пока его CI-ран не станет зелёным. В истории master больше
   не должно быть коммитов вида "chore(deps): bump X" без предшествующего
   "CI #N — success" run на том же SHA.

Не делай.
- НЕ требовать `Coverage (llvm-cov)` как обязательный статус — coverage
  работает с `continue-on-error: true` (GITHUB-P2 spec deviation 7),
  требование его зелёности убило бы merge всех PR с нестабильным
  покрытием.
- НЕ включать `restrictions` (ограничение push по пользователям/командам)
  — для solo-репозитория это избыточно и заблокирует сам мейнтейнер.
- НЕ удалять dependabot.yml — он остаётся (с ignores из Промпта 1), но
  теперь его PR'ы обязаны проходить CI перед merge.
- НЕ ставить `enforce_admins=false` — это разрешит администратору
  форсить merge красного PR "ручным" путём, что вернуло бы паттерн
  blind-merge.
```

---

## Промпт 3. [ВЫСОКО] Pages: починить `peaceiris/actions-mdbook@v2.0.0` (Node 16 deprecated) + trigger hardening

```
Контекст. .github/workflows/pages.yml в build-джобе использует:

    - name: Setup mdBook
      uses: peaceiris/actions-mdbook@v2.0.0
      with:
        mdbook-version: '0.4.52'

`peaceiris/actions-mdbook@v2.0.0` — экшн под Node.js 16 (см. action.yml
в репо https://github.com/peaceiris/actions-mdbook/blob/v2.0.0/action.yml,
`runs.using: node16`). GitHub Actions deprecation timeline:
  - 12 Sep 2024: Node 16 actions перестали запускаться на ubuntu-22.04
    runner-image (после многочисленных продлений с первоначальной даты
    1 Jun 2023);
  - 11 Oct 2024: удаление Node 16 runner'а из hosted images
    (https://github.blog/changelog/2024-10-11-github-actions-deprecating-node-16/).

Эффект: steps "Setup mdBook" падает с ошибкой "The runner failed to
execute" или просто silent exit. Это видно на страницах Actions:
  - GitHub Pages #29 (commit 0e314ed "bump actions/deploy-pages 4→5") — failed
  - GitHub Pages #30 (commit 71a89c9 "[S4-UI-P01]") — failed

(Триггер Pages на эти коммиты объясняется paths-фильтром `docs/**` /
`.github/workflows/pages.yml` — `0e314ed` модифицировал pages.yml
dependabot'ом; `71a89c9` добавил docs/ADR/0031-professional-ui-replacement.md
— оба path'а триггерят Pages.)

Дополнительно в pages.yml есть `actions/configure-pages@v4` (уже v4, ОК)
и `actions/upload-pages-artifact@v5` (v5, ОК после dependabot bump
`8531a00`) и `actions/deploy-pages@v5` (v5, ОК после `0e314ed`). То есть
единственный ломающийся action — `peaceiris/actions-mdbook`.

Задача. Заменить устаревший `peaceiris/actions-mdbook@v2.0.0` на
`cargo install mdbook` напрямую — это стандартный способ для Rust-based
инструментов, не требует сторонних GitHub Actions и не зависит от
node-версии runner-image.

1. В .github/workflows/pages.yml заменить шаг:

       - name: Setup mdBook
         uses: peaceiris/actions-mdbook@v2.0.0
         with:
           mdbook-version: '0.4.52'

   на:

       - name: Install mdBook
         # [GITHUB-P18] peaceiris/actions-mdbook@v2.0.0 was Node 16,
         # deprecated by GitHub (Oct 2024) — Pages #29/#30 died at
         # this step. Switch to cargo install: no third-party action,
         # no node-versions, runner-portable (works on ubuntu-latest
         # and on the RUNS_ON self-hosted Windows runner with Rust
         # toolchain installed).
         # [GITHUB-P06] shell: bash — the local Windows runner's
         # default pwsh treats `&&` differently; explicit bash.
         shell: bash
         run: |
           # Pin mdbook to 0.4.52 (reproducible build; bump deliberately).
           cargo install mdbook --version 0.4.52 --locked

2. Добавить перед шагом "Install mdBook" шаг установки Rust toolchain,
   если в build-джобе его ещё нет (pages.yml сегодня не имеет
   dtolnay/rust-toolchain). Добавить:

       - name: Install Rust toolchain
         # [GITHUB-P06] local runner pins gnu-flavor (matches dev
         # machine rustup default; msvc flavor would need MSVC).
         uses: dtolnay/rust-toolchain@stable
         with:
           toolchain: ${{ vars.RUNS_ON && 'stable-x86_64-pc-windows-gnu' || 'stable' }}

3. Добавить Swatinem/rust-cache@v2, чтобы mdbook не пересобирался с
   исходников при каждом запуске (cargo install ~1.5 min cold, ~5 s warm):

       - name: Cargo cache (mdbook)
         uses: Swatinem/rust-cache@v2
         with:
           shared-key: pages-mdbook

4. Триггер paths-фильтр ужесточить: сейчас Pages запускается на любой
   change в docs/**, но `docs/ADR/*.md` изменения (коммиты вроде
   `71a89c9 [S4-UI-P01] ui-adr-0031`) НЕ должны триггерить rebuild
   Pages — ADR'ы не входят в книгу. Уточнить paths:

       on:
         push:
           branches: [master]
           paths:
             - 'docs/src/**'      # mdbook source root
             - 'docs/book.toml'   # mdbook config
             - 'README.md'        # copied into docs/src/README.md
             - 'analytics/ARCHITECT3.md'  # copied into docs/src/index.md
             - '.github/workflows/pages.yml'

   (Убираем `'docs/**'` — слишком широко; `'mdbook.toml'` → `'docs/book.toml'`
    — build шаг пишет book.toml в docs/book.toml, поэтому триггер должен
    смотреть именно туда. `'book/**'` убираем — нет такого пути.)

5. НЕ трогать шаг "Build mdBook" — он уже использует `cd docs && mdbook
   build` и работает корректно, как только mdbook установлен.

Требования к коммиту. Сообщение:
`[GITHUB-P18] pages: replace peaceiris/actions-mdbook@v2.0.0 (Node 16 deprecated) with cargo install mdbook + tighten paths-filter`

Проверка.

1. Локально:
   - `cargo install mdbook --version 0.4.52 --locked` (один раз, ~1.5 мин) — exit 0.
   - `cd docs && mdbook build` — exit 0, создан `docs/book/` с index.html.
   - `git diff .github/workflows/pages.yml` — diff содержит замену action
     на cargo install + добавление rust-toolchain + rust-cache + уточнение
     paths-filter.

2. После push в master (на коммите, который меняет pages.yml — это
   paths-filter сам себя триггерит):
   - GitHub Pages #N — build job "Install mdBook" — exit 0, ~5 s при
     cache-hit, ~1.5 мин при cold cache.
   - Pages #N+1 (для следующего push, который трогает README.md или
     analytics/ARCHITECT3.md) — также зелёный.
   - Следующий push, который трогает только `docs/ADR/XXXX.md` (ADR
     без изменения README/ARCHITECT3) — НЕ запускает Pages (paths-filter
     skip), что и должно быть.

3. На https://reider85.github.io/strangecoin/ — обновлённый контент
   в течение ~3 мин после зелёного Pages run.

Не делай.
- НЕ заменять `peaceiris/actions-mdbook` на `actions/setup-mdbook`
  (несуществующий официальный action) — это выдуманный путь.
- НЕ bumping peaceiris/actions-mdbook до @v2.0.1 или @v2.0.2 — автор
  перестал поддерживать экшн (последний релиз Sep 2024, не мигрировал
  на Node 20); cargo install = надёжнее.
- НЕ ставить `continue-on-error: true` на шаг "Install mdBook" —
  отказ установки = отказ build = должны видеть красный, а не тишина.
- НЕ удалять шаг "Setup Pages" (`actions/configure-pages@v4`) — он
  нужен для корректной публикации в GitHub Pages environment.
```

---

## Промпт 4. [ВЫСОКО] prerelease.yml: сделать «skipped» видимым — добавить commit-comment на красный CI

```
Контекст. .github/workflows/prerelease.yml в create-dev-tag джобе имеет:

    if: ${{ github.event.workflow_run.conclusion == 'success' }}

Это правильный гейт (GITHUB-P12): не релизить на красном CI. Но побочный
эффект — пока CI красный, prerelease-ран помечается "skipped", и в списке
Actions это выглядит как тихое отсутствие активности. Мейнтейнер не видит
сигнала "dev-tag не создаётся, потому что CI красный" — он видит просто
"skipped" (то же самое, что и при `gh release view` idempotency-skip).

12+ пушей подряд с "skipped" prerelease остались незамеченными ~13 часов,
пока пользователь не спросил явно. Это плохо для feedback-loop'а.

Задача. Добавить в prerelease.yml второй джоб `notify-skipped`, который
срабатывает когда CI failed и пишет commit-comment на SHA-head с
понятным текстом "dev-tag NOT created because CI red — see CI run #N".

1. В .github/workflows/prerelease.yml после джоба `create-dev-tag`
   добавить:

       notify-skipped:
         name: Notify on skipped dev-tag (CI was red)
         runs-on: ubuntu-latest
         # [GITHUB-P19] the inverse of create-dev-tag's gate: this job
         # runs ONLY when CI failed, leaving a visible breadcrumb on the
         # CI head SHA — otherwise a red CI shows up only as "skipped"
         # in the Actions list, indistinguishable from the idempotency
         # skip path. Without this, the maintainer does not see that
         # dev-tag pipeline has stopped producing releases.
         if: ${{ github.event.workflow_run.conclusion != 'success' }}
         permissions:
           contents: read
           pull-requests: write   # commit-comments need this scope
         timeout-minutes: 3
         steps:
           - name: Post commit comment
             env:
               GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
               HEAD_SHA: ${{ github.event.workflow_run.head_sha }}
               CI_RUN_ID: ${{ github.event.workflow_run.id }}
               CI_CONCLUSION: ${{ github.event.workflow_run.conclusion }}
               REPO: ${{ github.repository }}
             shell: bash
             run: |
               set -euo pipefail
               body=$(cat <<EOF
               ⚠️ **Dev-tag pipeline skipped**: CI conclusion was \`${CI_CONCLUSION}\`
               (not \`success\`). The \`vX.Y.Z-dev.N\` pre-release was NOT created
               for this commit.

               - CI run: https://github.com/${REPO}/actions/runs/${CI_RUN_ID}
               - Once CI is green again, the next \`workflow_run\` event will
                 resume creating dev-tags automatically.

               Investigate the failing CI job above. Common causes:
               - dependabot major bump (secp256k1 0.29→0.33, toml 0.5→1.x —
                 see [GITHUB-P16] ignore rules);
               - cargo fmt drift (run \`cargo fmt --all\` locally);
               - clippy::correctness regression (run
                 \`cargo clippy --workspace --all-targets -- -D clippy::correctness\`).
               EOF
               )
               gh api --method POST \
                 -H "Accept: application/vnd.github+json" \
                 "/repos/${REPO}/commits/${HEAD_SHA}/comments" \
                 -f body="$body"

2. Не трогать существующий джоб `create-dev-tag` и его шаги — он уже
   корректно настроен после GITHUB-P12 (conclusion gate, head_sha
   checkout, rust-toolchain, rust-cache, idempotency check).

3. Concurrency group оставить как есть (`prerelease-${{ github.event.
   workflow_run.id || github.run_id }}`) — два джоба в одном workflow_run
   будут в одной группе. Поскольку `notify-skipped` имеет `if:
   conclusion != 'success'`, а `create-dev-tag` имеет `if: conclusion
   == 'success'`, они никогда не запустятся одновременно — conflict
   невозможен.

Требования к коммиту. Сообщение:
`[GITHUB-P19] prerelease: notify-skipped job — post commit-comment when CI red so dev-tag silence is visible`

Проверка.

1. Локально:
   - `actionlint .github/workflows/prerelease.yml` exit 0 (если
     actionlint установлен; иначе — `yq '.jobs | keys'
     .github/workflows/prerelease.yml` показывает оба джоба:
     `create-dev-tag`, `notify-skipped`).
   - `git diff .github/workflows/prerelease.yml` — добавлен ТОЛЬКО
     джоб notify-skipped; create-dev-tag джоб не изменён.

2. После push в master (на следующем красном CI):
   - workflow_run(CI completed, conclusion=failure) → prerelease.yml
     запускается.
   - create-dev-tag джоб — skipped (если смотрит условие).
   - notify-skipped джоб — runs, exit 0.
   - На странице `github.com/Reider85/strangecoin/commit/<SHA>` под
     коммитом появляется commit-comment со ссылкой на failed CI run
     и текстом "Dev-tag pipeline skipped".

3. После следующего зелёного CI:
   - workflow_run(CI completed, conclusion=success) → prerelease.yml.
   - create-dev-tag джоб — runs, создаёт `v3.0.0-dev.N+1`.
   - notify-skipped джоб — skipped (как и должен).

4. Побочный эффект: когда CI failed, мейнтейнер получает email от
   GitHub "Reider85/strangecoin: New commit comment" — это и есть
   сигнал "pipeline остановлен, разберись".

Не делай.
- НЕ менять существующий `if: conclusion == 'success'` в create-dev-tag
  джобе — это правильный gate; этот промпт добавляет ТОЛЬКО
  параллельный уведомляющий джоб, не ослабляет существующий.
- НЕ ставить `if: always()` на notify-skipped — он должен срабатывать
  ТОЛЬКО на failed/cancelled/neutral conclusion, не на success (там
  свой джоб делает comment через "Post commit comment" шаг).
- НЕ использовать `pull-requests: write` для create-dev-tag джоба —
  ему нужен только `contents: write` (уже есть); pull-requests-write
  нужен только для commit-comments на notify-skipped.
- НЕ дублировать логику создания dev-tag в notify-skipped — его задача
  только уведомить, не создать релиз.
```

---

## Промпт 5. [СРЕДНЕ] Аудит `actions/github-script@v9` (после bump 7→9) на новые deprecation'ы

```
Контекст. 16 Nov 2026 17:37 UTC dependabot смержил коммит 4da092a
"chore(deps): bump actions/github-script from 7 to 9" — major bump через
две версии (v7 → v8 → v9). Ранее коммит 53b204c [GITHUB-P15] ci: audit
github-script usage (steps/context.<unknown> bugs) закрывал известные
баги из CI3 Prompt 4 для v7. После bump 7→9 нужно повторить аудит:

  v8 changelog (https://github.com/actions/github-script/releases/tag/v8.0.0):
    - Updated to Node 20 (раньше Node 16);
    - Bundled @octokit/* packages обновлены до v6+;
    - Несовместимость: @octokit/webhooks types изменились — это влияет
      только на TypeScript-юзеров (на strangecoin github-script вызывается
      из inline JS в YAML, не из .ts файла — риск низкий, но проверить).

  v9 changelog (https://github.com/actions/github-script/releases/tag/v9.0.0):
    - Updated @octokit/core to v6+;
    - Несовместимость: `context.payload` теперь `unknown` вместо `any`
      в типах — для inline JS НЕ влияет (JS не типизирован);
    - Несовместимость: убран дефолтный `github.rest` (теперь требует
      `getOctokit(token)` явно, если нужен REST-клиент) — НО проверка
      текущего кода star-alert.yml показывает, что используется
      `fetch(url, ...)` (нативный Node fetch, не github.rest), поэтому
      этого breaking change НЕТ в кодовой базе strangecoin.

Текущее использование github-script в репо (rg 'actions/github-script' .github/):

  .github/workflows/star-alert.yml:71    uses: actions/github-script@v9

Это ЕДИНСТВЕННОЕ использование. Скрипт:
  - читает process.env.DISCORD_WEBHOOK_URL;
  - шлёт POST на Discord webhook через `fetch`;
  - НЕ использует `github.rest` / `context.payload` / `context.tag` /
    `steps.<id>.outputs` (баги, которые чинились в GITHUB-P15).

То есть на первый взгляд bump 7→9 безопасен. Но есть СКРЫТАЯ проблема:

  star-alert.yml env-блок:
    env:
      DISCORD_WEBHOOK_URL: ${{ secrets.DISCORD_WEBHOOK_URL }}
      COUNT: ${{ steps.snap.outputs.count }}
      DELTA24: ${{ steps.snap.outputs.delta24 }}
      MODE: ${{ github.event_name }}

  Внутри github-script@v9 `${{ steps.snap.outputs.count }}` interpolируется
  ДО запуска скрипта (GitHub Actions expression expansion на этапе
  парсинга YAML). Если `steps.snap.outputs.count` — пустая строка (что
  возможно, если шаг `snap` завершился с `exit 1` в одном из guard'ов
  `case "${stars}"`), то env-переменная COUNT станет пустой строкой, и
  в JS `process.env.COUNT || '?'` сработает корректно. Это OK.

  Но: star-alert.yml line 46-48:
    if: >-
      github.event_name == 'workflow_dispatch' ||
      github.event.workflow_run.conclusion == 'success'

  В v9 из-за перехода на Node 20 есть известная (см.
  https://github.com/actions/github-script/issues/460) проблема с тем,
  что inline-JS использует `core.notice()` / `core.setFailed()` —
  эти функции продолжают работать, но `core.debug()` теперь по
  дефолту не выводится в логи без `ACTIONS_STEP_DEBUG=true` repo
  variable. На strangecoin это не критично.

Задача. Провести повторный аудит github-script@v9:

1. Прогнать actionlint с правилом "no-deprecated-context":
       actionlint -ignore 'SC.*' .github/workflows/star-alert.yml

   Если actionlint не установлен: `cargo install actionlint` (через
   cargo-binstall быстрее — `cargo binstall actionlint`).

2. В .github/workflows/star-alert.yml проверить, что строка 71:
       uses: actions/github-script@v9
   уже актуальна (после bump 4da092a). Если по какой-то причине ещё
   v7 — bump до v9.

3. В шаге `Send Discord alert`:
   - проверить, что нет обращений к `context.tag` / `context.steps`
     / `context.payload.releases` — это все GITHUB-P15 закрытые баги,
     нужно убедиться, что не вернулись при bump;
   - проверить, что `core.notice()` / `core.setFailed()` используются
     корректно (есть оба вызова — star-alert.yml line 81 и 111).

4. Если в будущем появится ещё один github-script usage —
   СРАЗУ проводить аудит по чек-листу GITHUB-P15 (CI3):
   a. Никаких `steps.<id>.outputs` — это не в контексте github-script.
   b. Никаких `context.tag` / `context.steps` / `context.release` —
      нет таких свойств.
   c. Если нужен REST-вызов — `const octokit = getOctokit(process.env.
      GH_TOKEN); await octokit.rest.repos.createRelease({...})`.
   d. Для чтения outputs предыдущих шагей — env-vars с `${{ steps.X.
      outputs.Y }}` interpolированными ВНЕ скрипта.

5. Документировать в `analytics/GITHUB.md` секцией "§11 github-script@v9
   audit post-bump" — что проверено, что OK, что требует внимания при
   будущих bump'ах.

Требования к коммиту. Если правок не требуется (только документация):
`[GITHUB-P20] docs: github-script@v9 audit (no code changes — fetch+env+core.notice pattern is safe across v7→v8→v9)`

Если правки нужны:
`[GITHUB-P20] ci: github-script@v9 fixes (указать что именно)`

Проверка.

1. Локально:
   - `actionlint .github/workflows/*.yml` exit 0 (или только
     non-blocking warnings для известных вещей вроде `vars.RUNS_ON &&
     fromJSON(...)` — это documented GitHub Actions limitation).
   - `git diff` показывает либо пустой diff (только документация), либо
     минимальные правки в star-alert.yml.

2. После push в master:
   - Star alert workflow (если сработал триггер workflow_run на
     star-monitor completion) — exit 0, Discord webhook (если secret
     задан) отправлен успешно.
   - Либо `core.notice('DISCORD_WEBHOOK_URL secret is not set...')` —
     тоже OK (exit 0, graceful skip).

Не делай.
- НЕ "профилактически" переписывать github-script на `actions/github-
  request-action` или `curl` — текущий inline JS проще и работает.
- НЕ добавлять `continue-on-error: true` на star-alert шаг — он
  уже graceful (return при отсутствующем webhook).
- НЕ bumping github-script с v9 на будущий v10 без явной причины —
  это повторит ту же audit-задачу.
- НЕ дублировать чек-лист GITHUB-P15 — ссылаться на него в новой
  секции GITHUB-P20.
```

---

## Промпт 6. [СРЕДНЕ] eframe 0.22 → 0.27+ (или 0.30+): настоящий фикс objc2 macOS-сборки вместо pin 1.98.1

```
Контекст. .github/workflows/release.yml после коммита ba39472 [S3-P15]
ci: pin release toolchain 1.98.1 + cross g++ (macos/linux-aarch64)
пинирует Rust toolchain в release-матрице к 1.98.1:

    - uses: dtolnay/rust-toolchain@stable
      with:
        toolchain: ${{ vars.RUNS_ON && 'stable-x86_64-pc-windows-gnu' || '1.98.1' }}

Обоснование из самого коммита:
> rustc 1.99 stable (Nov 2026) broke objc2 0.3.0-beta.3 (E0277 in
> ivar_forwarding_impls.rs; winit 0.28/glutin/accesskit macos chain)
> -> both macos cells of run 43922667672 died in <1min.
> 1.98.1 = known-good combo; pin = reproducible builds; bump
> deliberately when the macos GUI stack is upgraded.

Тот же workaround уже применён в nightly.yml (deviation 10c — pin
macOS cells to toolchain 1.99.1). Но pin к старой Rust — это
работающее-пока-не-сломается решение: через 6-12 месяцев stable
runner-image может удалить 1.98.1 из `rustup toolchain list`, и
release.yml начнёт падать с "toolchain 1.98.1 not found".

Настоящий фикс — миграция eframe с 0.22 (Sep 2023) на 0.27+ (Apr 2025)
или 0.30+ (последняя стабильная). Это потянет за собой:
  - winit 0.28 → 0.30 ( breaking: `Event::WindowEvent` signature changed);
  - glutin 0.30 → 0.32 ( breaking: surface creation API);
  - accesskit 0.11 → 0.17;
  - objc2 0.3.0-beta.3 → 0.3.0-beta.5 (или 0.3.1 stable после релиза).

Cargo.toml сегодня:
    eframe = { version = "0.22", optional = true }
    # eframe 0.22 uses winapi without declaring features; winit 0.28 (its
    # transitive dep) moved to windows-sys, so nothing in the graph enables
    # winapi features and eframe fails to compile with "no `winuser` in
    # `um`". Declare them explicitly.
    winapi = { version = "0.3", optional = true, features = [...] }

eframe 0.27+ переходит на `windows-sys` напрямую (см.
https://github.com/emilk/egui/blob/0.27.0/CHANGELOG.md#0270---2024-04-13),
поэтому winapi-обход можно удалить.

Задача. Мигрировать eframe с 0.22 на 0.30+ (это Stage 4 задача — UI-P02
в analytics/BACKLOG.md, помечена "design-only", но blocker macOS-сборки
ускоряет её).

Поскольку это полноценная миграция GUI-кода (eframe 0.22 → 0.30 имеет
~8 breaking releases между ними), задача разделяется на 3 подшага:

  Шаг A. Оценка объёма миграции.
    1. cargo tree -p eframe --depth 2 — получить текущий sub-tree.
    2. Запустить `cargo upgrade -p eframe --to 0.30` (cargo-edit) —
       только в Cargo.toml, без lockfile.
    3. cargo check -p strangecoin --features gui --no-default-features
       --features net,vm-wasm,legacy-migrate,gui — собрать список
       ошибок компиляции.
    4. Для каждой ошибки — классифицировать: «API rename» (1-строка
       fix), «trait bound changed» (требует rewriting call site),
       «feature removed» (требует alternative crate).

  Шаг B. Собственно миграция (по найденным breaking changes):
    1. Обновить Cargo.toml: `eframe = { version = "0.30", optional = true }`.
    2. Удалить `winapi = { version = "0.3", optional = true, features = [...] }`
       блок целиком (eframe 0.27+ не зависит от winapi).
    3. Обновить src/gui/*.rs (если есть) под новый API:
       - `eframe::egui::Context::request_repaint_after(secs)` →
         `ctx.request_repaint_after(secs.into())`;
       - `NativeOptions::default()` → `NativeOptions { viewport:
         ViewportBuilder::default().with_inner_size(...), ..Default::
         default() }`;
       - `egui::FontDefinitions::default()` → `ctx.style_mut()` и т.д.
    4. Прогнать `cargo check -p strangecoin --features gui` — exit 0.

  Шаг C. Снять workaround:
    1. В .github/workflows/release.yml вернуть toolchain на `stable`
       (убрать pin `1.98.1`).
    2. В .github/workflows/nightly.yml убрать matrix.toolchain=1.99.1
       на macOS cells (deviation 10c больше не нужна).
    3. В AGENTS.md 'Stage 4 Pin Policy' добавить eframe 0.30 и
       fluent (=x.y.z) — task уже помечен UI-P01 в analytics/BACKLOG.md.

Этот промпт — это НЕ одиночный коммит, а СПРИНТ (UI-P02 в Roadmap).
Оформить как серию коммитов `[UI-P02-A]`, `[UI-P02-B]`, `[UI-P02-C]`,
каждый из которых проходит CI зелёным.

Требования к коммиту. Минимум три коммита:
- `[UI-P02-A] deps: bump eframe 0.22 → 0.30 + remove winapi workaround (cargo check passes on ubuntu-latest)`
- `[UI-P02-B] gui: migrate eframe 0.22 → 0.30 API (NativeOptions, ViewportBuilder, font API)`
- `[UI-P02-C] ci: unpin release/nightly toolchain 1.98.1/1.99.1 — objc2 now stable on rustc 1.100+`

Проверка.

1. После [UI-P02-A]:
   - `cargo check -p strangecoin --features gui --no-default-features
     --features net,vm-wasm,legacy-migrate,gui` exit 0.
   - Удалён блок `winapi = { ... }` из Cargo.toml.
   - `cargo tree -p eframe | head -5` показывает eframe 0.30+.

2. После [UI-P02-B]:
   - `cargo build --release -p strangecoin --features gui` exit 0.
   - Бинарник `target/release/strangecoin` собирается и запускается
     на Linux (smoke test: `./strangecoin --help` exit 0).
   - На macOS (если есть доступ к macos runner или локальному Mac):
     `cargo build -p strangecoin --features gui --target aarch64-apple-darwin`
     exit 0 (objc2 больше не ломается на rustc 1.100+).

3. После [UI-P02-C]:
   - release.yml: toolchain: ${{ vars.RUNS_ON && '...' || 'stable' }}
     (вернулись к stable, без pin).
   - nightly.yml: matrix.toolchain удалён из macOS cells.
   - На следующем v*-tag push release.yml собирает все 6 платформ зелёным.
   - На ближайшем nightly.yml запуске cross-build обе macOS cells —
     зелёные без пина к 1.99.1.

Не делай.
- НЕ "попробовать" eframe 0.34 (dependabot уже предлагал PR #7
  "Bump eframe from 0.22.0 to 0.34.3" 9 Nov 2026, CI #150 failed —
  слишком большой скачок; делай 0.30 как промежуточный).
- НЕ оставлять winapi workaround "на всякий случай" — eframe 0.27+
  от него полностью избавился.
- НЕ снимать pin `1.98.1` ДО завершения миграции (Шаг C после B).
- НЕ удалять `eframe = { ..., optional = true }` — feature-gate
  остаётся, gui не входит в default-feature set для headless-сборок
  (spec GITHUB-P2 deviation 5).
```

---

## Промпт 7. [НИЗКО] release.yml: выровнять версии `actions/upload-artifact` / `actions/download-artifact` + убрать deprecated `merge-multiple`

```
Контекст. В .github/workflows/release.yml после серии dependabot bumps
(ce55021 "actions/upload-artifact 4→7" Nov 9, c34d096 "actions/download-
artifact 4→8" Nov 10) сложился version-skew:

    actions/upload-artifact@v7   (release.yml lines 138, 149, 170, 259)
    actions/download-artifact@v8 (release.yml lines 185, 232, 272)

В actions/download-artifact@v4 (release notes см.
https://github.com/actions/download-artifact/releases/tag/v4.0.0) была
удалена опция `merge-multiple: true` (раньше позволяла слить несколько
artifacts в одну директорию; v4+ клонит каждый artifact в отдельную
subdirectory с тем же именем, что и artifact).

В release.yml aggregate-hashes джоб использует:
    - name: Download all checksum files
      uses: actions/download-artifact@v8
      with:
        pattern: "*-checksum"
        path: checksums
        # merge-multiple would flatten all artifacts into one directory and
        # overwrite identically-named hash.txt files (only 1 of 6 would survive).

Комментарий правильный (merge-multiple НЕ используется), но в действиях
`sign` и `release` джобах — `actions/download-artifact@v8` без
`pattern`/`merge-multiple`, скачивает ВСЕ artifacts в `artifacts/`:
    - name: Download all artifacts
      uses: actions/download-artifact@v8
      with:
        path: artifacts

В v4+ это создаёт `artifacts/strangecoin-linux-x86_64/`,
`artifacts/strangecoin-macos-x86_64/`, и т.д. — каждый artifact в своей
поддиректории. Это корректно работает с `find artifacts -type f -exec cp
{} release/ \;` в шаге "Collect release assets".

Проблем нет, но есть мелкая косметика: version-skew между upload (v7) и
download (v8). В actions/* экосистеме рекомендуют держать major-версии
согласованными (v4 + v4 или v5 + v5), потому что v4+ ввёл новое
поведение artifacts (immutable, именованные, 90-day retention), и
смешивание с v3 дало бы конфликт. v7 upload + v8 download — оба уже на
новом ядре, конфликтов НЕТ, но экшн-команда GitHub рекомендует держать
их в одном major-версии.

Дополнительно: `actions/upload-pages-artifact@v5` (pages.yml line 115)
был bump из v3 (commit 8531a00 "actions/upload-pages-artifact 3→5") —
v4 deprecated `retention-days` (но мы его не задаём), v5 добавила
поддержку `compression-level`. Тоже ОК.

Задача. Привести версии экшнов в release.yml к одной major-линейке:

1. В .github/workflows/release.yml выровнять:
   - `actions/upload-artifact@v7` → `actions/upload-artifact@v4` (или
     дождаться v8 upload-artifact и поднять download до v8 — но это
     будущее; сейчас upload v4 — последняя stable).
   - `actions/download-artifact@v8` → `actions/download-artifact@v4`.

   Обоснование rollback: v4 upload/download — это current long-stable
   release на actions/github-artifacts репо. v5+ upload был отменён
   (см. https://github.com/actions/upload-artifact/issues/499 — v5
   ревертнулись в v4 из-за breaking changes в handling). Если в этой
   future-timeline v5+ действительно опубликован и работает — оставь
   v7 upload + v7 download (или v8 upload + v8 download). Главное —
   major-линейка одинаковая.

2. Если в этой future-timeline v7 upload / v8 download действительно
   работают корректно (зависит от официальной поддержки) — оставить
   как есть, но ДОБАВИТЬ комментарий в release.yml header:
   ```
   # NOTE: upload@v7 + download@v8 — version skew, but both use the
   # v4+ artifact core (named, immutable, 90-day retention). v7 upload
   # is future-stable; v8 download is future-stable. If a v7 upload
   # breaks, rollback both to v4.
   ```

3. Если rollback до v4 — обновить `.github/dependabot.yml` ignore-список
   (из Промпта 1):
   ```
   - dependency-name: "actions/upload-artifact"
     update-types: ["version-update:semver-major"]
   - dependency-name: "actions/download-artifact"
     update-types: ["version-update:semver-major"]
   ```
   (Это предотвратит будущие blind-merge major bumps, которые ломают
   release.yml)

Требования к коммиту. Сообщение:
`[GITHUB-P21] release: pin actions/upload-artifact + download-artifact to v4 (long-stable; dependabot major-ignore)`

Или (если остаёмся на v7/v8):
`[GITHUB-P21] release: document upload@v7/download@v8 skew + add dependabot major-ignore to prevent breaking bumps`

Проверка.

1. Локально:
   - `actionlint .github/workflows/release.yml` exit 0.
   - `git diff .github/workflows/release.yml` — только изменения версий
     экшнов (и добавление комментария, если выбран второй вариант).

2. После push (через dependabot, который НЕ должен больше открывать PR
   на major bump этих двух):
   - На ближайшем v*-tag push (или workflow_dispatch) release.yml
     запускается зелёным, все 6 матриц собираются, aggregate-hashes
     видит 6 (или 5+ fallback) hash.txt, sign шлёт cosign подписи,
     release создаёт GitHub Release.

3. На странице Insights → Dependency graph → Dependabot — у
   `actions/upload-artifact` и `actions/download-artifact` плашка
   "Ignored: version-update:semver-major".

Не делай.
- НЕ rollback до v3 actions/upload-artifact — v3 deprecated,
  используете устаревший Node 12/16 runner.
- НЕ дублировать ignore-правила в dependabot.yml — они уже добавлены
  в Промпте 1 для crypto-крейтов; добавь только actions/upload-artifact
  и actions/download-artifact если решено их pin к v4.
- НЕ удалять `pattern: "*-checksum"` в aggregate-hashes шаге — это
  правильная защита от скачивания ВСЕХ artifacts (включая cosign-
  signatures), что переполнило бы path.
```

---

## Итоговый чек-лист применения CI4

```
После применения всех 7 промптов по порядку:

□ Промпт 1 (GITHUB-P16): dependabot.yml — ignore crypto minor + grouped
  bumps; AGENTS.md pin policy updated. CI #N (после 4c2101f) зелёный,
  создаётся v3.0.0-dev.3.

□ Промпт 2 (GITHUB-P17): branch protection на master — required status
  checks (Lint, Test-core, Source-gates) + merge queue. Следующий
  dependabot PR НЕ может быть смержен без зелёного CI.

□ Промпт 3 (GITHUB-P18): pages.yml — peaceiris/actions-mdbook заменён на
  cargo install mdbook. Pages #N+1 зелёный на коммите, который меняет
  README.md / analytics/ARCHITECT3.md / .github/workflows/pages.yml.

□ Промпт 4 (GITHUB-P19): prerelease.yml — notify-skipped джоб. На
  следующем красном CI под коммитом появляется commit-comment со ссылкой
  на failed CI run + текстом "Dev-tag pipeline skipped".

□ Промпт 5 (GITHUB-P20): star-alert.yml — аудит github-script@v9
  (без правок или с минимальными), секция в GITHUB.md.

□ Промпт 6 (UI-P02): eframe 0.22 → 0.30, снят pin toolchain 1.98.1 в
  release.yml и 1.99.1 в nightly.yml. macOS cells в release/nightly
  собираются на stable Rust 1.100+.

□ Промпт 7 (GITHUB-P21): release.yml — upload/download-artifact
  major-линейка выровнена + dependabot major-ignore.

После всех 7 промптов:
- pipeline «push → CI green → dev-tag → GitHub Pre-release» работает
  детерминированно: каждый зелёный пуш = один новый dev-тег;
- dependabot НЕ может протащить breaking major bump в master вслепую;
- Pages собирается зелёным на триггерных коммитах;
- notify-skipped делает тишину «pipeline остановлен» видимой за
  минуты, а не часы;
- macOS-сборка release.yml не зависит от pin старой Rust 1.98.1.
```
