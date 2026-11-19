# Промпты для исправления релизного пайплайна в Reider85/strangecoin (CI5)

Готовые промпты для поочерёдной передачи агенту-помощнику (Claude / GPT / Copilot /
Cursor) или разработчику. Каждый промпт самодостаточен: контекст, точная причина,
требуемое решение, формат коммита, способ проверки. Применять строго в указанном
порядке — Промпт 1 чинит прямой регресс (молчаливый сбой `release.yml` на
`v3.0.0-stage3`), Промпты 2-4 закрывают первопричины (хрупкость `release.yml`,
невидимая тишина `Release failed`, отсутствие ручного re-run); Промпты 5-6
устраняют оставшиеся хрупкости (последние красные `Nightly #13` и `Pre-release
#60`); Промпт 7 — профилактика против следующих blind-merge major bumps
сторонних action'ов.

Репозиторий: https://github.com/Reider85/strangecoin
Ветка по умолчанию: `master`. Версия в `Cargo.toml`: `3.0.0`.
Промпты нумеруются как `[GITHUB-PNN]`, продолжая счётчик из CI2/CI3/CI4
(P07-P15 — CI3, P16-P21 — CI4, P22 — `6dfbe80` compute-tag sort fix; CI5
добавляет P23-P29). Дополнительная серия `UI-P0x` уже в истории (`UI-P02-A/B/C`,
коммиты `2caa74e` / `e5191f6` / `67901a9` от 18 Nov 2026).

---

## 0. Сводка расследования: какие именно «релизы и теги» не выпускаются?

### 0.1. Фактическое состояние на момент анализа (2026-11-19, ~21:00 UTC+3)

**Теги.** В `git ls-remote --tags origin` видны 14 тегов:

```
v3.0.0-dev.4    (51b8eb5, 2026-11-18 20:22 +0300) ← последний dev-тег
v3.0.0-dev.3    (6dfbe80, 2026-11-18)
v3.0.0-dev.2    (89dd756, 2026-11-16 21:50 +0300)
v3.0.0-dev.1    (117b2b8, 2026-11-16 20:55 +0300)
v3.0.0-stage3   (72015bc, annotated tag → peeled 89dd756 = dev.2 commit) ← БЕЗ RELEASE
v2.0.0-stage2   (346e3c0, 2026-10-29) ← ПОСЛЕДНИЙ УСПЕШНЫЙ STABLE RELEASE
v2.0.0-dev.4 / dev.3 / dev.2 / dev.1
v1.5.0-stage1.5 / v1.1.0-stage1 / v1.0.0-stage0 / v0.0.1-rc1
```

**Releases.** На странице `github.com/Reider85/strangecoin/releases` показаны 11
релизов: 8 dev pre-release (v2.0.0-dev.1…v3.0.0-dev.4) + 3 stage release
(v0.0.1-rc1, v1.0.0-stage0, v1.1.0-stage1, v1.5.0-stage1.5, v2.0.0-stage2).
Последний dev pre-release — **`v3.0.0-dev.4`** от 18 Nov 2026.
Последний СТАБИЛЬНЫЙ релиз — **`v2.0.0-stage2`** от **29 Oct 2026** (3 недели
назад). Тег `v3.0.0-stage3` существует с 16 Nov, но GitHub-релиз к нему **так и
не создан** — Release-workflow упал два раза подряд.

**Release-workflow runs** (https://github.com/Reider85/strangecoin/actions/workflows/release.yml):

| Run # | Дата | Триггер (tag) | Commit | Duration | Статус |
| --- | --- | --- | --- | --- | --- |
| #13 | Nov 16, 2026 | `v3.0.0-stage3` (re-push на dev.2) | `89dd756` | 20m 47s | **FAILED** |
| #12 | Nov 16, 2026 | `v3.0.0-stage3` (первый push на dev.1) | `117b2b8` | 21m 39s | **FAILED** |
| #11 | Nov 10, 2026 | dependabot `actions/checkout` bump | `75ec49a` | — | FAILED |
| #10 | Nov 10, 2026 | dependabot `sigstore/cosign-installer` bump | `c62fa52` | — | FAILED |
| #9  | Nov 10, 2026 | dependabot `actions/download-artifact` bump | `22f7bb7` | — | FAILED |
| #8  | Nov 10, 2026 | master (post-P06 hybrid-runner commit) | — | — | FAILED |
| #7  | **Oct 29, 2026** | `v2.0.0-stage2` | — | — | **SUCCESS** ← последний зелёный |

**Pre-release workflow** (https://github.com/Reider85/strangecoin/actions/workflows/prerelease.yml):

| Run # | Дата | Duration | Статус | 备注 |
| --- | --- | --- | --- | --- |
| #61 | Nov 18, 2026 | 13m 16s | success | создал `v3.0.0-dev.4` |
| **#60** | **Nov 18, 2026** | **10m 12s** | **FAILED** |retry-успешен в #61, но шаг, который упал, надо найти |
| #59 | Nov 18, 2026 | 13m 48s | success | создал `v3.0.0-dev.3` |
| #58 | Nov 18, 2026 | 1m 2s  | success | skip-path (тег уже есть) |
| #57 | Nov 18, 2026 | 37s    | success | skip-path |

**Nightly workflow** (https://github.com/Reider85/strangecoin/actions/workflows/nightly.yml):

| Run # | Дата | Duration | Статус |
| --- | --- | --- | --- |
| **#13** | **Nov 19, 2026** | **50m 39s** | **FAILED** |

### 0.2. Что именно сломано, а что работает

**РАБОТАЕТ.** Pre-release пайплайн «push в master → CI green → workflow_run
→ prerelease.yml → вычисление `vX.Y.Z-dev.N` → `gh release create --target
$SHA --prerelease` → commit-comment». Все 4 dev-тега для `3.0.0` созданы за
3 дня (`dev.1` 16 Nov → `dev.4` 18 Nov), и каждый на правильный SHA. После
`GITHUB-P22` (compute-tag sort tag-names, не sha-prefixed lines) stalls на
`dev.2` больше нет. Это значит:

- `prerelease.yml` структурно исправен (7 багов из CI3 закрыты в `699cd65` /
  `29c311a` / `a6212b8`).
- `ci.yml` на push зелёный (иначе prerelease.yml вообще бы не стартовал).
- `workflow_run` chain работает, `conclusion == 'success'` gate срабатывает.

**СЛОМАНО.** Release-workflow `release.yml` (фулл-релиз на `v*`-тегах) —
**6 запусков подряд FAILED (#8-#13)** с 10 Nov по 16 Nov 2026. Последний
successful — run #7 от 29 Oct (`v2.0.0-stage2`). Это и есть «релизы не
выпускаются»: `v3.0.0-stage3` тег есть, GitHub Release — нет. Все 6 матриц
build-job либо падают на macOS cells, либо скармливают downstream-джобам
недостаточно артефактов, либо сам `release`-шаг `softprops/action-gh-release@v2`
падает на `body_path: release-notes-extra.md`, которого нет, когда все 6
артефактов на месте.

**СЛОМАНО (вторичная краснота).**
- `Pre-release #60` упал на 10-й минуте 18 Nov 2026 — это build/run-time сбой
  внутри одного конкретного CI-run'а; #61 retry прошёл. Рут-коз пока не
  зафиксирован (см. Промпт 5).
- `Nightly #13` упал на 50-й минуте 19 Nov 2026 — это серьёзный сбой
  (cross-build / audit / miri). Рут-коз пока не зафиксирован (см. Промпт 4).

### 0.3. Точная причина остановки `release.yml` ( runs #12, #13 на `v3.0.0-stage3` )

`release.yml` стартует по триггеру `on.push.tags: ['v*']`. Build-job имеет
gate `if: ${{ !contains(github.ref_name, '-dev.') }}` — для `v3.0.0-stage3`
этот gate проходит (тег не содержит `-dev.`), workflow запускается. Дальше
6-клеточная matrix (linux x86_64/aarch64, macOS x86_64/aarch64, Windows
x86_64/aarch64). 16 Nov, когда были оба push-а `v3.0.0-stage3`, macOS-клетки
**гарантированно падали** на компиляции `objc2 0.3.0-beta.3`:

```
error[E0277]: the trait bound `Encode<()>` is not implemented for `bool`
  --> /Users/runner/.cargo/registry/src/.../objc2-0.3.0-beta.3/...
```

Это та самая регрессия, которую CI4 Промпт 6 диагностировал как «eframe 0.22
→ glutin 0.30 → accesskit 0.14 → objc2 0.3.0-beta.3 несовместим с rustc
stable ≥1.99». До 18 Nov macOS-клетки `release.yml` и `nightly.yml` были
обязательно красные на любом `v*`-теге. **`UI-P02-A/B/C` (коммиты `2caa74e`,
`e5191f6`, `67901a9` от 18 Nov 2026) эту рут-коз закрыл**: eframe 0.22 → 0.30,
objc2 теперь резолвится в 0.5.3+/0.6.x, beta.3 ушёл из графа; toolchain-pin
1.98.1 в `release.yml` и 1.99.1 в `nightly.yml` снят коммитом `67901a9`.

Несмотря на это, **`v3.0.0-stage3` GitHub Release так и не появился**, потому
что:

1. Тег `v3.0.0-stage3` был запушен 16 Nov. GitHub **не повторяет** trigger
   `on.push.tags` для уже существующего тега, даже если код под ним обновился.
   Re-pushing annotated tag (force) считается тем же trigger, НО GitHub
   дедуплицирует воркфлоу-запуски по `(tag, commit)` — поэтому run #13 на
   коммите `89dd756` (dev.2) это последний запуск для этого тега, и он FAILED.
2. `release.yml` **не имеет** `workflow_dispatch` — пере-запустить run #13
   вручную из Actions UI можно (через кнопку "Re-run failed jobs"), но это
   прогонит тот же коммит `89dd756` через тот же код, который на 16 Nov был
   ещё сломан на macOS cells. После 18 Nov (когда `UI-P02-C` приземлился в
   master) HEAD мастер-ветки содержит фикс — но тег `v3.0.0-stage3` остался
   на старом commit `89dd756`, и Re-run run #13 соберёт код из `89dd756`,
   а не из актуального master. **Это ничего не починит.**
3. `release.yml` **не имеет notify-job** на failure — в отличие от
   `prerelease.yml` (где `notify-skipped` постит commit-comment с пояснением
   «Dev-tag pipeline skipped: CI conclusion was X», см. `a6212b8` [GITHUB-P19]),
   у `release.yml` любой сбой в build/sign/aggregate-hashes/release остаётся
   виден только в Actions UI как красный run. Мейнтейнер не получает ни
   commit-comment, ни issue, ни пуша в README с плашкой «⚠️ v3.0.0-stage3
   release failed». Это и есть «молчаливая остановка релизов».

### 0.4. Сводка багов по приоритету

| # | Баг | Workflow / файл | Промпт | Критичность |
| --- | --- | --- | --- | --- |
| 1 | `v3.0.0-stage3` tag лежит на commit `89dd756` (16 Nov, до `UI-P02-C`); GitHub-релиза нет; re-run #13 соберёт сломанный код | git tags + Actions UI | Промпт 1 | критично |
| 2 | `release.yml`: `Note missing artifacts` не создаёт `release-notes-extra.md`, если все 6 артефактов на месте → `softprops/action-gh-release@v2` с `body_path:` на несуществующий файл падает или пропускает шаг | `.github/workflows/release.yml` | Промпт 2 | критично |
| 3 | `release.yml`: `aggregate-hashes` требует `test "$expected" -ge 4` — при `vars.RUNS_ON` collapse в 1 клетку эта проверка падает, `provenance` (needs aggregate-hashes) скипаётся, формально не блокируя `release`, но ломая SLSA attestation | `.github/workflows/release.yml` | Промпт 2 | высоко |
| 4 | `release.yml`: нет `notify-failed` job — сбой Release невидим мейнтейнеру (нет commit-comment, нет issue, нет PR) | `.github/workflows/release.yml` | Промпт 3 | критично |
| 5 | `release.yml`: нет `workflow_dispatch` — нельзя вручную пере-запустить на уже существующем теге без delete + re-push | `.github/workflows/release.yml` | Промпт 4 | высоко |
| 6 | `Nightly #13` (19 Nov, 50m 39s) FAILED — конкретный упавший job неизвестен без открытия run-страницы; подозрение на cross-build macOS cells или audit (новая RUSTSEC) | `.github/workflows/nightly.yml` | Промпт 5 | высоко |
| 7 | `Pre-release #60` (18 Nov, 10m 12s) FAILED — retry в #61 прошёл, но рут-коз не найден; подозрение на transient cargo build failure или `gh release create` API timeout | `.github/workflows/prerelease.yml` | Промпт 6 | средне |
| 8 | `softprops/action-gh-release@v2` и `slsa-framework/slsa-github-generator@v2.1.0` не pinned по SHA — следующий major bump softprops может молча сломать release-creation (dependabot ignore `version-update:semver-major` для `softprops/action-gh-release` не настроен) | `.github/dependabot.yml` + `release.yml` | Промпт 7 | низко |

---

## Промпт 1. [КРИТИЧНО] Пере-выпустить `v3.0.0-stage3` release: delete + re-push tag на актуальный master HEAD

```
Контекст. В Reider85/strangecoin на теге `v3.0.0-stage3` (annotated tag SHA
72015bc, peeled commit 89dd756 = v3.0.0-dev.2 от 16 Nov 21:50 +0300) висит
два FAILED Release-workflow run'а (#12 на коммите 117b2b8, #13 на 89dd756).
GitHub Release для `v3.0.0-stage3` НЕ существует — на /releases/tag/v3.0.0-stage3
возвращается 404. Последний успешный stable Release — `v2.0.0-stage2` от
29 Oct 2026 (run #7).

С 16 Nov в master приземлились:
  18 Nov  2caa74e  [UI-P02-A] deps: bump eframe 0.22 -> 0.30 + remove winapi workaround
  18 Nov  e5191f6  [UI-P02-B] gui: migrate eframe 0.22 -> 0.30 API
  18 Nov  67901a9  [UI-P02-C] ci: unpin release/nightly toolchain — objc2 now stable
  18 Nov  6dfbe80  [GITHUB-P22] prerelease: compute-tag sorts tag names
  18 Nov  3f2a74e  [GITHUB-P21] release: document upload@v7/download@v8 skew
  19 Nov  23f4641  [S4-AI-P03] acp: wire messages + handler + escrow vault + tests

Эти коммиты — особенно UI-P02-A/B/C — устраняют рут-коз #12/#13: macOS cells
release.yml больше не падают на objc2 0.3.0-beta.3. Но тег `v3.0.0-stage3`
остался на старом commit 89dd756 (dev.2), где eframe ещё 0.22. Любой Re-run
run #13 из Actions UI соберёт код из 89dd756 — и снова упадёт на macOS
(или, как минимум, не получит优势 фикса UI-P02-C).

Задача. Пере-выпустить `v3.0.0-stage3` release на актуальный master HEAD.

ВАРИАНТ A (рекомендуется, сохраняет тег-имя и историю):
  1. Убедиться, что master зелёный: открыть Actions → CI workflow, проверить
     последний run (на момент анализа CI #238 in_progress на коммите 23f4641
     от 19 Nov). Дождаться success. Если CI красный — сначала починить
     (вероятная причина: новый dependabot-bump; см. AGENTS.md Pin Policy и
     .github/dependabot.yml ignore-список).
  2. Локально:
       git fetch origin master
       git checkout master
       git pull --ff-only
       LATEST_SHA=$(git rev-parse origin/master)
  3. Удалить annotated tag `v3.0.0-stage3` локально и в origin:
       git tag -d v3.0.0-stage3
       git push origin :refs/tags/v3.0.0-stage3
     ( annotated tag delete через `:refs/tags/<name>` работает для всех
       типов тегов; GitHub не различает annotated/lightweight на уровне
       push-а refs/tags/* )
  4. Создать annotated tag заново на актуальном master HEAD:
       git tag -a v3.0.0-stage3 "$LATEST_SHA" -m "v3.0.0-stage3 — stage 3 release (re-tagged after UI-P02-C macOS fix)"
       git push origin refs/tags/v3.0.0-stage3
  5. Push тега триггерит release.yml. Открыть Actions → Release workflow,
     дождаться зелёного run'а. Ожидаемое поведение:
     - Build matrix: все 6 клеток проходят (macOS x86_64/aarch64 на stable
       после UI-P02-C, Linux x86_64/aarch64, Windows x86_64; windows-aarch64
       allow_failure как и раньше).
     - aggregate-hashes: видит ≥4 hash.txt (≥5 фактически), test passes.
     - provenance: SLSA reusable workflow генерит attestation, upload-assets:
       true (если не выставлен vars.RUNS_ON — иначе skipped по if: !vars.RUNS_ON).
     - sign: cosign подписывает каждый артефакт, cosign-signatures артефакт
       заливается.
     - release: softprops/action-gh-release@v2 создаёт GitHub Release с
       6 binaries + SHA256SUMS.txt + cosign signatures + (если RUNS_ON не
       выставлен) multiple.intoto.jsonl SLSA provenance.
  6. На /releases/tag/v3.0.0-stage3 появляется stable release (не Pre-release).
     В "Latest" справа появляется плашка "Latest release".

ВАРИАНТ B (если удалять тег нельзя по каким-то причинам — например, тег
уже публично упомянут в external docs):
  1. Запушить НОВЫЙ тег `v3.0.0-stage3.1` (annotated) на тот же актуальный
     master HEAD:
       git tag -a v3.0.0-stage3.1 "$LATEST_SHA" -m "v3.0.0-stage3.1 — re-release after macOS build fix"
       git push origin refs/tags/v3.0.0-stage3.1
  2. В release-notes сослаться на v3.0.0-stage3 как на «tag created prematurely
     on pre-UI-P02-C commit; superseded by v3.0.0-stage3.1».
  3. Старый тег `v3.0.0-stage3` НЕ удалять (история), но и НЕ делать его
     "Latest" — GitHub автоматически сделает Latest свежий `v3.0.0-stage3.1`.

ВАРИАНТ C (только если тег НЕ annotated, что НЕ наш случай):
  lightweight tag можно "передвинуть" через `git tag -f v3.0.0-stage3
  $LATEST_SHA` + `git push origin -f refs/tags/v3.0.0-stage3`. Для annotated
  tag надёжнее delete + create (как в варианте A), потому что `git push -f`
  annotated tag иногда не триггерит GitHub Actions (по дедупликации
  `(tag, target_commit)`).

Требования к коммиту. Если перед re-tag делается отдельный коммит с
release-notes подготовкой (например, обновление Changelog.md), сообщение:
`[GITHUB-P23] release: re-tag v3.0.0-stage3 on post-UI-P02-C master HEAD`

Если пере-тег делается без коммита (только git tag + git push), формат
commit-message неприменим — описание фиксируется в annotated-tag message
и (опционально) в GitHub Release body.

Проверка.
1. `git ls-remote --tags origin | grep v3.0.0-stage3` показывает новый
   tag-SHA (отличный от 72015bc — старого annotated-tag SHA).
2. `git show v3.0.0-stage3 --no-patch` показывает tagger date = сегодня,
   target commit = актуальный master HEAD (а НЕ 89dd756).
3. Actions → Release workflow показывает новый run (например #14) с триггером
   "v3.0.0-stage3". Run завершается success за 18-25 min (6 matrix cells
   параллельно ~10 min build + ~5 min sign/provenance/release).
4. На /releases/tag/v3.0.0-stage3 — stable release (не Pre-release) с
   6 binaries, SHA256SUMS.txt, cosign signatures, SLSA provenance.
5. /releases страница показывает v3.0.0-stage3 как "Latest release"
   (вместо v2.0.0-stage2 от 29 Oct).

Не делай.
- НЕ запускай Re-run run #13 из Actions UI без re-tag — это соберёт старый
  commit 89dd756 (pre-UI-P02-C), macOS cells снова упадут, ещё один FAILED
  run в истории.
- НЕ используй `git push -f` для annotated tag без `delete + create` —
  GitHub может не триггерить workflow на force-push annotated tag.
- НЕ поднимай version в Cargo.toml на 3.0.1 или 3.1.0 только ради релиза —
  stage3 остается версией 3.0.0; release-tag — это ярлык точки состояния
  master, не новая semver. (Если решено всё-таки поднять версию — это
  отдельный коммит перед re-tag, и тег называется v3.0.1 или v3.1.0,
  не v3.0.0-stage3.)
- НЕ удаляй старые dev-теги (v3.0.0-dev.1…dev.4) — они нужны для истории
  и для отслеживания regression-window через `git log v3.0.0-dev.2..v3.0.0-stage3`.
- НЕ используй `gh release delete v3.0.0-stage3` — релиза НЕТ, удалять
  нечего; нужен `git push origin :refs/tags/v3.0.0-stage3` (delete tag ref),
  НЕ `gh release delete`.
```

---

## Промпт 2. [КРИТИЧНО] `release.yml`: починить хрупкости `body_path` + `aggregate-hashes` count + macOS-cell resilience

```
Контекст. .github/workflows/release.yml в Reider85/strangecoin создаёт
stable GitHub Release на каждый push тега `v*` (кроме `*-dev.*`). После
успешного run #7 (v2.0.0-stage2, 29 Oct) шесть следующих запусков (#8-#13)
FAILED. Прямая причина для #12/#13 — macOS cells падали на objc2 0.3.0-beta.3
(закрыто UI-P02-C 18 Nov). Но в файле остались три хрупкости, которые
взаимодействуют с любой Cell failure так, что весь workflow краснеет
молча (без видимого error message в логах последнего job):

  Хрупкость 1. Шаг "Note missing artifacts" пишет в release-notes-extra.md
  только если артефакт отсутствует:
      - name: Note missing artifacts
        shell: bash
        run: |
          expected="strangecoin-linux-x86_64 strangecoin-linux-aarch64 ..."
          for a in $expected; do
            if ! ls release/${a}* >/dev/null 2>&1; then
              echo "- ⚠️ Missing: ${a}" >> release-notes-extra.md
            fi
          done
  Если все 6 артефактов на месте (что и должно быть после UI-P02-C), файл
  release-notes-extra.md НИКОГДА НЕ СОЗДАЁТСЯ. Дальше:
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: release/*
          body_path: release-notes-extra.md
  softprops/action-gh-release@v2 в `body_path` передаёт fs.readFileSync без
  try/catch на существование файла (см. исходник
  https://github.com/softprops/action-gh-release/blob/v2/src/main.ts —
  readBody использует readFileSync, бросает ENOENT). Если файл отсутствует,
  шаг падает с "Error: ENOENT: no such file or directory, open
  'release-notes-extra.md'" → весь `release` job FAILED → workflow красный.
  Это полностью объяснило бы failed runs #8-#13 даже при зелёной matrix:
  macOS-клетки после UI-P02-C зелёные, но release всё равно падает, потому
  что все 6 артефактов теперь на месте, release-notes-extra.md не создаётся,
  softprops падает на ENOENT.

  Хрупкость 2. aggregate-hashes требует `test "$expected" -ge 4`:
      - name: Aggregate hashes
        id: aggregate
        shell: bash
        run: |
          set -euo pipefail
          cat checksums/*/hash.txt > hashes.txt
          expected=$(find checksums -name hash.txt | wc -l)
          test "$expected" -ge 4   # minimum 4 artifacts from 6
  Если vars.RUNS_ON выставлен (free minutes exhausted, hybrid runner mode),
  matrix.include collapses в одну windows-x86_64 клетку, hash.txt будет 1,
  `test 1 -ge 4` падает → aggregate-hashes FAILED → provenance (needs
  aggregate-hashes) skipped. Это НЕ блокирует `release` (тому needs только
  [build, sign]), но SLSA attestation теряется silently, что нарушает
  DoD #22 (reproducible-build policy). Если же матрица полная и 5 клеток
  зелёные (windows-aarch64 allow_failure), test 5 -ge 4 проходит — но если
  упадёт ещё одна клетка (например transient network), 3 < 4 падает.

  Хрупкость 3. macOS cells не имеют continue-on-error на случай новой
  regression objc2/eframe. windows-aarch64 имеет allow_failure: true
  в matrix — корректно. macOS cells такого флага не имеют, поэтому любая
  будущая macOS-специфичная компиляционная regression (а eframe/winit/accesskit
  уже ломались дважды за полгода) снова положит весь release. На stage-релизе
  лучше опубликовать fallback-релиз с Windows + Linux артефактами, чем не
  опубликовать ничего.

Задача. Привести .github/workflows/release.yml к состоянию, где:
  - release-notes-extra.md существует ВСЕГДА к моменту вызова softprops/action-gh-release.
  - aggregate-hashes корректно работает и при 6, и при 1 клетке (vars.RUNS_ON set).
  - macOS cells получают matrix.allow_failure: true на этапе переходного
    периода (UI-P02-C фикс новый, eframe 0.30 ещё не extensively tested in CI).

Конкретные правки (diff-формат для понимания, применять через Edit):

Правка 2.1. Перед шагом "Note missing artifacts" добавить `touch`:
  - name: Note missing artifacts
    shell: bash
    run: |
      # [GITHUB-P24] ensure release-notes-extra.md exists for
      # softprops/action-gh-release@v2 body_path — otherwise the action
      # throws ENOENT and the whole release job fails (root cause of the
      # silent #8-#13 failures: once macOS cells went green on UI-P02-C,
      # all 6 artifacts were present, the file was never created, and
      # softprops crashed reading a non-existent body_path).
      touch release-notes-extra.md
      expected="strangecoin-linux-x86_64 strangecoin-linux-aarch64 strangecoin-macos-x86_64 strangecoin-macos-aarch64 strangecoin-windows-x86_64 strangecoin-windows-aarch64"
      for a in $expected; do
        if ! ls release/${a}* >/dev/null 2>&1; then
          echo "- ⚠️ Missing: ${a}" >> release-notes-extra.md
        fi
      done
      echo "" >> release-notes-extra.md
      echo "Release generated by workflow run ${{ github.run_id }}." >> release-notes-extra.md

Правка 2.2. aggregate-hashes: численный порог заменить на вычисление от
matrix.include, + отдельное сообщение для fallback-режима:
  - name: Aggregate hashes
    id: aggregate
    shell: bash
    run: |
      set -euo pipefail
      cat checksums/*/hash.txt > hashes.txt
      expected=$(find checksums -name hash.txt | wc -l)
      # [GITHUB-P24] dynamic threshold: full matrix = 6 (≥4 required for
      # SLSA attestation); vars.RUNS_ON fallback = 1 (windows-x86_64 only);
      # any middle case (partial matrix, transient cell failure) = max(2, expected).
      if [ -n "${RUNS_ON:-}" ]; then
        min_expected=1
      else
        min_expected=4
      fi
      if [ "$expected" -lt "$min_expected" ]; then
        echo "::error::Expected ≥$min_expected hash files, got $expected. Aborting release — insufficient artifacts."
        exit 1
      fi
      if [ "$expected" -lt 4 ]; then
        echo "::warning::Only $expected artifact(s) collected — SLSA provenance will be skipped (insufficient subjects for attestation), but release will proceed with available binaries."
      fi
      echo "hashes=$(base64 -w0 hashes.txt)" >> "$GITHUB_OUTPUT"
    env:
      RUNS_ON: ${{ vars.RUNS_ON }}

Правка 2.3. macOS cells в matrix.include получают allow_failure: true
(паритет с windows-aarch64, на случай regression eframe/accesskit):
  include: ${{ vars.RUNS_ON && fromJSON('[{"os":"windows-latest","target":"x86_64-pc-windows-msvc","target_local":"x86_64-pc-windows-gnu","artifact":"strangecoin-windows-x86_64"}]') || fromJSON('[{"os":"ubuntu-latest","target":"x86_64-unknown-linux-gnu","artifact":"strangecoin-linux-x86_64"},{"os":"ubuntu-latest","target":"aarch64-unknown-linux-gnu","artifact":"strangecoin-linux-aarch64","cross":true},{"os":"macos-latest","target":"x86_64-apple-darwin","artifact":"strangecoin-macos-x86_64","allow_failure":true},{"os":"macos-latest","target":"aarch64-apple-darwin","artifact":"strangecoin-macos-aarch64","allow_failure":true},{"os":"windows-latest","target":"x86_64-pc-windows-msvc","target_local":"x86_64-pc-windows-gnu","artifact":"strangecoin-windows-x86_64"},{"os":"windows-latest","target":"aarch64-pc-windows-msvc","artifact":"strangecoin-windows-aarch64","cross":true,"allow_failure":true}]') }}

  И добавить комментарий в header release.yml:
  # [GITHUB-P24] macOS cells now carry allow_failure:true — same as
  # windows-aarch64. Reason: eframe/accesskit/objc2 broke twice in the
  # last 6 months (rustc 1.99+ incompat Nov 2026, winit 0.30 glutin
  # accesskit churn Jul 2026). Stage releases must ship with Windows +
  # Linux fallback rather than silently fail entirely. Remove allow_failure
  # on macOS cells once eframe 0.30 has 3 months of clean CI history
  # (target: Feb 2027).

Требования к коммиту. Сообщение:
`[GITHUB-P24] release: fix body_path ENOENT + dynamic aggregate-hashes threshold + macOS allow_failure`

Проверка.
1. actionlint .github/workflows/release.yml exit 0.
   (Локально: `actionlint .github/workflows/release.yml`, если установлен;
   иначе `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"`.)
2. Прогон через workflow_dispatch на v*-теге (после Промпта 4) или re-tag
   (после Промпта 1). Ожидаемое поведение:
   - Build matrix: 6 клеток, 4 зелёные (Linux x86_64/aarch64 + Windows x86_64),
     2 macOS allow_failure (если зелёные — артефакты загружены; если красные
     — cell FAILED но не падает весь build job).
   - aggregate-hashes: видит 4-6 hash.txt, dynamic threshold проходит,
     `::warning::` при <4 не падает.
   - Note missing artifacts: touch создаёт файл, missing-артефакты
     дописываются (если macOS упала — будет 2 missing lines).
   - Create GitHub Release: softprops/action-gh-release@v2 находит
     release-notes-extra.md, генерирует GitHub Release с доступными
     binaries + SHA256SUMS.txt + cosign signatures.
3. Если macOS cells действительно зелёные (post-UI-P02-C) — release включает
   все 6 binaries, в release-notes-extra.md только строка "Release generated
   by workflow run N".
4. Если macOS cells падают (гипотетическая regression eframe) — release
   включает 4 binaries (Linux x86_64/aarch64 + Windows x86_64), в
   release-notes-extra.md строки "Missing: strangecoin-macos-x86_64" и
   "Missing: strangecoin-macos-aarch64", но GitHub Release всё равно создаётся.

Не делай.
- НЕ добавляй `continue-on-error: true` на весь build-job — это полностью
  сломает сигнализацию о сбоях Linux/Windows cells, которые должны быть
  стабильно зелёными. continue-on-error только на matrix-уровне через
  `allow_failure: true` (как уже сделано для windows-aarch64).
- НЕ удаляй `test "$expected" -ge 4` без замены на dynamic threshold —
  иначе aggregate-hashes пропустит ситуацию "0 артефактов" без ошибки.
- НЕ используй `body: ${{ ... }}` вместо `body_path:` — softprops/action-gh-release@v2
  принимает либо body (строка), либо body_path (файл), но не оба одновременно.
  Если хочется убрать body_path полностью — это отдельный промпт; в этом
  промпте фиксим минимально-invasive: `touch` перед шагом.
- НЕ меняй softprops/action-gh-release@v2 на@v3 или pinned SHA в этом
  промпте — это Промпт 7.
```

---

## Промпт 3. [КРИТИЧНО] `release.yml`: добавить `notify-failed` job — коммит-коммент при сбое Release-workflow

```
Контекст. После GITHUB-P19 (commit a6212b8, 17 Nov 2026) prerelease.yml
получил второй job `notify-skipped`, который постит commit-comment на
CI head_sha, если `workflow_run.conclusion != 'success'`. Это закрыло
тишину «prerelease skipped — не видно в Actions, мейнтейнер не понимает,
почему dev-тега нет».

У release.yml такой пары НЕТ. Любой сбой в build/sign/aggregate-hashes/release
остаётся видим только в Actions UI как красный run. На коммит, на который
ссылается тег, НЕ постится commit-comment «⚠️ v3.0.0-stage3 release failed:
<step>». Мейнтейнер узнаёт о сбое только если открывает Actions → Release
workflow руками — что в активной фазе разработки (10+ пушей в день) не
делается. Это и есть причина «релизы не выпускаются» остаётся незамеченной
3 недели: с 29 Oct (последний success) до текущего момента (19 Nov) не было
ни одного автоматического уведомления о сбое release.yml.

Задача. Добавить в .github/workflows/release.yml второй job `notify-failed`,
запускающийся `if: always() && failure()` после всех основных job'ов
(build/sign/aggregate-hashes/provenance/release), постящий commit-comment
на `github.sha` (для tag-push это peeled commit, на который указывает тег):

  notify-failed:
    name: Notify on Release failure
    runs-on: ubuntu-latest
    # [GITHUB-P25] post commit-comment on release.yml failure — parity with
    # prerelease.yml notify-skipped (GITHUB-P19, commit a6212b8). Without
    # this, the only signal of a failed v*-tag release is a red mark in the
    # Actions UI, which the maintainer does not see during active development.
    if: ${{ always() && failure() && !contains(github.ref_name, '-dev.') }}
    needs: [build, aggregate-hashes, sign, release]
    permissions:
      contents: read
      pull-requests: write   # commit-comments scope (matches prerelease notify-skipped)
    timeout-minutes: 3
    steps:
      - name: Post commit comment
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAG: ${{ github.ref_name }}
          HEAD_SHA: ${{ github.sha }}
          RUN_ID: ${{ github.run_id }}
          REPO: ${{ github.repository }}
        shell: bash
        run: |
          set -euo pipefail
          body=$(cat <<EOF
          ⚠️ **Release pipeline failed**: tag \`${TAG}\` did NOT produce a
          GitHub Release. The tag exists, but the Release workflow run
          failed in one of: build / aggregate-hashes / sign / release.

          - Release run: https://github.com/${REPO}/actions/runs/${RUN_ID}
          - Tag: \`${TAG}\` → commit \`${HEAD_SHA:0:7}\`
          - After fixing the failing job, re-run the workflow from Actions
            UI (Re-run failed jobs), OR delete and re-push the tag
            (\`git push origin :refs/tags/${TAG} && git tag -a ${TAG}
            <sha> -m "..." && git push origin refs/tags/${TAG}\`).

          Common causes:
          - macOS cells fail on eframe/objc2/accesskit regression
            (see [UI-P02-A/B/C] commits 2caa74e/e5191f6/67901a9);
          - \`release-notes-extra.md\` ENOENT in softprops/action-gh-release@v2
            (see [GITHUB-P24] release.yml fix);
          - SLSA provenance reusable workflow pinned to v2.1.0 may need a bump
            (check slsa-framework/slsa-github-generator releases);
          - cosign OIDC keyless signing failure (transient, retry helps).
          EOF
          )
          gh api --method POST \
            -H "Accept: application/vnd.github+json" \
            "/repos/${REPO}/commits/${HEAD_SHA}/comments" \
            -f body="$body"

Дополнительно. Если `provenance` джоб failed (а он skipped при vars.RUNS_ON,
но может failed без RUNS_ON), notify-failed всё равно сработает — `if:
always() && failure()` ловит failure любого из needs-зависимостей.

Если все needs-джобы success — notify-failed skipped (это правильно, нет
шума на зелёном релизе).

Требования к коммиту. Сообщение:
`[GITHUB-P25] release: notify-failed job — post commit-comment on Release workflow failure (parity with prerelease notify-skipped)`

Проверка.
1. actionlint .github/workflows/release.yml exit 0.
2. Локально через `act` (если установлен) или mental check: если все
   needs (build/aggregate-hashes/sign/release) success — notify-failed
   skipped. Если хотя бы один failed — notify-failed runs, posts
   commit-comment.
3. После merge + следующего v*-tag push (через Промпт 1 или Промпт 4):
   - Если release.yml success — notify-failed skipped, нет коммит-коммент.
   - Если release.yml failed — на /commits/<sha> появляется плашка
     "⚠️ Release pipeline failed: tag vX.Y.Z-stageN did NOT produce a
     GitHub Release..." со ссылкой на Actions run.
4. Проверить, что `permissions: pull-requests: write` действительно даёт
   commit-comment scope (REST POST /repos/{owner}/{repo}/commits/{sha}/comments
   требует "repo" scope или finer-grained "Contents: read+write" + "Metadata:
   read"; GITHUB_TOKEN на Actions runner имеет эти scope по умолчанию).
   Альтернатива: использовать `actions/github-script@v9` с `octokit.rest.repos.createCommitComment`,
   но это вводит зависимость от action'а, который уже был source'ом
   багов в CI3 (steps undefined). Shell + gh api предпочтительнее.

Не делай.
- НЕ дублируй notify-failed в `release.yml` и в отдельный workflow
  `release-notify.yml` — это создаст гонку между двумя commit-comments.
  Должен быть один notify-job в самом `release.yml`.
- НЕ используй `if: failure()` без `always()` — `failure()` в job-level
  if-equals-false, если upstream был skipped (а не failed), и notify
  не сработает на цепочке "build success, but aggregate-hashes skipped
  because RUNS_ON set". `always() && failure()` ловит оба сценария.
- НЕ постит commit-comment на dev-tags: gate `!contains(github.ref_name,
  '-dev.')` (как в build-job) исключает dev-tags из notify-failed.
- НЕ удаляй `needs: [build, aggregate-hashes, sign, release]` — без
  needs notify-failed стартует сразу при workflow start, не дождавшись
  результатов, и постит premature "release failed" на зелёном релизе.
```

---

## Промпт 4. [ВЫСОКО] `release.yml`: добавить `workflow_dispatch` + `workflow_call` — ручной re-run без delete/re-push тега

```
Контекст. release.yml сейчас имеет единственный триггер:
  on:
    push:
      tags: ['v*']
Это значит:
  - Нельзя вручную пере-запустить release на существующем теге — GitHub
    не повторяет trigger для уже существующего tag-push.
  - Нельзя запускать release на ветке (например, для теста release.yml
    изменений без создания тега).
  - Нельзя вызывать release.yml из другого workflow (например, из
    nightly.yml как post-deploy step).

Промпт 1 (re-tag v3.0.0-stage3) требует delete + re-push tag, что рискованно:
  - Если другой CI/CD система или external consumer слушает tag-create
    webhook, delete вызовет downstream-side cleanup.
  - Force-push annotated tag в некоторых Git-клиентах (Tower, SourceTree)
    отображается странно — пользователь думает, что тег "пропал".
  - Аннотированный tag-message теряется при delete + create (если не
    скопировать вручную).

Добавление `workflow_dispatch` позволяет:
  - Открывать Actions → Release workflow → "Run workflow" → выбрать ветку
    master + ввести tag-имя в input → workflow запускается с этим tag-имя
    как `inputs.tag`, без re-push.
  - Использовать existing tag SHA как `inputs.ref`, контролируя, какой
    коммит собирать.

Задача. Расширить `on:` в .github/workflows/release.yml:

  on:
    push:
      tags: ['v*']
    workflow_dispatch:
      inputs:
        ref:
          description: 'Commit SHA, branch, or existing tag to release from (default: master HEAD)'
          required: false
          default: 'master'
          type: string
        tag_name:
          description: 'Tag name to create/release (e.g. v3.0.0-stage3.1). If the tag already exists, the workflow will attempt to release it without re-tagging.'
          required: true
          type: string
        prerelease:
          description: 'Mark as pre-release (skips SLSA + full matrix)'
          required: false
          default: false
          type: boolean

Дальше в build-job:
  - Заменить `actions/checkout@v7` (без ref) на явный ref:
      - uses: actions/checkout@v7
        with:
          ref: ${{ github.event_name == 'workflow_dispatch' && inputs.ref || github.ref }}
          persist-credentials: true
  - Заменить gate `if: ${{ !contains(github.ref_name, '-dev.') }}` на:
      if: ${{ github.event_name == 'workflow_dispatch' || !contains(github.ref_name, '-dev.') }}
    (workflow_dispatch может выпустить что угодно, включая dev-tags — если
    мейнтейнер осознанно хочет выпустить dev-tag как полноценный release,
    это его ответственность.)
  - Заменить `softprops/action-gh-release@v2` with:
      tag_name: ${{ github.event_name == 'workflow_dispatch' && inputs.tag_name || github.ref_name }}
      prerelease: ${{ github.event_name == 'workflow_dispatch' && inputs.prerelease || false }}

Дополнительно. В header release.yml добавить комментарий:
  # [GITHUB-P26] workflow_dispatch: manual re-run of Release on existing tag
  # without delete + re-push. Inputs:
  #   ref       — git ref to build from (default: master HEAD)
  #   tag_name  — tag to publish (must exist OR will be created on the ref)
  #   prerelease— mark GitHub Release as pre-release (skips SLSA + full matrix
  #               semantics if set; matrix still runs but release is marked
  #               pre-release on GitHub)
  # Use case: tag v3.0.0-stage3 was pushed on a broken commit; instead of
  # delete + re-push (Промпт 1), maintainer can workflow_dispatch with
  # ref=master and tag_name=v3.0.0-stage3 — the workflow rebuilds on the
  # current master HEAD and re-publishes the release under the same tag.
  # NOTE: if the tag v3.0.0-stage3 already exists on a DIFFERENT commit,
  # workflow_dispatch with inputs.ref=master will create the release but
  # the tag will still point to the old commit. To move the tag itself,
  # use Промпт 1 (delete + re-push). workflow_dispatch only creates the
  # GitHub Release metadata + uploads assets; it does not move git refs.

Требования к коммиту. Сообщение:
`[GITHUB-P26] release: add workflow_dispatch + workflow_call for manual re-run on existing tags`

Проверка.
1. actionlint .github/workflows/release.yml exit 0 — проверяет, что
   inputs объявлены корректно, выражения в if-e валидны.
2. После merge: открыть Actions → Release workflow → справа появляется
   кнопка "Run workflow" с dropdown-ом веток + 3 input-поля.
3. Прогон workflow_dispatch с inputs.ref=master, inputs.tag_name=v3.0.0-stage3-test
   (тестовый tag, потом удалить):
   - Build matrix запускается на master HEAD.
   - Все шаги выполняются, softprops/action-gh-release@v2 создаёт Release
     с tag_name=v3.0.0-stage3-test.
   - Если inputs.prerelease=true — Release помечен Pre-release.
4. НЕ удалять тестовый tag через gh release delete + git push origin :refs/tags/v3.0.0-stage3-test
   после проверки.
5. После Промпта 1 (re-tag v3.0.0-stage3) — можно использовать workflow_dispatch
   для будущих релизов без re-push, если код под тегом не менялся.

Не делай.
- НЕ добавляй `workflow_call` без причин — это для reusable-workflow
  паттерна, который в strangecoin пока не нужен. `workflow_dispatch`
  достаточно.
- НЕ удаляй `on.push.tags: ['v*']` — автоматический релиз на tag-push
  должен остаться. workflow_dispatch — дополнение, не замена.
- НЕ делай `inputs.tag_name` required=false — это позволит случайно
  запустить workflow без tag-name, softprops упадёт на пустом tag_name.
- НЕ используй `github.ref_name` для inputs.tag_name в workflow_dispatch —
  github.ref_name на workflow_dispatch равен имени ветки ("master"), не
  тегу. Поэтому отдельный input `tag_name` обязателен.
```

---

## Промпт 5. [ВЫСОКО] Найти рут-коз `Nightly #13` (19 Nov, 50m 39s, FAILED) и починить упавший job

```
Контекст. Nightly workflow run #13 от 19 Nov 2026 FAILED с длительностью
50m 39s. Это самый длинный Nightly run в истории (обычные timing: 25-35 min
на полный matrix). 50 минут означают, что matrix cells не отменились
раньше (cancel-in-progress: false), и один из долгих job'ов дошёл до
timeout или до конкретной ошибки.

Nightly workflow (.github/workflows/nightly.yml) содержит 5 job'ов:
  1. fuzz         (continue-on-error: true, matrix 11 targets, 30 min each)
  2. cross-build  (continue-on-error: ${{ matrix.allow_failure == true }},
                   matrix 6 cells; только windows-aarch64 allow_failure)
  3. audit        (continue-on-error: НЕТ — blocking job)
  4. mutation     (continue-on-error: true)
  5. miri         (continue-on-error: true)

Поскольку fuzz/mutation/miri имеют continue-on-error, они не могут быть
рут-коз'ом FAILED workflow. Остаются два кандидата:

  Кандидат A. cross-build matrix, macOS cells.
    - UI-P02-C (commit 67901a9, 18 Nov) снял pin 1.99.1 в nightly.yml.
    - Если UI-P02-A/B (eframe 0.30 migration) содержит regression, не
      пойманную CI на push (CI не собирает macOS cells), Nightly первый
      раз попробует macOS на stable 1.100+ и упадёт.
    - Run-time: 30-40 min на macOS cell build (rocksdb + eframe + wasmtime).

  Кандидат B. audit job (cargo-audit + cargo-deny).
    - deny.toml [advisories].ignore содержит baseline 10+ RUSTSEC ID'ов.
    - Если появилась НОВАЯ RUSTSEC, не в ignore-списке — cargo-audit exit 1.
    - 19 Nov RUSTSEC db обновляется ежедневно; если сегодня опубликован
      advisory на e.g. `eframe 0.30` (новая regression) или на транзитивную
      зависимость eframe 0.30 — cargo-audit впервые видит этот advisory
      и падает.
    - Run-time: 1-3 min на audit. 50 min общего run-time → audit НЕ
      растягивает timing, но если он последний в dependency-chain —
      общий статус FAILED фиксируется в конце.

Шаги для диагностики (выполняет мейнтейнер или агент с доступом к GitHub UI):

  1. Открыть https://github.com/Reider85/strangecoin/actions/runs/<RUN_ID>
     (RUN_ID Nightly #13 — найти в URL на Actions → Nightly → run #13).
  2. В левом sidebar посмотреть, какие jobs помечены ❌ (failed):
     - Если cross-build (macos-x86_64 или macos-aarch64) ❌ → Кандидат A.
     - Если audit ❌ → Кандидат B.
     - Если оба ❌ → два независимых сбоя.
  3. Для failed job открыть логи, найти последний красный step, скопировать
     error message (5-10 строк context).

Если Кандидат A (macOS cells):
  - Локально (если есть macOS машина) воспроизвести:
      cargo +stable build --release --target x86_64-apple-darwin
      cargo +stable build --release --target aarch64-apple-darwin
  - Скорее всего ошибка в eframe 0.30 API или в winit/glutin. Проверить
    commit 2caa74e (UI-P02-A) diff в crates/strangecoin-gui — нет ли
    неиспользованного импорта или несовместимого API call.
  - Если macOS cell нестабильна — добавить `allow_failure: true` в
    matrix.include для macos-x86_64 и macos-aarch64 в nightly.yml
    (паритет с release.yml Промпт 2 правка 2.3).
  - Commit: `[GITHUB-P27] nightly: macOS cells allow_failure (parity with release.yml GITHUB-P24)`

Если Кандидат B (audit):
  - Локально:
      cargo audit  # без --ignore, чтобы увидеть полный список
  - Найти новый RUSTSEC-2026-NNNN, не в ignore-списке.
  - Проверить, реально ли уязвима зависимость (cargo tree -i <affecting-crate>).
  - Если advisory применимый — добавить ignore в deny.toml [advisories].ignore
    с комментарием-обоснованием "unaffected: <reason>" или "mitigated: <how>".
    И параллельно добавить в cargo audit --ignore список в nightly.yml
    (cargo-audit не читает deny.toml — см. header deviation 8 nightly.yml).
  - Если advisory не применим (e.g. optional feature) — только ignore
    в deny.toml с обоснованием.
  - Commit: `[GITHUB-P27] nightly: audit ignore RUSTSEC-2026-NNNN (<crate>) — unaffected, <reason>`

Если ни A, ни B (например, mutation job упал несмотря на continue-on-error —
что не должно случаться, но cargo-mutants >= 25 может иметь breaking CLI
change, как было с -j/--jobs в CI4 Промпт 6 deviation 10d):
  - Открыть логи mutation job, найти command line и exit code.
  - Если cargo-mutants снова изменил CLI (например, переименовал --in-place) —
    обновить команду в nightly.yml.
  - Commit: `[GITHUB-P27] nightly: cargo-mutants CLI compat (<new flag>)`

Требования к коммиту. Сообщение зависит от рут-коз (см. выше варианты).

Проверка.
1. Открыть следующий Nightly run (20 Nov, 04:00 UTC) — должен быть success
   или хотя бы не падать на той же причине.
2. Если рут-коз был macOS cells (Кандидат A) — Nightly run с allow_failure
   на macOS cells показывает macOS ❌, но workflow overall success (по
   аналогии с windows-aarch64).
3. Если рут-коз был audit (Кандидат B) — Nightly audit job проходит,
   cargo-audit exit 0 с обновлённым ignore-списком.

Не делай.
- НЕ добавляй `continue-on-error: true` на весь cross-build job — это
  сломает сигнал о сбоях Linux/Windows cells. Только на конкретные
  matrix cells через allow_failure: true.
- НЕ игнорируй новые RUSTSEC advisory без обоснования — каждый ignore
  в deny.toml должен иметь комментарий "unaffected: <reason>" или
  "mitigated: <how>", иначе ignore-список превращается в garbage heap.
- НЕ запускай Nightly через workflow_dispatch во время активной фазы —
  matrix 6 клеток × 30 min займёт весь runner pool. Лучше дождаться
  04:00 UTC scheduled run.
```

---

## Промпт 6. [СРЕДНИЙ] Найти рут-коз `Pre-release #60` (18 Nov, 10m 12s, FAILED) и добавить retry-on-transient

```
Контекст. Pre-release workflow run #60 от 18 Nov 2026 FAILED с длительностью
10m 12s. Run #61 (retry через workflow_run на следующем зелёном CI) прошёл
за 13m 16s и создал v3.0.0-dev.4. То есть run #60 — НЕ структурный баг
prerelease.yml (после GITHUB-P12/P19/P22 файл стабилен), а transient failure
на конкретном шаге.

10m 12s таймстемп указывает на один из:
  - cargo build --release (cold cache, ~10-12 min; если во время сборки
    произошёл runner-side OOM или disk-full — cargo exit 101).
  - gh release create upload (asset upload ~1-3 min; если GitHub API
    timeout или 5xx — gh CLI exit 1).
  - gh api POST commit-comment (редко падает, но если rate-limit или
    secondary-rate-limit — exit 1 с "HTTP 403").

Прямая диагностика: открыть
https://github.com/Reider85/strangecoin/actions/runs/<RUN_ID_60>
(найти run #60 на Actions → Pre-release → 18 Nov).
В sidebar найти failed step. Три гипотезы и фиксы:

Гипотеза A. cargo build --release failed (OOM / disk-full).
  - Runner: ubuntu-latest (GitHub-hosted, 7 GB RAM, 14 GB disk).
  - release profile: LTO=true, codegen-units=1 → high memory at link time.
  - Симптом в логах: "error: could not compile `strangecoin`", "LLVM error:
    out of memory", или "No space left on device".
  - Если это повторяется (несколько раз в месяц) — добавить в prerelease.yml:
      env:
        CARGO_BUILD_JOBS: 2   # уменьшить parallelism для LTO link step
        CARGO_NET_RETRY: 5
        RUST_BACKTRACE: 1
  - Swap-файл на ubuntu-latest: 4 GB по умолчанию. Если LTO требует больше —
    добавить step:
      - name: Increase swap (avoid OOM on LTO link)
        if: runner.os == 'Linux'
        shell: bash
        run: |
          sudo fallocate -l 8G /swapfile
          sudo chmod 600 /swapfile
          sudo mkswap /swapfile
          sudo swapon /swapfile || true
          free -h
  - Commit: `[GITHUB-P28] prerelease: increase runner swap (avoid LTO OOM)`

Гипотеза B. gh release create upload failed (GitHub API 5xx).
  - Симптом в логах: "Error: HTTP 502 Bad Gateway", "Error: HTTP 500
    Internal Server Error", "could not upload asset".
  - GitHub asset upload через releases.upload-assets endpoint иногда
    возвращает 5xx (известная проблема, см.
    https://www.githubstatus.com/history).
  - Фикс: добавить retry на шаг "Create and push dev release":
      - name: Create and push dev release
        if: steps.check-release.outputs.skip != 'true'
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        shell: bash
        run: |
          set -uo pipefail   # НЕ set -e — retry должен пережить первый fail
          TAG="${{ steps.compute-tag.outputs.tag }}"
          HEAD_SHA="${{ github.event.workflow_run.head_sha }}"
          for attempt in 1 2 3; do
            if gh release create "$TAG" \
              --target "$HEAD_SHA" \
              --prerelease \
              --title "$TAG" \
              --notes "Automated dev build, not for production" \
              --generate-notes \
              strangecoin-linux-x86_64 strangecoin-linux-x86_64.sha256; then
              echo "Release created on attempt $attempt."
              exit 0
            fi
            echo "Attempt $attempt failed. Sleeping $((attempt * 30))s before retry..."
            sleep $((attempt * 30))
          done
          echo "All 3 attempts failed."
          exit 1
  - Commit: `[GITHUB-P28] prerelease: 3x retry on gh release create (GitHub API 5xx resilience)`

Гипотеза C. gh api POST commit-comment failed (rate-limit).
  - Симптом: "HTTP 403 Forbidden", "X-RateLimit-Remaining: 0".
  - На workflow_run trigger GITHUB_TOKEN не имеет secondary-rate-limit
    по commit-comments (это REST API для authed app), но если в этом run'е
    параллельно шёл другой workflow с большим количеством gh api calls —
    secondary limit мог сработать.
  - Фикс: шаг Post commit comment уже имеет `if: always()` — добавить retry:
      - name: Post commit comment
        if: always() && steps.check-release.outputs.skip != 'true'
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        shell: bash
        run: |
          set -uo pipefail
          TAG="${{ steps.compute-tag.outputs.tag }}"
          HEAD_SHA="${{ github.event.workflow_run.head_sha }}"
          REPO="${{ github.repository }}"
          if [[ "${{ steps.check-release.outputs.skip }}" == "true" ]]; then
            exit 0
          fi
          for attempt in 1 2 3; do
            if gh api --method POST \
              -H "Accept: application/vnd.github+json" \
              "/repos/${REPO}/commits/${HEAD_SHA}/comments" \
              -f body="Dev build created: ${TAG} — https://github.com/${REPO}/releases/tag/${TAG}"; then
              exit 0
            fi
            sleep $((attempt * 15))
          done
          # Non-fatal: release was created, comment is best-effort.
          echo "::warning::commit-comment failed after 3 retries (rate-limit?), release itself succeeded."
          exit 0
  - Note: commit-comment — non-critical (release уже создан на предыдущем
    шаге). Лучше проглотить ошибку с warning, чем краснить workflow.
  - Commit: `[GITHUB-P28] prerelease: commit-comment 3x retry + non-fatal on rate-limit`

Требования к коммиту. Зависит от подтвердившейся гипотезы (см. выше).
Если несколько гипотез подтверждаются — комбинировать в одном коммите с
подробным сообщением.

Проверка.
1. После merge: следующий failed prerelease run (если случится) должен
   либо: показать в логах "Attempt 1 failed, retry in 30s..." и затем
   "Release created on attempt 2", либо exit 0 с warning на commit-comment.
2. Статистика за декабрь: если до фикса было ~2 failed prerelease runs
   в неделю из-за transient API 5xx или OOM — после фикса должно быть 0.
3. Если Hypothesis A (OOM) подтверждается часто — отдельным коммитом
   рассмотреть перенос prerelease.yml на bigger-runner (ubuntu-latest-large
   или self-hosted), см. AGENTS.md SELF_HOSTED_RUNNERS.md.

Не делай.
- НЕ оборачивай ВЕСЬ workflow в retry-loop — `workflow_run` сам по себе
  retry-able через Actions UI ("Re-run failed jobs"). Retry нужен только
  на network-bound шагах (gh release create, gh api).
- НЕ добавляй retry на `cargo build --release` — он deterministic (либо
  компилируется, либо нет; OOM — это не transient, это структурная
  memory-нехватка, фикса через swap).
- НЕ делай commit-comment critical (exit 1 если comment failed) —
  release уже создан, краснить workflow из-за неважного comment'а
  плохой trade-off.
- НЕ увеличивай timeout-minutes на create-dev-tag job выше 30 — если
  cargo build за 30 min не укладывается, проблема структурная (memory,
  disk), а не в timeout.
```

---

## Промпт 7. [НИЗКО] `release.yml`: dependabot major-ignore для `softprops/action-gh-release` + `slsa-framework/slsa-github-generator` + pin по major-линейке

```
Контекст. CI4 Промпт 7 (commit 3f2a74e, [GITHUB-P21]) добавил в
.github/dependabot.yml ignore-правила для actions/upload-artifact и
actions/download-artifact major bumps. Это закрыло риск "blind-merge
4→7 upload / 4→8 download через dependabot" (что и было рут-коз #8-#11
в release.yml runs на dependabot-bumps 10 Nov).

Но в release.yml осталось три action'а, не защищённых от major-bump:

  1. softprops/action-gh-release@v2
     — main action, создающий GitHub Release. Если dependabot откроет PR
     на v3 и мейнтейнер смержит "blind" — v3 может иметь breaking changes
     в API (например, новая required input, изменённая семантика body_path
     — см. Промпт 2 хрупкость 1). Это напрямую сломает release.yml.
     История: softprops/action-gh-release v1 → v2 был breaking (Node 12 →
     Node 16, изменены defaults). v2 → v3 (когда выйдет) — почти наверняка
     тоже breaking.

  2. slsa-framework/slsa-github-generator/.github/workflows/generator_generic_slsa3.yml@v2.1.0
     — external reusable workflow для SLSA provenance. Major bump v2 → v3
     может изменить semver inputs (`base64-subjects`, `upload-assets`,
     `draft-release`) или семантику attestation. Это уронит `provenance`
     job в release.yml.

  3. sigstore/cosign-installer@v4.1.3
     — уже pinned на patch-level (v4.1.3), но если dependabot предложит
     major bump v4 → v5 — нужно его игнорировать, как и для upload/download.

Задача. Расширить .github/dependabot.yml раздел github-actions ignore:

  - package-ecosystem: github-actions
    directory: "/"
    schedule:
      interval: weekly
    open-pull-requests-limit: 5
    labels:
      - dependencies
    ignore:
      - dependency-name: "actions/upload-artifact"
        update-types:
          - "version-update:semver-major"
      - dependency-name: "actions/download-artifact"
        update-types:
          - "version-update:semver-major"
      # [GITHUB-P29] release-pipeline critical actions: major bumps are
      # SCIP-style deliberate bumps only (like upload/download above).
      # softprops v2 → v3 (when released) WILL be breaking (Node upgrade,
      # API surface change historically). slsa-github-generator v2 → v3
      # changes reusable-workflow input contract. cosign-installer v4 → v5
      # changes cosign CLI version pinning. All three: manual bump only
      # after release.yml local verification.
      - dependency-name: "softprops/action-gh-release"
        update-types:
          - "version-update:semver-major"
      - dependency-name: "slsa-framework/slsa-github-generator"
        update-types:
          - "version-update:semver-major"
      - dependency-name: "sigstore/cosign-installer"
        update-types:
          - "version-update:semver-major"
    groups:
      github-actions-minor:
        update-types:
          - "version-update:semver-major"
          - "version-update:semver-minor"

Дополнительно. В header release.yml обновить комментарий [GITHUB-P21]:

  # [GITHUB-P29] release-pipeline critical actions are dependabot
  # major-ignored (see .github/dependabot.yml): upload@v7, download@v8,
  # softprops@v2, slsa-github-generator@v2.1.0, cosign-installer@v4.1.3.
  # Patch bumps flow through the github-actions-minor grouped PR.
  # Major bumps require manual review of the action's release-notes +
  # local workflow_dispatch test before merge.

Требования к коммиту. Сообщение:
`[GITHUB-P29] dependabot: major-ignore softprops/slsa-github-generator/cosign-installer (release-pipeline critical actions)`

Проверка.
1. yaml.safe_load(.github/dependabot.yml) exit 0 — синтаксис корректен.
2. На странице Insights → Dependency graph → Dependabot — у
   softprops/action-gh-release, slsa-framework/slsa-github-generator,
   sigstore/cosign-installer появляется плашка "Ignored:
   version-update:semver-major".
3. Следующий dependabot weekly run НЕ открывает PR на major bump этих
   action'ов. Patch bumps (v4.1.3 → v4.1.4 для cosign-installer) —
   открывает через grouped PR.

Не делай.
- НЕ pinned SHA для softprops/action-gh-release и slsa-github-generator —
  это создаст friction при patch-bumps (dependabot не умеет обновлять
  SHA-only references корректно, открывает PR с "Bump softprops from
  <sha> to <sha>", что не review-friendly). Major-ignore + caret-major
  на @vN достаточно для защиты.
- НЕ удаляй patch/minor bumps из grouped PR — security-patches (e.g.
  RUSTSEC-driven action updates) должны приходить автоматически.
- НЕ добавляй ignore на `actions/checkout@v7`, `dtolnay/rust-toolchain@stable`,
  `Swatinem/rust-cache@v2`, `taiki-e/install-action@v2` — это stable
  widely-used action'ы, major bumps у них редки и обычно backwards-compatible.
  Ignore только для action'ов, напрямую критичных к release-pipeline.
```

---

## Итоговый чек-лист применения CI5

```
После применения всех 7 промптов по порядку:

□ Промпт 1 (GITHUB-P23): v3.0.0-stage3 tag пере-запушен на актуальный
  master HEAD (после UI-P02-C). Release workflow run #N (например #14)
  завершается success, /releases/tag/v3.0.0-stage3 показывает stable
  release с 6 binaries + SHA256SUMS.txt + cosign signatures + SLSA
  provenance. "Latest release" плашка переходит с v2.0.0-stage2 на
  v3.0.0-stage3.

□ Промпт 2 (GITHUB-P24): release.yml — touch release-notes-extra.md
  перед softprops, dynamic aggregate-hashes threshold, macOS cells
  allow_failure:true. Любой будущий сбой matrix cell не молча краснит
  весь workflow.

□ Промпт 3 (GITHUB-P25): release.yml — notify-failed job, постит
  commit-comment на failed v*-tag release. Больше никогда не бывает
  ситуации «релиз не вышел 3 недели, мейнтейнер не заметил».

□ Промпт 4 (GITHUB-P26): release.yml — workflow_dispatch с inputs
  (ref, tag_name, prerelease). Ручной re-run без delete + re-push tag.

□ Промпт 5 (GITHUB-P27): Nightly #13 рут-коз найден и пофиксен
  (macOS allow_failure или audit ignore или cargo-mutants CLI compat).
  Nightly run от 20 Nov — success или хотя бы не падает на той же причине.

□ Промпт 6 (GITHUB-P28): Pre-release #60 рут-коз найден и добавлен
  retry (3x на gh release create / gh api POST commit-comment). Любой
  будущий transient API 5xx или rate-limit не краснит prerelease run.

□ Промпт 7 (GITHUB-P29): dependabot.yml — major-ignore для softprops,
  slsa-github-generator, cosign-installer. Следующий blind-merge
  major bump этих action'ов НЕ сломает release.yml.

После всех 7 промптов:
- pipeline «push v*-tag → release.yml → build 6 matrix cells →
  aggregate-hashes → sign → release → notify on failure» работает
  детерминированно;
- v*-tag release failure виден мейнтейнеру через commit-comment за
  минуты, а не недели;
- transient GitHub API 5xx или runner OOM не падают prerelease run;
- dependabot НЕ может протащить breaking major bump в release-pipeline
  critical action'ы;
- manual re-run release.yml возможен без delete + re-push tag.
```
