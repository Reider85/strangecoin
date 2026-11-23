# Промпты для устранения «push есть, а релиза и тега нет» в Reider85/strangecoin (CI5)

> **Симптом, который видит пользователь:** в `master` падают коммиты (последний
> `9d7ec0f` от 23 Nov 2026 — `[S4-UI-P02] ui: design system + 14 components…`),
> а новый тег `vX.Y.Z-dev.N` и GitHub Release не появляются. Последний тег
> `v3.0.0-dev.7` создан 21 Nov 2026; после него — два дня тишины. При этом
> в списке Actionsworkflow `Pre-release` помечен как **completed successfully**
> (run #71 от 22 Nov, run #72 от 23 Nov). То есть workflow «зелёный», а тега нет.

---

## Почему push есть, а релиза и тега нет (диагноз)

### Цепочка причинно-следственных связей

```
push в master  ──►  workflow "CI" запускается  ──►  CI FAILS (red)
                                                          │
                                                          ▼  workflow_run event(conclusion = "failure")
                          workflow "Pre-release" срабатывает на completed CI
                                                          │
                                                          ▼
                job create-dev-tag:  if: workflow_run.conclusion == 'success'   ← FALSE
                                                          │
                                                          ▼   SKIPPED
                job notify-skipped:  if: workflow_run.conclusion != 'success'   ← TRUE
                                                          │
                                                          ▼   RUNS, постит commit-comment на head_sha
                workflow "Pre-release" overall conclusion = success
                (потому что единственный выполненный job — notify-skipped — вышел в 0)
                                                          │
                                                          ▼
                              ❌ ТЕГ НЕ СОЗДАН  ❌  РЕЛИЗА НЕТ
```

### Ключевое наблюдение

`prerelease.yml` **НЕ сломан**. Гейт
`if: ${{ github.event.workflow_run.conclusion == 'success' }}` (строка 36) — это
намеренная защита, добавленная в CI3.md / `[GITHUB-P12]`: не публиковать
dev-сборку с красного CI. Она работает ровно так, как задумано.

**Проблема лежит на уровень ниже — красный CI.** Пока CI красный, гейт
корректно блокирует создание тега, и `Pre-release` помечается «success»
только потому, что его единственный выполненный job — это `notify-skipped`,
а не `create-dev-tag`.

### Факты из репозитория на 23 Nov 2026

| Параметр | Значение | Где подтверждено |
|---|---|---|
| `version` в `Cargo.toml` | `3.0.0` | `Cargo.toml` строка 2 |
| Последний тег | `v3.0.0-dev.7` (21 Nov 2026) | github.com/Reider85/strangecoin/tags |
| Ожидаемый следующий тег (когда CI позеленеет) | `v3.0.0-dev.8` | логика шага `Compute next dev tag` в `prerelease.yml` |
| Последний коммит в `master` | `9d7ec0f` — `[S4-UI-P02] ui: design system + 14 components…` (23 Nov 2026) | github.com/Reider85/strangecoin/commits/master |
| CI run #248 (на пуше от 23 Nov) | **failed** | github.com/Reider85/strangecoin/actions/workflows/ci.yml |
| CI runs #240–#245, #247 (с 20 Nov) | **failed** (только #246 — success, он и создал `dev.7`) | там же |
| Pre-release run #71 (22 Nov), #72 (23 Nov) | completed success, **тег не создан** | github.com/Reider85/strangecoin/actions/workflows/prerelease.yml |
| Nightly run #16 (22 Nov) | failed | /actions/workflows/nightly.yml |

### Что НЕ является причиной (опровергнутые гипотезы)

1. **«`prerelease.yml` снова сломан»** — нет, файл содержит уже применённый
   fix из CI3.md (`[GITHUB-P12]` в шапке, гейт на строке 36, retry 3× на
   `gh release create` из `[GITHUB-P28]`, `head_sha`-checkout из `[GITHUB-P22]`).
2. **«Права `contents: write` отозваны»** — нет, на уровне workflow
   `permissions: contents: write` присутствует (строка 21–22), и `notify-skipped`
   успешно постит commit-comment (значит токен валиден и права есть).
3. **«`Compute next dev tag` неверно считает номер»** — нет, `[GITHUB-P22]`
   фикс уже в файле (вырезает `refs/tags/` перед `sort -V`).
4. **«Stargazer loop снова петляет»** — нет, stargazer-обновление вынесено
   в отдельный `stargazer.yml` (cron, без commit-back в master), как и
   предписывал CI3.md / `[GITHUB-P12]`.

### Что ЕСТЬ причиной

**Красный CI.** Workflow `CI` (`.github/workflows/ci.yml`) падает на 4 из
последних 9 запусков (#240–#245, #247, #248). Любой из 4 его job'ов может
быть виновником:

| Job | `continue-on-error`? | Что проверяет | Типичная причина падения |
|---|---|---|---|
| `lint` | нет | `cargo fmt --all -- --check` + `cargo clippy -- -A warnings -D clippy::correctness` | fmt-drift после крупного UI-коммита; clippy::correctness регрессия |
| `test-core` | нет | `cargo nextest run --workspace` + `cargo test --workspace --doc` | broke test после `[S4-UI-P02]` |
| `coverage` | **да** | `cargo llvm-cov` | не красит badge, не виновник |
| `source-gates` | нет | 3 rg-аудита: balances mutation, `#[allow]` без justification, Stage-0 arith | новый код нарушает `BUG-S0-022` или `BUG-S1-012` |

Самый вероятный сценарий: крупный UI-рефакторинг `[S4-UI-P02]` (commit
`9d7ec0f`) либо принёс код с `cargo fmt` drift, либо新增 файл, нарушающий
`source-gates` (например, прямое `.balances.insert(...)` вне
`state_cache.rs` / `block_executor.rs` / `chain_selector.rs`, или
`#[allow(dead_code)]` без комментария-justification на предыдущей строке).
Точный ответ даст только лог CI run #248 — см. Промпт 1.

---

## Сводка действий по приоритету

| # | Приоритет | Что делает | Файл | Ожидаемый коммит |
|---|---|---|---|---|
| 1 | КРИТИЧНО | Вытащить лог CI #248, найти провалившийся job и класс ошибки | — (диагностика) | — |
| 2 | КРИТИЧНО | Применить точечный fix под класс ошибки из Промпта 1 (4 ветки) | `src/`, `tests/`, `crates/` | `[GITHUB-P31] fix: <root cause> — restore green CI (unblock dev-tag)` |
| 3 | КРИТИЧНО | Запушить fix, дождаться зелёного CI, убедиться, что `Pre-release` создал `v3.0.0-dev.8` | — (верификация) | — |
| 4 | ВЫСОКО | Сделать «Pre-release success, но тега нет» громким сигналом для мейнтейнера | `.github/workflows/prerelease.yml` | `[GITHUB-P32] prerelease: loud signal when create-dev-tag skipped (check-run annotation)` |
| 5 | СРЕДНИЙ | Локальный pre-push hook: `cargo fmt --check` + `clippy::correctness` до пуша в master | `.githooks/pre-push`, `push_script.sh` | `[GITHUB-P33] chore: pre-push hook for fmt + clippy::correctness (CI5 regression guard)` |

---

## Промпт 1. [КРИТИЧНО] Вытащить лог провалившегося CI run #248 и определить виновный job

**Цель:** получить точный ответ на вопрос «какой job и какой step в CI #248
упал, и какой класс ошибки». Без этого Промпт 2 — стрельба вслепую.

**Контекст для агента:** работаем в репозитории `Reider85/strangecoin`,
default branch `master`. CI workflow лежит в `.github/workflows/ci.yml` и
содержит 4 job'а: `lint`, `test-core`, `coverage` (continue-on-error,
**не виновник**), `source-gates`. Последний упавший run — CI #248 от
23 Nov 2026, на коммите `9d7ec0f` (или ближайшем к нему `60f248c`).

**Шаги, которые нужно выполнить:**

1. Авторизоваться в `gh` CLI локально (`gh auth status`). Если не авторизован —
   попросить пользователя выполнить `gh auth login` с scope `repo` и `workflow`.
2. Получить список job'ов упавшего run'а:
   ```bash
   gh run view --repo Reider85/strangecoin <RUN_ID_248> --json jobs \
     --jq '.jobs[] | {name, conclusion, startedAt, completedAt}'
   ```
   где `<RUN_ID_248>` — длинный числовой ID run'а #248. Его можно получить так:
   ```bash
   gh run list --repo Reider85/strangecoin --workflow ci.yml \
     --limit 5 --json databaseId,conclusion,headSha,createdAt \
     --jq '.[] | select(.conclusion=="failure")'
   ```
3. Для каждого job с `conclusion == "failure"` скачать лог:
   ```bash
   gh run view --repo Reider85/strangecoin <RUN_ID_248> --log-failed \
     > /tmp/ci248-failed.log 2>&1
   ```
4. Извлечь из лога:
   - имя провалившегося job'а (`lint` / `test-core` / `source-gates`);
   - имя провалившегося step'а;
   - первые 50 строк секции `##[error]` / `error\[E...\]` /
     `Diff in` (для fmt) / `panicked at` (для test) / `Unjustified allow at`
     (для source-gates);
   - файл и номер строки, на которую указывает ошибка.
5. Классифицировать ошибку в один из 4 классов:
   - **FMT** — `cargo fmt --all -- --check` репортит `Diff in <file>` → Промпт 2, ветка A.
   - **CLIPPY-CORRECTNESS** — `cargo clippy` репортит `error: <lint>` с
     `-D clippy::correctness` → Промпт 2, ветка B.
   - **TEST-FAILURE** — `cargo nextest run` или `cargo test --doc` репортит
     `FAILED` / `panicked at` → Промпт 2, ветка C.
   - **SOURCE-GATE** — один из трёх rg-аудитов падает с явным
     `Direct balances mutation outside whitelisted files` /
     `Unjustified allow at <file>:<line>` /
     `Stage 0 summary arithmetic` → Промпт 2, ветка D.

**Артефакт на выходе:** короткий отчёт в чат:
```
CI #248 failure report
- failing job: <name>
- failing step: <name>
- error class: <FMT | CLIPPY-CORRECTNESS | TEST-FAILURE | SOURCE-GATE>
- root file:line: <path>:<line>
- first 20 lines of error output:
  <paste>
- recommended branch of Prompt 2: <A | B | C | D>
```

**Acceptance:** в отчёте явно назван один из 4 классов и указан
файл:строка. Если класс определить не удалось — вернуться к шагу 3 и
поднять лог полностью (`gh run view --log`), а не только `--log-failed`.

---

## Промпт 2. [КРИТИЧНО] Применить точечный fix под класс ошибки из Промпта 1

**Цель:** устранить root cause, найденный в Промпте 1, минимальным
коммитом, после которого CI #249 (или следующий) станет зелёным.

**Контекст для агента:** после fix'а НЕ нужно бампать версию в `Cargo.toml`
(она уже `3.0.0`, и `Compute next dev tag` сам вычислит `dev.8`). НЕ нужно
трогать `prerelease.yml` — он не сломан (см. диагноз выше). НЕ нужно
создавать тег вручную — это сделает workflow `Pre-release`, как только CI
позеленеет.

**Общие шаги для всех 4 веток:**
```bash
cd <path-to-local-strangecoin-clone>
git checkout master
git pull --ff-only origin master
git checkout -b github-p31-fix-ci-red
```

### Ветка A. `cargo fmt` drift

```bash
cargo fmt --all -- --check           # подтвердить наличие drift
cargo fmt --all                       # применить авто-fix
git diff --stat                       # убедиться, что меняются только .rs файлы
cargo nextest run --workspace --no-fail-fast   # тесты не должны сломаться от fmt
```
**Acceptance:** `cargo fmt --all -- --check` выходит в 0.

### Ветка B. `clippy::correctness` регрессия

```bash
cargo clippy --workspace --all-targets -- -A warnings -D clippy::correctness 2>&1 | tee /tmp/clippy.log
# Прочитать /tmp/clippy.log, найти error: строки
# Для каждой ошибки: открыть файл, исправить root cause clippy::correctness
# (НЕ подавлять через #[allow] — это correctness-класс, allow недопустим по
# Deviation 5 в ci.yml)
cargo clippy --workspace --all-targets -- -A warnings -D clippy::correctness   # exit 0
```
**Acceptance:** `cargo clippy --workspace --all-targets -- -A warnings -D clippy::correctness` выходит в 0. `#[allow(clippy::correctness)]` **не добавлен**.

### Ветка C. Test failure

```bash
cargo nextest run --workspace --no-fail-fast 2>&1 | tee /tmp/test.log
cargo test --workspace --doc           2>&1 | tee /tmp/doc-test.log
# Найти FAILED тесты в /tmp/test.log и /tmp/doc-test.log
# Для каждого: открыть тест, открыть тестируемый код, понять — сломался ли
# тест (тестовое ожидание устарело) или сломался код (регрессия).
#   - Если сломался КОД: починить код, тест не трогать.
#   - Если устарел ТЕСТ (например, после [S4-UI-P02] изменился API): обновить
#     ожидания в тесте, в commit message указать «test: update <test> for S4-UI-P02 API».
cargo nextest run --workspace --no-fail-fast   # exit 0
cargo test --workspace --doc                   # exit 0
```
**Acceptance:** оба выхода в 0.

### Ветка D. Source-gate нарушение

Прочитать, какой из трёх gate'ов упал (см. шаг 5 Промпта 1):

- **`Direct balances mutation outside whitelisted files`** (`BUG-S0-022`):
  найти нарушение через `rg -n '\.balances\.(insert|remove|get_mut)' src/` или
  `rg -n '\.balances\s*=[^=]' src/`. Перенести мутацию в `state_cache.rs` /
  `block_executor.rs` / `chain_selector.rs` (единственные whitelist-файлы).
  Если это невозможно — обсудить с мейнтейнером расширение whitelist, но
  **не коммитить** `#[allow]` workaround.

- **`Unjustified allow at <file>:<line>`** (`BUG-S1-012`):
  добавить `// justification: <причина>` комментарий на строку выше
  `#[allow(dead_code)]` или `#[allow(clippy::await_holding_lock)]`.
  Без justification gate не пройдёт.

- **`Stage 0 summary arithmetic`** (`BUG-S1-014`):
  запустить локально `python3 scripts/audit/check_stage0_arith.py`,
  исправить несоответствие в Stage 0 summary docs.

```bash
# Верификация source-gate'ов локально — повторяет 3 step'а из ci.yml:
set -euo pipefail
raw=$(rg -n '\.balances\.(insert|remove|get_mut)' src/ --glob '!state_cache.rs' --glob '!block_executor.rs' || true)
assign=$(rg -n '\.balances\s*=[^=]' src/ --glob '!state_cache.rs' --glob '!chain_selector.rs' || true)
test -z "$raw$assign"
# + allow(dead_code|await_holding_lock) gate
# + Stage 0 arith
python3 scripts/audit/check_stage0_arith.py
```
**Acceptance:** все 3 gate'а проходят локально.

### Общий финал (после любой из веток A/B/C/D)

```bash
# Полная верификация — повторяет все 3 red job'а из ci.yml локально:
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -A warnings -D clippy::correctness
cargo nextest run --workspace --no-fail-fast --retries 2
cargo test --workspace --doc
# source-gates — 3 шага выше
git add -A
git commit -m "[GITHUB-P31] fix: <root cause one-liner> — restore green CI (unblock dev-tag)

CI runs #240-#245, #247, #248 were red on <failing job>, blocking the
Pre-release workflow_run gate (prerelease.yml line 36:
  if: github.event.workflow_run.conclusion == 'success').
As a result vX.Y.Z-dev.N tag was not created for 2 days after v3.0.0-dev.7
(21 Nov 2026). Root cause: <2-3 line explanation>.

Fix: <one-line description>.

Verification: <fmt + clippy + nextest + doc-test + source-gates all green locally>.

Refs: CI5.md Промпт 2 (branch <A|B|C|D>), CI3.md [GITHUB-P12] gate."
```
```bash
git push origin github-p31-fix-ci-red
gh pr create --repo Reider85/strangecoin \
  --base master --head github-p31-fix-ci-red \
  --title "[GITHUB-P31] fix: <root cause> — restore green CI (unblock dev-tag)" \
  --body-file - <<'EOF'
## Why
CI red since 20 Nov (#240–#245, #247, #248). `Pre-release` workflow_run gate
`conclusion == 'success'` (prerelease.yml:36) correctly skipped `create-dev-tag`,
so no new `v3.0.0-dev.N` tag was created after `v3.0.0-dev.7`.

## Root cause
<from Prompt 1 report>

## Fix
<from the branch A/B/C/D applied>

## Verification
- [ ] `cargo fmt --all -- --check` — 0
- [ ] `cargo clippy --workspace --all-targets -- -A warnings -D clippy::correctness` — 0
- [ ] `cargo nextest run --workspace --no-fail-fast --retries 2` — 0 failed
- [ ] `cargo test --workspace --doc` — 0 failed
- [ ] 3 source-gates (BUG-S0-022 / BUG-S1-012 / BUG-S1-014) — all green locally

## After merge
Expected: CI #249 (or next) green → `Pre-release` workflow_run fires with
`conclusion == 'success'` → `create-dev-tag` runs (NOT `notify-skipped`) →
tag `v3.0.0-dev.8` + GitHub Release appear automatically.
EOF
```

**Acceptance:** PR смёрджен в `master`, следующий CI run — зелёный.

---

## Промпт 3. [КРИТИЧНО] После мёрджа — верифицировать, что `v3.0.0-dev.8` создан автоматически

**Цель:** подтвердить end-to-end, что fix из Промпта 2 действительно разблокировал
релизный конвейер, и закрыть вопрос «push есть, тега нет».

**Шаги:**

1. После мёрджа PR из Промпта 2 дождаться завершения CI:
   ```bash
   gh run list --repo Reider85/strangecoin --workflow ci.yml \
     --limit 3 --json databaseId,conclusion,headSha,createdAt
   ```
   Ожидаемый `conclusion` для свежего run'а на коммите fix'а: `success`.

2. Сразу после завершения CI запустится `Pre-release` (trigger
   `workflow_run: workflows: ["CI"], types: [completed]`). Дождаться его
   завершения:
   ```bash
   gh run list --repo Reider85/strangecoin --workflow "Pre-release" \
     --limit 3 --json databaseId,conclusion,headSha,createdAt
   ```

3. **Ключевая проверка — какой job внутри `Pre-release` реально выполнился.**
   «Workflow success» ≠ «тег создан»: если `notify-skipped` снова отработал
   (значит CI всё-таки красный), тега не будет. Проверить:
   ```bash
   gh run view --repo Reider85/strangecoin <PRERELEASE_RUN_ID> --json jobs \
     --jq '.jobs[] | {name, conclusion}'
   ```
   **Ожидаемый результат:** job `Create dev tag (vX.Y.Z-dev.N)` имеет
   `conclusion == "success"`, а job `Notify on skipped dev-tag` — `skipped`.
   Если наоборот — CI всё ещё красный, вернуться к Промпту 1.

4. Проверить, что тег и релиз созданы:
   ```bash
   gh release view v3.0.0-dev.8 --repo Reider85/strangecoin \
     --json tagName,name,isPrerelease,url,assets
   git ls-remote --tags origin 'v3.0.0-dev.*'
   ```
   **Ожидаемый результат:** `v3.0.0-dev.8` присутствует, `isPrerelease: true`,
   в `assets` есть `strangecoin-linux-x86_64` + `.sha256`.

5. Проверить commit-comment на head_sha коммита fix'а:
   ```bash
   gh api /repos/Reider85/strangecoin/commits/<FIX_SHA>/comments \
     --jq '.[] | {body: .body[0:120], created_at}'
   ```
   **Ожидаемый результат:** хотя бы один комментарий с текстом
   `Dev build created: v3.0.0-dev.8 — https://github.com/Reider85/strangecoin/releases/tag/v3.0.0-dev.8`.

**Acceptance:** в репозитории присутствует тег `v3.0.0-dev.8`, привязанный к
коммиту fix'а, и соответствующий GitHub Release (pre-release) с Linux-бинарём
и sha256. Если этого нет — НЕ создавать тег вручную, а вернуться к Промпту 1
(CI всё ещё красный на самом деле, либо workflow_run gate не сработал).

**Антипаттерн (НЕ делать):** не создавать тег через `gh release create v3.0.0-dev.8`
вручную — это замаскирует проблему, и следующий красный CI снова тихо заблокирует
релиз. Конвейер должен создавать тег сам; если он этого не делает — чинить
конвейер, а не подменять его.

---

## Промпт 4. [ВЫСОКО] Сделать «Pre-release success, но тега нет» громким сигналом

**Цель:** ситуация, в которой workflow помечен «success», а тег не создан,
по определению **тихая** — единственный текущий сигнал это commit-comment
от `notify-skipped`, который мейнтейнер видит только если подписан на
commit-comments (почти никто не подписан). Добавить loud-сигнал в список
Actions: **красный check-run** на head_sha, когда `create-dev-tag` skipped.

**Контекст для агента:** не ломать существующий `notify-skipped` job — он
уже постит commit-comment. Добавить рядом новый шаг, который создаёт
**x-unchecked check-run** через GitHub API ( тот же `secrets.GITHUB_TOKEN`,
та же permission `contents: write` — она покрывает `POST /repos/{o}/{r}/check-runs`).

**Изменение в `.github/workflows/prerelease.yml`**, в job `notify-skipped`,
добавить второй step после `Post commit comment`:

```yaml
      - name: Create red check-run on CI head SHA
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          HEAD_SHA: ${{ github.event.workflow_run.head_sha }}
          CI_RUN_ID: ${{ github.event.workflow_run.id }}
          CI_CONCLUSION: ${{ github.event.workflow_run.conclusion }}
          REPO: ${{ github.repository }}
        shell: bash
        run: |
          set -euo pipefail
          # check-runs требуют external app id и name; workflow_run-triggered
          # runs не имеют своего check-suite, поэтому публиковать нужно как
          # standalone check-run с status=completed, conclusion=neutral,
          # но с title и summary, которые горят красным в UI Actions.
          gh api --method POST \
            -H "Accept: application/vnd.github+json" \
            "/repos/${REPO}/check-runs" \
            -F name="Pre-release (dev-tag)" \
            -F head_sha="${HEAD_SHA}" \
            -F status="completed" \
            -F conclusion="skipped" \
            -F details_url="https://github.com/${REPO}/actions/runs/${CI_RUN_ID}" \
            -f title="⚠️ Dev-tag skipped: CI conclusion was ${CI_CONCLUSION}" \
            -f summary="No vX.Y.Z-dev.N tag was created for this commit because CI did not conclude with 'success'. See CI5.md for the gate semantics."
```

**Commit message:**
```
[GITHUB-P32] prerelease: loud signal when create-dev-tag skipped (check-run)

Pre-release workflow_run gate `conclusion == 'success'` (line 36) correctly
skips `create-dev-tag` when CI is red. But the only existing signal was a
commit-comment from `notify-skipped` — invisible to maintainers who don't
subscribe to commit-comments. The workflow itself was marked 'success'
because the single executed job (`notify-skipped`) returned 0.

Add a `check-run` (conclusion=skipped, but title starts with ⚠️ and summary
explains the skip) on the CI head SHA. This surfaces the silent skip in the
Actions UI commit checks list and in the PR checks rollup, so a maintainer
sees "Pre-release skipped dev-tag" without digging into Actions history.

Ref: CI5.md Промпт 4. Does NOT change the gate — the gate is correct
(see CI3.md [GITHUB-P12]).
```

**Acceptance:**
- На коммите с красным CI в списке checks появляется запись
  `⚠️ Dev-tag skipped: CI conclusion was failure` от `Pre-release (dev-tag)`.
- На коммите с зелёным CI (после Промпта 2) эта запись отсутствует — вместо
  неё `create-dev-tag` сам публикует релиз.

---

## Промпт 5. [СРЕДНИЙ] Локальный pre-push hook: `cargo fmt --check` + `clippy::correctness` до пуша в master

**Цель:** предотвратить повторение ситуации — red CI с 20 Nov по 23 Nov
был вызван коммитом, который локально можно было отловить до пуша.
CI3.md чинил `prerelease.yml`; CI5.md чинит красный CI. Этот промпт
закрывает третью линию обороны: не пускать такой коммит в `master` вообще.

**Контекст для агента:** в репозитории уже есть `.githooks/` каталог
(см. дерево репозитория). Использовать его, а не создавать хук в
`.git/hooks/` (который не под версионированием).

**Создать файл `.githooks/pre-push`:**
```bash
#!/bin/bash
# Pre-push guard: catch the two most common CI-red causes BEFORE the push
# lands on master and triggers Pre-release's workflow_run gate
# (CI5.md [GITHUB-P33]).
#
# Activated via: git config core.hooksPath .githooks
# (already set by repo's CONTRIBUTING.md, but verify on clone).

set -euo pipefail

# Only guard pushes to master / main / develop (the branches CI listens to).
ref_lines=$(cat)
while read -r local_ref local_sha remote_ref remote_sha; do
  case "$remote_ref" in
    refs/heads/master|refs/heads/main|refs/heads/develop) ;;
    *) continue ;;
  esac

  echo "[pre-push] Running fmt + clippy::correctness on $(pwd) before push to ${remote_ref#refs/heads/}..."

  # fmt drift — same command as ci.yml lint step.
  if ! cargo fmt --all -- --check; then
    echo "[pre-push] FAIL: cargo fmt drift detected."
    echo "[pre-push] Run \`cargo fmt --all\` and re-stage before pushing."
    exit 1
  fi

  # clippy correctness — same flag order as ci.yml lint step (Deviation 5:
  # -A warnings first, then -D clippy::correctness wins on overlap).
  if ! cargo clippy --workspace --all-targets -- -A warnings -D clippy::correctness; then
    echo "[pre-push] FAIL: clippy::correctness regression detected."
    echo "[pre-push] Fix the correctness issue (do NOT #[allow] it)."
    exit 1
  fi

  echo "[pre-push] OK: fmt + clippy::correctness clean. Pushing."
done <<< "$ref_lines"
exit 0
```

**Активация в `CONTRIBUTING.md`** (раздел «Setup»), добавить строку:
```bash
git config core.hooksPath .githooks
```

**Обновить `push_script.sh`** — текущий скрипт делает `git push --force`
на ветку `github-p09-pages` и НЕ активирует hooks. Добавить в начало:
```bash
# [GITHUB-P33] CI5 regression guard: activate repo hooks before push.
git config core.hooksPath .githooks
```
И убрать hardcoded Russian comment с инструкцией «вставить токен» —
он утечёт в публичный репозиторий. Использовать `gh auth login` вместо
`echo "Token: [вставить токен…]"`.

**Commit message:**
```
[GITHUB-P33] chore: pre-push hook for fmt + clippy::correctness (CI5 regression guard)

CI was red on master from 20 Nov (#240) through 23 Nov (#248), all on
preventable causes (cargo fmt drift / clippy::correctness regressions).
The Pre-release workflow_run gate `conclusion == 'success'` correctly
blocked dev-tag creation, but the maintainer didn't see the silent skip
until noticing "no new tag in 2 days" — see CI5.md.

Add .githooks/pre-push that runs the exact same `cargo fmt --check` and
`cargo clippy -- -A warnings -D clippy::correctness` commands as ci.yml's
lint job, but locally, before any push to master/main/develop lands on the
remote. This catches the 2 most common CI-red classes at the earliest
possible point.

Also tighten push_script.sh: set core.hooksPath, remove the hardcoded
"insert token" comment (leaks into public repo) — gh auth is the path.
```

**Acceptance:**
- `git config core.hooksPath .githooks` выполнен локально.
- `touch src/dummy.rs && echo "fn main(){let x=1;}" >> src/dummy.rs &&
  git add src/dummy.rs && git commit -m test && git push origin master`
  отклоняется pre-push хуком с сообщением `FAIL: cargo fmt drift detected`.
- После `cargo fmt --all` тот же push проходит.

---

## Порядок применения

```
Промпт 1 (диагностика CI #248)
        │
        ▼
Промпт 2 (применить fix — ветка A / B / C / D по результату Промпта 1)
        │
        ▼
Промпт 3 (верифицировать v3.0.0-dev.8 создан автоматически)
        │
        ▼  ← здесь исходный симптом «push есть, тега нет» уже устранён
Промпт 4 (loud-сигнал для будущих silent-skip'ов)
        │
        ▼
Промпт 5 (pre-push guard, чтобы CI не красел от fmt/clippy снова)
```

**Минимально достаточный набор для закрытия симптома:** Промпты 1 + 2 + 3.
Промпты 4 и 5 — превентивные, чтобы ситуация не повторилась на следующем
крупном UI-коммите.

---

## Известные ограничения этого документа

1. **Точный root cause красного CI неизвестен без логов run #248.** Промпт 1
   написан так, чтобы вытащить эти логи и классифицировать ошибку; Промпт 2
   даёт fix-рецепт на каждый из 4 возможных классов. Угадывать класс здесь
   было бы недобросовестно — нужен реальный лог.
2. **Все команды `gh` требуют авторизации** (`gh auth login` с scope
   `repo` + `workflow`). Без этого Промпт 1 не выполним.
3. **CI #248 мог быть запущен на коммите `60f248c` (snapshot 45), а не на
   `9d7ec0f`** — GitHub запускает workflow на HEAD на момент пуша, а
   `9d7ec0f` и `60f248c` оба от 23 Nov. Промпт 1 идентифицирует точный
   `head_sha` через `gh run list --json headSha`.
4. **Если CI #248 упал из-за flaky-теста** (а не реальной регрессии) —
   Промпт 2 ветка C всё равно применима: найти flaky-тест, добавить
   `--retries 2` (уже есть в `ci.yml`) или пометить `#[ignore]` с
   обоснованием. Не перезапускать run руками — это маскирует проблему.
