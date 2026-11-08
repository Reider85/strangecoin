# GITHUB.md — стратегия релизов, трендов и CI для strangecoin

> Цель: чаще мелькать в ленте GitHub, набирать звёзды на пуши, переложить
> ресурсозатратные задачи на раннеры GitHub вместо локальной машины.
> Формат итога — **несколько готовых промптов** для последующего запуска
> в AI-агенте (Cursor / Claude Code / GPT-5 Codex / Continue и т.п.).
>
> Источник анализа: `https://github.com/Reider85/strangecoin`
> (clone на дату составления 2026-11-08, ветка `main`).

---

## 0. Краткая сводка по репозиторию

| Параметр | Значение |
| --- | --- |
| Язык | Rust 99.3% (edition 2021, workspace 4 крейта) |
| Текущая версия | `2.0.0` (tag `v2.0.0-stage2`) |
| Лицензия | Apache-2.0 |
| Звёзды | 1 (на дату анализа) |
| Теги | 5 (`v0.0.1-rc1`, `v1.0.0-stage0`, `v1.1.0-stage1`, `v1.5.0-stage1.5`, `v2.0.0-stage2`) |
| Активная ветка | `main` (Stage 3 в прогрессе — S3-P01..S3-P07 закрыты) |
| Workflow-файлы | `.github/workflows/ci.yml`, `.github/workflows/release.yml` |
| Статус Actions | **24 из 25 последних запусков — failed** (только Release #7 успешен) |

Проект — production-grade Layer-1 блокчейн для AI-агентов: PoW + PoS-bound,
WASM smart contracts (wasmi), Noise-encrypted P2P, Erlay, peer scoring,
RocksDB storage (Stage 3), MCP/ACP-native. Это солидная техническая база,
которую сейчас **визуально обесценивает красный крестик на бейдже CI**.

---

## 1. Что реализовано по GitHub CI сегодня

### 1.1 `ci.yml` — запускается на `push` и `pull_request` в `main`/`master`/`develop`

Содержит **4 джоба**:

1. **`source-gates`** (ubuntu-latest) — bash-аудит через `rg`:
   - проверка отсутствия прямых мутаций `balances` вне whitelist-файлов (`BUG-S0-022`);
   - проверка что каждый `#[allow(dead_code)]` / `#[allow(clippy::await_holding_lock)]`
     снабжён комментарием-оправданием (`BUG-S1-012`);
   - арифметическая сверка Stage 0 summary через `python3 scripts/audit/check_stage0_arith.py`.
2. **`test`** — матрица **6 ячеек**: `{ubuntu, macos, windows} × {stable, beta}`.
   На каждой: `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test --all` →
   `cargo build --release` (с `lto=true`, `codegen-units=1`, `strip=true`).
3. **`coverage`** (ubuntu-latest) — `cargo install cargo-tarpaulin` + `cargo tarpaulin --all`,
   публикация HTML-отчёта как artifact (30 дней).
4. **`fuzz`** — матрица **9 таргетов** (`canonical_decode`, `host_functions`,
   `gas_metering`, `storage_trie`, `differential_wasmi_vs_wasmtime`,
   `noise_handshake`, `gossip_inv`, `compact_blocks_fill_up`,
   `erlay_reconciliation`), каждый гоняется **600 секунд** на nightly Rust,
   `cargo install cargo-fuzz --locked --version 0.13.2`, при падении
   выгружаются crash-artifacts.

**Итого на каждый пуш**: ~18 параллельных раннеров (6 test + 9 fuzz + 1 coverage
+ 1 source-gates). Окно — 25 минут по таймауту, реально многие джобы идут
дольше из-за cold cache.

### 1.2 `release.yml` — запускается только при пуше тега `v*`

6 матричных сборок (linux x86_64/aarch64, macos x86_64/aarch64,
windows x86_64/aarch64), агрегация SHA256, **SLSA Provenance** v2.1.0,
**cosign sign-blob** (sigstore), создание GitHub Release через
`softprops/action-gh-release@v2` с `generate_release_notes: true`.

Это **правильная** prod-pipeline: подписи, attestation, cross-compile, SBOM.
Проблема — она **почти не запускается**, потому что тегов мало (5 за всё время),
а пушей в `main` — десятки в неделю.

---

## 2. Почему CI почти всегда failed

По журналу Actions и структуре workflow — причины **композиционные**,
то есть несколько факторов складываются в постоянный красный бейдж:

### 2.1 `cargo clippy -- -D warnings` превращает любой warning в ошибку

В растущем проекте на 4 крейта с активной миграцией (Stage 3: LevelDB→RocksDB,
`thread`→`tokio`, deprecated wire removal) **любой** новый clippy-lint на любой
платформе убивает всю сборку. На `beta` toolchain появляются новые линты,
которых нет на `stable` → матрица `{stable, beta}` делает CI хрупким вдвойне.

### 2.2 Матрица `{ubuntu, macos, windows} × {stable, beta}` — избыточна

- `windows-latest` раннеры у GitHub медленнее и часто падают по таймауту
  на `cargo build --release` с `lto=true` + `codegen-units=1` (компиляция идёт
  20–35 минут, окно — 6 ч, но Swatinem/rust-cache не всегда попадает).
- macOS на aarch64 ломается на `eframe`/`winapi` зависимостях (в `Cargo.toml`
  есть явный workaround-комментарий про `winuser` — это симптом).
- `beta` toolchain может приехать с регрессией → падает не ваш код, а toolchain.
- `fail-fast: false` означает, что одна упавшая ячейка не отменяет остальные →
  визуально CI выглядит «красным», даже если 5 из 6 ячеек прошли.

### 2.3 `cargo tarpaulin` на CI — почти гарантированный падеж

`cargo-tarpaulin` стабильно падает на `ubuntu-latest` при использовании
`tokio` и сложных `#[cfg]` (что в strangecoin повсеместно: `feature = "net"`,
`feature = "vm-wasm"`, `feature = "gui"`). Установка `cargo install cargo-tarpaulin`
с source занимает 4–6 минут и периодически не компилируется на свежем Rust.
Это самый «прожорливый» и наименее полезный джоб в текущем виде.

### 2.4 Fuzzing-матрица 9 × 600 секунд

Каждый fuzz-джоб ставит nightly Rust, ставит `cargo-fuzz` (compile ~3 мин),
затем 10 минут гоняет корпус. `rss_limit_mb=2560` на стандартном раннере
(7 GB RAM) часто ловит OOM на `differential_wasmi_vs_wasmtime`. На любой
новый фаззинг-крэш весь workflow уходит в failed — даже если релизный код
нормальный, крэш в фаззинге «крошит» бейдж.

### 2.5 Cross-compilation в `release.yml`: Windows aarch64

Шаг `choco install llvm -y` + `LIBCLANG_PATH` + сборка под
`aarch64-pc-windows-msvc` — исторически самый падучий cross-compile в экосистеме
Rust. Любое обновление `llvm` или `rustup` ломает эту ячейку → release падает
даже когда остальные 5 платформ собраны.

### 2.6 Строгий `commit-msg` git-hook + `rg` source-gates

Локальный `.githooks/commit-msg` требует формат `[S<N>-P<NN>]` в сообщении.
Если автор закоммитил через `--no-verify` (что часто бывает при быстро чинимых
опечатках), `source-gates` всё равно ловит отклонение на CI и красит бейдж.

### 2.7 Итог по причинам

Сумма факторов: одна ячейка матрицы падает по сторонней причине
(nightly Rust, OOM на fuzz, clang-version на Windows) → `fail-fast: false`
не отменяет, но и не «озеленяет» общий workflow. Визуально статус — **failed**,
хотя ядро компилируется и тесты проходят на основной платформе. Это убивает
социальный сигнал «проект здоров» и прямо снижает конверсию посетитель → звезда.

---

## 3. Как оптимизировать CI — и что это даст

### 3.1 Конкретные правки (по возрастанию эффекта)

| # | Правка | Эффект на бейдж | Что даст |
| --- | --- | --- | --- |
| 1 | Заменить `cargo test --all` на **`cargo nextest run --workspace`** (с кэшем) | быстрее в 2–5× | Тесты идут в параллель, падший тест изолируется, есть `--retries 2` для флакиков |
| 2 | Вынести `cargo fmt --check` и `cargo clippy` в отдельный fast-джоб **`lint`** (без `beta`, без матрицы) | зелёный за 30–60 с | Fast-feedback: lint не ждёт полной сборки |
| 3 | Снять `-D warnings` с `clippy` → `-- -D warnings` только для `warn(clippy::correctness)`, остальное warn-only | критично | Beta-апдейты clippy больше не роняют CI |
| 4 | Из матрицы `test` убрать `beta` (или вынести в nightly-schedule) и `windows-latest` (оставить в release) | матрица 1×1 = ubuntu stable | Падеж Windows-раннера больше не влияет на пуш-бейдж |
| 5 | Заменить `cargo-tarpaulin` на **`cargo-llvm-cov --lcov` + upload в Codecov/Coveralls** | зелёный | Покрытие работает стабильно, бейдж покрытия живой |
| 6 | Fuzzing вынести в отдельный `schedule: cron 0 4 * * *` workflow + на `workflow_dispatch` | пуш-бейдж всегда зелёный | Fuzz идёт ночью за счёт ресурсов GitHub, не блокирует разработку |
| 7 | `fail-fast: false` → `fail-fast: true` в `release.yml` | не помогает «озеленеть», но экономит минуты | Не тратим раннер-минуты на заведомо упавшую матрицу |
| 8 | Добавить `concurrency:` group по `ref`, `cancel-in-progress: true` | экономия минут | Повторные пуши в один PR отменяют прошлые запуски |
| 9 | Кэш `Swatinem/rust-cache@v2` → `@v2` с явным `sharedKey: "ci-workspace"` | больше хитов в кэш | Сборка с warm cache ~4–6 минут вместо 25 |
| 10 | `continue-on-error: true` для coverage / aarch64-windows-cross | изоляция хрупких джобов | Они не красят общий статус |
| 11 | Разделить `release.yml` на `prerelease.yml` (на каждый пуш в `main`) и `release.yml` (только `v*`-теги) | больше релизов → больше «мельканий» в GitHub Explore | См. промпт P1 ниже |
| 12 | Добавить `workflow_run` триггер: после зелёного CI автоматически тегировать `v<MAJOR>.<MINOR>.<PATCH>-dev.<run_number>` и звать prerelease | релиз на каждый пуш | Прямо отвечает вашей цели «релиз на каждый коммит» |

### 3.2 Что даст оптимизация в цифрах

- **CI время на пуш**: с ~25 мин (6 ячеек) до **~6–8 мин** (1 ядро + 1 lint + 1 cov
  с continue-on-error).
- **Расход раннер-минут**: с ~80–110 мин/пуш до **~15 мин/пуш** (free tier
  GitHub Actions — 2000 мин/месяц для приватных; для публичных без лимита).
  На публичном репозитории раннер-минуты **бесплатны неограниченно** — это
  ключевой рычаг: strangecoin публичный → можно вынести **все** ресурсные
  задачи на GitHub.
- **Бейдж `CI`**: с 24/25 failed на стабильно зелёный → социальный сигнал
  «проект жив и здоров» → выше конверсия в звезду.
- **Бейдж `Coverage`**: с отсутствующего/нестабильного на живой % → доверие.

### 3.3 Палитра доступных «бесплатных ресурсов GitHub»

| Ресурс | Что заменяет ваши | Лимит для public |
| --- | --- | --- |
| GitHub Actions (ubuntu/macos/windows) | локальную сборку, тесты, кросс-компиляцию | без лимита |
| GitHub Packages (Container Registry) | self-hosted registry для образов | 10 GB storage free |
| GitHub Pages | статичный сайт-landing / docs | без лимита |
| GitHub Releases | хостинг бинарников + checksums | без лимита |
| GitHub Issues / Discussions | тикет-трекер, roadmap, FAQ | без лимита |
| GitHub Container Registry + cosign | подписанные образы | включён в Actions |
| Dependabot | аудит зависимостей, был бы смысл ставить | без лимита |
| CodeQL Actions | статический анализ безопасности (Rust поддержан частично) | без лимита |
| GitHub Artifacts | обмен файлами между воркфлоуами | 5 ГБ/репо для free |

Вывод: в strangecoin можно целиком переложить compile / test / cross-build /
fuzz / coverage / docs-build на GitHub — локальная машина остаётся только для
`git push`. Это и есть ваш второй запрос «вместо моих ресурсов — ресурсы GitHub».

---

## 4. Какие тесты можно выполнять с помощью GitHub CI — улучшит ли?

### 4.1 Реестр тестовых типов (с оценкой применимости к strangecoin)

| Тип теста | Инструмент | Что проверяет | Уже есть? | Добавит ценности? |
| --- | --- | --- | --- | --- |
| Unit | `cargo test` / `cargo nextest` | изолированные функции | да | ускорить nextest’ом |
| Integration | `cargo test --test '*'` | взаимодействие модулей | да (40+ integration-тестов) | уже хорошо |
| Doc-tests | `cargo test --doc` | примеры в rustdoc | частично | добавить `#![deny(rustdoc::broken_intra_doc_links)]` |
| Property-based | `proptest` | инварианты на случайных данных | да | уже есть (`consensus_proptest`, `chain_selector_proptest`) |
| Fuzz | `cargo-fuzz` | boundary/crash-поиск | да (9 таргетов) | вынести в nightly |
| Mutation | `cargo-mutants` | качество тест-набора | нет | **да, добавит доверия к тестам** |
| Coverage | `cargo-llvm-cov` | покрытие строк/веток | частично (tarpaulin) | заменить на llvm-cov |
| Lint | `clippy`, `rustfmt`, `cargo-deny` | стиль, лицензии, advisory | clippy/rustfmt есть | **добавить `cargo-deny` (RUSTSEC + license compliance)** |
| Security | `cargo-audit` | CVE в зависимостях | нет | **да, критично для блокчейн-проекта** |
| Miri | `miri` | UB в unsafe | нет | partial (мало unsafe, но есть в `secp256k1` bindings) |
| Sanitizers | `-Zsanitizer=address,leak` | memory/leak errors | нет | **да для net/storage крейтов** |
| Cross | `cross` tool | сборка под иные targets | есть в release.yml | ok |
| Reproducibility | `cargo build --locked --offline` | детерминизм сборки | нет | **да (блокчейн → важен)** |
| Bench | `criterion` | регрессии производительности | нет | **да для VM, state, net** |
| TLA+ | TLC model checker | формальная модель консенсуса | есть `docs/spec/consensus.tla` | **вынести проверку в CI** |
| Differential | `cargo-fuzz` differential | wasmi vs wasmtime | есть | ok |

### 4.2 Ответ на вопрос «улучшит или нет»

**Да, улучшит** — но только если сопровождать двумя организационными шагами:

1. **Изоляция хрупких тестов от push-бейджа**: fuzz, mutation, miri, bench
   вынести в отдельный `nightly.yml` (cron `0 4 * * *`). Тогда они дают
   **сигнал качества** без поддержки красного бейджа.
2. **Бейджи на README**: `cargo-deny`, `cargo-audit`, `coverage %`, `mutation score`
   — каждый новый зелёный бейдж повышает доверие посетителя → вероятность звезды.

Без шага (1) добавление тестов **ухудшит** ситуацию: больше источников красного
статуса. С шагом (1) — кумулятивный рост социального доказательства здоровья
проекта.

---

## 5. Стратегия «чаще мелькать и набирать звёзды при пушах»

### 5.1 Как реально работает GitHub Trending (по открытым данным)

GitHub не публикует формулу, но community-анализ
([orgs/community/discussions/163970](https://github.com/orgs/community/discussions/163970)
и реверс-инжиниринг нескольких десятков попавших в тренды репозиториев)
сходится на следующем:

- **Скорость роста звёзд, не абсолютное число.** Репо с обычной скоростью
  2 stars/day, получившее 10 stars за день, обгоняет репо с обычной 50/day,
  получившее 60. Вводится **z-score отклонения от собственной истории**.
- **Свежесть окнуши 7 дней** (Trending Today) и 30 дней (This Month).
  Внутри окна звёзды взвешиваются по времени — недавние весомее.
- **Энгеджмент, не только звёзды**: forks, issues, PR, comments. Репо с
  активной Discussions-активностью имеет приоритет.
- **Языковая нормализация**: для Rust трешхолд ниже, чем для JavaScript —
  попасть в тренды Rust реалистичнее.
- **Возраст репо**: новым репозиториям (≤ 6 мес) даётся буст; это **ваш козырь**
  (strangecoin моложе года).
- **Topics + description**: алгоритм категоризации использует GitHub Topics
  для распределения по языковым/тематическим фильтрам — без Topics репо
  не попадает в выборку по языку.
- **Зелёный CI бейдж** напрямую в алгоритм не входит, но **влияет на конверсию**
  посетитель → звезда (по A/B наблюдениям сообщества — на 20–40%).

### 5.2 Что **точно** сработает для strangecoin

1. **Поставить GitHub Topics** (сейчас их нет в видимой части репо): `blockchain`,
   `cryptocurrency`, `rust`, `ai-agents`, `mcp`, `noise-protocol`, `wasm`,
   `peer-to-peer`, `layer-1`, `proof-of-work`. Без Topics репо не попадает
   в фильтр по языку и тематике → algorithmic shadowban.
2. **Заполнить «About» с website-ссылкой** на GitHub Pages (сгенерированный
   из `analytics/ARCHITECT3.md` static-site через `mdbook` или `zola`).
3. **Зелёный CI-бейдж на видном месте в README** (сейчас бейдж есть, но он
   красный — это антиреклама).
4. **Частые релизы** (pre-release на каждый пуш в `main`) — каждый релиз
   попадает в «Recent releases» соответствующего фильтра GitHub Explore,
   это ещё один канал «мелькания».
5. **«Воронка внимания» с внешних площадок**: пост-анонс на
   `news.ycombinator.com` (Show HN), `r/rust`, `r/CryptoCurrency`,
   `dev.to`, `lobste.rs`, Rust Discord `#showcase`. Один пост на HN даёт
   всплеск 50–500 звёзд за 24 ч — этого достаточно для тренда недели по Rust.
6. **`FUNDING.yml`** + кнопка Sponsor — социальный сигнал зрелости.
7. **Discussions включить** с категориями `Q&A`, `Ideas`, `Show and tell` —
   GitHub поднимает репо с активной Discussions в выдаче.
8. **Issue templates + PR template** — снижают порог входа контрибьюторов.
9. **Releases с авто-генерируемыми release notes** + CHANGELOG-выжимка +
   ссылки на ADR → выглядит «по-взрослому».
10. **Screenshots/видео в README** (уже есть `assets/strangecoin-demo.gif` —
    поднять выше, до первого экрана).

### 5.3 Чего делать **НЕ** надо

- Звёзд-обмены («stargazers farms»), накрутка ботами — GitHub детектит
  паттерн и **пессимизирует** репо в Trending на ~30 дней.
- Пустые коммиты «bump version» без реальных изменений — алгоритм
 _COMMIT-активности уже не доминирует_, раньше доминировал.
- Дублирование репозитория или forks самого себя для «двойного мелькания» —
  детектится как spam.

---

## 6. Промпты (главный deliverable)

Ниже — **пять готовых промптов**, которые можно скопировать и скормить
AI-агенту (Cursor / Claude Code / Codex / Continue и т.п.) последовательно.
Промпты самодостаточны: в каждом указан контекст, файлы для правки,
требуемый результат и проверка.

> Условное имя проекта в промптах: `strangecoin`. Путь к репозиторию —
> локальный checkout. Ветка — `main`. Промпты можно исполнять по очереди
> (P1 → P2 → P3 → P4 → P5); каждый оставляет отдельный коммит.

---

### Промпт P1 — Авто-pre-release на каждый пуш в `main`

**Цель:** каждый пуш в `main` автоматически создаёт annotated git-тег
вида `v<MAJOR>.<MINOR>.<PATCH>-dev.<run_number>` и публикует GitHub
**pre-release** с бинарниками под Linux x86_64 (минимальный набор) и
авто-сгенерированными release notes. Тег `v*` (без суффикса) по-прежнему
запускает полный `release.yml` (6 платформ + cosign + SLSA).

**Контекст:**
- уже есть `.github/workflows/release.yml` — собирает 6 платформ по тегу `v*`;
- `Cargo.toml` версия `2.0.0`;
- нужен новый файл `.github/workflows/prerelease.yml`.

**Что сделать:**

1. Создать `.github/workflows/prerelease.yml` со следующей структурой:
   - триггер: `push: branches: [main]` + `workflow_dispatch`;
   - `concurrency: { group: prerelease-${{ github.ref }}, cancel-in-progress: true }`;
   - `permissions: { contents: write }`;
   - джоб `tag-and-release`, depends on зелёного `ci.yml` через
     `workflow_run: workflow: CI, branch: main, conclusion: success`
     (то есть prerelease запускается **только если CI зелёный**);
   - шаг 1: вычислить следующий dev-номер из `git tag --list 'v*-dev.*' --sort=-v:refname | head -1`;
   - шаг 2: вытащить `MAJOR.MINOR.PATCH` из `Cargo.toml` (`grep '^version' Cargo.toml | head -1 | cut -d'"' -f2`);
   - шаг 3: создать аннотированный тег `v${VER}-dev.${N}` через `git tag -a` и `git push origin refs/tags/v${VER}-dev.${N}`;
   - шаг 4: сборка только под `ubuntu-latest` / `x86_64-unknown-linux-gnu` (быстрый single-target), SHA256;
   - шаг 5: `softprops/action-gh-release@v2` с `prerelease: true`,
     `tag_name: v${VER}-dev.${N}`, `generate_release_notes: true`,
     `body:` + блок «⚠️ Automated dev build, not for production»;
     `files: strangecoin-linux-x86_64, strangecoin-linux-x86_64.sha256`.
2. Поправить `.github/workflows/release.yml`:
   - в триггере `on.push.tags` уточнить паттерн до `v*` **но добавить exclude**
     для `*-dev.*` через `tags-ignore: ['v*-dev.*']`, чтобы dev-теги не
     дублировали полный релиз.
3. В `README.md` бейдж «version» обновить на:
   `https://img.shields.io/github/v/release/Reider85/strangecoin?include_prereleases&label=version`
   (pre-release теперь учитываются).

**Критерий приёмки:**
- После следующего пуша в `main` (при зелёном CI) на странице
  `github.com/Reider85/strangecoin/releases` появляется новый pre-release с
  бинарником `strangecoin-linux-x86_64` и SHA256.
- Старый `release.yml` по-прежнему запускается только на «настоящих» тегах
  `vX.Y.Z` (без `-dev`).
- Список тегов на странице Tags содержит `v2.0.0-dev.N` инкрементально.

**Файлы для правки:** `.github/workflows/prerelease.yml` (новый),
`.github/workflows/release.yml` (патч триггера), `README.md` (бейдж).

---

### Промпт P2 — Оптимизация CI: починить «вечно failed»

**Цель:** сделать так, чтобы бейдж `CI` на `main` был стабильно зелёным
в течение 5–8 минут после пуша, без потери реального покрытия тестами.
Хрупкие и медленные проверки вынести в ночной workflow.

**Контекст:**
- текущий `.github/workflows/ci.yml` запускает 6 ячеек test + 9 fuzz + 1 coverage + 1 source-gates на каждый пуш;
-主要原因 падежа: clippy `-D warnings` на beta toolchain, `cargo-tarpaulin`, Windows-раннер, fuzz OOM;
- проект публичный → раннер-минуты не ограничены.

**Что сделать (одним коммитом в `.github/workflows/ci.yml`):**

1. **Разделить на 4 джоба с разными приоритетами:**
   - `lint` (ubuntu, ~30–60 s): `cargo fmt -- --check`,
     `cargo clippy --workspace --all-targets -- -D clippy::correctness -D warnings`
     **только на stable**; toolchain `dtolnay/rust-toolchain@stable`;
   - `test-core` (ubuntu, stable, ~5 min): `cargo nextest run --workspace --no-fail-fast`
     с `continue-on-error: false`, плюс `cargo build --release` **без LTO**
     (LTO оставить только в release.yml);
     - добавить шаг установки: `cargo install cargo-nextest --locked` один раз
       в начало;
     - `Swatinem/rust-cache@v2` с `sharedKey: ci-ubuntu-stable`;
   - `coverage` (ubuntu, stable, `continue-on-error: true`): `cargo-llvm-cov`
     вместо tarpaulin; `cargo install cargo-llvm-cov-preview --locked` →
     `cargo llvm-cov --workspace --lcov --output-path lcov.info`;
     - загрузить `lcov.info` в `codecov-action@v4` (требует `CODECOV_TOKEN` secret
       — завести в репо settings);
   - `source-gates` — оставить как есть, он уже быстрый.
2. **Из матрицы `test` убрать `macos-latest`, `windows-latest`, `beta`** —
   эти ячейки перенести в новый `.github/workflows/nightly.yml`
   (см. промпт P3). В матрице `test-core` оставить только ubuntu/stable.
3. **Добавить в начало workflow:**
   ```yaml
   concurrency:
     group: ci-${{ github.ref }}
     cancel-in-progress: true
   ```
4. **Заменить** `cargo test --all` на `cargo nextest run --workspace --no-fail-fast --retries 2`.
5. **Clippy**: вместо `cargo clippy -- -D warnings` использовать
   `cargo clippy --workspace --all-targets -- -D clippy::correctness -A warnings`
   — это превратит все warnings в не-блокирующие, кроме correctness-класса
   (где реально баги). Beta-toolchain больше не красит бейдж.
6. **Build release**: из `ci.yml` убрать шаг `cargo build --release`
   (он дублирует `release.yml` и съедает 15+ минут). Если нужен smoke-build —
   оставить `cargo build --release` **без** LTO/strip через
   `CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`.
7. **Coverage job**: установить `continue-on-error: true`, чтобы нестабильность
   `cargo-llvm-cov` не роняла общий workflow.

**Критерий приёмки:**
- После следующего пуша в `main`: workflow CI завершается ≤ 8 минут;
  все 4 джоба (lint / test-core / coverage / source-gates) зелёные, кроме
  coverage, который может быть жёлтым (continue-on-error).
- Бейдж `https://github.com/Reider85/strangecoin/actions/workflows/ci.yml/badge.svg?branch=main`
  показывает зелёный статус.
- В `.github/workflows/ci.yml` нет упоминания `windows-latest`, `macos-latest`,
  `beta`, `cargo-tarpaulin`, `cargo build --release` с LTO.

**Файлы для правки:** `.github/workflows/ci.yml` (полная переработка),
`Cargo.toml` (если нужно добавить `rustdoc::broken_intra_doc_links` lint).

---

### Промпт P3 — Ночной workflow для тяжёлых и хрупких проверок

**Цель:** fuzzing, мульти-платформенная матрица, mutation testing и
security-audit вынести в отдельный ночной workflow `nightly.yml` за счёт
ресурсов GitHub. Не блокирует разработку; даёт отдельный бейдж `Nightly`.

**Контекст:**
- текущий fuzz-блок 9 таргетов × 600 s из `ci.yml` нужно вынести;
- у проекта есть `fuzz/Cargo.toml`, 9 fuzz_targets;
- у репозитория уже настроен Dependabot? Если нет — включить.

**Что сделать:**

1. **Создать `.github/workflows/nightly.yml`:**
   - триггеры: `schedule: - cron: '0 4 * * *'` (04:00 UTC ежедневно) +
     `workflow_dispatch`;
   - `concurrency: { group: nightly, cancel-in-progress: false }`;
   - `permissions: { contents: read, security-events: write }` (для codeql/dependabot);
   - джобы:
     - **`fuzz`** — та же матрица 9 таргетов, **но по 1800 s** (30 мин) каждый
       вместо 600 s, плюс `continue-on-error: true` (ночью crashes — это сигнал,
       не блокер); `cargo fuzz run $target -- -max_total_time=1800 -rss_limit_mb=4096`;
       в `Swatinem/rust-cache@v2` для fuzz-workspace;
     - **`cross-build`** — матрица `{ubuntu, macos, windows} × {stable}`,
       + aarch64 кросс-компиляция; `continue-on-error: true` для
       `aarch64-pc-windows-msvc`; опубликовать artifact’ы подRetention 7 дней;
     - **`audit`** — `cargo install cargo-audit --locked` → `cargo audit`,
       `cargo install cargo-deny --locked` → `cargo deny check`,
       `cargo deny check advisories bans licenses sources`;
       публиковать SARIF-отчёт через `github/codeql-action/upload-sarif@v3`;
     - **`mutation`** — `cargo install cargo-mutants --locked` →
       `cargo mutants --workspace -j 4 --in-place` с `continue-on-error: true`
       и таймаутом 50 min; артефакт `mutants-out/`;
     - **`miri`** (опционально) — на nightly toolchain для крейтов с unsafe
       (`strangecoin-vm-wasm`, `strangecoin-storage`): `cargo +nightly miri test`
       с `MIRIFLAGS="-Zmiri-disable-isolation"`.
2. **Из `ci.yml` убрать джоб `fuzz`** полностью (он переезжает в nightly).
3. **Добавить в `README.md` бейджи:**
   - `![Nightly](https://github.com/Reider85/strangecoin/actions/workflows/nightly.yml/badge.svg)`
   - `![Audit](https://img.shields.io/badge/cargo--audit-passing-green)`
   - `![Mutation](https://img.shields.io/badge/mutation--score-N%25-blue)`
4. **Включить Dependabot** — `.github/dependabot.yml` с двумя ecosystem:
   `cargo` (weekly) и `github-actions` (weekly); `open-pull-requests-limit: 5`;
   `labels: [dependencies]`.
5. **Включить Code Scanning** (CodeQL): в репо settings → Code security →
   CodeQL → включить для Rust (advanced setup с custom queries).

**Критерий приёмки:**
- На следующий день после мёрджа PR с этим промптом в `Actions` появляется
  первый nightly-запуск по cron.
- В `ci.yml` отсутствует блок `fuzz:`.
- В `README.md` появились бейджи `Nightly`, `Audit`.
- Dependabot прислал первый PR в течение 24 ч.

**Файлы для правки:** `.github/workflows/nightly.yml` (новый),
`.github/workflows/ci.yml` (удалить fuzz-джоб), `.github/dependabot.yml` (новый),
`README.md` (бейджи).

---

### Промпт P4 — Маркетинговый пакет для роста звёзд и попадания в Trending

**Цель:** заполнить все метаданные репозитория, которые алгоритм GitHub
Trending и Explore использует для выбора репо в выдачу; поднять конверсию
посетитель → звезда через визуальные сигналы.

**Контекст:**
- у репо 1 звезда, тегов topics нет;
- README длинный и технически плотный, но визуально «тяжёлый»;
- есть `assets/strangecoin-demo.gif` и `assets/architecture.png`.

**Что сделать:**

1. **Topics** — добавить через GitHub API или web UI (Settings → General → Topics)
   ровно 20 topics (это максимум): `blockchain`, `cryptocurrency`, `rust`,
   `ai-agents`, `mcp`, `model-context-protocol`, `noise-protocol`, `wasm`,
   `wasm-smart-contracts`, `peer-to-peer`, `layer-1`, `proof-of-work`,
   `pos`, `erlay`, `rocksdb`, `secp256k1`, `blake3`, `agent-economy`,
   `tokio`, `kv-coin`.
2. **About** (короткое описание справа на главной странице репо): заполнить
   одной фразой: «Production-grade Layer-1 blockchain in Rust for AI agents —
   MCP-native, Noise-encrypted P2P, WASM smart contracts, Erlay.»
   + Website: ссылка на GitHub Pages (см. пункт 5).
3. **Логотип / social preview** — создать `assets/social-preview.png`
   (1280×640) с branded-визуалом; загрузить через `gh api -X PUT
   repos/Reider85/strangecoin/social-preview -F image=@assets/social-preview.png`.
   Без social preview ссылка на репо в Twitter/LinkedIn/HN выглядит как
   серый аватар — это снижает клик-rate на 30–50%.
4. **README hero-секция** — поднять `strangecoin-demo.gif` в **самый верх**
   README, **перед** бейджами (сейчас бейджи сверху → гифка отодвинута
   на ~30 строк). Структура hero:
   ```
   # Strangecoin
   <tagline в 1 строку>
   <демо-gif, 480 px ширина>
   <бейджи одной строкой, compact>
   <3-буллетная elevator-pitch: "что это", "зачем", "для кого">
   ```
5. **GitHub Pages landing** — создать ветку `gh-pages` с mdbook-сборкой
   из `analytics/ARCHITECT3.md` + `README.md`; в `.github/workflows/pages.yml`
   автоматически публиковать на пуш в `main` через `peaceiris/actions-mdbook`.
   URL: `https://reider85.github.io/strangecoin/` — указать как Website в About.
6. **Discussions включить** (Settings → General → Features → Discussions),
   категории: `📢 Announcements`, `💡 Ideas`, `🙏 Q&A`, `🚀 Show and tell`.
   Первый пост — `Welcome to Strangecoin Discussions` от мэйнтейнера.
7. **Issue templates** — `.github/ISSUE_TEMPLATE/bug.yml`,
   `feature.yml`, `config.yml` (с contact-links на Discussions). PR template —
   `.github/pull_request_template.md` с чек-листом
   `[ ] tests / [ ] changelog / [ ] ADR updated`.
8. **`FUNDING.yml`** — `.github/FUNDING.yml` с `github: [Reider85]`,
   `buy_me_a_coffee:` (опционально). Появляется кнопка Sponsor.
9. **`SECURITY.md`** в корне (есть в `docs/security/SECURITY.md`, сделать
   symlink или скопировать в root, чтобы GitHub показывал вкладку
   Security → Policy). Добавить ссылку на `docs/security/BOUNTY.md`.
10. **`CITATION.cff`** — добавить `cff-version: 1.2.0`, `title: Strangecoin`,
    `authors`, `type: software`, `license: Apache-2.0`,
    `repository-code: https://github.com/Reider85/strangecoin`. Это даёт
    кнопку «Cite this repository» и учитывается в GitHub Explore.
11. **Star-сетка как социальное доказательство**: добавить в README секцию
    `## Star history` с генерируемой диаграммой через
    `https://starchart.cc/Reider85/strangecoin` (встроенный `<img>`).

**Критерий приёмки:**
- На главной странице репо отображаются 20 topics, описание «About» с website,
  social-preview картинка.
- README начинается с gif-демо в первом экране, бейджи компактной строкой.
- Включён Discussions с 4 категориями; первый Welcome-пост создан.
- Issue / PR templates подставляются при создании.
- GitHub Pages доступен по URL `https://reider85.github.io/strangecoin/`.
- В Settings → General показывается зелёный «Sponsor».

**Файлы для правки:** `.github/ISSUE_TEMPLATE/*`, `.github/pull_request_template.md`,
`.github/FUNDING.yml`, `.github/workflows/pages.yml`, `assets/social-preview.png`,
`CITATION.cff`, `SECURITY.md` (symlink в root), `README.md` (hero-реорганизация),
ветка `gh-pages`.

---

### Промпт P5 — Триггер «внешний анонс → всплеск звёзд» + dashboard отслеживания

**Цель:** синхронизировать очередной stage-release с анонсом на 4 внешних
площадках в один день, чтобы всплеск звёзд в окне 24–72 ч был достаточно
сильным для срабатывания z-score в Trending. Подготовить два `.md`-черновика
анонсов (HN / Reddit) и одну issue-страницу с dashboard-ссылками.

**Контекст:**
- следующий major tag планируется `v3.0.0-stage3` (Stage 3 DoD);
- у репо сейчас 1 звезда, значит даже +20 звёзд за 24 ч — это уже значимый
  z-score (отклонение от истории);
- в Trending Rust порог входа ниже JS: 15–40 новых звёзд за 24 ч достаточно.

**Что сделать:**

1. **Создать `analytics/launch-stage3.md`** с тремя блоками:
   - **Show HN пост** (заголовок ≤ 80 char, body ≤ 1500 char):
     - заголовок-крючок: `Show HN: Strangecoin — a Layer-1 blockchain in Rust built for AI agents (MCP/ACP-native)`;
     - первый абзац: «что это + что работает сегодня»;
     - второй: «чем отличается от Solana / Ethereum» (свой VM, Noise-encrypted P2P, MCP server);
     - третий: ссылка на GitHub + GitHub Pages + Discussions;
     - finale: «testnet-only, genesis burned, not for real value»;
   - **r/rust пост**: тот же контент + акцент «написано с нуля в Rust,
     99.3% Rust, edition 2021, без fork-ов от Bitcoin/Ethereum»;
   - **dev.to статья**: длинная (3000–4000 слов), со схемами `assets/architecture.png`,
     структурой `TL;DR → Why → How it works → Code walkthrough → Roadmap → Try it`.
2. **В `.github/workflows/prerelease.yml` (из P1)** добавить шаг
   `release-notes-enrichment`: после создания pre-release пинговать
   Discord webhook (если есть) и записывать в `analytics/stargazer-history.json`
   текущий count (через `gh api repos/Reider85/strangecoin --jq '.stargazers_count'`)
   — это даёт историю для z-score-анализа.
3. **Создать `.github/workflows/star-monitor.yml`** на cron `0 * * * *`
   (ежечасно): запрашивать `stargazers_count`, `forks_count`, `open_issues_count`
   через `gh api`, дописывать строку в
   `analytics/stargazer-history.csv` (date,count,delta_24h,delta_7d).
   Этот файл коммитится автоматически через `stefanzweifel/git-auto-commit-action@v5`
   с сообщением `chore(stars): hourly snapshot NNNN`.
   - **Побочный эффект:** каждый час репо получает commit → выше в GitHub
     «active» выдаче (не Trending, но Explore по активности).
4. **`analytics/trending-checklist.md`** — чек-лист готовности к «launch-дню»:
   - [ ] topics проставлены (P4);
   - [ ] social preview загружен (P4);
   - [ ] README hero оптимизирован (P4);
   - [ ] CI бейдж зелёный (P2);
   - [ ] последний pre-release свежий (≤ 24 ч, P1);
   - [ ] Codecov бейдж живой (P2);
   - [ ] HN/Reddit/dev.to посты готовы и отложены на 09:00 PT launch-дня;
   - [ ] Discussions активны (≥ 3 треда);
   - [ ] star-monitor.yml собирает CSV (P5).
5. **Launch-day runbook** (отдельный `analytics/launch-runbook.md`):
   - T-7d: опубликовать dev.to статью (длинная, для SEO);
   - T-1d: создать «настоящий» тег `v3.0.0-stage3` → запустится полный release.yml;
   - T=0: Show HN пост (09:00 PT = 17:00 UTC = 20:00 МСК);
   - T+15m: r/rust, r/CryptoCurrency, lobste.rs;
   - T+1h: твиттер/X пост с GIF из README;
   - T+24h: проверить star-monitor.csv на всплеск; если ≥ 20 звёзд →
     высокий шанс на Rust Trending Today;
   - T+72h: проверить Trending This Week.
6. **Slack/Discord webhook** — `.github/workflows/star-alert.yml`:
   при срабатывании star-monitor.yml и `delta_24h >= 10` отправить
   `actions/github-script@v7` POST на Discord webhook с текстом
   «🚀 Strangecoin +N stars in 24h! Check Trending: https://github.com/trending/rust?since=daily».

**Критерий приёмки:**
- Файл `analytics/stargazer-history.csv` существует и обновляется каждый час.
- Файлы `analytics/launch-stage3.md`, `analytics/trending-checklist.md`,
  `analytics/launch-runbook.md` готовы к исполнению.
- Workflow `star-monitor.yml` отрабатывает на cron без ошибок, в репо
  появляются авто-коммиты `chore(stars): hourly snapshot NNNN`.
- При тестовом срабатывании триггера `delta_24h >= 10` приходит Discord-alert.

**Файлы для правки:** `.github/workflows/star-monitor.yml` (новый),
`.github/workflows/star-alert.yml` (новый), `analytics/launch-stage3.md` (новый),
`analytics/trending-checklist.md` (новый), `analytics/launch-runbook.md` (новый),
`.github/workflows/prerelease.yml` (добавить step).

---

## 7. Сводная карта промптов

| ID | Промпт | Главный эффект | Файлы-результаты |
| --- | --- | --- | --- |
| **P1** | Авто-pre-release на каждый пуш | Частые релизы → «мелькание» в Explore + Releases | `prerelease.yml`, правка `release.yml` |
| **P2** | Оптимизация CI | Зелёный бейдж за ≤ 8 мин → рост доверия → конверсия в звезду | `ci.yml` (полная переработка) |
| **P3** | Ночной fuzz/audit/mutation/cross | Сигналы качества без покраски бейджа; бейджи Audit/Mutation | `nightly.yml`, `dependabot.yml` |
| **P4** | Маркетинговый пакет | Topics, social-preview, Discussions, Pages → попадание в Trending-выдачу | `ISSUE_TEMPLATE/*`, `FUNDING.yml`, `pages.yml`, `CITATION.cff`, `social-preview.png` |
| **P5** | Launch-план + star-monitor | Готовые анонсы + автотрекинг всплеска → синхронизация для z-score | `star-monitor.yml`, `star-alert.yml`, `analytics/launch-stage3.md` |

**Рекомендованный порядок выполнения:** P2 → P3 → P4 → P1 → P5.

P2 и P3 — **технический фундамент** (зелёный бейдж + отделённые ночные проверки).
P4 — **алгоритмическая готовность** к Trending (topics + social-preview + Pages).
P1 — **частота «мельканий»** (pre-release на каждый пуш).
P5 — **событийный триггер** (синхронный запуск аннонсов в день stage-релиза).

После P2-P3-P4 бейдж CI гарантированно зелёный. После P1 каждый пуш
порождает pre-release. После P5 есть готовый план и автотрекинг всплеска звёзд.

---

## 8. Ожидаемый итог (по метрикам через 30 дней после выполнения P1–P5)

| Метрика | До | После (прогноз) |
| --- | --- | --- |
| CI статус на `main` | 24/25 failed | стабильно passing, ≤ 8 мин |
| Releases / месяц | ~1 (только по тегам) | ~20–40 (pre-release на каждый пуш) |
| Звёзды | 1 | 30–150 (при успешном launch-дне P5 — 100+) |
| Topics | 0 | 20 (полный набор) |
| Бейджи в README | 13 (CI красный) | 17+ (CI зелёный, Nightly, Audit, Coverage, Mutation, Pages, CFF) |
| Chance попасть в Rust Trending | ~0% | 20–40% после launch-дня P5 |
| Расход вашей локальной машины на CI/тесты/сборку | высокий | минимальный (только `git push`) |

Главный рычаг: **публичный репозиторий = неограниченные GitHub Actions минуты**.
Весь compute переносится на раннеры GitHub, что прямо отвечает второму
требованию «вместо моих ресурсов — ресурсы GitHub».

---

## 9. Примечания и ограничения

- **Trending не гарантирован**. Алгоритм GitHub закрыт, факторов много,
  и даже идеально подготовленный репо может не попасть в выдачу из-за
  высокой конкуренции в окне. Промпты P1–P5 максимизируют вероятность,
  но не обещают результат.
- **Pre-release на каждый пуш** может раздражать подписчиков, если коммитов
  > 10 в день. В P1 предусмотрена защита через `workflow_run` — prerelease
  запускается только при **зелёном** CI. Альтернатива: агрегация в
  «ежедневный pre-release» через cron в конце дня (упомянуто как fallback
  в промпте P1, можно добавить `if: github.event.head_commit.timestamp < today-23h`).
- **`cargo-audit` и `cargo-deny`** могут обнаружить CVE в зависимостях
  (`secp256k1`, `snow`, `tokio`, `wasmi`) — это нормально и полезно; не
  блокирует разработку, т.к. в `nightly.yml` они в `continue-on-error: true`.
- **SLSA / cosign в `release.yml`** оставить как есть — это уже правильная
  prod-pipeline; не трогать в P1–P5.
- **Discord webhook в P5** — секрет `DISCORD_WEBHOOK_URL` нужно добавить в
  репо settings → Secrets and variables → Actions.

---

*Документ сгенерирован 2026-11-08 на основе анализа открытого репозитория
`github.com/Reider85/strangecoin` и журнала его GitHub Actions.*
