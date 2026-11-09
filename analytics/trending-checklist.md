# Trending launch checklist (GITHUB.md P5, item 4)

> Чек-лист готовности к launch-дню `v3.0.0-stage3`. Порядок действий —
> в `launch-runbook.md`; черновики постов — в `launch-stage3.md`.
>
> Статусы ниже проставлены **на 2026-11-09** по live-проверке
> (GitHub API + репозиторий) в рамках выполнения P5. Пункты,
> зависящие от repo settings или от живого запуска CI, помечены
> причиной — их нельзя закрыть из кода.

## Чек-лист

- [x] **Topics проставлены (P4)** — ✅ 18 topics присутствуют
      (`blockchain`, `rust`, `mcp`, `p2p`, `cryptocurrency`, `testnet`…;
      проверено через API 2026-11-09). Максимум 20 — свободны 2 слота.
- [ ] **Social preview загружен (P4)** — ❌ `GET .../social-preview` → 404,
      картинка не загружена (файл `assets/social-preview.png` в репо есть).
      Ручной шаг: `gh api -X PUT repos/Reider85/strangecoin/social-preview
      -F image=@assets/social-preview.png` или Settings → General →
      Social preview. **Блокер воронки HN/Twitter** (серый аватар −30–50% CTR).
- [x] **README hero оптимизирован (P4)** — ✅ гифка на первом экране
      перед бейджами (коммит `209a707`).
- [x] **CI бейдж зелёный (P2)** — ✅ последний запуск CI на `master` —
      run #153 `success` (2026-11-09T17:17Z). Примечание: запуски на
      PR (dependabot) могут быть красными — бейдж отслеживает `master`.
- [ ] **Последний pre-release свежий (≤ 24 ч, P1)** — ❌ **ни одного
      `-dev.*` pre-release не существует**: `prerelease.yml` падает на шаге
      «Create and push annotated dev tag» (запуски #2/#3/#4, последний
      2026-11-09T17:25Z). Известный дефект P1, **вне скоупа P5** —
      починить до T-1d (запуск полного `release.yml` на теге от этого не
      зависит, но канал «мельканий» через Releases будет мёртв).
- [ ] **Codecov бейдж живой (P2)** — ❌ в README бейдж отсутствует
      (сам upload llvm-cov в ci.yml есть, `fail_ci_if_error: false`).
      Опциональный шаг: добавить бейдж
      `https://codecov.io/gh/Reider85/strangecoin/branch/master/graph/badge.svg`
      после первого успешного upload (нужен `CODECOV_TOKEN` в secrets).
- [x] **HN / Reddit / dev.to посты готовы** — ✅ все три черновика в
      `analytics/launch-stage3.md` (HN title 75/80 char, body 1080/1500;
      dev.to ≈ 3200 слов). «Отложены на 09:00 PT launch-дня» — ручное
      действие по runbook (T=0 / T+15m), автопостинга нет.
- [ ] **Discussions активны (≥ 3 тредов)** — ❌ **Discussions на репо
      выключены**: проверка `github.com/Reider85/strangecoin/discussions`
      вернула **404** (P4, пункт 6 — settings-тумблер не был выполнен). Ручной шаг: Settings → General → Features →
      Discussions, категории `Announcements` / `Ideas` / `Q&A` /
      `Show and tell`, первый Welcome-пост → затем ≥ 3 треда до launch-дня.
- [x] **star-monitor.yml собирает CSV (P5)** — ✅ workflow создан в этом
      промпте; CSV посеян (`analytics/stargazer-history.csv`, baseline
      1 star / 0 forks / 9 open issues). Первый hourly-снапшот — после
      пуши в `master` + первый cron (см. runbook T-7d).

## Минимальный goto-go (если чинить только блокеры)

Перед T-днём обязательно закрыть три пункта:

1. **Pre-release канал (P1)** — починить push тега в `prerelease.yml`
   (сейчас падает; иначе Releases-канал мёртв).
2. **Social preview** — загрузить `assets/social-preview.png` (иначе все
   внешние ссылки выглядят как серый аватар).
3. **Discussions включить** — иначе ссылка из HN/dev.to-постов ведёт в 404.

Остальное — усилительное, не блокирующее.
