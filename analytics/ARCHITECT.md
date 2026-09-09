# ARCHITECT.md — Архитектура Strangecoin

Крупноблочное описание целевой архитектуры. Отправная точка — прототип `0.8.6`
(монолит `src/main.rs` 1756 строк + `src/wallet.rs`, Ed25519, LevelDB, сырой TCP,
PoW без подписей). Документ описывает структурную цель, к которой ведёт `ROADMAP.md`
(Этапы 0→7). Код эволюционирует итерациями; это «форма будущего», а не мгновенный скачок.

Уровень абстракции: блоки/подсистемы и их связи, без деталей каждой функции.
Соглашения по границам данных описаны на уровне «что проходит через границу».

---

## 1. Принципы архитектуры

1. **Три консенсусных аксиомы** — база всего остального:
   - вся валидность выводится из цепочки блоков (детерминированно), `balances` — только кэш;
   - криптографическая целостность: подпись Ed25519 на каждую транзакцию, хэш-цепочка блоков;
   - никакого недетерминизма: нет wall-clock времени в консенсусе, нет float, только
     `blockhash`/время блоков как источник энтропии.
2. **Ядро без побочных эффектов** — консенсус (`consensus`, `state`, `serialize`) —
   чистые функции без I/O; внешние эффекты (сеть, БД, GUI) живут в тонких периферийных
   обёртках. Это делает консенсус тестируемым, аудируемым и переиспользуемым.
3. **Один источник истины** — блокчейн. Mempool, кэш балансов, индексы — производные,
   пересчитываемые структуры.
4. **Жёсткие границы модулей** — единственная точка входа в каждую подсистему;
   запрещено лезть в приватные поля извне (неявно через сокеты, DB, GUI).
5. **Всё, что может быть атакой, — валидируется на входе**: лимиты размеров, подписи,
   время, формат, генезис.

---

## 2. Контекстная диаграмма (границы системы)

```
                        ┌───────────────────────────────┐
                        │          Внешний мир          │
    другие ноды (P2P) ─▶│  TCP   JSON-RPC   CLI   GUI  │◀─ оператор/пользователь
                        └───────────────┬───────────────┘
                                        │
                        ┌───────────────▼───────────────┐
                        │         Strangecoin Node      │
                        │  ──────────────────────────── │
                        │  network/p2p ──▶ core ──▶ storage│
                        │  api          ─▶ wallet (крипто)│
                        └───────────────────────────────┘
                                        │
                        ┌───────────────▼───────────────┐
                        │   LevelDB/redb + файлы        │
                        │   chain, state, transactions, │
                        │   keystore, config, network.json│
                        └───────────────────────────────┘
```

Внешние интерфейсы: **P2P-протокол** (бинарный), **JSON-RPC** (для инструментов),
**GUI/CLI** (оператор), **файловая система** (персистентность и keystore).

---

## 3. Функциональные подсистемы

### 3.1 `serialize` — канонические байты
Один источник кодировки для всего консенсуса. Никакой `serde_json` в хэш-путях.
- детерминированная бинарная кодировка `Transaction`, `Block`;
- `txid(Transaction) -> [u8;32]`, `block_hash_header(Block) -> [u8;32]`;
- версия формата (`format_version`) — страховка от ломающих изменений;
- golden-векторы в тестах защищают от регрессий кодировки.

### 3.2 `consensus` — правила валидности (чистое)
Все проверки «какой блок/цепочка валидны», без состояния и I/O.
- **PoW**: `target` в заголовке, `hash <= target`; ретаргетинг по скользящему окну;
- **время**: `median_time_past`, запрет timestamp из будущего;
- **эмиссия**: `block_reward_at_height`, halving, `max_supply`;
- константы консенсуса — в одном месте.

### 3.3 `state` — детерминированное исполнение
Превращает цепочку блоков в состояние (балансы, позднее — контрактный storage).
- чистые `apply_block`/`unapply_block`;
- результат — state root (Этап 1): дерево состояния, корень в каждом блоке;
- `balances` — кэш, пересчитываемый из цепочки.
Это фундамент, на котором позже строится смарт-контрактная VM (Этап 1.5).

### 3.4 `blockchain` — оркестратор консенсуса
Состоятельная корневая сущность узла, связывает `consensus` + `state` + `storage`.
- приём/крепление блока: валидация → применение состояния → сохранение;
- выбор вершин по **cumulative work** (сумме сложности), корректный реорг;
- публичный API: `add_block`, `apply_transaction`, `tip`, `settled_balances`.

### 3.5 `mempool` — пул неподтверждённых транзакций
Отдельная периферийная подсистема (не часть `blockchain`), владеющая своим
`Mutex<Mempool>`. Производное: очищается при включении в блок.
- валидация подписи/dup/nonce на `insert` (делегирует blockchain-методу);
- эвристика приоритизации по комиссии (комиссии — Этап 1);
- лимиты размера/количества (`MAX_PENDING_TXS`).
Мемпул не хранит данные в LevelDB и не знает про сеть — только in-memory
и взаимодействие с blockchain по публичным методам.

### 3.6 `network/p2p` — общение узлов
Бинарное кадрирование поверх TCP.
- `protocol.rs`: кодировка сообщений (`HELLO`, `GET_BLOCKCHAIN`, `BLOCKCHAIN`, `TX`,
  `IDENTIFY`, `ADDR`, `PING`), версионирование, лимиты размеров;
- `p2p.rs`: сокеты, таймауты, лимит соединений, баны, discovery;
- синхронизация: headers-first (Этап 1), передача недостающих блоков, cumulative work.
- шифрование транспорта (Noise/TLS) — Этап 2.

### 3.7 `storage` — персистентность
Единственный модуль, знающий про LevelDB (позже redb/rocksdb).
- ключи: chain, state, difficulty/target, nonces, отдельные транзакции по txid;
- снапшоты/чекпоинты состояния, прунинг, crash-recovery (без ручного удаления LOCK);
- индексы адрес→баланс/UTXO (Этап 3).

### 3.8 `wallet` — криптографический кошелёк
Именные ключи и подпись. `sign_transaction` подписывает канонические байты.
- Ed25519, keystore AES-256-GCM + PBKDF2 (≥ 210k итераций), уникальный nonce;
- `address_from_public_key` (bech32 — Этап 1);
- далее HD (BIP-39/44) — Этап 4.
Кошелёк не касается сети/БД; его эксплуатируют GUI/CLI/API.

### 3.9 `api` — JSON-RPC + CLI
Тонкий слой-фасад поверх `blockchain`/`wallet`:
- JSON-RPC (Bitcoin/Ethereum-подобные методы) — Этап 4;
- CLI: `--balance`, `--send --sign`, `--mine-once`, `--change-password`.
Среди целей — поддержка тестовых сценариев без GUI.

### 3.10 `gui` — клиент оператора (egui)
Потребитель `api`/`blockchain`/`wallet`. Показывает адрес/баланс/майнинг,
инициирует перевод (обязательно подписанный). Никогда не имеет доступа к внутренностям
консенсуса напрямую (только через методы).

### 3.11 `config` — параметры и генезис
- `genesis.json`: входные данные для детерминированного генезиса (timestamp,
  initial_holder, block_reward, halving, max_supply, target_block_time,
  retarget_interval, format_version). Сам генезис сверяется по `EXPECTED_GENESIS_HASH`
  из `consensus.rs` (10.6) — редактирование файла не меняет консенсус;
- `config.json`: только НЕсекретные параметры (порт, ip, name, node_mode) — нет пароля;
- `network.json`: публичные адреса пиров.

---

## 4. Потоки данных (главные сценарии)

### 4.1 Перевод средств (пользователь → блокчейн)
```
GUI/CLI ──send(to, amount)──▶ api
   └─ wallet.sign_transaction(tx_bytes) ──▶ Transaction{ sig, nonce }
api ──▶ blockchain.apply_transaction ──▶ [mempool.insert → валидация]
mempool ──▶ network.announce_tx ──▶ пиры
майнер: blockchain.mine_block( мемпул + coinbase ) ──▶ Block
blockchain: consensus.validate → state.apply_block → storage.save → network.announce_block
```

### 4.2 Синхронизация с пиром
```
network: HELLO → GET_BLOCKCHAIN(header) ──▶ пир отвечает недостающими блоками
blockchain: для каждого блока: consensus.validate → state.apply → storage.store
            выбор вершины по cumulative_work; при расхождении — реорг (unapply/apply)
mempool: отброс транзакций, попавших в чужие блоки (по txid)
```

### 4.3 Майнинг (без блокировки узла)
```
api/worker: читает tip + mempool → ищет nonce под target (чистая, без лока на blockchain)
найдено: consensus.validate(block) → state.apply_block → storage.commit (write-lock кратко)
новый блок от сети: прерывает поиск → пересчитывает tip/difficulty → продолжает
```

### 4.4 Старт узла
```
storage.load_state(chain, state_root, mempool) → blockchain.init
   → сверка кэша балансов с пересчётом из цепочки
   → genesis: если БД пуста — создать по genesis.json; иначе валидировать генезис
config.load → crypto (wallet) → network.start
```

---

## 5. Ключевые инварианты (нельзя нарушать)

| Инвариант | Где enforce |
|-----------|-------------|
| Вся валидность — из цепочки; `balances` — кэш | `state`, `blockchain` |
| Каждая транзакция подписана, `sender == pubkey` | `consensus`, `mempool` |
| `hash <= target` для каждого блока при его difficulty | `consensus` |
| `apply_block`/`unapply_block` — обратные чистые функции | `state` |
| Хэш/подпись — на канонических байтах (`serialize`), не JSON | `serialize` |
| Блок не содержит непроверимых транзакций/наград сверх эмиссии | `blockchain` |
| Размеры сообщений/блоков всегда ограничены до аллокации | `network/p2p`, `blockchain` |
| Генезис детерминирован и совпадает у всех узлов | `config/genesis` |
| Секреты не пишутся недиск и не логируются | `config`, `wallet`, `gui` |

---

## 6. Модульная карта (целевая структура каталогов)

```
src/
├── main.rs          # тонкий запуск: config → wallet → node → gui (или headless)
├── lib.rs           # экспорт публичного API для тестов/внешних крейтов
├── config.rs        # config.json, network.json; НЕ содержит генезис-константы
├── events.rs        # событийная шина (BlockApplied, TxReceived, Reorg, MiningProgress…)
├── serialize.rs     # канонические байты, txid, hash
├── consensus.rs     # PoW, time, emission, константы + EXPECTED_GENESIS_HASH (ЧИСТОЕ)
├── state.rs         # apply_block/unapply_block, state trie (ЧИСТОЕ)
├── address.rs       # address_from_public_key (→ bech32)
├── error.rs         # типизированные ошибки
├── blockchain/
│   ├── mod.rs       # Blockchain: validate, apply, select-tip, reorg
│   └── genesis.rs   # детерминированный генезис (build из параметров, сверка хэша)
├── mempool.rs       # пул неподтверждённых транзакций (ОТДЕЛЬНАЯ подсистема)
├── network/
│   ├── protocol.rs  # кадрирование/кодировка сообщений, network_id
│   ├── p2p.rs       # сокеты, таймауты, discovery, баны
│   └── sync.rs      # headers-first (Этап 1), выбор вершины по cumulative work
├── storage/
│   ├── mod.rs       # LevelDB/redb обёртки, снапшоты
│   └── schema.rs    # схема ключей
├── wallet.rs        # Ed25519, keystore, sign (без сети/БД)
├── vm/              # (Этап 1.5) WASM/RISC-V рантайм, ABI, газ, precompiles
├── api/
│   ├── jsonrpc.rs   # (Этап 4) JSON-RPC методы + subscriptions
│   └── cli.rs       # CLI команды (--no-gui сборка: только node)
└── gui/
    └── mod.rs       # egui-клиент (feature "gui", по умолчанию вкл)
tests/               # two_clients, reorg, double_spend, pow, emission, time, network
```

---

## 7. Слойность и запреты зависимостей

```
            gui / cli / api(jsonrpc)        ← ввод/вывод, подписка на события
                   │
              events (шина)                 ← pub/sub: GUI, метрики, API, тесты
                   │
        blockchain   ↔   mempool            ← оркестрация, реорг; мемпул — отдельная
          /    \          │                      периферийная подсистема
     network      storage │
        │             ┌───┘
        └── sync ─────┘
             \        /
       consensus / state / serialize        ← чистое ядро (0 I/O)
             |
   [wallet / vm — отдельные библиотеки, зависят только от serialize]
```

Правила:
- `consensus`, `state`, `serialize` **не знают** про network/storage/gui/db/events.
- `mempool` — не часть `blockchain`; их связь только через публичные методы.
- `wallet` — самостоятельный крейт (нет сети/БД/ноды); `vm` (Этап 1.5) — тоже.
- `storage` — единственный держатель LevelDB/redb.
- `api/gui` ходят в blockchain/network только через публичные методы; изменение
  состояния анонсируется через `events`, а не через прямой доступ к Mutex.
- Ничто ниже не зависит от GUI; GUI — feature, не обязательный компонент.

---

## 8. Стратегия эволюции (этапы ROADMAP → архитектура)

| Этап | Что меняется в архитектуре |
|------|----------------------------|
| **0** — санация | Подписи, каноническая сериализация, difficulty/time/эмиссия, лимиты, выделение модулей, типизированные ошибки, `RwLock`, тесты, CI |
| **1** — криптоядро | Merkle/state root в блоке, headers-first, комиссии, address→bech32, finality/checkpoint |
| **1.5** — смарт-контракты | В `state`: контрактные аккаунты + storage trie; новый компонент `vm/` (WASM/RISC-V), ABI, газ, precompiles, SCIP |
| **2** — сеть | Gossip + compact blocks/Erlay, discovery, handshake, peer scoring, шифрование транспорта |
| **3** — хранение | redb/rocksdb + снапшоты, индексы UTXO/баланс, прунинг |
| **4** — интерфейс | JSON-RPC, HD-кошелёк, индексатор, SDK `cargo-strangecoin`, local devnet |
| **5** — экономика | блоки комиссий/базы (EIP-1559), MEV-митигация, activation-height |
| **6/7** — безопасность/сообщество | TLA+ спека, формальная верификация VM, аудит, bug bounty, repro-сборки |

Ключевые «точки расширения», заложенные уже сейчас:
- **`serialize` + `format_version`** → безопасное введение контрактов и новых типов tx;
- **`state.apply_block`/state root** → фундамент для VM без переписывания консенсуса;
- **`address_from_public_key`** → смена base64→bech32 без ломки подписей;
- **типизированные ошибки + API-фасад** → добавление JSON-RPC без вторжения в ядро.

---

## 9. Нерешённые/отложенные архитектурные решения

- **VM-рантайм**: WASM (`wasmi`) vs RISC-V (`ckb-vm`) — решение на Этапе 1.5;
- **Дерево состояния**: Sparse Merkle Trie vs Verkle — влияет на stateless-клиенты;
- **Схема хранения**: остаться на LevelDB или мигрировать на redb/rocksdb — Этап 3;
- **Целевость узла/делегирование (PBS)** и **стейкинг** — влияние на модуль сети/экономики.

Решение по каждому фиксируется в `docs/ADR/` (Architecture Decision Records) до того,
как затронет консенсус (требование «изменения только через activation height»).

---

## 10. Видение автора: дополнения к архитектуре

Раздел фиксирует решения, которых не было в первоначальном документе. Пункты 10.1–10.5
должны соблюдаться с Этапа 0, остальные — по мере приближения соответствующих этапов.

### 10.1 Модель параллелизма (deadlock-free)

Текущий код: один глобальный `Mutex<Blockchain>` + разбросанные `mpsc`. Итоговая модель:

- **Потоки:** (а) сетевой event-loop + по соединению; (б) mining-worker; (в) sync-worker;
  (г) главный (GUI/CLI/headless). Никакой общий поток «всё внутри одного Mutex».
- **Владение данными:** каждая подсистема владеет своим состоянием:
  `blockchain → RwLock<Blockchain>`, `mempool → Mutex<Mempool>`,
  `storage → Mutex<Storage>`, `peers → Mutex<Vec>`.
- **Lock ordering (обязательное правило, чтобы не было дедлоков):**
  `storage` → `blockchain` → `mempool`. Обратный порядок запрещён. Взятие лока
  не должно пересекаться с сетевым I/O и дисковыми записями: под локом только
  in-memory переход, коммит — вне лока.
- **Никакой сети/диска под локом ядра:** консенсус-вызовы выполняются на данных
  в памяти; сетевой обмен и запись в LevelDB происходят после освобождения.
- **debug-инвариант:** single-writer на write-lock (например, только blockchain
  внутри блокировки изменяет цепочку; mempool не мутирует chain).

### 10.2 Типы узлов (full / light)

Один бинарник, режим — в конфиге (`node_mode`), чтобы серверы и клиенты не собирались
по-разному:
- **Full node** — полная цепочка + state; опционально майнинг.
- **Light client (SPV, Этап 1+)** — только заголовки + merkle/verkle-пробы; баланс
  проверяется через proof, а не через копию state. Не участвует в майнинге.
- **Archival node** (для индексатора/эксплорера, Этап 4) — полная история + state.

`network/sync.rs` — единственная точка реализации синхронизации, параметризуемая
`node_mode`; `api` — единый интерфейс для всех трёх режимов.

### 10.3 Headless-режим и feature flags

- Cargo features: `default = ["gui"]`; серверная сборка `cargo build --no-default-features`
  → демон без egui (сигналы SIGTERM/SIGINT → корректный `save_state`).
- Node как процесс-демон: systemd unit, Docker image; единственная точка входа —
  CLI/API, GUI только потребитель.
- GUI не является архитектурным компонентом консенсуса (см. правило «Ничто ниже не
  зависит от GUI»).

### 10.4 Событийная шина (`events`)

Замена «GUI сам читает Mutex» (main.rs:1454) и разбросанных mpsc:
- node публикует события: `BlockApplied`, `TransactionReceived`, `ChainReorged`,
  `MiningProgress`, `PeerConnected/Dropped`, `StatePersisted`.
- Подписчики: GUI (перерисовка), метрики (Этап 7), JSON-RPC `eth_subscribe`-подобные
  подписки (Этап 4), тесты (детерминированная проверка без sleep).
- Типы событий — в `events.rs`; шина — `broadcast` (mpsc + подписчики). Потоки пишут
  события через `tx.send`, никогда не блокируясь на блокировках подписчиков.

### 10.5 Идентификация сетей (mainnet/testnet/regtest)

- `network_id` в генезисе и в `HELLO`-сообщении: mainnet=1, testnet=2, regtest=3.
- Пиры с чужим `network_id` отбрасываются до любых данных. Это отделяет тестовую
  деятельность от «боевой» цепи и делает невозможным случайный merge.
- Все тестовые сценарии (P076–P080) работают на `regtest` с фейковым кликом времени
  (детерминизм), а не на mainnet-параметрах.

### 10.6 Генезис — консенсусный якорь, а не конфиг

- `genesis.json` — только *входные данные* для первой сборки генезиса.
- `EXPECTED_GENESIS_HASH` — константа в `consensus.rs`; узел при старте сверяет
  локальный генезис по хэшу и отказывается работать при расхождении.
- Параметры `genesis.json` меняются ТОЛЬКО через активацию по высоте, никогда —
  редактированием файла.

### 10.7 Workspace-структура (целевая, Этап 3+)

Монолит `src/` сохраняется до тех пор, пока это удобно для рефакторинга Этапа 0,
но целевая форма — workspace:

```
strangecoin-core     # consensus, state, serialize, blockchain-оркестрация (0 I/O)
strangecoin-net      # p2p, protocol, sync
strangecoin-node     # сборка узла: блокировки, потоки, события, хранение
strangecoin-wallet   # Ed25519, keystore, HD (без сети/БД)
strangecoin-api      # jsonrpc + cli
strangecoin-gui      # egui-клиент
```

Выгода: ядро можно аудировать/тестировать отдельно; независимые имплементации
(альтернативная нода, SDK) не тянут egui/LevelDB; `strangecoin-core` переиспользуется
в контрактном toolchain (Этап 4 SDK).

### 10.8 Уточнение «что делает blockchain»

Помимо оркестрации, `blockchain` владеет: выбором вершины (cumulative work),
применением форка (unapply/apply), инвариант-проверкой после каждого принятого блока
и «коммит-точкой» для storage. Mempool, сеть и GUI к этой логике не имеют доступа —
только через публичные методы `blockchain`.

### 10.9 Тестируемость как архитектурный требование

- Чистое ядро позволяет: золотые векторы, proptest-инварианты, fuzzing (сетевой вход,
  сериализация), дифференциальное тестирование VM (Этап 1.5).
- Детерминированный режим (regtest + fake clock) обязателен для всех сетевых тестов —
  никаких `sleep`-эвристик в проверках.
- CI (Этап 0, P081) гоняет все три платформы; `clippy -D warnings` — часть gate.

### 10.10 Что я НЕ стал бы делать в ближайшее время

- **Async-фреймворк (tokio)** на Этапе 0: threads + mpsc достаточно и проще для аудита;
  переход возможен на Этапе 2 вместе с handshake/gossip (ADR).
- **Своя VM-семантика с нуля**: берём WASM/RISC-V (см. Этап 1.5) — готовые
  интерпретаторы, фаззеры и LLVM-бэкенд.
- **Шардинг / PoS-замена PoW**: не трогаем до finality-гаджета (Этап 1) и стабильного
  консенсуса — любые такие изменения идут через SCIP + activation height.

---

## 11. Архитектурные решения (closing the gaps)

Раздел фиксирует жёсткие решения по выявленным рискам. Обязательны к соблюдению с Этапа 0/1.

### 11.1 Декомпозиция `blockchain` (устранение God-object)

`src/blockchain/` раскладывается на **четыре** независимых компонента:

| Компонент | Ответственность | Владеет состоянием |
|-----------|----------------|-------------------|
| `chain_selector.rs` | Tip selection по cumulative work, fork choice, reorg logic (unapply/apply), tie-breaking | `tip_height`, `tip_hash`, `total_work` |
| `block_executor.rs` | Валидация блока (`consensus.validate`), исполнение (`state.apply_block`), coinbase reward | нет (чистый) |
| `state_cache.rs` | `balances` кэш + invalidation, `nonces` кэш, пересчёт из цепочки при расхождении | `HashMap<Addr, Balance>`, `HashMap<Addr, Nonce>` |
| `blockchain_facade.rs` | Публичный API: `add_block`, `apply_tx`, `get_balance`, `get_tip`; делегирует выше | `RwLock<BlockchainInner>` (обёртка) |

**Правила:**
- `chain_selector` не знает про state/transactions — только заголовки и work.
- `block_executor` не знает про выбор вершины — только «примени этот блок на это состояние».
- `state_cache` — единственное место, где читаются балансы (GUI/API → `facade.get_balance`).
- `facade` — **единственная** точка входа для network/mempool/api/gui.

### 11.2 Events bus — multi-subscriber broadcast

```rust
// events.rs
use std::sync::{Arc, Mutex};
use crossbeam_channel::Sender;  // или flume/tokio::sync::broadcast

pub struct EventBus {
    subscribers: Mutex<Vec<Sender<NodeEvent>>>,
}
impl EventBus {
    pub fn subscribe(&self) -> Receiver<NodeEvent> { … }
    pub fn publish(&self, event: NodeEvent) { … }  // не блокируется на медленном подписчике
}
```

- **Никаких `std::sync::mpsc`** для broadcast — только multi-producer multi-consumer канал.
- Публикация неблокирующая: `try_send` + drop slow subscriber (или unbounded channel).
- События: `BlockApplied`, `BlockReorged { old_tip, new_tip }`, `TxAccepted`, `TxRejected`, `PeerScoreChanged`, `MiningStarted/Finished`.

### 11.3 Sync engine — разрыв цикла зависимостей

```
network (p2p.rs)          blockchain (facade)          sync (sync.rs)
      │                          │                          │
      ├── announces new block ──▶│                          │
      │                          ├── validate+apply ──────▶│ (internal)
      │                          │                          │
      ◀── requests missing blocks │                          │
      │                          │                          │
```

- `SyncEngine` **владеет** ссылками на `BlockchainFacade` и `NetworkService`.
- `NetworkService` не вызывает `blockchain` напрямую — только кладёт входящие блоки в `SyncEngine.inbox` (mpsc).
- `SyncEngine` — единственный, кто делает `validate → apply → announce`.
- `BlockchainFacade` не знает про `SyncEngine` (нет обратного вызова).

### 11.4 VM граница — trait в core, runtime отдельно

```rust
// strangecoin-core/src/vm/traits.rs
pub trait VmExecutor {
    fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, VmError>;
    fn gas_cost(&self, opcode: Opcode) -> u64;
}

// strangecoin-vm-wasm / strangecoin-vm-riscv — имплементации
```

- `core` зависит от **trait**, не от конкретного рантайма.
- `node` (сборочный крейт) выбирает реализацию: `wasmi` / `ckb-vm` / `risc0`.
- Это позволяет аудировать `core` без wasm-рантайма и тестировать VM изолированно.

### 11.5 Protocol messages — версионирование и deprecation

```rust
// network/protocol.rs
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello { version: u32, network_id: u8, .. },
    GetHeaders { from_height: u64 },      // ← headers-first (Этап 1)
    Headers(Vec<BlockHeader>),
    GetBlocks(Vec<BlockHash>),            // ← bodies по хэшам
    Block(Block),
    Tx(Transaction),
    Ping { nonce: u64 },
    // DEPRECATED (Этап 0 only, удалить в Этапе 1):
    #[deprecated(note = "Use GetHeaders/Headers")]
    GetBlockchain,
    #[deprecated]
    Blockchain(FullChain),
}
```

- `format_version` в `HELLO` — минимальная версия протокола, которую понимает узел.
- Пир с `version < MIN_PROTO_VERSION` отключается.

### 11.6 Peer scoring — отдельная подсистема

```
network/
  ├── peer_store.rs      # PeerInfo { addr, last_seen, score, banned_until }
  ├── peer_manager.rs    # score updates, ban logic, selection for sync
  └── connection_pool.rs # активные соединения, лимиты, backpressure
```

- `PeerStore` — данные (Mutex/RwLock), `PeerManager` — логика (чистая, тестируемая).
- `ConnectionPool` не знает про скор — только держит открытые соединения.
- Scoring factors: latency, invalid blocks/txs sent, protocol violations, uptime.
- Ban threshold настраивается, бан — временный (expire) + persistent (в network.json).

### 11.7 Tie-breaking rule для cumulative work

```rust
// chain_selector.rs
fn select_best(chains: &[Chain]) -> Chain {
    chains.iter()
        .max_by(|a, b| {
            a.total_work.cmp(&b.total_work)
                .then_with(|| b.tip_timestamp.cmp(&a.tip_timestamp)) // earliest wins
                .then_with(|| a.tip_hash.cmp(&b.tip_hash))          // lowest hash wins
        })
        .cloned()
}
```

- Правило: **больше work → раньше timestamp → меньше hash**. Детерминировано, нет гонок.

### 11.8 Mempool replacement policy (RBF)

```rust
// mempool.rs
pub enum InsertResult { Accepted, Replaced(Vec<TxId>), Rejected(Reason) }

pub fn insert(&mut self, tx: Transaction) -> InsertResult {
    if self.len() >= MAX_PENDING_TXS {
        // RBF: находим tx с меньшим feerate, которую можно заменить
        if let Some(victim) = self.find_replaceable(tx.feerate()) {
            self.remove(victim);
        } else {
            return Rejected(Reason::MempoolFull);
        }
    }
    // проверка подписи, nonce, дублей...
    Accepted
}
```

- `feerate = fee / tx_weight` (пока fee=0 — по размеру).
- `Replaced` возвращает список вытесненных txid для анонса `TxRejected` в events.

### 11.9 Storage schema — версионированные ключи + миграции

```
storage/
  ├── schema.rs           # KeyPrefix enum, encode_key(prefix, ...), decode_key
  ├── migrations.rs       # Migration { from_version, to_version, fn apply(db) }
  └── mod.rs              # Storage { db, current_schema_version }
```

- Префиксы: `c/` chain, `s/` state root, `b/` balances, `n/` nonces, `t/` txs, `d/` difficulty, `m/` meta.
- `current_schema_version` хранится в БД (`m/schema_version`).
- При старте: `while current < TARGET { apply_migration(current); current++ }`.
- LevelDB→redb миграция = отдельная миграция, не ручное копирование.

### 11.10 Wallet — roadmap к отдельному крейту

Текущее состояние (§6): `src/wallet.rs` в монолите.
Путь:
1. Этап 0: оставить в `src/`, но **public API** только `sign/verify/address/load/save`.
2. Этап 1: вынести в `crates/wallet/` внутри workspace, `strangecoin-core` зависит от `strangecoin-wallet` через trait `Signer`.
3. Этап 3: публикуем `strangecoin-wallet` на crates.io — независимый SDK для клиентов.

### 11.11 Чего НЕ делаем (явные anti-goals)

| Anti-goal | Причина |
|-----------|---------|
| `tokio` / async в Этапе 0 | threads + mpsc достаточно, сложнее аудит; переход на Этапе 2 (ADR) |
| Своя VM инструкция | WASM/RISC-V — готовые интерпретаторы, фаззеры, LLVM бэкенд |
| PoS / шардинг до Этапа 1 finality | консенсус должен стабилизироваться; изменения только через SCIP |
| `serde_json` в консенсусных путях | только бинарная `serialize`; JSON — только config/network/api |
| Глобальные `lazy_static`/`once_cell` для консенсусных констант | константы в `consensus.rs` как `pub const`; тесты подменяют через dependency injection |

---

## 12. Чек-лист соответствия (для PR review)

Каждый PR на Этапах 0–1 обязан закрывать соответствующие пункты:

```
[ ] 11.1 blockchain → 4 компонента (chain_selector, block_executor, state_cache, facade)
[ ] 11.2 events — multi-subscriber broadcast (не mpsc)
[ ] 11.3 sync engine — разрыв цикла network↔blockchain
[ ] 11.4 VM trait в core, runtime отдельно
[ ] 11.5 protocol messages — GetHeaders/Headers, deprecated legacy
[ ] 11.6 peer_store/peer_manager/connection_pool разделены
[ ] 11.7 tie-breaking: work → earliest timestamp → lowest hash
[ ] 11.8 mempool RBF policy implemented
[ ] 11.9 storage schema versioned + migrations
[ ] 11.10 wallet — public API only, path to separate crate documented
[ ] 11.11 anti-goals respected (no tokio, no custom VM, no PoS yet)
```
