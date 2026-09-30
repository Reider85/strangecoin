# ADR-0007: tokio on Stage 1 (постепенная миграция)

## Status

Accepted

## Context

Stage 0 нода построена на `std::thread` + `std::sync::mpsc` + блокирующем `std::net`:

| Подсистема | Модель | Где |
|---|---|---|
| Mining worker | `thread::spawn`, `mpsc::Receiver<MiningTask>` | `src/lib.rs` `Node::new()` |
| TCP accept | `thread::spawn`, `listener.incoming()` | `src/lib.rs` `Node::start_server()` |
| Per-connection | `thread::spawn` на каждый TCP-поток | `src/lib.rs` `Node::start_server()` |
| Sync loop | `thread::spawn` + `std::thread::sleep(100ms)` | `src/lib.rs` `run()` |
| GUI | блокирующий `eframe::run_native()` | `src/lib.rs` `run()` |
| Shutdown | `ctrlc::set_handler` + `Arc<AtomicBool>` | `src/lib.rs` `run()` |

Модель работает, но масштабируется ограниченно, и это становится блокером для Stage 2:

1. **Thread-per-connection не масштабируется.** При каждом входящем соединении создаётся OS-тред с полным стеком (по умолчанию 2 MiB на Windows). При 500+ пирах это десятки мегабайт стека и планировщик ОС, переключающийся между сотнями потоков. Gossip на Stage 2 (broadcast всем пирам) умножает количество соединений на число операций.
2. **Нет структурированной конкурентности.** `thread::spawn` возвращает `JoinHandle`, который в четырёх местах игнорируется. Отмена задачи, таймауты и backpressure выражаются вручную (`AtomicBool` + polling). Это источник «зависших» тредов: если `sync_blockchain()` зависнет на блокировке, поток останется жив и не будет виден ни в одном логе.
3. **Нет таймеров и select.** Sync loop реализован как `for _ in 0..10 { sleep(100ms) }` — ручная эмуляция интервала, которую невозможно отменить без флага. Stage 2 требует таймаутов на handshake, на download блоков и на пинг-keepalive.
4. **Блокирующий GUI на главном треде.** `eframe::run_native()` занимает тред, на котором он вызван, до закрытия окна. Это делает невозможным использовать тот же тред для async-работы.
5. **Anti-goal Stage 0 снимается.** ROADMAP3 Этап 0 явно запрещал tokio («No tokio until Stage 1»). На Stage 1 запрет снимается официально — это условие перехода к Этапу 1.

Stage 2 (gossip, Noise Protocol, Erlay) — это сотни одновременных async-операций с таймаутами: handshake, Dandelion-релей, Erlay-раунды. Ни одна из них не выражается в `std::thread` без самописного event loop.

При этом миграция «в лоб» запрещена риском, зафиксированным в ROADMAP3: «tokio миграция ломает существующие threads — mitigation: постепенный перенос». 48+ тестов Stage 0, 11 файлов интеграционных тестов и 4 места с `thread::spawn` — это работающий код, который нельзя ломать массовой правкой.

## Decision

Вводим **tokio** с features `rt-multi-thread`, `macros`, `sync`, `time`, `net`, `signal`, и переводим ноду на **гибридную модель**: async-рантайм является владельцем процесса, существующие блокирующие подсистемы продолжают жить на своих OS-тредах.

Ключевые решения:

- **Точка входа — `#[tokio::main]`.** `src/main.rs` становится `#[tokio::main] async fn main()`, вызывающим `strangecoin::run_async().await`. Рантайм инициализируется **до** инициализации `tracing_subscriber` внутри `run_async()`, чтобы все трейсы (включая трейсы самого рантайма) шли через subscriber.
- **`run()` разделяется на `run_async()` (async) и `run()` (sync-обёртка).** Вся подготовка (tracing, CLI, config, каналы, `Node`, TCP-сервер, `WalletApp`) живёт в `run_async()`. `run()` остаётся как sync-точка входа для тестов и утилит: `Runtime::new()?.block_on(run_async())`.
- **Sync loop мигрирует на `tokio::spawn`.** Это единственный существующий тред, который является чистым polling-циклом: `thread::spawn` + `for _ in 0..10 { sleep(100ms) }` заменяется на `tokio::spawn` + `tokio::time::interval`. Он уже не трогает блокирующие примитивы и не держит блокировок между итерациями. Все четыре места в коде (включая тесты) используют один и тот же паттерн.
- **Mining worker, TCP accept и per-connection остаются `std::thread::spawn`.** Прямое требование промпта S1-P10 («Существующие threads — НЕ трогать в этом промпте»). Майнинг — CPU-bound и блокирующий по `RwLock<Blockchain>`; перенос на `spawn_blocking` без изменения модели не даёт выигрыша. TCP-миграция на `tokio::net` — Stage 2, вместе с headers-first (S1-P16) и SyncEngine (S1-P18).
- **GUI запускается через `tokio::task::spawn_blocking`.** `eframe::run_native()` блокирующий; `spawn_blocking` выделяет ему отдельный тред из пула blocking-пула tokio, не занимая worker'ы. Асинхронный main может жить параллельно с окном GUI. Это снимает проблему «GUI на главном треде» без вынесения GUI в отдельный процесс.
- **Shutdown: `tokio::signal::ctrl_c()` + `AtomicBool`, `ctrlc` остаётся как fallback.** Первичный обработчик — async-таск `tokio::spawn`, который вызывает `tokio::signal::ctrl_c().await`, ставит `Arc<AtomicBool>` и ждёт 30 секунд (поведение P17 сохранено один в один). Fallback `ctrlc::set_handler` сохраняется параллельно: оба обработчика пишут в **один и тот же** `AtomicBool`, поэтому двойная установка безопасна — кто сработает первым, тот и инициирует shutdown, второй будет прерван `process::exit`. Причина: `tokio::signal` на Windows использует внутреннюю реализацию `SetConsoleCtrlHandler` с известными edge-cases при переподключении консоли; `ctrlc` — зрелая библиотека с отдельным обработчиком, покрывающим эти случаи. Стоимость нулевая (crate уже в зависимостях, один обработчик), а отказоустойчивость shutdown — критична для целостности LevelDB.
- **EventBus остаётся crossbeam** (ADR-0009), но получает **мост в async.** `EventBus::subscribe_async()` возвращает `tokio::sync::mpsc::Receiver<NodeEvent>`, перекачивая из crossbeam-канала в async-канал через фоновую задачу с `recv_timeout`. Это позволяет async-подсистемам (первый потребитель — SyncEngine в S1-P18) подписываться на события без блокировки воркеров. Мост — переходное решение; когда все потребители станут async, шина переедет на `tokio::broadcast` отдельным решением.
- **Shutdown-флаг не мигрирует на `tokio::sync::watch`/`CancellationToken` в этом промпте.** `Arc<AtomicBool>` читают четыре существующих треда; смена типа — это правка всех мест чтения, что нарушает требование «legacy threads intact». Миграция флага — часть Stage 2.

## Consequences

### Positive

- **Фундамент для Stage 2.** Gossip, Noise и Erlay — async by design; перенос сетевого стека на `tokio::net` больше не потребует переделки инфраструктуры, только самих обработчиков.
- **Sync loop стал отменяемым.** `tokio::time::interval` + `select!` на shutdown-сигнал вместо ручного `for _ in 0..10 { sleep() }`. Тред больше не может «залипнуть» незаметно — `JoinHandle` позволяет его дождаться и залогировать зависание.
- **GUI и async работают параллельно.** `spawn_blocking` изолирует блокирующий eframe; в будущем GUI-подписчик сможет получать события из tokio-канала без `try_recv()` в каждом кадре.
- **Таймауты становятся тривиальными.** `tokio::time::timeout` вместо ручных `connect_timeout`/`recv_timeout` — прямо то, что нужно S1-P16 (download-цикл) и S1-P18 (inbox backpressure).
- **EventBus-мост открывает путь к async-подписчикам** без переписывания продюсеров (mining, P2P — они остаются sync и публикуют в crossbeam как раньше).
- **Graceful shutdown не деградирован.** Поведение P17 (30-секундный таймаут, `process::exit(0)`) сохранено один в один; добавлен только async-путь.

### Negative

- **Новая зависимость и размер бинарника.** tokio с шестью features добавляет заметный вес к release-сборке (при `lto = true, codegen-units = 1` — минуты к времени компиляции).
- **Гибридная модель — временная.** В проекте теперь сосуществуют три способа конкурентности: `std::thread` (mining/TCP), `tokio::spawn` (sync loop), `crossbeam` (EventBus). Это плата за постепенность и она исчезнет по мере Stage 2.
- **EventBus-мост добавляет задержку и задачу.** `recv_timeout(100ms)` в мосту — это polling; теоретически можно заменить на уведомление, но crossbeam не умеет async-wakeup. Для событий, где важна низкая задержка (MiningStarted/Finished), потребители остаются на прямом crossbeam-канале; мост — только для async-подписчиков, которым задержка в 100ms допустима.
- **`spawn_blocking` ограничен пулом** (по умолчанию 512 потоков). При одном GUI-приложении это нерелевантно, но при future-виджетах нужно следить.
- **Двойной обработчик ctrl+c** выглядит избыточно и требует пояснения в коде (иначе следующий разработчик удалит один из них «как дубликат»).

### Neutral

- **`eframe` не меняется.** Это по-прежнему блокирующий цикл; изменён только способ его запуска.
- **Архитектура `strangecoin-core` не затронута** — core остаётся 0 I/O, без tokio. Проверка `rg "tokio" crates/strangecoin-core/src` → 0 обязательна.
- **Протокол P2P не меняется.** Length-prefixed framing, safe framing, rate limiter — всё как было.
- **`#[tokio::test]` требует feature `macros` + `rt`** — уже включены; тесты Stage 0 остаются синхронными `#[test]` и не требуют рантайма.

## Alternatives

### Alternative 1: async-std

- Pros: API похож на tokio, `async-std::task::spawn`, чуть проще для новичков.
- Cons: экосистема заметно меньше — crates, публикующие async-API (hyper, axum, rustls-async), ориентированы на tokio; async-std требует `async-compat`-мостов для tokio-экосистемы. Stage 2 (Noise, Erlay) будет зависеть от async-TLS/async-шифрования, для которого tokio — de-facto стандарт.
- Why not chosen: tokio доминирует в Rust async; интеграция с Stage 2 зависимостями будет дороже, чем выигрыш от простоты API.

### Alternative 2: Остаться на std::thread + самописный event loop (epoll/kqueue)

- Pros: нулевые зависимости, полный контроль.
- Cons: на Windows нужен IOCP, на Linux — epoll; кросс-платформенность требует `mio`/`polling`, что и есть tokio. Писать это вручную — это неделя работы на инфраструктуру, которая не является продуктом, и она будет хуже tokio по качеству (таймауты, backpressure, отмена).
- Why not chosen: переизобретение tokio без выигрыша; отвергнуто вместе с Alternative 3 в ROADMAP3.

### Alternative 3: Остаться на threads до Stage 2 (отложить решение)

- Pros: ноль изменений сейчас, ноль риска регрессий на Stage 1.
- Cons: Stage 2 всё равно потребует tokio; откладывая, мы переносим ту же миграцию на Stage 2, но уже **после** того, как gossip/Noise/Erlay напишутся на threads — то есть миграция станет объёмнее и рискованнее. Плюс ROADMAP3 Этап 1 явно требует «Ввести tokio» как критерий DoD.
- Why not chosen: миграция до Stage 2 дешевле; откладывание переносит риск на момент, когда цена ошибки выше.

### Alternative 4: Переписать всё на tokio сразу (big bang)

- Pros: чистая конечная архитектура, один способ конкурентности.
- Cons: затрагивает 4 места `thread::spawn`, mining, TCP, GUI и все интеграционные тесты одновременно. Риск из ROADMAP3 («tokio миграция ломает существующие threads») реализуется полностью; откатить такой регресс дорого. Нарушает прямое требование промпта S1-P10 п.5.
- Why not chosen: постепенная миграция — явная mitigation, зафиксированная в ROADMAP3.

## Related

- ROADMAP3 §Этап 1: «Ввести tokio (заменяет threads + mpsc). Подготовка к Stage 2 (gossip, Noise, Erlay)»
- ROADMAP3 §Risks: «tokio миграция ломает существующие threads — mitigation: постепенный перенос, tokio::spawn для новых подсистем, старые threads работают до Stage 2»
- ARCHITECT3 §10.2: целевая архитектура (async runtime, SyncEngine)
- ADR-0009: EventBus на crossbeam (остаётся; async-мост — следствие этого ADR)
- S1-P16: headers-first sync — первый кандидат на `tokio::net` + `tokio::time::timeout`
- S1-P18: SyncEngine (ADR-0010) — первый полноценный async-потребитель, его inbox будет `tokio::sync::mpsc`
- P17 (Stage 0): graceful shutdown — поведение сохранено, ctrlc остаётся как fallback
