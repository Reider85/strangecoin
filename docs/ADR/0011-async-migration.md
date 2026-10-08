# ADR-0011: Полная async-миграция mining и P2P (закрытие гибридной модели ADR-0007)

## Status

Accepted (2026-10-08) — исправление BUG-S0-021 (S2-P01 + S2-P02)

## Context

ADR-0007 ввёл tokio на Stage 1 гибридно: рантайм владеет процессом, но mining worker, TCP accept и per-connection обработчики намеренно остались на `std::thread` («старые threads живут до Stage 2»). К моменту закрытия Stage 1 (STAGE1_SUMMARY §5) это зафиксировано как deferred-work:

| Подсистема | Оставалась на | Место |
|---|---|---|
| Mining worker | `thread::spawn` + `mpsc::Receiver<MiningTask>` | `src/lib.rs` `Node::new()` |
| TCP accept | `thread::spawn` + `listener.incoming()` | `src/lib.rs` `Node::start_server()` |
| Per-connection | `thread::spawn` на каждый TCP-поток | `src/lib.rs` `Node::start_server()` |
| Headers-first sync | `std::net::TcpStream` + `set_read/write_timeout` | `src/network/sync.rs` |
| Outgoing gossip/sync | блокирующий `TcpStream::connect_timeout` | `src/lib.rs` `Node::sync_blockchain()` |

Смежные дефекты, которые миграция закрывает попутно:

1. **Токующий вызов в async-контексте.** Tokio sync-polling задача (`run_async`) каждую секунду вызывает блокирующий `sync_blockchain()` (fs I/O + TCP) прямо на runtime-воркере; mining-тред после находки блока — тот же блокирующий вызов inline.
2. **Write-lock на всё время PoW-поиска.** `BlockchainFacade::mine_block` держит `RwLock<Blockchain>` в write-режиме весь nonce-search; любой async-потребитель facade (SyncEngine, serve-headers) получает starvation. Полное устранение (mine на snapshot + CAS-commit) — консенсус-смежное и выносится в отдельный follow-up; здесь блокирующий код изолируется в `spawn_blocking`, чтобы как минимум не занимать воркеры рантайма.
3. **Скрытый дефект network_id.** Mining-тред после находки строил временный `Node` с жёстко зашитым `CHAIN_ID_REGTEST` и вызывал `sync_blockchain()` — gossip шёл с неверным network_id на не-regtest сетях.
4. **Thread-per-connection не масштабируется** до Stage 2 (Noise, Erlay, gossip broadcast всем пирам) — аргумент ADR-0007 §Context п.1 остаётся в силе и является прямым блокером сетевой зрелости.
5. **Гибридная модель — платящий техдолг.** Три механизма конкурентности сосуществуют (`std::thread`, `tokio::spawn`, crossbeam); ADR-0007 §Consequences/Negative явно назвал это временным.

## Decision

Переносим **мининг-воркер** и **весь P2P-стек ноды** с legacy-threads на tokio (S2-P01 + S2-P02). Ключевые решения:

- **Mining worker → tokio-задача.** `Node::new()` спавнит воркер через dual-mode (по образцу `sync_engine::spawn`): `Handle::try_current()` → `tokio::spawn`, иначе dedicated `std::thread` c current-thread runtime (совместимость с `#[test]`-контекстами без рантайма). Канал `MiningTask` — `tokio::sync::mpsc::UnboundedSender/Receiver` (GUI шлёт из blocking-потока eframe через синхронный `unbounded_send`).
- **CPU-bound и lock-bound секции — через `tokio::task::spawn_blocking`.** `apply_tx` + `mine_block` (write-lock + PoW-search) выполняются в blocking-пуле, не на воркерах рантайма. `mine_block` **не переписывается** — модель write-lock на время поиска сохраняется (см. Follow-up).
- **Пост-mine гossip — `await` асинхронного `sync_blockchain`.** Временный `Node` с hard-coded `CHAIN_ID_REGTEST` удаляется; network_id берётся из воркера.
- **TCP accept и per-connection → `tokio::net`.** `start_server` становится `async fn`: `tokio::net::TcpListener::bind`, accept-задача и per-connection-задачи через `tokio::spawn`; ручные `set_read/write_timeout` заменяются на `tokio::time::timeout(SYNC_IO_TIMEOUT)`.
- **`sync_headers_first` и `sync_blockchain` → async.** `src/network/sync.rs` переписывается на `tokio::net::TcpStream` + async-фрейминг; синхронный вариант удаляется. Чистые функции (`HeaderCache`, `plan_best_branch`, `walk_ancestry`) не затрагиваются. Gossip по-прежнему только пушит кандидатов в `Inbox` (ADR-0010) — инвариант «write-пути сети только в `sync_engine.rs`» сохраняется.
- **Async-фрейминг — в `protocol.rs` рядом с синхронным.** `read/write_length_prefixed_async` поверх `tokio::io::{AsyncReadExt, AsyncWriteExt}`; синхронные generic-версии сохраняются для интеграционных тестов с raw `TcpStream`. Чистые codec-функции (byte-slice) не меняются — **wire-протокол не меняется**.
- **Shutdown-флаг остаётся `Arc<AtomicBool>`** с polling-опросом (`interval`/`select!`). Причина: флаг — часть facade-API (`mine_block_inner` поллит его внутри nonce-loop, сигнатуры в `core`-смежном коде) и читается из GUI/тестов; миграция на `CancellationToken` — отдельный охватывающий рефакторинг (Follow-up). P17-семантика (30 s + `process::exit(0)`) сохраняется.
- **Остановка сервера — `abort()` JoinHandles в `Node::Drop`.** Модель «держать `TcpListener` в Mutex ради Drop» заменяется хранением `JoinHandle` accept-задачи; `Drop` синхронно вызывает `abort()` (достаточно для process-teardown; drain не требуется — флаг шатдауна и так останавливает циклы).
- **`sync_tx` (ChainSnapshot) → `tokio::sync::mpsc::UnboundedSender/Receiver`.** Последний std mpsc в пайплайне node-слоя; GUI-дрейн работает через синхронный `try_recv`.
- **`RateLimiter` и `SeenCache` остаются на std `Mutex`.** Короткие критические секции без I/O между lock/unlock — менять на tokio-mutex бессмысленно; зафиксировано как осознанное решение.
- **Legacy-текстовый протокол `GET_BLOCKCHAIN` остаётся** (асинхронизированный). Удаление текстовых сообщений — отдельное изменение протокола (ARCHITECT3 §3 Stage 2), не входящее в этот change set.
- **Гигиена включена:** удаляется мёртвый второй `network::Node` (`src/network/mod.rs`, нет вызовов вне файла); fs-I/O в `discover_peers`/`add_peer` выносится за пределы peers-`Mutex`.

## Consequences

### Positive

- **Гибридная модель ADR-0007 закрыта.** В `src/` не остаётся production `thread::spawn` (кроме dual-mode fallback для тестов без рантайма) — один механизм конкурентности для node-слоя.
- **Runtime-воркеры больше не блокируются.** Блокирующие вызовы (mine, gossip, peer discovery) изолированы в `spawn_blocking`/коротких секциях; sync-polling задача awaits, а не блокирует.
- **Таймауты единообразны.** `tokio::time::timeout` вместо `set_read/write_timeout`/`connect_timeout` — таймауты можно комбинировать с shutdown-флагом через `select!`, accept-цикл реагирует на шатдаун без входящего соединения.
- **Сотни одновременных P2P-соединений** планируются рантаймом, а не ОС-тредами — снимается блокер Stage 2 (Noise, Erlay, gossip broadcast).
- **Исправлен скрытый дефект network_id** в post-mine gossip (hard-coded REGTEST).
- **Peers-mutex больше не держится на fs-I/O** — читатели peers не ждут disk.

### Negative

- **Изменяется много тестов.** Интеграционные тесты сети (`network`, `network_id`, `sync_headers`, `events`) конвертируются в `#[tokio::test]` — стоимость регрессий на миграции тестов; минимизируется сохранением dual-mode спавна и поведенческой эквивалентностью async-версий.
- **`Node::Drop` становится агрессивнее** (`abort()` без drain задач). Риск «обрубить» in-flight запись минимален: LevelDB-коммиты синхронны внутри blocking-секций, а флаг шатдауна и 30-секундный бюджет P17 дают задачам корректно завершиться до `process::exit`.
- **Write-lock на время PoW сохраняется** (изолирован, но не устранён) — читатели facade по-прежнему ждут конца поиска блока.
- **Unbounded-каналы `MiningTask`/`sync_tx`** не имеют backpressure (как и прежние unbounded std mpsc — поведение не ухудшено, но и не улучшено).

### Neutral

- **Wire-протокол не меняется** — framing, codec-функции, форматы сообщений, rate-limiting-семантика идентичны Stage 1.
- **`strangecoin-core` не затронут** — `rg "tokio" crates/strangecoin-core/src` → 0 остаётся обязательным инвариантом.
- **Lock ordering blockchain→wallet** (src/error.rs) не меняется; `tests/concurrency.rs` — канонический gate.
- **Sync-engine write-пути** остаются только в `sync_engine.rs` — миграция не добавляет новых вызовов `adopt_candidate`/`apply_tx`/`save_state` из сети.
- **`run()` sync-обёртка и dual-mode спавн** сохраняются — утилиты и `#[test]`-контексты без рантайма работают как раньше.

## Alternatives

### Alternative 1: Оставить гибрид (ничего не делать)

- Pros: ноль риска регрессий.
- Cons: BUG-S0-021 остаётся open; thread-per-connection и блокирующие воркеры — прямой блокер Stage 2 (Noise/Erlay/gossip); техдолг растёт пропорционально новому коду, написанному в гибридной модели.
- Why not chosen: стоимость миграции растёт со временем; Stage 2 всё равно потребует переноса — чем позже, тем дороже.

### Alternative 2: Только S2-P01 (mining) без сети

- Pros: меньший change set.
- Cons: mining-после-find вызывает `sync_blockchain()` — блокирующая сеть осталась бы внутри mining-пути; выигрыш ограничен, половина debt остаётся.
- Why not chosen: переносы связаны; раздельное выполнение удваивает стоимость (двойная миграция call-site'ов).

### Alternative 3: Big-bang с `CancellationToken` + удалением legacy-протокола + redesign write-lock одновременно

- Pros: «чистая» конечная архитектура одним заходом.
- Cons: третья ревизия shutdown-семантики, изменения facade-API и протокола одновременно с async-миграцией — невозможно локализовать регрессию; нарушает принцип постепенности ROADMAP3 и правило «1 change set — 1 логическое изменение».
- Why not chosen: каждое из решений — отдельный охватывающий рефакторинг со своим follow-up (см. ниже); объединение делает откат невозможным.

### Alternative 4: `spawn_blocking` на всё (оставить логику sync, обернуть каждый вызов)

- Pros: минимальные правки сигнатур.
- Cons: async-код продолжает жить в sync-фукнциях с ручными таймаутами; `select!` и cancellation недоступны; accept-цикл по-прежнему зависает до входящего соединения — масштабируемость не достигается.
- Why not chosen: не закрывает суть бага (structured concurrency, таймауты, отмена).

## Follow-up (явно вне этого change set)

1. **Mine на snapshot + CAS-commit** — убрать write-lock на время PoW-поиска (консенсус-смежно, требуется ADR + SCIP-оценка).
2. **`CancellationToken` вместо `Arc<AtomicBool>`** — единая shutdown-семантика с дрейном задач; затрагивает facade-API и тесты.
3. **Удаление текстового `GET_BLOCKCHAIN`/`UPDATE_BLOCKCHAIN:`** — чистый бинарный протокол (ARCHITECT3 §3).
4. **Крейт `strangecoin-net`** (ARCHITECT3 §10.4) — перенос protocol/sync/gossip/noise из монолита.
5. **Noise (ADR-0015), Erlay (ADR-0016), peer scoring (ADR-0017)** — Stage 2 поверх этой миграции.

## Related

- BUG-S0-021 (`analytics/bugfixes-stage0.md`) — баг-каталог; S2-P01 (mining) + S2-P02 (network)
- ADR-0007: tokio on Stage 1 — гибридная модель, которую этот ADR закрывает
- ADR-0010: SyncEngine — `Inbox`/кандидаты; миграция сохраняет «только пуш, adoption в engine»
- ADR-0009: EventBus crossbeam — не меняется
- STAGE1_SUMMARY §5: deferred «mining threads → async» — закрывается этим ADR
- ARCHITECT3 §10.4: Stage 2 network crate — следующий шаг после этой миграции
- ROADMAP3 §Этап 2: сетевая зрелость (gossip, Noise, Erlay) — этот change set снимает блокер
