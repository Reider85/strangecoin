# Self-hosted runners — hybrid mode (GITHUB-P06)

> Резервный режим: когда бесплатные минуты GitHub Actions исчерпаны (или
> политика репозитория изменится), CI/релизные воркфлоу переключаются на
> **локальный self-hosted раннер** на этом Windows-хосте вместо
> GitHub-hosted раннеров. До первого ручного включения поведение воркфлоу
> **не меняется** — все они читают одну repo-variable.

Контекст и мотивация: `analytics/GITHUB.md` (P1–P5). Общая схема установки
раннера: `scripts/actions/setup-local-runner.ps1`.

## 1. Контракт переменной `RUNS_ON`

| Состояние | Значение | Эффект |
| --- | --- | --- |
| **не задана (дефолт)** | — | `runs-on` каждой джобы = `ubuntu-latest` / `matrix.os` — ровно то, что было до P6 |
| **задана (fallback)** | JSON-массив лейблов, ровно `["self-hosted","Windows","X64"]` | `runs-on` = этот массив; выполняются только те джобы/ячейки, которые помечены как доступные локально (см. §3) |

Формула во всех воркфлоу (единая):

```yaml
runs-on: ${{ vars.RUNS_ON && fromJSON(vars.RUNS_ON) || fromJSON('["ubuntu-latest"]') }}
```

Матричные джобы (`release.yml`/`nightly.yml` build) используют ту же формулу
с `format('["{0}"]', matrix.os)` в правой ветке и **job-level guard**
`(!vars.RUNS_ON || matrix.local)` — в fallback выполняются только ячейки
`local: true`.

**Переключение вручную** (нужен `gh` CLI с доступом к репо):

```powershell
# включить fallback (минуты исчерпаны):
gh variable set RUNS_ON --body '["self-hosted","Windows","X64"]'

# вернуться на GitHub-hosted (минуты сброшены/пополнены):
gh variable delete RUNS_ON
```

Repo-variable действует на **все** воркфлоу сразу — переключатель один.

## 2. Одноразовая установка раннера

1. GitHub → `Reider85/strangecoin` → Settings → Actions → Runners →
   **New self-hosted runner** (Windows x64) → скопировать токен из
   инструкции `config.cmd --url ... --token ...` (токен короткоживущий).
2. Из корня репозитория (PowerShell):

   ```powershell
   .\scripts\actions\setup-local-runner.ps1 -Token <registration-token>
   # опционально (нужен elevated-шелл): -InstallService
   ```

   Скрипт идемпотентен и делает: winget-prereqs (`rg`, `jq`, `gh`,
   `python`) → machine-PATH (`C:\msys64\mingw64\bin`) + machine
   `CARGO_HOME=C:\rt\cargo_home` + `config.toml` с windows-gnu
   linker/sysroot (repo `.cargo/config.toml` — gitignored, checkout его не
   привезёт) → `core.autocrlf=false` → rustup
   `stable-x86_64-pc-windows-gnu` → скачивание/регистрация actions/runner
   в **ASCII-путь** `C:\actions-runner` (профиль `C:\Users\ЗС` не годится).
3. Запуск раннера:
   - без сервиса: `& C:\actions-runner\run.cmd` (окно держать открытым);
   - с `-InstallService`: сервис стартует автоматически.
4. Проверка онлайн: `gh api repos/Reider85/strangecoin/actions/runners --jq '.runners[].name'`.

После этого — §1 (переключение) и §4 (тест).

## 3. Что где выполняется (матрица возможностей)

Джобы/ячейки, **не** помеченные «локально», в fallback **пропускаются**
(серый статус, не ошибка) — их нельзя выполнить на Windows-раннере.

| Воркфлоу | Джобы | GitHub-hosted (RUNS_ON unset) | Fallback (RUNS_ON set) |
| --- | --- | --- | --- |
| `ci.yml` | `lint`, `test-core`, `coverage`, `source-gates` | ubuntu | **локально**, все 4 |
| `prerelease.yml` | `tag-and-release` | linux-x86_64 artifact | **локально**, `strangecoin-windows-x86_64[.exe]` artifact |
| `release.yml` | `build` (6 ячеек) | все 6 платформ | **только** `windows-x86_64` (host gnu-triple вместо msvc); 5 остальных skipped |
| `release.yml` | `aggregate-hashes`, `sign`, `release` | ubuntu | **локально** (bash/git-bash) |
| `release.yml` | `provenance` (SLSA) | ubuntu | **skipped** — внешний reusable workflow нельзя перенацелить; fallback-релиз **без attestation** |
| `nightly.yml` | `fuzz` (9 таргетов × 30 мин) | ubuntu | **skipped** — cargo-fuzz/libFuzzer не собирается на этом Windows-хосте (AGENTS.md) |
| `nightly.yml` | `cross-build` (6 ячеек) | все 6 платформ | **только** `windows-x86_64`; остальные skipped |
| `nightly.yml` | `audit`, `mutation`, `miri` | ubuntu | **локально** (все — signal-only/continue-on-error) |
| `star-monitor.yml`, `star-alert.yml` | snapshot / alert | ubuntu | **локально** |
| `pages.yml` | `build`, `deploy` | ubuntu | **локально** |

Итог: fallback — **деградированный, но рабочий** режим: тесты, линты,
гейты, Windows-релизы, nightly-аудит идут; fuzz, macOS/Linux-сборки и SLSA
— нет (см. также §6).

## 4. Тест переключения (после установки раннера)

```powershell
gh variable set RUNS_ON --body '["self-hosted","Windows","X64"]'
gh workflow run CI --ref master        # или любым другим воркфлоу
gh run watch                           # job должен стать "Waiting for a runner"
                                       # и выполниться на именном локальном раннере
gh variable delete RUNS_ON             # вернуть штатный режим
```

## 5. Безопасность (обязательно прочитать)

- **Форк-PR никогда не выполняются локально.** Пока `RUNS_ON` задана,
  `ci.yml` и `pages.yml` пропускают `pull_request`, если head-ветка не из
  этого репозитория (`github.event.pull_request.head.repo.full_name ==
  github.repository`) — иначе чужой код из форка попадал бы на вашу
  машину. Push и same-repo PR идут локально.
- Дополнительно в репо settings рекомендуется:
  Settings → Actions → General → **Fork pull request workflows from
  outside collaborators → Require approval** (страховка, когда RUNS_ON
  не задана).
- Локальный раннер исполняет код с правами вашей учётки. Не запускайте
  его на непроверенных ветках/форках; токен регистрации одноразовый.
- Fallback-джобы используют стандартный `GITHUB_TOKEN` с теми же
  `permissions:` — ничего не расширяется.

## 6. Ограничения fallback-режима (осознанные, не сбои)

1. **Один раннер = последовательное исполнение.** 4 джобы CI идут одна за
   другой (≈30–60 мин суммарно на тёплом кэше вместо ≤8 мин параллельно
   на GitHub-hosted). Таймауты в `ci.yml` подняты (30/60/90/20) с учётом
   этого — на GitHub-hosted длительность не меняется.
2. **Пропуски по невозможности**: fuzz-матрица, macOS/Linux-ячейки
   release/nightly, SLSA-provenance (см. §3).
3. **`prerelease.yml` в fallback публикует Windows-бинарь** вместо
   Linux — имя артефакта меняется на `strangecoin-windows-x86_64[.exe]`.
4. **Кэш**: `Swatinem/rust-cache` работает и локально, но первый
   fallback-прогон холодный (сборка workspace ≈30+ мин на этом хосте —
   бюджет из AGENTS.md).
5. **`CARGO_BUILD_JOBS=2`** в `ci.yml` в fallback — фикс mass-link
   test-бинарей (AGENTS.md, windows-gnu); на GitHub-hosted = 4 (дефолт
   4-vCPU раннера, ничего не меняет).
6. **Публичный репозиторий**: минуты GitHub-hosted сейчас не ограничены
   (GITHUB.md §3.3) — режим нужен на случай смены политики/перевода в
   private/исчерпания лимита для larger runners.
7. **Не воспроизводится локально**: сами expressions `vars.*`/`fromJSON`
   и подбор лейблов проверяются только живым прогоном (§4) — первая
   верификация после пуша.

## 7. Troubleshooting

| Симптом | Причина → действие |
| --- | --- |
| Job висит `Waiting for a runner` | Раннер не запущен → `& C:\actions-runner\run.cmd` (или сервис); проверить §2.4 |
| Job ушёл на ubuntu, хотя RUNS_ON задана | Опечатка в значении переменной —must be exactly `["self-hosted","Windows","X64"]` (JSON, кавычки двойные) → `gh variable set` заново |
| `rg: command not found` в source-gates | winget не поставил ripgrep / PATH не обновился → установить вручную, проверить machine-PATH |
| `python: command not found` в gate 3 | Python не на PATH → winget `Python.Python.3.12` или ручная установка (gate сам фолбэчится `python3`→`python`) |
| `gh`/`jq` not found (star/prerelease enrichment) | См. prereqs скрипта §2.2 → `winget install GitHub.cli jqlang.jq` |
| `ld: cannot find ...libtest-*.rlib` / dlltool errors | Не выполнена §2.2 (machine-PATH mingw64 + `CARGO_HOME` config) → перезапустить setup-скрипт, **перезапустить раннер** (service читает machine-env при старте) |
| Job пропущен (`skipped`) в fallback | Ожидаемо для fuzz/provenance/non-local ячеек — §3; если пропущен неожиданный — проверить `local:`/`if:` в воркфлоу |
| Раннер «принял» job, но сборка падает на toolchain | `rustup toolchain install stable-x86_64-pc-windows-gnu` не выполнен → §2.3 |
| Нужно откатиться на GitHub-hosted | `gh variable delete RUNS_ON` — единственный переключатель, отката «частичного» нет |

## 8. Файлы

| Файл | Назначение |
| --- | --- |
| `scripts/actions/setup-local-runner.ps1` | Одноразовая установка/регистрация раннера + prereqs |
| `.github/workflows/*.yml` | Все 7 воркфлоу: `runs-on` читают `vars.RUNS_ON` (см. header-комментарии `[GITHUB-P06]`) |
| `analytics/GITHUB.md` | P1–P5 — исходная стратегия (P6 = этот режим, код в коммите `[GITHUB-P06]`) |
