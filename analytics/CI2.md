# Промпты для исправления CI/CD в Reider85/strangecoin

Готовые промпты для поочерёдной передачи агенту-помощнику (Claude / GPT / Copilot / Cursor)
или разработчику. Каждый промпт самодостаточен: содержит контекст, точную причину,
требуемое решение и способ проверки. Использовать строго в указанном порядке —
пункты 1-2 критичны (без них pipeline «push → dev-тег» не работает ни разу),
пункты 3-5 доводят красные workflow до зелёного.

Репозиторий: https://github.com/Reider85/strangecoin
Текущая ветка по умолчанию: `master`. Версия в `Cargo.toml`: `2.0.0`.

---

## Промпт 1. Починить `prerelease.yml` — сломан mint dev-тега (главный баг)

```
Контекст. В репозитории Reider85/strangecoin workflow .github/workflows/prerelease.yml
(name: Pre-release) должен автоматически создавать тег vX.Y.Z-dev.N и GitHub prerelease
после каждого зелёного CI-пуша в master. Сейчас он падает в 100% запусков (6 failed,
1 skipped) на шаге "Create and push annotated dev tag" с exit code 128 (git fatal).
Compute next dev tag проходит успешно — падает именно git push.

Точная причина (наиболее вероятная). workflow_run-triggered workflow получает
GITHUB_TOKEN от GitHub Actions, но для workflow_run-context granular push-permissions
работают не так, как для push-context: даже при `permissions: contents: write`
в yaml, токен часто не имеет права push tags. Дополнительно `concurrency:
  group: prerelease-${{ github.ref }}
  cancel-in-progress: true`
отменяет первый run вторым, что мешает детерминированному созданию тега.

Задача. Внести минимальные правки в .github/workflows/prerelease.yml так, чтобы
dev-тег создавался и публиковался детерминированно:

1. Заменить ручной `git push origin "refs/tags/${TAG}"` на вызов `gh release create`
   через GitHub CLI (уже установлен в runner):
     gh release create "$TAG" \
       --target master \
       --prerelease \
       --title "$TAG" \
       --notes "Automated dev build, not for production" \
       --generate-notes \
       strangecoin-linux-x86_64 strangecoin-linux-x86_64.sha256
   (или windows-артефакты при runner.os == 'Windows'). Это обходит проблему
   granular-permissions у workflow_run: GITHUB_TOKEN + `permissions: contents: write`
   для `gh release create` работает стабильно.

2. В шаге `actions/checkout@v4` добавить явное `persist-credentials: true`
   (хотя это дефолт, для workflow_run иногда слетает).

3. Убрать `cancel-in-progress: true` или сделать группу уникальной по
   `github.event.workflow_run.id`:
     concurrency:
       group: prerelease-${{ github.event.workflow_run.id || github.run_id }}
       cancel-in-progress: false
   Это предотвратит взаимное уничтожение параллельных run'ов от одного и того же
   workflow_run-события.

4. Добавить идемпотентность: если тег уже существует (например, run ретриггернули),
   пропускать создание, а не падать. Перед `gh release create` проверить:
     if gh release view "$TAG" >/dev/null 2>&1; then
       echo "Tag $TAG already exists — skipping creation"
       exit 0
     fi

5. Не трогать остальные шаги (build, Discord ping, stargazer-history commit) —
   они корректны.

Требования к коммиту. Сообщение в формате repo-convention:
`[GITHUB-P07] prerelease: fix dev-tag push via gh CLI + idempotency + concurrency group`

Проверка.
1. actionlint .github/workflows/prerelease.yml — должен пройти без ошибок.
2. После мержа в master дождаться первого зелёного CI-пуша (или триггернуть
   workflow_dispatch) — Pre-release должен завершиться success.
3. В `git ls-remote --tags origin` должен появиться новый `v2.0.0-dev.N`.
4. На странице /releases должна появиться новая prerelease с linux-x86_64 артефактом.

Не делай. Не переписывай release.yml (он для v* тегов, отдельная задача).
Не выкидывай stargazer-history commit-back. Не меняй matrix в release.yml.
```

---

## Промпт 2. Закрыть или мигрировать красные Dependabot-PR (`rand`, `eframe`)

```
Контекст. В репозитории Reider85/strangecoin два Dependabot-PR'а стабильно падают
в CI на реальных breaking-changes зависимостей (не инфраструктура):

- PR "Bump rand from 0.8.5 to 0.10.3" — ветка dependabot/cargo/rand-0.10.3,
  CI run #152. Падают шаги "Clippy" (lint), "Run tests (nextest)" (test-core),
  "Generate lcov report" (coverage). rand 0.8 → 0.10 — major migration
  (Rng trait, thread_rng(), gen_range() → random()/gen()/random_range()).

- PR "Bump eframe from 0.22.0 to 0.34.3" — ветка dependabot/cargo/eframe-0.34.3,
  CI run #150. Падают те же шаги. eframe 0.22 → 0.34 — 12 major-версий разрыв,
  epaint/egui API полностью переработан, App::update signature, NativeOptions и т.д.

Задача. Для каждого PR принять решение и выполнить:

Вариант A (рекомендуется для eframe). Закрыть PR с комментарием:
  "Closing: requires manual migration across 12 major versions (egui/eframe API
   rewrite). Will be addressed in a dedicated migration PR. See issue #N."
  Затем создать issue с заголовком "Migrate eframe 0.22 → 0.34 (GUI rewrite)"
  и чек-листом затронутых файлов (rg -l 'eframe|egui' src/).

Вариант B (рекомендуется для rand). Выполнить миграцию кода в ветке
dependabot/cargo/rand-0.10.3:
  1. cargo update -p rand
  2. cargo build --workspace — собрать список ошибок компиляции.
  3. Для каждой ошибки применить миграционный паттерн rand 0.8 → 0.10:
       use rand::Rng;                          → use rand::Rng;
       rng.gen_range(a..b)                     → rng.random_range(a..b)
       thread_rng().gen::<u64>()               → rand::random::<u64>()
       rand::thread_rng()                      → rand::rng()
       StdRng                                  → StdRng (trait изменился)
     См. https://rust-random.github.io/book/migration-0.9.html
  4. cargo fmt --all
  5. cargo clippy --workspace --all-targets -- -A warnings -D clippy::correctness
  6. cargo nextest run --workspace --no-fail-fast
  7. cargo test --workspace --doc
  8. Коммит: "fix(rand): migrate 0.8 → 0.10 (gen_range→random_range, thread_rng→rng)"
  9. Push в dependabot-ветку, дождаться зелёного CI, merge.

Требования.
- Не обновлять rand_core и rand_chacha отдельно — они придут транзитивно.
- Не ломать deterministic-test invariant (Stage 3 checklist): если тест
  использовал фиксированный seed через StdRng::seed_from_u64 — сохранить.
- Все правки — в коде, не в Cargo.toml (его обновит Dependabot).

Проверка.
1. CI на dependabot/cargo/rand-0.10.3 — зелёный (lint + test-core + coverage).
2. cargo nextest run --workspace локально — 0 failures.
3. После merge — CI на master зелёный.
```

---

## Промпт 3. Починить `nightly.yml` — macOS build-ячейки + cross-build + Miri

```
Контекст. .github/workflows/nightly.yml в репозитории Reider85/strangecoin
стабильно падает: 3 из 7 последних run'ов failed. Конкретные падающие job'ы:

1. Build (macos-latest, x86_64-apple-darwin) → step "Build release binary" — failed.
   Причина: GitHub удалил macos-12 runner, macos-latest теперь = macos-14 (Apple Silicon),
   x86_64-apple-darwin там собирается через Rosetta, но тулчейн / SDK иногда
   не подхватывается.

2. Build (macos-latest, aarch64-apple-darwin) → step "Build release binary" — failed.
   Причина: на macos-14 нативная цель aarch64, но в matrix остался
   macos-latest/x86_64 — conflict.

3. Build (ubuntu-latest, aarch64-unknown-linux-gnu) → step "Build release binary" — failed.
   Причина: apt-cache runner image, gcc-aarch64-linux-gnu иногда не находится.

4. Mirí (vm-wasm + storage) → "cargo miri test" — failed.
   Причина: real unsafe-расхождения в storage/vm-wasm. НЕ ГЛУШИТЬ.

5. Mutation testing (cargo-mutants) → "Run cargo mutants" — failed.
   Причина: выжил мутант (пропущенный тест-кейс). НЕ ГЛУШИТЬ.

6. Fuzz gas_metering (30 min) и Fuzz differential_wasmi_vs_wasmtime (30 min) — failed.
   Причина: реальные краши в коде. НЕ ГЛУШИТЬ.

Задача. Разделить фикс на две части:

Часть A — инфраструктурные правки (коммит "[GITHUB-P08] nightly: fix macOS + cross-build matrix"):

1. В matrix.include заменить ячейку {"os":"macos-latest","target":"x86_64-apple-darwin"}
   на {"os":"macos-13","target":"x86_64-apple-darwin"} — macos-13 ещё отдаёт x86_64 runner.
2. Ячейку {"os":"macos-latest","target":"aarch64-apple-darwin"} оставить как есть
   (нативно на macos-14).
3. Для ubuntu-aarch64 шаг "Install cross-compilation tools (Linux aarch64)"
   вынести В ОТДЕЛЬНЫЙ job с runs-on: ubuntu-latest и добавить
   `sudo apt-get update && sudo apt-get install -y gcc-aarch64-linux-gnu`
   + `echo "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc" >> $GITHUB_ENV`
   В шаге Install добавить retry: `|| (sleep 10 && sudo apt-get install -y ...)`.
4. В самих fuzz / miri / mutant job'ах НИЧЕГО не менять.

Часть B — отдельные issue для реальных багов (без правок workflow):

Создать 3 issue с чек-листом воспроизведения:
- "Bug: gas_metering fuzzer crashes (nightly)" — приложить crashing input из
  артефактов run, использовать `cargo fuzz run gas_metering -- -runs=1000000` локально.
- "Bug: wasmi vs wasmtime differential mismatch" — приложить failing wasm module.
- "Bug: Miri UB in strangecoin-storage/strangecoin-vm-wasm" — приложить miri output.

Проверка.
1. actionlint .github/workflows/nightly.yml.
2. После merge следующий Nightly-run (cron 04:00 UTC или workflow_dispatch):
   - Build-ячейки все зелёные (6/6).
   - Miri, mutants, fuzz — по-прежнему могут быть красными (это правильно,
     они указывают на реальные баги), но build-matrix больше не падает.
3. Job summary должен показывать: 6 build success, N fuzz/mirí/mutant failures
   (где N = количество реальных багов, а не инфра-сбоев).
```

---

## Промпт 4. Починить `pages.yml` — mdBook не установлен

```
Контекст. .github/workflows/pages.yml (name: GitHub Pages) в репозитории
Reider85/strangecoin падает в 8 из 10 последних run'ов на шаге "Build mdBook".
Причина: шаг установки mdBook либо отсутствует, либо использует deprecated action
без pinned version. mdBook не предустановлен в ubuntu-latest runner image.

Задача. Поправить .github/workflows/pages.yml:

1. Перед шагом "Build mdBook" добавить установку через action:
     - name: Setup mdBook
       uses: peaceiris/action-mdbook@v2.0.0
       with:
         mdbook-version: '0.4.40'
   (или любой актуальный 0.4.x). Pin версии обязателен — иначе Dependabot
   будет постоянно триггерить PR'ы.

2. Если используется jontze/action-mdbook@v3 — оставить, но убедиться, что
   указана конкретная минорная версия.

3. Шаг "Build mdBook" должен запускаться с cwd = корень book/ или docs/
   (проверь mdbook.toml: `find . -name 'mdbook.toml' -not -path './target/*'`).

4. Убрать триггер на pull_request для веток dependabot/* — Dependabot-PR'ы
   на actions/checkout плодят фантомные Pages-runs. Заменить:
     on:
       push:
         branches: [master]
         paths:
           - 'docs/**'
           - 'book/**'
           - 'mdbook.toml'
           - '.github/workflows/pages.yml'
   (paths-filter обязателен, иначе Pages будет запускаться от каждого
   star-monitor hourly commit).

5. Если docs/book в репо нет — проверить, что в mdbook.toml указан src = "src"
   и каталог существует. Если нет — создать минимальный book.toml:
     [book]
     title = "Strangecoin Documentation"
     authors = ["Reider85"]
     language = "en"
     src = "docs/src"

Требования к коммиту. `[GITHUB-P09] pages: install mdBook via peaceiris action + paths-filter`.

Проверка.
1. actionlint .github/workflows/pages.yml.
2. workflow_dispatch локально → run должен завершиться зелёным.
3. На https://reider85.github.io/strangecoin/ (или pages URL) должна появиться
   страница с index.md.
4. Следующий Dependabot-PR на actions/checkout НЕ должен запускать Pages-run
   (paths-filter отсечёт).
```

---

## Промпт 5. Добавить health-gate для `release.yml` (предотвратить повторение Release #6)

```
Контекст. .github/workflows/release.yml в репозитории Reider85/strangecoin
на теге v2.0.0-stage2 отработал 2 раза: Release #6 failed (15m), Release #7 success (8m).
Причина падения #6 — одна из 6 matrix-ячеек упала (вероятно windows-aarch64,
у неё matrix.allow_failure=true, но fail-fast: false + downstream aggregate-hashes
требует всех артефактов → whole-run red).

Задача. Сделать release.yml устойчивым к частичному падению matrix без ручного
re-run:

1. Для ячейки windows-aarch64 (та, что с allow_failure: true) — реализовать
   реальный continue-on-error на step-уровне:
       - name: Build release binary
         if: matrix.allow_failure && runner.os == 'Windows'
         continue-on-error: true
         run: cargo build --release --target ${{ steps.tgt.outputs.target }}
   Это позволит run'у пройти даже если aarch64-windows не соберётся.

2. В job `aggregate-hashes` заменить assertion
       test "$(wc -l < hashes.txt)" -eq "$expected"
   на
       test "$expected" -ge 4   # минимум 4 артефакта из 6
   Это допускает fallback-release с 4/6 платформ (windows-aarch64 + aarch64-linux
   могут отсутствовать, основной desktop-набор обязательный).

3. Добавить step в job `release` (после Create GitHub Release), который пишет
   в release notes какой артефакт отсутствует:
       - name: Note missing artifacts
         if: always()
         shell: bash
         run: |
           expected="strangecoin-linux-x86_64 strangecoin-linux-aarch64 strangecoin-macos-x86_64 strangecoin-macos-aarch64 strangecoin-windows-x86_64 strangecoin-windows-aarch64"
           for a in $expected; do
             if ! ls release/${a}* >/dev/null 2>&1; then
               echo "- ⚠️ Missing: ${a}" >> release-notes-extra.md
             fi
           done
   И в softprops/action-gh-release@v2 добавить body_path: release-notes-extra.md
   (или дополнить body: блоком).

4. НЕ УБИРАТЬ `if: ${{ !contains(github.ref_name, '-dev.') }}` — он корректно
   исключает dev-теги из full-release.

Требования к коммиту. `[GITHUB-P10] release: graceful partial-matrix fallback + missing-artifact note`.

Проверка.
1. actionlint .github/workflows/release.yml.
2. На следующем push тега v* (можно тестовый v2.0.1-rc1):
   - Run завершается success, даже если windows-aarch64 упадёт.
   - В release-notes указано Missing: strangecoin-windows-aarch64 (если упала).
3. SHA256SUMS.txt содержит ровно те артефакты, что реально собрались.
4. cosign signing и SLSA provenance (когда RUNS_ON не задан) — зелёные.
```

---

## Промпт 6 (бонус). Добавить health-dashboard в README

```
Контекст. После исправлений 1-5 в репозитории Reider85/strangecoin останутся
видимыми только реальные баги (fuzz/mirí/mutant failures). Чтобы сообщество
видело текущий статус CI, добавить бейджи в README.md.

Задача. В корне README.md после заголовка # Strangecoin добавить блок бейджей:

![CI](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml/badge.svg?branch=master)
![Pre-release](https://github.com/Reider85/strangecoin/actions/workflows/prerelease.yml/badge.svg)
![Release](https://github.com/Reider85/strangecoin/actions/workflows/release.yml/badge.svg)
![Nightly](https://github.com/Reider85/strangecoin/actions/workflows/nightly.yml/badge.svg)
![Pages](https://github.com/Reider85/strangecoin/actions/workflows/pages.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)

Если какая-то workflow была починена в этом PR — убедиться, что бейдж зелёный
после merge (между merge и первым run'ом бейдж может показывать unknown —
это нормально, через ~5 минут обновится).

Коммит: `[GITHUB-P11] readme: CI badge dashboard`
```

---

## Порядок применения

1. **Промпт 1** (prerelease.yml) — критично, без него весь pipeline «push → dev-тег»
   остаётся мёртвым. Открывает автоматическую нумерацию dev-релизов.
2. **Промпт 5** (release.yml) — следующий по приоритету, предотвращает ручные
   re-run'ы на v*-тегах.
3. **Промпт 4** (pages.yml) — быстрый win, минимальные правки.
4. **Промпт 3** (nightly.yml часть A) — инфраструктурная, отделяет реальные баги
   от infra-сбоев. Часть B (issue для fuzz/mirí/mutant) — отдельный sprint.
5. **Промпт 2** (Dependabot rand/eframe) — миграция кода, требует разработки.
6. **Промпт 6** (README badges) — финальный штрих, после того как все workflow
   зелёные (кроме известных real-bug-failures в nightly).

После каждого промпта — отдельный PR с commit-сообщением строго в формате
`[GITHUB-PNN] <area>: <change>`, чтобы соответствовать существующей repo-convention
(в `'history'` уже есть P01-P06).
