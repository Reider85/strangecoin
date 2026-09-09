# ANALYSIS.md — Критический разбор ARCHITECT.md, ARCHITECT2.md, ROADMAP.md и prompt.md

**Автор обзора:** независимый архитектурный анализ
**Дата:** 2026-09-09
**Цель:** критически оценить четыре ключевых документа из `analytics/` репозитория
strangecoin, выявить сильные и слабые стороны, зафиксировать упущения, которые должны
быть учтены в альтернативной архитектуре `ARCHITECT3.md` и плане её достижения
`ROADMAP2.md`. Обзор выполнен с позиции прагматичного архитектора блокчейн-систем,
ориентирующегося на production-readiness с учётом современного состояния индустрии
(2025–2026).

> **Замечание о нумерации файлов.** В запросе упоминается `ARCHITECH2.md` (сочетание
> «CH»), но в репозитории файл называется `ARCHITECT2.md` (сочетание «CT»). В данном
> анализе используется корректное имя `ARCHITECT2.md`. Содержательно это одно и то же
> продолжение `ARCHITECT.md`.

---

## Методология

Каждый из четырёх документов анализируется по трём осям:

- **Согласен** — решения и формулировки, которые выдерживают критику и переходят в
  `ARCHITECT3.md` практически без изменений.
- **Не согласен** — позиции, с которыми рецензент расходится; для каждой указана
  альтернатива, закладываемая в `ARCHITECT3.md`.
- **Упущено** — темы, отсутствующие в документе, но критичные для целевой архитектуры.

После покусочного анализа приводится сводная таблица перекрёстных упущений и
стратегические рекомендации, ведущие напрямую к решениям в `ARCHITECT3.md`/`ROADMAP2.md`.

Исходные документы:
- `ARCHITECT.md` — 606 строк, целевая архитектура v1.
- `ARCHITECT2.md` — 174 строки, критика зрелости ARCHITECT.md и пробелы до production.
- `ROADMAP.md` — 185 строк, дорожная карта с этапами 0→7.
- `prompt.md` — 75 КБ, 117 промптов для AI-агентов на Этап 0.

---

## 1. Сводная таблица оценок

| Документ | Согласен (топ-7) | Не согласен (топ-7) | Упущено (топ-7) |
|----------|------------------|---------------------|-----------------|
| **ARCHITECT.md** | чистое ядро; три аксиомы; детерминированный генезис; workspace; декомпозиция blockchain на 4 компонента; events bus; sync engine | Ed25519; PoW как долгосрочный; halving + max_supply; redb; «no PoS prematurely» как anti-goal; сохранение монолита до Этапа 3 | TLA+ спека; threat model (STRIDE); MEV mitigation; AA (ERC-4337); state rent/expiry; SCIP; license |
| **ARCHITECT2.md** | 13 критических уязвимостей; «balances problem» как тавтология; WASM (wasmi); Verkle; critical path; reproducible builds; TLA+; threat model | «Casper FFG без стейкинга» (логическая ошибка); redb вместо RocksDB; AA на Этапе 1; WIT ABI; Block-STM на Этапе 1.5 | Hybrid PoW→PoS миграция; tail emission; secp256k1/BLS вместо Ed25519; strangler pattern; license; indexer; testnet faucet |
| **ROADMAP.md** | приоритет Этапа 0; mainnet freeze; canonical serialization; OOM fix; shadow-fork; Noise transport; SDK + devnet; immunefi day-1 | Finality gadget на Этапе 1; Block-STM; AA на Этапе 1.5; state rent; EIP-1559 + multidim fees; PoW как финал | chain_id; nonce; transaction hash; mempool basic rules; graceful shutdown; license; tail emission; PoS migration |
| **prompt.md** | 117 промптов; правильный порядок (crypto→serialize→consensus→storage→network); КГ; cargo check; proptest; модуляризация; CI gate | Ed25519; стиль `println!` для production; сохранение LevelDB-формата; 117 промптов как over-fragmentation; `serde` в consensus путях | TLA+; threat model; license; reproducible builds; единый `Config` struct; graceful shutdown; metrics |

Подробное покусочное обоснование — в разделах 2–5.

---

## 2. ARCHITECT.md

Документ описывает целевую архитектуру v1: 12 разделов от принципов до чек-листа
соответствия. Сильный фундамент с инженерной дисциплиной, но с пробелами, часть
которых закрывается только в `ARCHITECT2.md`, а часть остаётся открытой.

### 2.1 С чем согласен

1. **Три консенсусные аксиомы (§1.1).** Вся валидность выводится из цепочки блоков
   детерминированно, `balances` — только кэш; криптографическая целостность через
   подпись Ed25519 + хэш-цепочку; никакого wall-clock в консенсусе, никакого float.
   Это правильный набор фундаментальных ограничений: они делают ядро воспроизводимым
   в любой реализации и устраняют целый класс тонких багов (гонки времени,
   недетерминизм операций с плавающей точкой, расхождения кэша балансов).

2. **Ядро без побочных эффектов (§1.2).** `consensus`, `state`, `serialize` — чистые
   функции без I/O; сеть/БД/GUI живут в тонких периферийных обёртках. Это базовое
   правило аудируемости: чистое ядро можно прогонять через proptest, fuzzing и
   model-checking без моков. Архитектурно это означает, что `core` крейт можно
   публиковать на crates.io и использовать в альтернативных нодах, SDK, индексаторах.

3. **Один источник истины — блокчейн (§1.3).** Mempool, кэш балансов, индексы —
   производные, пересчитываемые структуры. Это устраняет целый класс багов
   рассинхронизации (главную проблему v0.8.6, где `balances` мутируется напрямую в
   `main.rs:1423`) и делает reorg корректным: rollback состояния — это пересчёт из
   предыдущей точки, а не инкрементальный откат кэша.

4. **Жёсткие границы модулей (§1.4).** Единственная точка входа в каждую подсистему;
   приватные поля недоступны извне даже через сокеты/DB/GUI. Это формальное правило,
   которое в Rust выражается через `pub(crate)` и модульную инкапсуляцию. Без него
   любой рефакторинг превращается в russian roulette — особенно в блокчейн-коде,
   где «вроде работает» может скрывать критическую уязвимость.

5. **Каноническая бинарная сериализация (§3.1).** Один источник кодировки для всего
   консенсуса, `format_version` для будущих изменений, golden-векторы в тестах.
   Использование `serde_json::to_string` для хэшей — классическая ошибка, которая в
   v0.8.6 приводит к недетерминизму (порядок ключей в JSON, escape-правила). Бинарная
   каноническая сериализация — единственный production-grade подход.

6. **Декомпозиция `blockchain` на 4 компонента (§11.1).** `chain_selector` (fork
   choice), `block_executor` (validate+apply), `state_cache` (кэш балансов/нонсов),
   `facade` (public API). Это устраняет God-object и делает каждый компонент
   тестируемым изолированно. Особенно ценно разделение «выбор вершины» и «применение
   блока» — они имеют разные инварианты и могут эволюционировать независимо.

7. **`SyncEngine` разрывает цикл network↔blockchain (§11.3).** Network кладёт блоки
   в inbox sync-engine, который единственный вызывает `validate → apply → announce`.
   Без этого разрыва возникает либо циклическая зависимость (network ↔ blockchain),
   либо утечка ответственности в один из модулей. Это паттерн, который регулярно
   встречается в зрелых кодовых базах (reth, lighthouse, substrate).

8. **Events bus — multi-subscriber broadcast (§11.2).** Замена «GUI сам читает Mutex»
   на типизированные события `BlockApplied`, `ChainReorged`, `TxAccepted`. Это
   правильная архитектура для decoupling: GUI/метрики/JSON-RPC subscriptions/тесты —
   все подписываются на одну шину, не залезая в ядро.

9. **Network ID (mainnet/testnet/regtest, §10.5).** Пиры с чужим `network_id`
   отбрасываются до любых данных. Это отделяет тестовую деятельность от боевой цепи
   и делает невозможным случайный merge. Тестовые сценарии работают на `regtest` с
   fake clock — детерминизм, без `sleep`-эвристик.

10. **Генезис как консенсусный якорь (§10.6).** `genesis.json` — только входные данные;
    `EXPECTED_GENESIS_HASH` — константа в `consensus.rs`. Узел при старте сверяет
    локальный генезис и отказывается работать при расхождении. Это правильное
    разделение «конфиг» vs «консенсус»: редактирование файла не меняет правила игры.

11. **Workspace-структура (§10.7).** `strangecoin-core/net/node/wallet/api/gui` —
    целевая форма, в которой ядро аудируется отдельно и переиспользуется в SDK без
    egui/LevelDB. Это паттерн экосистемы Rust (substrate, reth, lighthouse), и он
    критичен для долгосрочной поддерживаемости.

12. **Tie-breaking rule для cumulative work (§11.7).** «Больше work → раньше
    timestamp → меньше hash» — детерминированное правило, без гонок. В Bitcoin та же
    идея реализована неявно через`first-seen`, что создаёт debug-сложности; явное
    правило лучше.

13. **Mempool RBF (§11.8).** `feerate = fee / tx_weight`, `find_replaceable`,
    `Replaced(Vec<TxId>)` для анонса `TxRejected`. Это production-grade замена
    наивного FIFO, защищающая от spam и обеспечивающая ценовое обнаружение в мемпуле.

14. **Storage schema versioning + migrations (§11.9).** Префиксы `c/`, `s/`, `b/`...,
    `current_schema_version` в БД, прогон миграций при старте. Это единственный
    способ эволюционировать схему без ручного копирования данных — то, чего не
    хватает v0.8.6 с его `main.rs:188` «ручное удаление LOCK».

15. **VM trait в core, runtime отдельно (§11.4).** `core` зависит от `VmExecutor`
    trait, а не от конкретного рантайма; `node` выбирает реализацию. Это позволяет
    аудировать ядро без WASM-рантайма и тестировать VM изолированно, а также
    потенциально заменять wasmi на zkVM в будущем без переделки консенсуса.

### 2.2 С чем не согласен

1. **Ed25519 для подписей транзакций (§3.8, §5).** Ed25519 — отличный алгоритм, но в
   блокчейн-индустрии стандарт de facto — ECDSA secp256k1 (Bitcoin, Ethereum, все
   EVM-совместимые цепи). Это означает: MetaMask, WalletConnect, Ledger, Trezor —
   всё работает с secp256k1, а интеграция Ed25519 требует custom-кода. Если
   планируется PoS-фаза с агрегацией подписей, нужен BLS12-381. Ed25519 не
   поддерживает aggregation.

   **Альтернатива в ARCHITECT3.md:** secp256k1 для EOA-подписей (совместимость),
   BLS12-381 для консенсусных подписей (PoS-фаза, агрегация). Ed25519 оставляется
   только как опция для internal node-to-node auth.

2. **PoW как долгосрочный консенсус (§3.2, §10.10).** В 2026+ PoW — устаревшая модель
   для новых цепей: энергозатраты, централизация майнинга (ASIC), низкая finality,
   слабая устойчивость к 51%-атакам при маленьком хешрейте. Anti-goal «no PoS
   prematurely» в §11.11 закрывает дверь к эволюции.

   **Альтернатива:** Hybrid PoW→PoS: PoW для fair launch первые 1-2 года, затем
   миграция на PoS через activation height. Стейкинг native token (не отдельный
   governance token). Архитектура должна с самого начала проектировать точку
   перехода — иначе PoS будет «too late».

3. **Halving + max_supply (§3.2).** Копирование Bitcoin без обоснования. Фиксированный
   supply cap создаёт дефляционный spiral risk: hodling → низкая ликвидность →
   майнерам невыгодно → security budget collapse. Через 30+ лет Bitcoin столкнётся с
   этой проблемой; новой цепи не нужно повторять ошибку.

   **Альтернатива:** Tail emission (как Monero): 0.6%/год после первичного cap,
   обеспечивает постоянный security budget и защиту от hoarding. Архитектурно это
   `block_reward_at_height = max(base_reward, tail_emission_rate * total_supply)`.

4. **redb вместо RocksDB (§11.9, §3.7).** redb — pure Rust, ACID, без CGO — звучит
   хорошо, но в production его battle-testing значительно ниже, чем RocksDB (Bitcoin
   Core, Ethereum reth, Hyperledger). Для одного разработчика выбор «проверенного
   решения» важнее «эстетической чистоты».

   **Альтернатива:** RocksDB с column families (как в reth). Это требует CGO, но даёт
   надежность, отлаженный tuning, и готовые рецепты для pruning/snapshots.

5. **«No PoS prematurely» как anti-goal (§11.11).** Это закрывает дверь к эволюции.
   Архитектура должна с самого начала закладывать точку перехода, даже если
   реализация приходит позже. Иначе PoS-миграция станет таким же переписыванием, как
   The Merge в Ethereum (5+ лет).

   **Альтернатива:** PoS-фаза явно зафиксирована в roadmap (Stage 7), с
   placeholder-типами в core уже на Stage 1 (`ValidatorSet`, `Attestation`).

6. **Сохранение монолита `src/` до Этапа 3 (§10.7).** Это создаёт технический долг:
   код, который «работает в монолите», трудно вынести в крейт из-за неявных
   зависимостей. Лучше strangler pattern: с самого начала выделять `core` крейт,
   даже если он пустой, и постепенно переносить туда функциональность.

   **Альтернатива:** Strangler pattern. На Stage 0 — монолит фиксируется (sanitization).
   На Stage 1 — создаётся `strangecoin-core` крейт, в него переносится `serialize`,
   `consensus`, `state`. На Stage 2 — `strangecoin-net`, `strangecoin-wallet`. На
   Stage 3 — `strangecoin-node`, `strangecoin-api`, `strangecoin-gui`. К Stage 4
   монолит `main.rs` превращается в тонкий launcher.

7. **`tokio` как anti-goal на Этапе 0 (§10.10, §11.11).** threads + mpsc достаточно для
   прототипа, но для production-grade P2P (gossip, Erlay, Noise, async I/O) async
   необходим. Откладывание tokio создаёт двойной rewrite: сначала streams, потом
   async. Лучше начать с tokio на Stage 1, когда сеть становится нетривиальной.

   **Альтернатива:** Tokio на Stage 1 (вместе с headers-first sync). На Stage 0
   действительно достаточно threads (только sanitization, без новой сети).

8. **WIT/Wasm Component Model для ABI (§1, §11.4).** Wasm Component Model — слишком
   новый стандарт (W3C, 2023+), мало tooling, мало аудитов. Для одного разработчика
   это недопустимый риск.

   **Альтернатива:** Простой JSON ABI (как в Ethereum) для MVP, с заделом на
   WIT-миграцию через `format_version`. На Stage 4+ — переход на WIT, когда
   tooling дозреет.

### 2.3 Что упущено

1. **Формальная спецификация консенсуса (TLA+).** Не упомянута в §3-12, только в
   ROADMAP.md Этап 6 (далеко). Без model-checking невозможно проверить safety
   (no double-spend, no inflation) и liveness (no deadlock) на граничных случаях
   (reorg глубины N, time-warp attacks, selfish mining). ARCHITECT2.md правильно
   указывает: TLA+ спека нужна с Этапа 0.

2. **Threat model (STRIDE).** Нет анализа векторов атак: eclipse, partition, selfish
   mining, MEV, long-range attacks (для будущего PoS), nothing-at-stake. Без этого
   архитектура описывает «что строить», но не «от чего защищаться».

3. **MEV mitigation.** Упоминается только в §8 таблице этапов (Этап 5). Без
   дизайна threshold encryption mempool или PBS с самого начала, MEV-векторы
   (frontrunning, censoring, sandwich attacks) становятся системными.

4. **Account Abstraction (ERC-4337 analog).** Не упоминается. Без AA нет
   спонсируемых транзакций, социального восстановления, gas abstraction. Это
   критично для UX и для массового adoption.

5. **State rent / expiry (EIP-4444).** Не упоминается. Без pruning — неограниченный
   рост диска. ARCHITECT2.md правильно указывает: проектироваться должно с Этапа 1
   (state trie design), не откладываться на потом.

6. **Stateless validation (Verkle vs SMT).** В §9 указано как «нерешённое», но без
   крайнего срока. Без stateless witnesses light-клиенты не могут верифицировать
   блоки без полного state — это критично для scaling.

7. **Parallel execution (Block-STM).** Не упоминается. Без parallel execution
   последовательная валидция — узкое горлышко TPS. Архитектурно это `ExecutionContext`
   с dependency tracking; нужно закладывать с Stage 1.5.

8. **Upgrade governance (SCIP).** Упоминается в §8 Этап 5, но не зафиксировано в
   архитектуре. Без `consensus_version` + activation height хардфорки рискованны.

9. **Reproducible builds / cosign.** Не упоминается. Без воспроизводимых сборок
   невозможно верифицировать, что бинарник соответствует исходному коду. Должно
   быть с Stage 0 в CI.

10. **License.** Не упоминается. Без лицензии код нельзя легально
    использовать/форкать — это блокирует любой adoption.

11. **Replay protection (chain_id).** Не в инвариантах (§5). Без `chain_id` транзакции
    из testnet валидны в mainnet и наоборот.

12. **Nonce / sequence number.** Косвенно в §3.5, но не в инвариантах. Без replay
    protection внутри одной цепи.

13. **Transaction hash = commitment.** Не в инвариантах. Нужен уникальный
    идентификатор для mempool, rollback, tracking.

14. **Mempool basic rules.** Не в инвариантах. Проверка баланса/nonce/подписи/
    дедупликация.

15. **Graceful shutdown.** Не в инвариантах. Нужен корректный `Drop` для LevelDB + flush
    буферов (замена `main.rs:188` «ручное удаление LOCK»).

16. **Event log / receipts (для смарт-контрактов).** Не упоминается. Без logs невозможно
    строить explorer, indexer, dApps.

17. **Block gas limit (для VM).** Не упоминается. Даже фиксированный лимит защищает от
    блоков, которые валидируются вечно.

18. **P2P message framing с max size.** В §3.6 есть «лимиты размеров», но не как
    инвариант. Нужен proper framing: length-prefixed, с проверкой до аллокации.

19. **Rate limiting на P2P.** Не упоминается. Лимит сообщений/сек от одного пира.

20. **Configuration management.** Не упоминается. Сейчас `config.json` + `config.toml`
    + секция `[wallet]` в `Cargo.toml` — бардах. Нужен единый `Config` struct с
    валидацией.

21. **Testnet faucet.** Не упоминается. Без faucet нет тестировщиков.

22. **Block explorer.** Упоминается косвенно в §3.9 как «CLI для ноды», но не как
    web-explorer.

23. **Snapshot sync.** Не упоминается. Без snapshot sync новые узлы синхронизируются
    часами.

24. **Metrics (Prometheus/OpenTelemetry).** Не упоминается. Без метрик невозможно
    мониторить production-ноду.

25. **JSON-RPC compatibility layer (eth_*).** Не упоминается. Совместимость с
    `eth_sendTransaction`, `eth_getBalance` позволит использовать MetaMask/WalletConnect.

26. **Indexer (Subgraph/sqd).** Не упоминается. Без GraphQL API для дApps.

27. **ZK-VM integration.** Не упоминается. RISC Zero / SP1 / ckb-vm zkVM — это
    будущее stateless validation.

28. **Staking для finality gadget.** Упоминается «PoW + finality» без определения, что
    такое finality. Casper FFG по определению требует стейкинга — это противоречие
    в ARCHITECT2.md.

29. **Light client protocol.** Упоминается в §10.2, но не детализирован. Нужен
    спецификация wire-формата и proof structure.

30. **Fee abstraction / paymaster.** Не упоминается. Paymaster платит любым токеном —
    критично для UX.

---

## 3. ARCHITECT2.md

Документ — критика зрелости ARCHITECT.md. Сильная диагностическая часть (13
уязвимостей, пробелы до production), но местами противоречивый в рекомендациях.

### 3.1 С чем согласен

1. **13 критических уязвимостей точно диагностированы (§1.1).** Нет подписей,
   `validate_chain` не проверяет PoW, тавтологическая валидация балансов, нет
   эмиссии, недетерминированный генезис, адреса без контрольной суммы, OOM-вектор,
   пароль в открытом виде, ad-hoc синхронизация, неканоническая сериализация, нет
   валидации времени, нет тестов. Это исчерпывающий список blockers для любого
   production-запуска.

2. **«Balances problem» как тавтология (§1.1, п.3).** Сильный анализ: `expected_balances`
   инициализируется из `self.balances` (копией), а не из генезиса — это делает
   валидацию тавтологией. Манипуляции с `balances` в БД не обнаруживаются. Это
   концептуальная ошибка, а не просто баг.

3. **Фундаментальные компоненты отсутствуют полностью (§1.2).** Mempool, P2P протокол,
   state root/Merkle, fee market, finality gadget, checkpoint sync, JSON-RPC/CLI, HD
   wallet, indexer/explorer. Список корректный — всё это нужно для production.

4. **ARCHITECT.md — сильный фундамент, но с критическими пробелами (§7).** Это
   взвешенная оценка: документ не «плохой», а «незавершённый». Пробелы (TLA+, threat
   model, MEV, AA, state rent, upgrade governance) — конкретные и закрытые.

5. **Смарт-контрактов нет; нужен MVP с WASM, аккаунтами, gas, Verkle (§4).** Это
   правильный диагноз. Нельзя перепрыгивать к смарт-контрактам без фундамента.

6. **Critical Path: сначала Этап 0, потом 1, потом 1.5 (§5).** Это правильный порядок.
   Смарт-контракты на сломанном консенсусе = потеря средств пользователей.

7. **TLA+ спека нужна (§2.2).** Без model-checking — неизвестны граничные случаи
   reorgов, time-warp, selfish mining.

8. **Threat model (STRIDE) нужна (§2.2).** Без анализа векторов атак архитектура
   описывает «что строить», но не «от чего защищаться».

9. **MEV mitigation нужен до Этапа 2 (§2.2).** MEV — системный риск (frontrunning,
   censoring).

10. **Fee market / EIP-1559 на Этапе 1 (§2.2).** Без динамической базовой комиссии —
    spam, нестабильные комиссии.

11. **Account Abstraction заранее (§2.2).** Без AA нет спонсируемых транзакций,
    социального восстановления.

12. **State rent / expiry как часть state trie design (§2.2).** Без pruning —
    неограниченный рост диска.

13. **Stateless validation (Verkle) до Этапа 1 (§2.2).** Без witness — light клиенты
    не верифицируют блоки.

14. **Parallel execution (Block-STM) (§2.2).** Последовательное исполнение — узкое
    горлышко TPS.

15. **Upgrade governance (SCIP) на Этапе 1 (§2.2).** Без активации по высоте —
    хардфорки рискованны.

16. **Reproducible builds / cosign на Этапе 0 (§2.2).** Без воспроизводимых сборок —
    нельзя верифицировать бинарник.

17. **WASM (wasmi) — правильный выбор VM (§2.3).** Mature toolchain, no_std support,
    детерминированный, готовые фаззеры, LLVM-бэкенд для контрактов на Rust.

18. **Verkle Trie — правильный выбор state tree (§2.3).** Stateless validation
    witnesses критичны.

19. **60+ пунктов Definition of Done (§6).** Полный чек-лист по слоям: Consensus,
    Network, Storage, VM, Wallet, API, Security, Governance.

### 3.2 С чем не согласен

1. **«PoW + Casper FFG без стейкинга» (§2.3).** Логическое противоречие: Casper FFG
   по определению требует стейкинга (validators lock up capital, get slashed for
   misbehavior). Без стейкинга это не Casper, это скорее checkpointing (как Bitcoin
   P2P checkpoints). Архитектурно это разные механизмы с разными trust assumptions.

   **Альтернатива:** Явно разделить: Stage 1 — PoW + rolling checkpoints (weak
   subjectivity), Stage 7 — PoW→PoS migration с настоящим Casper FFG (staking,
   slashing, validator set rotation).

2. **redb для storage (§2.3).** См. аргумент в §2.2.4 выше: RocksDB battle-tested.

3. **Account Abstraction на Этапе 1 (§2.2).** Слишком рано. AA добавляет attack
   surface (paymaster, bundler, EntryPoint precompile). Нужна зрелая VM и gas
   metering. На Этапе 1 нет даже смарт-контрактов.

   **Альтернатива:** AA на Stage 5 (после Stage 1.5 VM и Stage 4 dev-experience).

4. **WIT/Wasm Component Model для ABI (§4.2).** Слишком новый стандарт (W3C, 2023+),
   мало tooling, мало аудитов. Для одного разработчика недопустимый риск.

   **Альтернатива:** Простой JSON ABI для MVP, WIT migration на Stage 4+.

5. **Block-STM на Этапе 1.5 (§4.3).** Cutting-edge research (Aptos, 2022+). Сначала
   нужна корректная последовательная валидация, потом — параллельная. Для MVP это
   over-engineering.

   **Альтернатива:** Block-STM на Stage 3+ (после Stage 1.5 базовой VM и Stage 2
   network maturity).

6. **Fee market / EIP-1559 на Этапе 1 (§2.2).** EIP-1559 — отличная идея, но для MVP
   лучше начать с простого фиксированного gas limit. EIP-1559 требует сложной
   динамики base fee, которая в маленькой цепи нестабильна.

   **Альтернатива:** EIP-1559 на Stage 5 (после того, как экономика стабилизируется).

7. **Перенос MEV design до Этапа 2 (§2.2).** MEV mitigation — production-grade
   инфраструктура (Flashbots, ~2023). Для новой цепи MEV изначально минимален.

   **Альтернатива:** MEV design (threshold encryption mempool) на Stage 5, как
   placeholder в архитектуре с Stage 1.

### 3.3 Что упущено

1. **Hybrid PoW→PoS миграция как стратегия.** Не упоминается. ARCHITECT2.md фиксирует
   «Casper FFG без стейкинга», но не рассматривает реальную PoS-миграцию.

2. **Tail emission как альтернатива max_supply + halving.** Не упоминается.
   Дефляционный spiral risk не анализируется.

3. **secp256k1 / BLS12-381 вместо Ed25519.** Не упоминается. Совместимость с
   экосистемой (MetaMask, Ledger) не рассматривается.

4. **Strangler pattern как стратегия миграции.** Не упоминается. Путь «модуль за
   модулем» не зафиксирован.

5. **License (MIT/Apache-2.0).** Не упоминается. Критическая ошибка.

6. **Indexer / SDK / devnet.** Упоминаются в §4.4 как «архитектурные интерфейсы»,
   но без деталей реализации.

7. **Testnet faucet.** Не упоминается. Без faucet нет тестировщиков.

8. **Light client protocol.** Упоминается, но не детализирован.

9. **MEV-Relay subnet.** Упоминается в §3 «изменения в существующих разделах» для
   `network`, но не как архитектурный компонент.

10. **Dandelion++ / Portal Network.** Упоминаются без объяснения зачем.

---

## 4. ROADMAP.md

Документ — дорожная карта с этапами 0→7. Сильная диагностика текущего состояния,
но местами over-engineered для одного разработчика.

### 4.1 С чем согласен

1. **Приоритет Этапа 0 — абсолютно верно (§«Что делать в первую очередь»).** Без
   подписей транзакций, валидации difficulty и детерминированного генезиса всё
   остальное бессмысленно. Это не roadmap, а precondition.

2. **Все 13 критических проблем точно диагностированы.** См. §3.1.1 — анализ
   идентичен ARCHITECT2.md, что подтверждает валидность диагноза.

3. **Mainnet freeze — ключевой инсайт.** Параметры эмиссии, генезис и ретаргетинг
   должны быть неизменны после запуска. Это разделяет «игрушку» и «деньги». Любые
   изменения после запуска — только через activation height.

4. **Каноническая сериализация (§Этап 0).** JSON для хэшей — классическая ошибка
   новичков. Согласен на 100%.

5. **OOM-вектор (§Этап 0).** `vec![0; length]` — критическая DoS-уязвимость, которую
   часто упускают.

6. **WASM/RISC-V вместо рукописного байткода (§Этап 1.5).** Профессиональный
   уровень мышления. Собственный байткод = годы аудита.

7. **Multidimensional fees (EIP-4844/7706, §Этап 1.5).** Продвинутая идея, показывает
   глубокое понимание современных проблем Ethereum.

8. **Verkle Trie + stateless validation (§Этап 3).** Амбициозно, но правильно — это
   направление, в котором движутся Ethereum и Polkadot.

9. **Compact blocks (BIP 152) + Erlay (§Этап 2).** Эффективный relay на 10k+ узлов.

10. **P2P transport encryption (Noise/TLS, §Этап 2).** Защита от сниффинга и MITM.

11. **Headers-first sync (§Этап 1).** Вместо пересылки всей цепочки.

12. **HD-кошелёк (BIP-39/44, §Этап 4).** Стандарт для user-facing кошельков.

13. **SDK cargo-strangecoin (§Этап 4).** Foundry-аналог: `new`, `test`, `deploy`,
    `verify`. Это критично для dev-experience.

14. **Local devnet (anvil-аналог, §Этап 4).** Мгновенный старт, pre-funded аккаунты,
    time-travel, fork mainnet.

15. **Fuzzing harness (§Этап 4).** `cargo-fuzz` целевые контракты + VM из коробки.

16. **Indexer (Subgraph/sqd-аналог, §Этап 4).** GraphQL API для дApps.

17. **Emission curve фиксируется до mainnet freeze (§Этап 5).** 21M cap, halving
    каждые 4 года, tail emission 0.5%/год для безопасности PoW. (Здесь я согласен
    с принципом фиксации, но не с конкретной моделью — см. §4.2 ниже.)

18. **MEV mitigation (§Этап 5).** PBS или threshold encryption.

19. **Consensus versioning + activation height (§Этап 5).** Обязательно для любого
    изменения параметров.

20. **SCIP процесс (§Этап 5).** Strangecoin Improvement Proposals.

21. **TLA+ / PlusCal спецификация (§Этап 6).** Model-checking safety/liveness.

22. **Threat model, внешний аудит, bug bounty (§Этап 6).**

23. **Immunefi integration с day-1 (§Этап 6).** Правильный подход — bug bounty с
    первого дня, не после audit.

24. **Reproducible builds, cosign/sigstore (§Этап 6).**

25. **Shadow-fork testing (§Этап 6).** Replay mainnet blocks на staging перед
    апгрейдами.

26. **Formal verification harness (§Этап 6).** K-framework / Boogie / Coq для
    критических контрактов.

27. **Протокольная спецификация (§Этап 7).** Форматы, правила консенсуса, кодировка.

28. **Документация оператора ноды, метрики/мониторинг (§Этап 7).**

29. **Лицензия, руководство по контрибуции (§Этап 7).**

### 4.2 С чем не согласен

1. **Finality gadget на Этапе 1 (Casper FFG/Grandpa/Tendermint).** Для одного
   разработчика это over-engineering. PoW с 10-минутными блоками и 6 подтверждениями
   — достаточно для MVP. Finality gadget добавляет сложность, которую сложно
   правильно реализовать и аудировать.

   **Альтернатива:** Finality gadget отложить до Stage 2+, после того как PoS
   миграция станет реальной (Stage 7). На Stage 1 — PoW + checkpointing (weak
   subjectivity), как Bitcoin.

2. **Parallel execution (Block-STM) на Этапе 1.5.** Block-STM — cutting-edge research
   (Aptos, 2022+). Для проекта на стадии 0.8.x это не приоритет. Сначала нужна
   корректная последовательная валидация, потом — параллельная.

   **Альтернатива:** Block-STM на Stage 3+, после Stage 1.5 базовой VM.

3. **Account Abstraction на Этапе 1.5.** Слишком сложно для ранней стадии. Добавляет
   attack surface (paymaster, bundler, EntryPoint precompile).

   **Альтернатива:** AA на Stage 5, после Stage 1.5 VM и Stage 4 dev-experience.

4. **State rent / expiry.** Важно, но спорно. Ethereum отказался от state rent в
   пользу statelessness + pruning. Может быть, лучше скопировать их подход?

   **Альтернатива:** State rent — опционально на Stage 3+. Обязательно: state
   pruning + statelessness (Verkle witnesses) на Stage 3.

5. **MEV mitigation (PBS / threshold encryption, §Этап 5).** PBS — production-grade
   инфраструктура (Flashbots, ~2023). Для новой цепи MEV изначально минимален.

   **Альтернатива:** MEV design (threshold encryption mempool) на Stage 5, как
   placeholder в архитектуре с Stage 1.

6. **EIP-1559 + multidimensional fees (§Этап 1.5).** EIP-1559 — отличная идея, но
   multidimensional fees (EIP-7706) ещё не принят в Ethereum. Для новой цепи лучше
   начать с простого фиксированного gas limit, а потом эволюционировать.

   **Альтернатива:** Фиксированный gas limit на Stage 1.5, EIP-1559 на Stage 5,
   multidim fees — research на Stage 6+.

7. **PoW как долгосрочный консенсус.** PoW в 2026+ — устаревшая модель. Энергозатраты,
   централизация майнинга (ASIC), низкая finality. Для новой цепи лучше сразу выбрать
   PoS или PoW→PoS миграцию.

   **Альтернатива:** Hybrid PoW→PoS: PoW для genesis/fair launch первые 1-2 года,
   затем переход на PoS (как Ethereum).

8. **21M cap + halving (§Этап 5).** Копирование Bitcoin без обоснования. Фиксированный
   supply cap создаёт дефляционный spiral risk (hodling → низкая ликвидность).

   **Альтернатива:** Tail emission (как Monero: 0.6%/год) для постоянной безопасности
   сети. Или динамическая эмиссия, привязанная к TVL/активности.

9. **Ed25519 для подписей транзакций.** Ed25519 — хорош, но в блокчейнах стандарт de
   facto — ECDSA secp256k1 (Bitcoin, Ethereum) или BLS (Ethereum 2.0, для агрегации
   подписей). Ed25519 усложняет интеграцию с существующими кошельками (MetaMask,
   Ledger).

   **Альтернатива:** secp256k1 (ECDSA) или BLS12-381 (если планируется PoS/сигнатурная
   агрегация).

10. **LevelDB → redb/rocksdb (§Этап 3).** redb — хорош, но RocksDB (или даже LMDB) —
    battle-tested в production (Bitcoin Core, Ethereum). Для одного разработчика
    лучше взять проверенное решение, чем экспериментировать.

    **Альтернатива:** RocksDB + паттерн «column families» (как в reth).

11. **WIT / Wasm Component Model для ABI (§Этап 1.5).** Слишком новый стандарт (W3C,
    2023+). Мало tooling, мало аудитов.

    **Альтернатива:** Простой JSON ABI (как в Ethereum) для MVP, потом — переход на
    стандартизированный формат.

12. **Отсутствие лицензии (§Этап 7).** Это критическая ошибка. Без лицензии код
    нельзя легально использовать/форкать. Должно быть с Stage 0.

    **Альтернатива:** MIT/Apache-2.0 (как в Rust-экосистеме) или GPL-3.0 (если
    хотите copyleft). С Stage 0.

13. **Staking для finality gadget (§Этап 5).** «native token или отдельный?» —
    вопрос оставлен открытым. Это критичное решение, которое влияет на экономику.

    **Альтернатива:** Native token staking (не отдельный governance token). См.
    ARCHITECT3.md §7.

### 4.3 Что упущено

1. **Replay protection (chain_id).** Без `chain_id` транзакции из testnet валидны в
   mainnet и наоборот. Классическая атака.

2. **Nonce / sequence number.** Сейчас нет защиты от replay внутри одной цепи. Один
   и тот же `sender` может отправить одну транзакцию бесконечно.

3. **Transaction hash = commitment.** Нужен уникальный идентификатор транзакции для
   отслеживания, мемпула, отката.

4. **Mempool с basic rules.** Даже простой: проверка баланса, nonce, подписи,
   дедупликация. Без мемпула нет «pending» транзакций.

5. **Graceful shutdown.** Сейчас «ручное удаление LOCK» (`main.rs:188`). Нужен
   корректный `Drop` для LevelDB + flush буферов.

6. **Event log / receipts.** Без логов невозможно строить explorer, indexer, dApps.
   Ethereum без `logs` — не Ethereum.

7. **Block gas limit.** Даже фиксированный лимит (например, 1M gas) защищает от
   блоков, которые валидируются вечно.

8. **P2P message framing с max size.** Не просто «лимит размера», а proper framing:
   length-prefixed, с проверкой до аллокации.

9. **Rate limiting на P2P.** Защита от spam: лимит сообщений/сек от одного пира.

10. **Configuration management.** Сейчас `config.json` + `config.toml` + секция
    `[wallet]` в `Cargo.toml` — бардах. Нужен единый `Config` struct с валидацией.

11. **Testnet faucet.** Без faucet нет тестировщиков.

12. **Block explorer (минимальный).** Даже CLI-эксплорер (`strangecoin-cli block
    12345`).

13. **Snapshot sync.** Без snapshot sync новые узлы синхронизируются часами.

14. **Metrics (Prometheus/OpenTelemetry).** Мониторинг ноды: peers, mempool size,
    block time, sync status.

15. **JSON-RPC compatibility layer.** Совместимость с `eth_sendTransaction`,
    `eth_getBalance` позволит использовать MetaMask/WalletConnect.

16. **Tail emission как альтернатива max_supply.** Не рассматривается.

17. **Hybrid PoW→PoS миграция.** Не упоминается.

18. **secp256k1 / BLS12-381 вместо Ed25519.** Не упоминается.

19. **Strangler pattern как стратегия миграции.** Не упоминается.

20. **License (MIT/Apache-2.0).** Не упоминается с Stage 0.

21. **Bug bounty program + Immunefi integration с day-1.** Упоминается в Этапе 6, но
    не зафиксировано как day-1 invariant.

---

## 5. prompt.md

Документ — 117 промптов для AI-агентов (opencode/Codex/Claude) на Этап 0.
Декомпозиция отличная, но с теми же стратегическими ошибками, что и исходные
документы.

### 5.1 С чем согласен

1. **Декомпозиция Этапа 0 на 117 discrete prompts.** Отличный подход для
   AI-агентов: каждый промпт — самостоятельное задание с критериями готовности.
   Это позволяет параллелить работу и верифицировать результаты.

2. **Порядок выполнения: криптография → сериализация → консенсус → хранение → сеть.**
   Правильный dependency order: нельзя валидировать блок без подписей, нельзя
   хранить без канонической сериализации, нельзя синхронизировать без валидации.

3. **Сокращение КГ (критерии готовности).** Удобно для verification: каждый промпт
   имеет чёткие критерии, по которым AI-агент (или ревьюер) может проверить
   завершённость.

4. **Общий контекст для всех промптов.** Тип `Transaction`, `Block`, `Blockchain`,
   кошелёк Ed25519, keystore PBKDF2+AES-256-GCM. Это устраняет повторение контекста
   в каждом промпте и даёт AI общую картину.

5. **Принцип «не коммитить без явной просьбы».** Правильный для AI-agentic workflow:
   AI может делать несколько итераций, прежде чем результат готов к коммиту.

6. **Принцип «cargo check и cargo test после каждого задания».** Обязателен для
   постепенной валидации. AI-агент не должен оставлять неработающий код.

7. **Структура по разделам: A. Подписи, B. Сериализация, C. Consensus, D. Storage,
   E. Network, F. Tests, G. CI.** Логичное разделение по слоям.

8. **Каноническая структура подписанной транзакции (P001).** `signature: String`,
   `nonce: u64`, `is_coinbase: bool`, обратная совместимость через `#[serde(default)]`.
   Это правильный подход к постепенной миграции.

9. **Replay protection через chain_id, nonce.** Правильно зафиксировано.

10. **Median-time-past timestamp validation.** Правильный подход к защите от
    timestamp manipulation.

11. **Детерминированный генезис.** Правильный подход к консенсусной стабильности.

12. **Лимиты размеров (закрыть OOM).** Правильная защита от DoS.

13. **Модуляризация.** Правильный путь к поддерживаемому коду.

14. **Property-based тесты (proptest).** Правильный подход к проверке инвариантов
    консенсуса.

15. **CI с clippy -D warnings.** Правильный gate для quality.

### 5.2 С чем не согласен

1. **Ed25519 для подписей.** Повторяет ту же ошибку, что и ARCHITECT.md/ROADMAP.md.
   Нужно secp256k1 (совместимость с MetaMask/Ledger) и BLS12-381 (для будущего PoS).

2. **117 промптов — over-fragmentation.** Это создаёт координационную сложность:
   117 раз контекст загружается, 117 раз AI-агент начинает с нуля. Лучше 30-50
   более крупных промптов, каждый из которых закрывает логический блок.

3. **Отсутствие промптов для TLA+ спецификации.** Должна быть с Этапа 0.

4. **Отсутствие промптов для threat model.** Должна быть с Этапа 0.

5. **Отсутствие промптов для лицензии.** Должна быть с Этапа 0 (MIT/Apache-2.0).

6. **Отсутствие промптов для reproducible builds / cosign.** Должно быть с Этапа 0.

7. **Принцип «стиль проекта — println!» (общий контекст).** Это анти-паттерн для
   production. Нужно `log` crate (или `tracing`) с уровнями логирования. `println!`
   нельзя отключить в production, нельзя фильтровать, нельзя отправлять в структурированный лог.

8. **Сохранение совместимости с LevelDB форматом (P001).** Это создаёт extra
   complexity: каждый промпт должен учитывать legacy формат. Лучше мигрировать сразу
   на новую каноническую сериализацию.

9. **`serde` как зависимость для консенсусных путей.** Только для config/api, не для
   hashes. `serde_json::to_string` для хэшей — это источник недетерминизма.

10. **`rusty_leveldb` как зависимость.** Лучше сразу планировать миграцию на
    redb/RocksDB. Сохранение `rusty_leveldb` создаёт технический долг.

### 5.3 Что упущено

1. **Промпты для license выбора (MIT/Apache-2.0).** Должен быть первым промптом.

2. **Промпты для TLA+ спецификации.** Должны быть в разделе F (Tests).

3. **Промпты для threat model (STRIDE).** Должны быть в новом разделе H (Security).

4. **Промпты для reproducible builds / cosign.** Должны быть в разделе G (CI).

5. **Промпты для Bug bounty program / Immunefi.** Должны быть в новом разделе H.

6. **Промпты для Configuration management (единый Config struct).** Должны быть в
   новом разделе D2.

7. **Промпты для graceful shutdown (Drop для LevelDB).** Должны быть в разделе D.

8. **Промпты для Metrics (Prometheus/OpenTelemetry).** Должны быть в новом разделе I
   (Operations).

9. **Промпты для hybrid PoW→PoS миграции (хотя бы placeholder).** Должны быть в
   новом разделе J (Future).

10. **Промпты для secp256k1 / BLS migration.** Должны быть в разделе A.

11. **Промпты для indexer / GraphQL API.** Должны быть в будущем разделе K
    (Indexer).

12. **Промпты для SDK `cargo-strangecoin`.** Должны быть в будущем разделе L (SDK).

13. **Промпты для devnet (anvil-аналог).** Должны быть в будущем разделе M (Devnet).

14. **Промпты для fuzzing harness (cargo-fuzz targets).** Должны быть в разделе F.

15. **Промпты для differential testing VM.** Должны быть в будущем разделе N (VM
    testing).

16. **Промпты для JSON-RPC eth_* compatibility.** Должны быть в будущем разделе O
    (API).

17. **Промпты для snapshot sync.** Должны быть в разделе E (Network).

18. **Промпты для block explorer.** Должны быть в будущем разделе P (Explorer).

19. **Промпты для testnet faucet.** Должны быть в будущем разделе Q (Faucet).

20. **Промпты для rate limiting на P2P.** Должны быть в разделе E.

21. **Промпты для shadow-fork testing.** Должны быть в будущем разделе R (Shadow-fork).

22. **Промпты для formal verification harness.** Должны быть в будущем разделе S
    (Formal verification).

---

## 6. Перекрёстные упущения (отсутствуют во всех 4 документах)

Эти темы не упоминаются ни в одном из четырёх документов, но критичны для целевой
архитектуры:

1. **Hybrid PoW→PoS миграция как явная стратегия.** Все документы фиксируют PoW как
   «долгосрочный» или «premature PoS» как anti-goal. Реальная PoS-миграция (с
   стейкингом, slashing, validator set rotation) не рассматривается.

2. **Tail emission как альтернатива max_supply + halving.** Все документы копируют
   Bitcoin модель без обоснования. Дефляционный spiral risk не анализируется.

3. **secp256k1 / BLS12-381 вместо Ed25519.** Все документы фиксируют Ed25519.
   Совместимость с экосистемой (MetaMask, Ledger) не рассматривается.

4. **Strangler pattern как стратегия миграции.** Все документы описывают либо
   «модуль за модулем» (без явной паттерн-имени), либо «сохранение монолита до
   Этапа 3». Strangler pattern не упоминается.

5. **License с Stage 0.** Ни в одном документе license не упоминается как day-1
   requirement. Это критическая ошибка: без license код нельзя легально
   использовать/форкать.

6. **Configuration management (единый Config struct).** Ни в одном документе не
   упоминается как architectural invariant. Текущий бардах (`config.json` +
   `config.toml` + секция `[wallet]` в `Cargo.toml`) не фиксируется как проблема.

7. **Replay protection (chain_id) как invariant.** Упоминается в prompt.md, но не в
   ARCHITECT.md/ARCHITECT2.md/ROADMAP.md как architectural invariant.

8. **Nonce / sequence number как invariant.** Та же ситуация.

9. **Transaction hash = commitment как invariant.** Та же ситуация.

10. **Mempool basic rules как invariant.** Та же ситуация.

11. **Graceful shutdown как invariant.** Та же ситуация.

12. **Event log / receipts (для смарт-контрактов).** Не упоминается ни в одном
    документе.

13. **Block gas limit.** Не упоминается ни в одном документе.

14. **Rate limiting на P2P.** Не упоминается ни в одном документе.

15. **Testnet faucet.** Не упоминается ни в одном документе (кроме косвенно в
    ROADMAP.md Этап 4).

16. **Block explorer.** Не упоминается ни в одном документе как архитектурный
    компонент.

17. **Snapshot sync.** Не упоминается ни в одном документе.

18. **Metrics (Prometheus/OpenTelemetry).** Не упоминается ни в одном документе
    (кроме косвенно в ROADMAP.md Этап 7).

19. **JSON-RPC compatibility layer (eth_*).** Не упоминается ни в одном документе
    как архитектурный компонент.

20. **Indexer (Subgraph/sqd-аналог).** Упоминается в ROADMAP.md Этап 4, но не как
    архитектурный компонент.

---

## 7. Стратегические рекомендации

На основе анализа выше, рекомендации для `ARCHITECT3.md` и `ROADMAP2.md`:

1. **Консенсус:** Hybrid PoW→PoS. PoW bootstrapping первые ~2 года, затем PoS
   миграция через activation height. Native token staking, BLS12-381 для
   агрегации подписей. Это устраняет долгосрочный risk PoW (energy, ASIC
   centralization, low finality) и обеспечивает путь эволюции.

2. **Эмиссия:** Tail emission (Monero-style, 0.6%/год после первичного cap).
   Защита от дефляционного spiral risk, постоянный security budget.

3. **Криптография:** secp256k1 для EOA-подписей (совместимость с MetaMask/Ledger),
   BLS12-381 для консенсусных подписей (PoS-фаза, агрегация). Ed25519 — только
   опция для internal node-to-node auth.

4. **Стратегия миграции:** Strangler pattern. На Stage 0 — фиксация монолита. На
   Stage 1 — создаётся `strangecoin-core` крейт, в него переносится `serialize`,
   `consensus`, `state`. Постепенно весь монолит превращается в тонкий launcher.

5. **Смарт-контракты:** WASM (wasmi) в отдельном крейте. Host functions, газ, ABI
   через простой JSON (на MVP) с заделом на WIT migration. Параллельная разработка
   с ядром.

6. **State tree:** Verkle Trie для stateless validation witnesses. Решение
   зафиксировано с Stage 1 (не «нерешённое»).

7. **Storage:** RocksDB с column families (как в reth). Battle-tested, проверенный
   tuning.

8. **Async runtime:** Tokio на Stage 1 (с headers-first sync). Не откладывать до
   Stage 2.

9. **Threat model:** STRIDE анализ с Stage 0. Таблица 9+ векторов атак с
   митигациями в архитектуре.

10. **TLA+ спецификация:** С Stage 0 (не Stage 6). Safety/liveness properties,
    model-checking scope.

11. **MEV mitigation:** Threshold encryption mempool как архитектурный компонент с
    Stage 1 (placeholder), реализация на Stage 5.

12. **Account Abstraction:** ERC-4337 analog на Stage 5 (после Stage 1.5 VM и
    Stage 4 dev-experience).

13. **State rent / expiry:** Statelessness + pruning с Stage 3. State rent —
    опционально на Stage 3+.

14. **Upgrade governance (SCIP):** `consensus_version` + activation height с
    Stage 1. Любые изменения параметров — через SCIP + activation height.

15. **Reproducible builds / cosign:** С Stage 0 в CI. Воспроизводимые сборки,
    подписанные релизы (cosign/sigstore).

16. **License:** MIT/Apache-2.0 с Stage 0. Первый commit.

17. **Missing invariants:** Replay protection (chain_id), nonce, transaction hash,
    mempool basic rules, graceful shutdown, event log, block gas limit, rate
    limiting — все как architectural invariants в ARCHITECT3.md §5.

18. **Dev-experience:** SDK `cargo-strangecoin`, local devnet (anvil-аналог),
    indexer (Subgraph/sqd-аналог), block explorer, testnet faucet, JSON-RPC eth_*
    compatibility, metrics (Prometheus) — все как архитектурные компоненты с
    Stage 4.

19. **Fuzzing harness:** `cargo-fuzz` targets + differential testing VM с Stage 1.5.

20. **Formal verification:** K-framework / Boogie / Coq для критических контрактов
    с Stage 6.

---

## 8. Резюме

**Strangecoin v0.8.6 — прототип уровня «hello world blockchain»**, уязвимый ко всем
атакам. Все четыре документа (`ARCHITECT.md`, `ARCHITECT2.md`, `ROADMAP.md`,
`prompt.md`) дают сильный диагностический фундамент, но содержат стратегические
пробелы:

- **Консенсус:** фиксация PoW как долгосрочного закрывает путь к эволюции. Нужен
  hybrid PoW→PoS.
- **Эмиссия:** копирование Bitcoin без обоснования. Нужен tail emission.
- **Криптография:** Ed25519 без учёта совместимости. Нужен secp256k1/BLS12-381.
- **Миграция:** отсутствие явной стратегии. Нужен strangler pattern.
- **Безопасность:** TLA+ и threat model отложены на поздние этапы. Нужны с Stage 0.
- **Dev-experience:** SDK, devnet, indexer, explorer, faucet — упомянуты, но не как
  архитектурные компоненты.
- **License:** отсутствует. Критическая ошибка.

**Рекомендация:** в `ARCHITECT3.md` зафиксировать hybrid PoW→PoS, tail emission,
secp256k1/BLS12-381, strangler pattern, threat model, TLA+ spec, MEV mitigation
(placeholder), AA, statelessness, SCIP governance, reproducible builds — как
architectural invariants с Stage 0/1. В `ROADMAP2.md` — путь к этой архитектуре через
зависимости (без timeline), с явными Definition of Done по этапам.

См. `ARCHITECT3.md` для целевой архитектуры и `ROADMAP2.md` для пути к ней.
