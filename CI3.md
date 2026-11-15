# Промпты для исправления релизного пайплайна в Reider85/strangecoin (CI3)

Готовые промпты для поочерёдной передачи агенту-помощнику (Claude / GPT / Copilot /
Cursor) или разработчику. Каждый промпт самодостаточен: контекст, точная причина,
требуемое решение, формат коммита, способ проверки. Применять строго в указанном
порядке — Промпт 1 + 2 критичны, без них pipeline «push → dev-тег + GitHub
Prerelease» не работает ни разу, Промпты 3-4 доводят остальные красные workflow
до зелёного и убирают рекурсивный пет-бэк-коммит.

Репозиторий: https://github.com/Reider85/strangecoin
Ветка по умолчанию: `master`. Версия в `Cargo.toml`: `2.0.0`.
Промпты нумеруются как `[GITHUB-PNN]`, продолжая счётчик из CI2 (P07-P10 уже в
истории).

---

## Почему нет релиза и тега на каждом push в master (диагноз)

Ветка master сейчас настроена так, что каждый push должен проходить цепочку:

```
push → ci.yml (lint + test-core + coverage + source-gates)
     → workflow_run(completed) → prerelease.yml
                                → checkout
                                → compute vX.Y.Z-dev.N tag
                                → gh release view (idempotency)
                                → cargo build --release
                                → gh release create --prerelease
                                → Ping Discord (commit comment)
                                → Update stargazer history (push в master!)
```

На сегодняшний день эта цепочка сломана на 6 уровнях. Не релизится ни разу:

1. **CI на master КРАСНЫЙ** для всех коммитов после `[GITHUB-P08]` (CI run #171,
   Nov 14 2026 — последний зелёный). Дальше CI #172-177 все failed. Локально
   воспроизводится одной командой:
   ```
   $ cargo fmt --all -- --check
   Diff in crates/strangecoin-core/tests/witness.rs:487
   - "01",                    // WITNESS_FORMAT_VERSION
   + "01",                                      // WITNESS_FORMAT_VERSION
   ...
   ```
   Коммит `[S3-P11] stateless: witness wire codec + light client e2e (SMT-based)`
   добавил в `crates/strangecoin-core/tests/witness.rs` golden-vector
   `concat!(...)` с ручной раскладкой комментариев, которая не соответствует
   выводу `rustfmt`. CI lint шаг `cargo fmt --all -- --check` падает на
   ненулевом exit code → весь workflow `CI` помечается failed. Пока CI красный,
   любой последующий шаг (включая prerelease) либо вообще не запускается, либо
   запускается на нерабочем коде.

2. **`prerelease.yml` не отсекает красный CI**. Триггер
   ```
   on:
     workflow_run:
       workflows: ["CI"]
       types: [completed]
       branches: [master]
   ```
   срабатывает на **любое** завершение CI, включая `failure`. На job нет
   `if: ${{ github.event.workflow_run.conclusion == 'success' }}`. В итоге:
   push → CI красный → workflow_run(completed) срабатывает → prerelease всё
   равно бежит → пытается собрать релиз на сломанном коде → падает на одном из
   шагов. Это объясняет серию из 13 failed prerelease runs (#11-#14, все Nov 14)
   длиной по 11-15 минут — это не таймаут, это падение `cargo build` или
   следующего шага.

3. **Шаг `Ping Discord` 100% падает** на `ReferenceError: steps is not defined`.
   Действие `actions/github-script@v7` НЕ экспонирует контекст `steps` —
   доступны только `github`, `context`, `core`, `glob`, `io`, `exec`, `require`,
   `getOctokit` (см. https://github.com/actions/github-script).
   Скрипт шага:
   ```js
   if (steps.check-release.outputs.skip == 'true') { ... }
   ```
   выбрасывает ReferenceError. Шаг имеет `if: always()` → запускается ВСЕГДА
   → каждый workflow гарантированно помечается failed. Даже если dev-тег и
   GitHub Release УЖЕ создались на предыдущем шаге, общий статус workflow —
   `failure`, и в списке Actions это выглядит как «релиз не вышел».

4. **`context.tag` не существует**. В `actions/github-script` у объекта
   `context` нет свойства `tag` (есть `sha`, `ref`, `repo`, `actor`,
   `event_name`, `payload`, `run_id`, `workflow`, `serverUrl` и т.д.). Строка
   ```js
   const { tag } = context;
   ```
   делает `tag = undefined`. Сообщение в commit-comment выглядит как
   «🚀 New dev build created: undefined» — шаг не падает на этом (undefined
   конкатенируется как строка), но пользы ноль.

5. **Шаг `Update stargazer history` пушит в master** — это рекурсивный цикл:
   ```
   push в master → CI → workflow_run(completed) → prerelease
   → curl api.github.com/repos/... → stargazers_count → sed -i README.md
   → git commit → git push в master → CI → workflow_run → prerelease → ...
   ```
   В истории master видно, как плодятся коммиты `Update stargazer count to N`
   и `chore(stars): hourly snapshot N` (по 5+ в день от одного апстрим-пуша).
   Каждый такой коммит заново триггерит CI и prerelease. Чтобы pipeline
   «один push → один dev-тег» работал детерминированно, stargazer-обновление
   надо вынести из prerelease.yml и из release.yml в отдельный cron-job
   (hourly или daily), который не пишет в master в принципе (обновляет
   отдельный артефакт в gh-pages / dedicated branch / Discussions topic).

6. **`actions/checkout@v4` без `ref: github.event.workflow_run.head_sha`**.
   Для `workflow_run` триггера `actions/checkout` по умолчанию берёт текущий HEAD
   мастер-ветки на момент запуска workflow, а НЕ тот SHA, который запустил CI.
   Если между началом CI и стартом prerelease в master успели запушить ещё
   коммит, prerelease затэгает НЕ тот коммит, который пользователь пушнул. Это
   проявляется не каждый раз, но в активной фазе разработки ломает
   детерминированность «один push → один тег → один SHA». Дополнительно
   `gh release create --target master` указывает на master, а не на конкретный
   SHA — если между CI и prerelease пришёл новый коммит, release привяжется к
   новому HEAD. Правильный паттерн:
   ```yaml
   - uses: actions/checkout@v4
     with:
       ref: ${{ github.event.workflow_run.head_sha }}
       persist-credentials: true
   ```
   и в `gh release create`:
   ```
   --target ${{ github.event.workflow_run.head_sha }}
   ```

7. **Нет `dtolnay/rust-toolchain@stable` и нет `Swatinem/rust-cache@v2`**.
   prerelease.yml использует системный Rust из runner-image (ubuntu-latest
   ≈ rust 1.100 через rustup). Это работает, но:
   - Версия не зафиксирована → при обновлении runner-image может несинхронно
     меняться с `ci.yml` / `release.yml` / `nightly.yml`, где toolchain
     фиксирован через `dtolnay/rust-toolchain@stable`.
   - Нет кеша → каждый prerelease собирает `cargo build --release` (LTO=true,
     codegen-units=1) с нуля. Полный workspace (rocksdb, wasmtime, secp256k1,
     eframe, tokio) в release-профиле занимает 10-15 мин — именно такие
     рантаймы видны у failed prerelease runs. С `Swatinem/rust-cache@v2`
     cache-hit даёт 2-3 мин.

8. **`gh release create --notes "..." --generate-notes` НЕ конфликтует**.
   Проверено через локальный `gh release create --help`:
   > Additional release notes can be prepended to automatically generated
   > notes by using the `--notes` flag.

   Этот флаг легален, не причина падения (гипотеза из CI2 закрыта). Однако
   `--target master` лучше заменить на `--target ${{ github.event.workflow_run.head_sha }}`
   (см. пункт 6), а `gh` рекомендовано передать `env: GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`
   явно — на `workflow_run` триггере `GITHUB_TOKEN` инжектируется, но `gh`
   ищет сначала `GH_TOKEN`; явное `GH_TOKEN` надёжнее на всех версиях CLI.

### Сводка багов по приоритету

| # | Баг | Workflow | Промпт | Критичность |
|---|-----|----------|--------|-------------|
| 1 | `cargo fmt` drift в `tests/witness.rs` ломает CI | ci.yml | Промпт 1 | критично |
| 2 | Нет `if: conclusion == 'success'` на job | prerelease.yml | Промпт 2 | критично |
| 3 | `steps` undefined в github-script → workflow всегда failed | prerelease.yml | Промпт 2 | критично |
| 4 | `context.tag` undefined в commit-comment | prerelease.yml | Промпт 2 | высоко |
| 5 | `Update stargazer history` пушит в master (рекурсия) | prerelease.yml + release.yml | Промпт 3 | высоко |
| 6 | `checkout` без `head_sha` + `--target master` (race) | prerelease.yml | Промпт 2 | высоко |
| 7 | Нет `rust-toolchain` + нет `rust-cache` | prerelease.yml | Промпт 2 | средне |
| 8 | Нет явного `GH_TOKEN` для `gh` | prerelease.yml | Промпт 2 | низко |

---

## Промпт 1. [КРИТИЧНО] Привести CI к зелёному: починить `cargo fmt` drift в `crates/strangecoin-core/tests/witness.rs`

```
Контекст. В репозитории Reider85/strangecoin на master CI красный с run #172
(коммит "[S3-P11] stateless: witness wire codec + light client e2e (SMT-based)",
Nov 14 2026) и до текущего HEAD (commit 427dc52 "[GITHUB-P10] release: graceful
partial-matrix fallback", CI run #177 failed). Локально воспроизводится:
  cd <repo-root>
  cargo fmt --all -- --check
выводит Diff в crates/strangecoin-core/tests/witness.rs:487 — ручная раскладка
комментариев в golden-vector concat!(...) не соответствует выводу rustfmt.
CI lint шаг `cargo fmt --all -- --check` падает на ненулевом exit code → весь
workflow `CI` failed → workflow_run(completed) триггерит prerelease.yml на
сломанном коде → dev-тег не мечатся.

Точная причина. Коммит S3-P11 добавил функцию `witness_codec_golden_vector`
в crates/strangecoin-core/tests/witness.rs. Внутри неё — `let expected = concat!(...)`
с многострочным hex-представлением witness-байтов и комментарии, выровненные
вручную:
    "01",                                             // WITNESS_FORMAT_VERSION
    "77777777...",                                    // pre_state_root
    "00000002",                                       // proof count
    "00000005", "616c696365",                         // "alice"
    ...
Автор выровнял `//` комментария по самой длинной строке, но rustfmt в
текущей версии (stable 1.100) форматирует `concat!(...)` иначе — кладёт
каждый строковый литерал на свою строку и применяет свой алгоритм
выравнивания. Расхождение и есть drift.

Задача. Привести `crates/strangecoin-core/tests/witness.rs` к выводу rustfmt:

1. Запустить `cargo fmt --all` — это перепишет только раскладку `concat!(...)`
   в `witness_codec_golden_vector` (и любые другие мелкие drift'ы в файле).
   Перед коммитом посмотреть diff:
     git diff crates/strangecoin-core/tests/witness.rs
   и убедиться, что изменения только cosmetic (переставлены комментарии и
   строки), а содержимое `concat!` (строковые литералы hex-байтов) не
   изменено. Если `cargo fmt --all` меняет что-то ещё по репо — тоже
   закоммитить (это и есть смысл шага «repo-wide cargo fmt --all»).

2. Прогнать локально:
     cargo fmt --all -- --check
   должно выйти без diff и без ненулевого exit code.

3. Прогнать локально:
     cargo clippy -p strangecoin-core --all-targets -- -A warnings -D clippy::correctness
   должно выйти без ошибок correctness (warning unused dependency `base64`
   допустим — он обрабатывается в отдельном промпте).

4. Прогнать тест:
     cargo test -p strangecoin-core --test witness
   — `witness_codec_golden_vector` должен пройти: hex-строки в `concat!(...)`
   неизменны, меняется только whitespace/comments, decoded bytes те же.

5. НЕ переписывать сам тест — НЕ менять ожидаемый hex-вектор, НЕ добавлять
   `#[rustfmt::skip]` (это маскирует drift, а не фиксит его). Если `cargo fmt`
   почему-то хочет переписать содержимое строковых литералов (что не должно
   случиться — `concat!` принимает только строки как есть) — остановиться и
   сообщить, не коммитить.

Требования к коммиту. Сообщение в формате repo-convention:
`[GITHUB-P11] style: cargo fmt drift from S3-P11 (witness golden-vector layout)`

Проверка.
1. Локально: `cargo fmt --all -- --check` exit 0.
2. После push в master:
   - CI lint job должен пройти (зелёный).
   - Если это единственная проблема — весь workflow `CI` должен стать
     success (test-core, coverage, source-gates уже зелёные по той же
     причине — `cargo fmt --all` затронет только раскладку комментариев в
     witness.rs, не функциональность).
3. После зелёного CI prerelease.yml автоматически стартует в рамках
   workflow_run-цепочки — проверить, что он доходит до шага `Build artifacts`
   без падения на checkout/compute-tag.

Не делай. Не вноси правки в `prerelease.yml` в этом промпте — это отдельный
Промпт 2. Не добавляй `#[rustfmt::skip]` — маскировка drift'а не решает
проблему. Не переписывай hex-вектор руками — пусть `cargo fmt --all` сам
выровняет.
```

---

## Промпт 2. [КРИТИЧНО] Переписать `prerelease.yml` — 7 багов, единственным коммитом

```
Контекст. .github/workflows/prerelease.yml в Reider85/strangecoin должен на
каждый зелёный push в master создавать тег vX.Y.Z-dev.N и GitHub Prerelease с
Linux x86_64 артефактом. Сейчас workflow падает в 100% запусков (#11-#14
failed, #15 in_progress, все Nov 14 2026). В git ls-remote --tags origin нет
ни одного тега вида v2.0.0-dev.* — то есть релиз не создался ни разу после
ввода workflow (commit 699cd65 "[GITHUB-P07] prerelease: fix dev-tag push
via gh CLI + idempotency + concurrency group").

В текущем файле 7 багов, каждый из них ломает workflow independently:

  Баг 1. Нет gate'а на conclusion == 'success'.
    on.workflow_run.types: [completed] срабатывает на любой исход CI,
    включая failure. На job нет `if: ${{ github.event.workflow_run.conclusion
    == 'success' }}`. Когда CI красный (текущая ситуация), prerelease всё
    равно стартует, тратит 14 мин на `cargo build --release`, и падает.

  Баг 2. `actions/checkout@v4` без `ref: github.event.workflow_run.head_sha`.
    Для workflow_run-триггера checkout по умолчанию берёт текущий HEAD master,
    а НЕ SHA, который запустил CI. В активной фазе разработки между CI и
    workflow_run успевает приземлиться новый коммит → prerelease тэгает не
    тот SHA. Дополнительно `gh release create --target master` привязывает
    release к текущему HEAD master, а не к проверенному CI-коммиту.

  Баг 3. Нет `dtolnay/rust-toolchain@stable` — prerelease использует системный
    Rust из runner-image (≈ 1.100). Работает, но версия не зафиксирована и не
    синхронизирована с ci.yml / release.yml / nightly.yml.

  Баг 4. Нет `Swatinem/rust-cache@v2` — каждый prerelease собирает
    `cargo build --release` (LTO=true, codegen-units=1) с нуля, 10-15 минут на
    полный workspace (rocksdb, wasmtime, secp256k1, eframe, tokio).

  Баг 5. Шаг `Ping Discord` использует `steps.check-release.outputs.skip`
    внутри `actions/github-script@v7`. Контекст `steps` НЕ экспонируется
    github-script (см. https://github.com/actions/github-script — доступны
    только `github`, `context`, `core`, `glob`, `io`, `exec`, `require`,
    `getOctokit`). Скрипт выбрасывает
    `ReferenceError: steps is not defined`, шаг с `if: always()` падает →
    workflow помечается failed, даже если dev-тег и GitHub Release уже
    создались на предыдущем шаге.

  Баг 6. `const { tag } = context;` — у `context` в github-script нет свойства
    `tag`. Переменная `tag` = undefined. Сообщение в commit-comment
    выглядит как "🚀 New dev build created: undefined".

  Баг 7. Шаг `Update stargazer history` делает `git commit -m "..."` + `git push`
    в master. Это создаёт рекурсивный цикл: каждый prerelease-push в master
    триггерит новый CI → новый prerelease → новый stargazer-push. В истории
    master видно, как от одного апстрим-коммита плодятся "Update stargazer
    count to N" и "chore(stars): hourly snapshot N" по 5+ в день.

Задача. Полностью переписать .github/workflows/prerelease.yml единственным
коммитом. Целевое поведение:

  - Триггер: `workflow_run` на CI completed + success на master. НЕ
    срабатывать на failure/cancelled.
  - Job-level gate: `if: ${{ github.event.workflow_run.conclusion == 'success' }}`.
  - Checkout: `ref: ${{ github.event.workflow_run.head_sha }}`,
    `persist-credentials: true` (для gh CLI).
  - Установить Rust stable через `dtolnay/rust-toolchain@stable` (как в
    ci.yml / release.yml).
  - Включить `Swatinem/rust-cache@v2` с `sharedKey: prerelease-linux-x86_64`.
  - Идемпотентность: если тег уже существует (`gh release view` exit 0),
    завершать workflow success без сборки.
  - Сборка: `cargo build --release` (только Linux x86_64 — Release matrix
    делает всё остальное на v*-тегах).
  - Создание релиза: `gh release create "$TAG" --target ${{ github.event.workflow_run.head_sha }} --prerelease --title "$TAG" --notes "Automated dev build, not for production" --generate-notes strangecoin-linux-x86_64 strangecoin-linux-x86_64.sha256`,
    с `env: GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`.
  - Уведомление: вместо github-script с `steps`/`context.tag` — shell-шаг,
    который читает `${{ steps.compute-tag.outputs.tag }}` через `env:` и
    делает `gh api` POST на commit-comment endpoint (REST: POST
    /repos/{owner}/{repo}/commits/{sha}/comments). Не использовать
    `actions/github-script@v7` — там нет `steps`.
  - Concurrency: group по `github.event.workflow_run.id`,
    `cancel-in-progress: false` (уже есть — оставить).
  - УДАЛИТЬ шаг `Update stargazer history` из prerelease.yml полностью.
    Stargazer-snapshot переезжает в отдельный cron-job (Промпт 3).

Целевой YAML (заменить содержимое файла целиком):

```yaml
name: Pre-release

# [GITHUB-P12] prerelease: full rewrite — 7 bugfixes (conclusion gate, head_sha
# checkout, rust-toolchain, rust-cache, github-script steps undefined,
# context.tag undefined, stargazer push-loop removed).
#
# Triggers only on GREEN CI for master. Tags vX.Y.Z-dev.N on the exact CI
# commit (workflow_run.head_sha) — no race with later pushes. Linux x86_64
# binary only; full 6-platform matrix is release.yml on v*-tags.
#
# Stargazer-history snapshot moved out to .github/workflows/stargazer.yml
# (hourly cron, no commit-back to master) — was creating a recursive loop:
# push → CI → prerelease → stargazer-push to master → CI → prerelease → ...

on:
  workflow_run:
    workflows: ["CI"]
    types: [completed]
    branches: [master]

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always

concurrency:
  group: prerelease-${{ github.event.workflow_run.id || github.run_id }}
  cancel-in-progress: false

jobs:
  create-dev-tag:
    name: Create dev tag (vX.Y.Z-dev.N)
    runs-on: ubuntu-latest
    # Bug 1 fix: skip prerelease when CI did not succeed.
    if: ${{ github.event.workflow_run.conclusion == 'success' }}
    timeout-minutes: 30

    steps:
      - name: Checkout CI head commit
        uses: actions/checkout@v4
        with:
          # Bug 2 fix: pin to the SHA that ran CI, not current master HEAD.
          ref: ${{ github.event.workflow_run.head_sha }}
          persist-credentials: true

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Cargo cache
        uses: Swatinem/rust-cache@v2
        with:
          sharedKey: prerelease-linux-x86_64

      - name: Compute next dev tag
        id: compute-tag
        shell: bash
        run: |
          set -euo pipefail
          VERSION=$(grep '^version = ' Cargo.toml | head -1 | cut -d '"' -f2)
          LATEST_DEV_TAG=$(git ls-remote --tags origin "v${VERSION}-dev.*" \
            | sed 's#refs/tags/##' | sed 's#\^{}##' | sort -V | tail -1 || true)
          if [[ -n "$LATEST_DEV_TAG" ]]; then
            CURRENT_NUM=$(echo "$LATEST_DEV_TAG" | grep -oE '[0-9]+$')
            NEXT_NUM=$((CURRENT_NUM + 1))
          else
            NEXT_NUM=1
          fi
          TAG="v${VERSION}-dev.${NEXT_NUM}"
          echo "tag=$TAG" >> "$GITHUB_OUTPUT"
          echo "Will create tag: $TAG"

      - name: Check if release already exists
        id: check-release
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        shell: bash
        run: |
          TAG="${{ steps.compute-tag.outputs.tag }}"
          if gh release view "$TAG" >/dev/null 2>&1; then
            echo "Tag $TAG already exists — skipping creation"
            echo "skip=true" >> "$GITHUB_OUTPUT"
          else
            echo "skip=false" >> "$GITHUB_OUTPUT"
          fi

      - name: Build release binary (Linux x86_64)
        if: steps.check-release.outputs.skip != 'true'
        shell: bash
        run: |
          set -euo pipefail
          cargo build --release
          cp target/release/strangecoin strangecoin-linux-x86_64
          shasum -a 256 strangecoin-linux-x86_64 > strangecoin-linux-x86_64.sha256

      - name: Create and push dev release
        if: steps.check-release.outputs.skip != 'true'
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        shell: bash
        run: |
          set -euo pipefail
          TAG="${{ steps.compute-tag.outputs.tag }}"
          HEAD_SHA="${{ github.event.workflow_run.head_sha }}"
          gh release create "$TAG" \
            --target "$HEAD_SHA" \
            --prerelease \
            --title "$TAG" \
            --notes "Automated dev build, not for production" \
            --generate-notes \
            strangecoin-linux-x86_64 strangecoin-linux-x86_64.sha256

      - name: Post commit comment
        if: always() && steps.check-release.outputs.skip != 'true'
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        shell: bash
        run: |
          set -euo pipefail
          TAG="${{ steps.compute-tag.outputs.tag }}"
          HEAD_SHA="${{ github.event.workflow_run.head_sha }}"
          REPO="${{ github.repository }}"
          if [[ "${{ steps.check-release.outputs.skip }}" == "true" ]]; then
            echo "Tag $TAG already exists — skipping comment"
            exit 0
          fi
          gh api \
            --method POST \
            -H "Accept: application/vnd.github+json" \
            "/repos/${REPO}/commits/${HEAD_SHA}/comments" \
            -f body="Dev build created: ${TAG} — https://github.com/${REPO}/releases/tag/${TAG}"
```

Требования к коммиту. Сообщение в формате repo-convention:
`[GITHUB-P12] prerelease: full rewrite — conclusion gate + head_sha + rust-cache + github-script fix`

Проверка.
1. actionlint .github/workflows/prerelease.yml — без ошибок.
   (Локально: `actionlint .github/workflows/prerelease.yml` если стоит; иначе
   через `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/prerelease.yml'))"`.)
2. Убедиться, что в новом YAML нет ни одного использования контекста `steps`
   внутри блока `actions/github-script@v7` (таких блоков вообще не должно
   остаться — уведомление переведено на `gh api` в shell-шаге).
3. После merge (Промпт 1 уже в master, CI зелёный) дождаться первого
   workflow_run-triggered prerelease. Ожидаемое поведение:
   - Job стартует только если conclusion == 'success' (на красном CI prerelease
     не запускается совсем — в Actions не появляется новый run).
   - Compute-tag: TAG=v2.0.0-dev.1 (т.к. dev-тегов в origin нет).
   - Check-release: skip=false (тег не существует).
   - Build: cargo build --release — с cache-hit ~3 мин, с cache-miss ~12 мин.
   - Create release: gh release create успешно — в git ls-remote --tags origin
     появляется refs/tags/v2.0.0-dev.1, в /releases появляется prerelease с
     strangecoin-linux-x86_64 артефактом.
   - Post commit comment: gh api POST коммит-коммент с текстом "Dev build
     created: v2.0.0-dev.1 — https://github.com/Reider85/strangecoin/releases/tag/v2.0.0-dev.1".
   - Workflow статус: success.
4. Повторный push (или re-run workflow_run) с тем же SHA должен завершиться
   success с skip=true (tag already exists) — без сборки, без duplicate tag.
5. В истории master НЕ должны появляться новые коммиты вида
   "Update stargazer count to N" или "chore(stars): hourly snapshot N" —
   stargazer переехал в отдельный workflow (Промпт 3).

Не делай.
- Не возвращай `actions/github-script@v7` для commit-comment — там нет `steps`,
  повторишь тот же баг. Используй `gh api` из shell.
- Не возвращай шаг `Update stargazer history` в этот файл — он переезжает в
  .github/workflows/stargazer.yml (Промпт 3).
- Не добавляй `--no-default-features` в `cargo build --release` — релизный
  билд должен собирать с теми же features, что и `release.yml` на v*-тегах
  (default = net + vm-wasm + gui + legacy-migrate), иначе dev-артефакт
  расходится с продакшн-релизом.
- Не меняй `on.workflow_run.types: [completed]` на `[requested]` —后者 не
  срабатывает на workflow_run (его нет в spec). Gate делается через job-level
  `if: conclusion == 'success'`.
- Не меняй триггер на `on.push.branches: [master]` — тогда prerelease будет
  стартовать ДО того, как CI подтвердит, что код собирается. Текущий
  workflow_run-паттерн правильный, нужно только добавить conclusion-gate.
```

---

## Промпт 3. [ВЫСОКО] Вынести «Update stargazer history» из prerelease.yml + release.yml в отдельный cron-job `stargazer.yml`

```
Контекст. В Reider85/strangecoin шаг `Update stargazer history` живёт сейчас
в двух местах:
  - .github/workflows/prerelease.yml (после Create dev release)
  - .github/workflows/release.yml    (упоминается в комментариях; шаг мог
    быть удалён в P10, проверить: rg -n 'stargazer' .github/workflows/release.yml)

Обе копии делают одно и то же:
  curl -s "https://api.github.com/repos/${{ github.repository }}" \
    | jq '.stargazers_count' > stargazers.txt
  STARS=$(cat stargazers.txt)
  sed -i "s/⭐ [0-9]*/⭐ $STARS/" README.md
  git commit -m "Update stargazer count to $STARS"
  git push

Этот `git push` в master создаёт рекурсивный цикл:
  push в master → CI → prerelease → stargazer step → git push в master
  → CI → prerelease → stargazer step → git push в master → ...

В истории master видно, как один апстрим-коммит порождает серию
"Update stargazer count to N" / "chore(stars): hourly snapshot N" — по 5+
коммитов в день, не несущих функциональных изменений. Дополнительно:
  - `curl api.github.com/repos/...` без auth-токена rate-limited после 60
    запросов/час. В логах видны `Update stargazer count to null` (jq вернул
    null из-за 403-rate-limit response) — это и есть «звёзды=null» в README.
  - `sed -i "s/⭐ [0-9]*/⭐ $STARS/"` ломается, если STARS=null: вставляет
    `⭐ null stars` вместо числа.
  - На fork-PR и на push в dependabot-ветки git push падает с
    `insufficient permissions` (токен workflow_run не имеет push в чужой
    fork), что ещё больше краснит Actions badge.

Задача.

1. Создать НОВЫЙ workflow .github/workflows/stargazer.yml:
   - Триггер: cron '0 * * * *' (раз в час) + workflow_dispatch.
   - runs-on: ubuntu-latest.
   - permissions: contents: write (для commit-back в master — если решено
     сохранить commit-back), ИЛИ contents: read + push в отдельную ветку
     `gh-pages-meta` (рекомендуется — не загрязняет master).
   - Шаги:
     a. checkout master (shallow, depth=1).
     b. curl GitHub API С АВТОРИЗАЦИЕЙ (`Authorization: Bearer ${{ secrets.GITHUB_TOKEN }}`),
        чтобы не ловить 403-rate-limit:
            curl -sS -H "Authorization: Bearer ${{ secrets.GITHUB_TOKEN }}" \
                 -H "Accept: application/vnd.github+json" \
                 "https://api.github.com/repos/${{ github.repository }}" \
              | jq -r '.stargazers_count' > stars.txt
     c. Если `stars.txt` пуст или содержит "null" — exit 0 (ничего не
        делаем, не коммитим null в README).
     d. Если commit-back в master:
        - sed -i на README.md, только если STARS — число (regex `^[0-9]+$`).
        - git config user.email "action@github.com", user.name "GitHub Action".
        - git add README.md
        - git commit -m "chore(stars): hourly snapshot $(date -u +%Y%m%d-%H%M)"
        - git push (с токеном checkout@v4 persist-credentials: true).
     e. ДОБАВИТЬ guard: если HEAD master тот же, что в прошлый час (по
        `git rev-parse HEAD`), и stars не изменился — НЕ коммитить (exit 0).
        Это снижает noise.

   Целевой YAML:

   ```yaml
   name: Stargazer snapshot

   # [GITHUB-P13] stargazer: hourly snapshot, moved out of prerelease.yml
   # and release.yml (those workflows were doing git push to master on every
   # CI run → recursive loop of "Update stargazer count to N" commits). Auth
   # via GITHUB_TOKEN to avoid 403 rate-limit (was producing "null" updates).
   # No push if stars unchanged or unparseable.

   on:
     schedule:
       - cron: '0 * * * *'   # hourly
     workflow_dispatch:

   permissions:
     contents: write

   concurrency:
     group: stargazer
     cancel-in-progress: false

   jobs:
     snapshot:
       runs-on: ubuntu-latest
       timeout-minutes: 5
       steps:
         - uses: actions/checkout@v4
           with:
             persist-credentials: true

         - name: Fetch current stars
           env:
             GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
           shell: bash
           run: |
             set -euo pipefail
             stars=$(gh api repos/${{ github.repository }} --jq '.stargazers_count')
             case "$stars" in
               ''|null)
                 echo "Stars API returned empty/null — rate-limited or private repo; skip"
                 echo "stars=" >> "$GITHUB_OUTPUT"
                 exit 0
                 ;;
               *[!0-9]*)
                 echo "Stars API returned non-numeric: '$stars'; skip"
                 echo "stars=" >> "$GITHUB_OUTPUT"
                 exit 0
                 ;;
             esac
             echo "stars=$stars" >> "$GITHUB_OUTPUT"
             echo "Current stars: $stars"

         - name: Update README snapshot
           if: steps.stargazer.outputs.stars != ''
           shell: bash
           run: |
             set -euo pipefail
             STARS="${{ steps.stargazer.outputs.stars }}"
             if grep -q '⭐ [0-9]' README.md; then
               # Replace existing star count.
               sed -i "s/⭐ [0-9][0-9]*/⭐ $STARS/" README.md
             elif grep -q '⭐ null' README.md; then
               # Recover from a previous null-write.
               sed -i "s/⭐ null/⭐ $STARS/" README.md
             else
               # No marker yet — append to top of README after the first
               # heading line, so future snapshots have a stable anchor.
               awk -v stars="⭐ $STARS stars" '
                 NR==1 && /^#/ { print; print ""; print stars; print ""; next }
                 { print }
               ' README.md > README.md.new && mv README.md.new README.md
             fi
             git diff --stat README.md

         - name: Commit if README actually changed
           if: steps.stargazer.outputs.stars != ''
           shell: bash
           run: |
             set -euo pipefail
             if git diff --quiet -- README.md; then
               echo "README unchanged (stars same as last commit) — skip"
               exit 0
             fi
             git config user.email "action@github.com"
             git config user.name  "GitHub Action"
             git add README.md
             git commit -m "chore(stars): hourly snapshot $(date -u +%Y%m%d-%H%MZ)"
             git push
   ```

   (Заменить `steps.stargazer.outputs.stars` references на `env.STARS` если
   не использовать step-id+outputs — выше просто показан один из вариантов.
   Главное: STARS валидируется как число перед sed, null/empty → exit 0.)

2. УДАЛИТЬ шаг `Update stargazer history` из .github/workflows/prerelease.yml
   (Промпт 2 уже это делает при полной переписке — убедиться, что в
   финальной версии файла этого шага нет).

3. УДАЛИТЬ шаг `Update stargazer history` из .github/workflows/release.yml,
   если он там остался (проверить `rg -n 'Update stargazer\|stargazers_count'
   .github/workflows/release.yml`). Если в release.yml шаг отсутствует —
   только добавить новый workflow stargazer.yml.

4. Опционально: дополнительно писать исторический снапшот в
   analytics/stargazer-history.csv (файл уже существует, см. ls analytics/) —
   добавить шаг в stargazer.yml, который дописывает строку
   `timestamp,stars,commit_sha` в CSV (без commit-back в master, только в
   gh-pages-ветку или как artifact upload).

Требования к коммиту. Сообщение в формате repo-convention:
`[GITHUB-P13] stargazer: hourly snapshot cron-job, decoupled from prerelease/release`

Проверка.
1. actionlint .github/workflows/stargazer.yml.
2. workflow_dispatch на свежем clone — должен завершиться success. Если
   звёзд 0 (private/fresh repo) или API rate-limit — exit 0, не падает.
3. После первого hourly cron — в master НЕ должен появляться лишний
   коммит, если звёзды не изменились.
4. В истории master больше нет "Update stargazer count to N" от prerelease.
5. README.md содержит корректный `⭐ <number> stars` (не `⭐ null stars`).

Не делай.
- Не оставляй stargazer-step в prerelease.yml — это восстановит рекурсивный
  цикл.
- Не пиши в master напрямую через `gh api` POST commit — это ещё хуже, чем
  git push (нет атомарности с деревом). Используй git commit + git push.
- Не используй `curl` без Authorization — будет ловить 403 после 60
  запросов/час и писать "null" в README (как сейчас).
- Не вставляй шаг в release.yml (он тоже запускается на v*-тегах —
  повторил бы цикл для full releases).
```

---

## Промпт 4. [СРЕДНИЙ] Зафиксировать `conclusion == 'success'` gate и `head_sha` checkout в любых будущих `workflow_run`-workflow'ах (defensive check)

```
Контекст. В Reider85/strangecoin уже есть один `workflow_run`-workflow
(prerelease.yml). Любой будущий workflow, который захочет реагировать на
CI/master, скопирует паттерн и может повторить те же баги: запуск на красном
CI, race-condition с checkout, и т.д.

Задача. Добавить короткий раздел в analytics/GITHUB.md (он уже существует —
ls analytics/GITHUB.md) под названием "P7. workflow_run defensive checklist"
после P6 (или в конец документа, если P7 уже занят — выбрать следующий
свободный ID). Содержание:

  ## P7. workflow_run defensive checklist

  Любой workflow, запускаемый через `on.workflow_run`, ДОЛЖЕН:

  1. Иметь на job-level (или на каждом job):
         if: ${{ github.event.workflow_run.conclusion == 'success' }}
     Без этого workflow срабатывает и на failure, и на cancelled CI. На
     красном master это означает: workflow стартует, тратит runner-минуты,
     и падает на неактуальном коде — смазывает сигнал «master красный».

  2. В actions/checkout явно указывать:
         ref: ${{ github.event.workflow_run.head_sha }}
     Без этого checkout берёт текущий HEAD мастер-ветки на момент запуска
     workflow — а не тот SHA, который запустил CI. Если между CI и
     workflow_run в master успели запушить новый коммит, workflow работает
     с другим кодом, чем CI валидировал.

  3. НЕ использовать контекст `steps` внутри `actions/github-script@v7`.
     Этот action не экспонирует `steps` (см.
     https://github.com/actions/github-script). Чтение
     `steps.<id>.outputs.<name>` выбрасывает `ReferenceError: steps is not
     defined`. Если нужно значение из предыдущего shell-шага — передавай его
     через `env:` (например `env.TAG: ${{ steps.compute-tag.outputs.tag }}`),
     либо используй `gh api` из bash-шага вместо github-script.

  4. НЕ использовать `context.<step_id>` или `context.tag` в github-script —
     у `context` нет динамических свойств для step-outputs. Доступные
     свойства: `repo`, `sha`, `ref`, `workflow`, `action`, `actor`,
     `event_name`, `event`, `run_id`, `run_number`, `run_attempt`,
     `serverUrl`, `apiUrl`, `graphqlUrl`, `payload`.

  5. НЕ пушить в master из `workflow_run`-workflow (рекурсивный цикл:
     push → CI → workflow_run → push → CI → ...). Любые фоновые
     обновления README, badge-счётчиков, stargazer-history и т.п. — в
     отдельный cron-job (см. .github/workflows/stargazer.yml как образец),
     с guard "skip если не изменилось".

  6. Передавать `GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}` явно в `env:` для
     `gh` CLI — на workflow_run auto-injected `GITHUB_TOKEN` обычно работает,
     но `gh` ищет сначала `GH_TOKEN`, явное объявление надёжнее на всех
     версиях CLI.

Требования к коммиту. Сообщение:
`[GITHUB-P14] docs: P7 workflow_run defensive checklist in analytics/GITHUB.md`

Проверка.
1. Документ открывается и рендерится на GitHub (Markdown lint: заголовки
   последовательны, нет broken links).
2. При следующем появлении `workflow_run` в репо (например, новый workflow
   для nightly-summary, dependabot-summary и т.п.) — автор читает этот
   чек-лист и НЕ повторяет баги P12/P13.
```

---

## Промпт 5. [НИЗКИЙ] Проверить и при необходимости подчистить `actions/github-script@v7` во всех workflow'ах (rg audit)

```
Контекст. Промпт 2 убирает `actions/github-script@v7` из prerelease.yml. В
репо могут быть другие workflow'ы, использующие тот же action с такими же
багами (steps undefined, context.<unknown>). Требуется аудит.

Задача.

1. rg -n 'actions/github-script@v' .github/workflows/ — найти все
   использования. Для каждого:
   - Прочитать блок `with: script: ...`.
   - Если скрипт обращается к `steps.<id>.outputs.<name>` или `steps.<id>.conclusion`
     → ЭТО БАГ (steps не определён в github-script).
   - Если скрипт обращается к `context.tag`, `context.step`, `context.job`
     → ЭТО БАГ (таких свойств нет).
   - Если скрипт обращается к `context.sha`, `context.ref`, `context.repo`,
     `context.actor`, `context.event_name`, `context.payload` → OK.
   - Если скрипт обращается к `github.repository`, `github.event.*`,
     `github.ref`, `github.sha`, `github.run_id` и т.п. → OK (это валидные
     свойства github context в github-script).

2. Для каждого найденного бага — заменить обращение к `steps` на `env:`
   передачу через `with:` (если это input action) или убрать github-script и
   переписать шаг на shell + `gh api` (как в Промпте 2 для commit-comment).

3. Для каждой найденной ссылки `context.<unknown>` — заменить на
   `${{ steps.<id>.outputs.<name> }}` через env, либо на `context.payload.<field>`,
   если поле лежит в event-payload.

4. Дополнительно: проверить, что ни один workflow не делает `git push` в
   мастер-ветку изнутри `workflow_run`-триггера. Список кандидатов:
   rg -n 'git push' .github/workflows/ — каждое использование проверить, что
   оно НЕ в workflow_run-context. Если push в master есть — вынести в
   отдельный cron-job (по образцу Промпта 3).

Требования к коммиту. Сообщение:
`[GITHUB-P15] ci: audit github-script usage (steps/context.<unknown> bugs)`

Проверка.
1. rg -n 'actions/github-script' .github/workflows/ — каждое использование
   прогнать через mental check: нет обращений к steps.*, нет обращений к
   context.<unknown_property>.
2. actionlint на всех .github/workflows/*.yml — без ошибок.
3. Если найденные баги были в nightly.yml или pages.yml — после merge
   следующий run не должен падать на тех шагах.

Не делай.
- Не переписывай github-script на чистый shell «just in case» — action
  сам по себе легальный, баги в обращении к steps/context.<unknown>.
  Переписывай только если найден реальный баг.
- Не добавляй `permissions: contents: write` в workflow'ы, которые не
  пушат в репо — это нарушает least-privilege.
```

---

## Порядок применения

1. **Промпт 1** (cargo fmt drift в witness.rs) — критично, без него CI
   остаётся красным и prerelease.yml стартует на сломанном коде даже после
   Промпта 2. Открывает дорогу всем остальным.
2. **Промпт 2** (полная переписка prerelease.yml) — критично, исправляет 7
   багов сразу. После merge + зелёного CI должен создаться первый
   `v2.0.0-dev.1` тег и GitHub Prerelease.
3. **Промпт 3** (stargazer cron-job + удаление push-step из prerelease/release)
   — высоко, убирает рекурсивный цикл. Применять сразу после Промпта 2,
   иначе новый prerelease.yml снова начнёт порождать stargazer-коммиты
   (если stargazer-step случайно вернётся при регрессии).
4. **Промпт 4** (P7 docs checklist в GITHUB.md) — средне, профилактика
   повторения багов в будущих workflow_run-workflow'ах.
5. **Промпт 5** (rg audit github-script) — низко, но его стоит прогнать
   до закрытия темы: в репо могут быть другие workflow'ы с тем же багом
   `steps undefined`, которые пока не проявлялись только потому, что
   workflow_run-цепочка никогда не доходила до них.

После каждого промпта — отдельный PR с commit-сообщением строго в формате
`[GITHUB-PNN] <area>: <change>`, продолжая счётчик из CI2 (P07-P10 уже в
истории; CI3 добавляет P11-P15).

После завершения всех 5 промптов pipeline «push в master → CI зелёный →
dev-тег vX.Y.Z-dev.N + GitHub Prerelease с linux-x86_64 артефактом» должен
работать детерминированно:
- На красном CI — prerelease не стартует (нет фантомных failed runs).
- На зелёном CI — prerelease запускается на том же SHA, что валидировал CI.
- С cache-hit — сборка занимает 2-3 минуты.
- Тег и release создаются через `gh release create --target $SHA`.
- Уведомление — через `gh api POST .../commits/{sha}/comments` (без github-script).
- В master не появляется побочных коммитов от самого prerelease.
