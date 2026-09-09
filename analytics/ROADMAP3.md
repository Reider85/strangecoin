# ROADMAP2.md — Путь к ARCHITECT3.md

**Версия документа:** 2.0 (альтернатива ROADMAP.md)
**Дата:** 2026-09-09
**Цель:** определить путь от текущего прототипа `0.8.6` к целевой архитектуре
`ARCHITECT3.md`.

## Введение

Этот документ — альтернативная дорожная карта, ведущая к архитектуре
`ARCHITECT3.md` (hybrid PoW→PoS, tail emission, secp256k1/BLS, WASM via wasmi,
Verkle Trie, RocksDB, strangler pattern migration, threat model + TLA+ с Stage 0).

В отличие от `ROADMAP.md`, который:
- фиксирует PoW как долгосрочный консенсус;
- использует Ed25519 и halving + max_supply;
- откладывает TLA+/threat model/reproducible builds на Stage 6;
- не упоминает license, replay protection, nonce как invariants.

`ROADMAP2.md`:
- явно проектирует PoW→PoS миграцию на Stage 7;
- использует secp256k1/BLS12-381 и tail emission;
- закладывает TLA+/threat model/reproducible builds/license с Stage 0;
- фиксирует все 22 invariants из `ARCHITECT3.md` §5 как day-1 requirements.

---

## Принципы планирования

1. **Без timeline.** Документ описывает **последовательность зависимостей**, а не
   даты. Это позволяет гибко реагировать на находки аудитов, занятость
   разработчиков, emergence новых технологий. Стадия считается завершённой по
   достижении **Definition of Done**, не по календарю.

2. **Hard dependencies.** Каждая стадия имеет явные **preconditions** — что
   должно быть завершено до её начала. Например, Stage 1.5 (WASM) требует Stage 1
   (state root, Verkle Trie), потому что без state root контрактный storage не
   может быть валиден.

3. **Activation height для любых изменений консенсуса.** После Stage 0 (mainnet
   freeze пред-состояния) любые изменения правил консенсуса — через SCIP +
   activation height. Это включает: PoW→PoS миграцию (Stage 7), изменения
   эмиссии, новые типы транзакций, новые precompiles, изменения gas cost.

4. **Strangler pattern для миграции.** Каждая стадия после Stage 0 переносит
   одну или несколько подсистем в отдельный крейт (`strangecoin-core`, `-net`,
   `-storage`, `-wallet`, `-vm-wasm`, `-node`, `-api`, `-gui`, `-indexer`).
   `src/main.rs` постепенно превращается в тонкий launcher.

5. **Security-first.** Threat model (STRIDE), TLA+ спецификация, fuzzing harness,
   bug bounty (Immunefi), reproducible builds (cosign) — все заложены с Stage 0,
   не откладываются на потом.

6. **Definition of Done per stage.** Каждая стадия имеет чёткие критерии
   завершения: список фичей, тестов, ADRs, обновлений документации. Стадия
   считается завершённой только по достижении всех критериев.

7. **Backward compatibility.** После Stage 0 (sanitization) любые изменения должны
   сохранять совместимость с предыдущими mainnet-блоками. Несовместимые изменения
   — через hard fork + activation height (см. `ARCHITECT3.md` §8.3).

8. **One ADR per architectural decision.** Каждое архитектурное решение (выбор
   RocksDB, переход на tokio, активация PoS) фиксируется в `docs/ADR/` до того,
   как затронет код.

---

## Карта зависимостей

```
Stage 0: Санация прототипа (на монолите)
   │
   ├──▶ Stage 1: Криптоядро + Verkle Trie (strangler: strangecoin-core крейт)
   │       │
   │       ├──▶ Stage 1.5: WASM смарт-контракты (strangecoin-vm-wasm крейт)
   │       │       │
   │       │       └──▶ Stage 4: Dev-experience (SDK, devnet, indexer, JSON-RPC eth_*)
   │       │               │
   │       │               └──▶ Stage 5: Экономика (tail emission, EIP-1559, MEV, AA)
   │       │                       │
   │       │                       └──▶ Stage 7: PoS миграция
   │       │
   │       └──▶ Stage 2: Сетевая зрелость (gossip, Noise, Erlay, tokio)
   │               │
   │               └──▶ Stage 3: Хранение и масштабируемость (RocksDB, stateless)
   │                       │
   │                       └──▶ Stage 6: Безопасность и формальная верификация
   │
   └──▶ [Сквозные треки: Security, Governance, Documentation, Operations]
```

**Жёсткие зависимости:**
- Stage 1 → Stage 0 (нельзя проектировать Verkle Trie без чистого state).
- Stage 1.5 → Stage 1 (нельзя строить VM без state root).
- Stage 2 → Stage 1 (нельзя делать gossip без канонической сериализации).
- Stage 3 → Stage 2 (нельзя мигрировать на RocksDB без понимания sync).
- Stage 4 → Stage 1.5 + Stage 2 (SDK/devnet без VM и сети бесполезны).
- Stage 5 → Stage 4 (экономика без dev-experience не тестируема).
- Stage 7 → Stage 5 + Stage 6 (PoS без экономики и security невозможен).

**Мягкие зависимости (могут идти параллельно):**
- Stage 1.5 и Stage 2 (после Stage 1).
- Stage 3 и Stage 4 (после Stage 2 и Stage 1.5).
- Stage 6 (security) начинается с Stage 0, продолжается параллельно.

---

## Этап 0 — Санация прототипа (на монолите)

### Цели

Превратить «hello world blockchain» с критическими уязвимостями в минимально
безопасный прототип, готовый к постепенной декомпозиции. **Это precondition для
всего остального** — без подписей транзакций, валидации difficulty и
детерминированного генезиса всё бессмысленно.

### Зависимости

- Текущий прототип `0.8.6` (монолит `src/main.rs` 1756 строк + `src/wallet.rs`).

### Задачи

**Криптография:**
- [ ] Заменить Ed25519 на secp256k1 (ECDSA) для подписей транзакций (совместимость с
      MetaMask/Ledger). Сохранить Ed25519 только для internal node-to-node auth
      (если нужно).
- [ ] Подпись и проверка каждой транзакции: `sign_transaction` вызывается
      обязательно, верификация при приёме и синхронизации; `sender` обязан
      совпадать с публичным ключом подписанта.
- [ ] Replay protection: `chain_id` в каждой транзакции (mainnet=1, testnet=2,
      regtest=3).
- [ ] Nonce / sequence number: `account.nonce` строго инкрементируется; tx с
      `nonce <= account.nonce` отвергается.
- [ ] Transaction hash = commitment: `txid` выводится из канонических байтов всей
      транзакции (включая подпись).

**Сериализация:**
- [ ] Каноническая (бинарная, детерминированная) сериализация транзакций/блоков.
- [ ] Подпись и хэш строятся на ней, не на `serde_json::to_string`.
- [ ] `format_version` в каждой кодировке (для будущих изменений).

**Консенсус:**
- [ ] Проверка `difficulty` в `validate_chain`: `hash.starts_with("0".repeat(difficulty))`.
- [ ] Запрет снижения `difficulty` иначе как по правилу ретаргетинга.
- [ ] Алгоритм ретаргетинга: скользящее окно по реальному времени блоков (epoch
      или sliding window).
- [ ] Median-time-past timestamp validation (окно 11 блоков).
- [ ] Запрет timestamp больше чем на 2 часа в будущем.
- [ ] Детерминированная валидация: балансы выводятся из генезиса, `self.balances`
      только кэш; несоответствие = откат.
- [ ] Детерминированный генезис: `genesis.json` как входные данные,
      `EXPECTED_GENESIS_HASH` в `consensus.rs` как якорь.

**Эмиссия:**
- [ ] Награда за блок + tail emission (см. `ARCHITECT3.md` §7.1).
- [ ] `block_reward_at_height(h, total_supply)` с tail emission формулой.
- [ ] Убрать искусственные лимиты майнинга (1000 итераций / 5 сек).
- [ ] Параметры эмиссии (`max_supply_pre_tail`, `tail_rate`, `halving_interval`)
      фиксируются в генезисе до mainnet freeze.

**Лимиты и DoS-защита:**
- [ ] Лимиты размера: `MAX_MESSAGE_SIZE`, `MAX_BLOCK_SIZE`, `MAX_TX_SIZE`.
- [ ] Length-prefixed framing с проверкой ДО аллокации (закрыть OOM-вектор
      `vec![0; length]` в `src/main.rs:961`).
- [ ] Rate limiting на P2P: лимит сообщений/сек от одного пира.

**Модуляризация (на монолите):**
- [ ] Разбить на модули: `blockchain/`, `consensus/`, `network/`, `mempool/`,
      `storage/`, `api/`, `cli/`, `gui/`.
- [ ] Типизированные ошибки (`error.rs`).
- [ ] `RwLock` вместо `Mutex<Blockchain>` (где нужно чтение).

**Configuration management:**
- [ ] Единый `Config` struct с валидацией (замена `config.json` + `config.toml` +
      секции `[wallet]` в `Cargo.toml`).
- [ ] Секреты только в keystore (AES-256-GCM + PBKDF2 ≥210k iters), не в config.
- [ ] Пароль в `config.json:4` — удалить.

**Mempool (basic):**
- [ ] `mempool.insert`: проверка подписи, dup, nonce, chain_id, balance.
- [ ] `MAX_PENDING_TXS` лимит.
- [ ] `mempool.broadcast` через gossip (если есть сеть).

**Graceful shutdown:**
- [ ] `Drop` для storage/wallet/network.
- [ ] Сигналы SIGTERM/SIGINT → корректный `save_state`.
- [ ] Удалить «ручное удаление LOCK» (`main.rs:188`).

**Тесты и CI:**
- [ ] Юнит-тесты (property-based: proptest) для правил консенсуса.
- [ ] Интеграционные тесты: `two_clients`, `reorg`, `double_spend`, `pow`,
      `emission`, `time`, `network`.
- [ ] CI: GitHub Actions, 3 платформы (Linux/macOS/Windows), `clippy -D warnings`.
- [ ] Устранение 20 warning'ов и мёртвого кода (`mining_thread`, `config.toml`,
      секция `[wallet]` в `Cargo.toml`).

**Security & Documentation:**
- [ ] **License: MIT/Apache-2.0.** LICENSE файл в корне. Первый commit.
- [ ] **Reproducible builds:** CI gate с cosign/sigstore signatures, SLSA
      provenance.
- [ ] **Threat model (STRIDE):** `docs/security/THREAT_MODEL.md` с 25 векторами
      атак (см. `ARCHITECT3.md` §6).
- [ ] **TLA+ спецификация:** `docs/spec/consensus.tla` (skeleton) — safety
      properties (no double-spend, no inflation), liveness (no deadlock).
- [ ] **Bug bounty program:** Immunefi integration с day-1.
- [ ] Логирование через `tracing` crate (не `println!`), structured logging.

### Definition of Done

- [ ] Все 13 критических проблем из `ARCHITECT2.md` §1.1 закрыты.
- [ ] Все 22 инварианта из `ARCHITECT3.md` §5 enforce.
- [ ] Threat model (STRIDE) документ написан и ревьюнут.
- [ ] TLA+ skeleton спецификация консенсуса написана (properties формализованы).
- [ ] Reproducible builds в CI.
- [ ] License файл в корне.
- [ ] Все тесты проходят: `cargo test` green.
- [ ] `clippy -D warnings` green на 3 платформах.
- [ ] Bug bounty program активна на Immunefi.
- [ ] ADR-0001: "Why secp256k1 instead of Ed25519" написан.
- [ ] ADR-0002: "Why tail emission instead of halving + max_supply" написан.
- [ ] ADR-0003: "Why hybrid PoW→PoS" написан.
- [ ] `Changelog.md` обновлён: `1.0.0 — sanitized prototype`.

### Риски

- **Risk: secp256k1 миграция ломает существующие адреса.** Mitigation: Stage 0
  работает на testnet; mainnet ещё не запущен. После mainnet freeze изменения
  адресов — только через hard fork + activation height.
- **Risk: tail emission формула требует точной арифметики.** Mitigation:
  property-based tests на overflow, edge cases (total_supply = 0, max u64).
- **Risk: TLA+ спецификация находит bugs в правилах консенсуса.** Mitigation:
  это желаемый outcome — Stage 0 именно для этого.

### ADRs required

- ADR-0001: secp256k1 вместо Ed25519
- ADR-0002: Tail emission вместо halving + max_supply
- ADR-0003: Hybrid PoW→PoS как стратегия
- ADR-0004: License choice (MIT/Apache-2.0)
- ADR-0005: `tracing` вместо `println!`

---

## Этап 1 — Криптоядро + Verkle Trie (strangler: strangecoin-core крейт)

### Цели

Перенести чистое ядро (`serialize`, `consensus`, `state`, `economics`,
`governance`) в отдельный крейт `strangecoin-core`. Ввести Verkle Trie для
stateless validation. Ввести `consensus_version` + activation height
механизм (для будущих changes). Ввести events bus.

### Зависимости

- Stage 0 завершён (Definition of Done достигнут).

### Задачи

**Strangler: создание `strangecoin-core`:**
- [ ] Создать пустой `crates/strangecoin-core/`.
- [ ] Перенести `serialize.rs` (канонические байты, txid, hash).
- [ ] Перенести `consensus.rs` (PoW rules, time, emission constants, chain_id).
- [ ] Перенести `state.rs` (apply_block/unapply_block).
- [ ] Перенести `economics/` (emission.rs, fee_market.rs placeholders).
- [ ] Перенести `governance/` (scip.rs, consensus_version, activation_height).
- [ ] Все они — чистые функции (0 I/O), что упрощает перенос.
- [ ] `src/main.rs` обновляется, чтобы использовать `strangecoin-core` как
      зависимость.
- [ ] Тесты переносятся в `crates/strangecoin-core/tests/`.

**State root (Verkle Trie):**
- [ ] Реализовать Verkle Trie (или взять готовый crate, например
      `verkle-trie`).
- [ ] `state.root_after(block) == block.state_root` (инвариант #19).
- [ ] `StateWitness` для stateless validation (light-клиенты верифицируют блоки
      без полного state).
- [ ] `state.apply_block` обновляет Verkle root.
- [ ] `block.state_root` поле в заголовке блока.

**Merkle root транзакций:**
- [ ] Merkle root транзакций в каждом блоке (для SPV).
- [ ] `merkle_root(transactions) -> [u8;32]`.

**Headers-first sync:**
- [ ] `GET_HEADERS(from_height) → HEADERS(Vec<BlockHeader>)`.
- [ ] `GET_BLOCKS(Vec<BlockHash>) → BLOCKS(Vec<Block>)`.
- [ ] Выбор вершины по cumulative work.

**Mempool RBF:**
- [ ] `feerate = fee / tx_weight` (пока fee=0 — по размеру).
- [ ] `find_replaceable`, `Replaced(Vec<TxId>)` для анонса `TxRejected`.

**Events bus:**
- [ ] `EventBus` (crossbeam channel, multi-subscriber broadcast).
- [ ] События: `BlockApplied`, `BlockReorged`, `TxAccepted`, `TxRejected`,
      `MiningStarted/Finished`, `PeerScoreChanged`, `StatePersisted`.
- [ ] Подписчики: GUI (перерисовка), метрики, тесты.
- [ ] Никаких `std::sync::mpsc` для broadcast.

**Tie-breaking rule:**
- [ ] `select_best(chains)`: больше work → раньше timestamp → меньше hash.
- [ ] Детерминировано, нет гонок.

**Async runtime (tokio):**
- [ ] Ввести tokio (заменяет threads + mpsc).
- [ ] Это подготовка к Stage 2 (gossip, Noise, Erlay).

**Network ID:**
- [ ] `network_id` в генезисе и в `HELLO`: mainnet=1, testnet=2, regtest=3.
- [ ] Пиры с чужим `network_id` отбрасываются.

**Address bech32:**
- [ ] `address_from_public_key(pubkey) -> Address` (bech32 с контрольной суммой).
- [ ] HRP: `sc1...` (mainnet), `tsc1...` (testnet), `rsc1...` (regtest).

**Decomposition blockchain:**
- [ ] `chain_selector.rs` (tip selection, fork choice, reorg).
- [ ] `block_executor.rs` (validate + apply).
- [ ] `state_cache.rs` (balances/nonces cache).
- [ ] `blockchain_facade.rs` (public API).
- [ ] `consensus_manager.rs` (consensus_version, activation height).

**Sync engine:**
- [ ] `SyncEngine` владеет ссылками на `BlockchainFacade` и `NetworkService`.
- [ ] `NetworkService` не вызывает `blockchain` напрямую — только кладёт входящие
      блоки в `SyncEngine.inbox` (mpsc).
- [ ] `SyncEngine` — единственный, кто делает `validate → apply → announce`.

### Definition of Done

- [ ] `crates/strangecoin-core/` создан, все чистые функции перенесены.
- [ ] Verkle Trie реализован, `state.root_after(block) == block.state_root`.
- [ ] Headers-first sync работает (test: new node sync за разумное время).
- [ ] Events bus работает (test: 3 subscribers получают события).
- [ ] Tie-breaking rule детерминирован (property-based test).
- [ ] tokio введён, все подсистемы используют async.
- [ ] Network ID в HELLO, piры с чужим network_id отбрасываются.
- [ ] bech32 addresses работают (test: round-trip encode/decode).
- [ ] Blockchain декомпозирован на 5 компонентов (4 + consensus_manager).
- [ ] Sync engine разрывает цикл network↔blockchain.
- [ ] `consensus_version` + activation height mechanism работает.
- [ ] All Stage 0 invariants still enforced (no regressions).
- [ ] ADR-0006: "Why Verkle Trie instead of SMT" написан.
- [ ] ADR-0007: "Why tokio on Stage 1" написан.
- [ ] ADR-0008: "Why RocksDB (planning) instead of redb" написан.
- [ ] `Changelog.md` обновлён: `1.1.0 — core extracted + Verkle + headers-first`.

### Риски

- **Risk: Verkle Trie implementation сложна.** Mitigation: взять готовый crate
  если есть, иначе реализовать минимальную версию с property-based tests.
- **Risk: tokio миграция ломает существующие threads.** Mitigation: постепенный
  перенос, `tokio::spawn` для новых подсистем, старые threads работают до Stage 2.
- **Risk: Strangler pattern может «застрять» — перенос идёт медленно.** Mitigation:
  чёткие DoD критерии, каждый PR закрывает одну подсистему.

### ADRs required

- ADR-0006: Verkle Trie vs SMT
- ADR-0007: Tokio на Stage 1
- ADR-0008: RocksDB план (для Stage 3)
- ADR-0009: Events bus design (crossbeam vs flume vs tokio::broadcast)
- ADR-0010: Sync engine architecture

---

## Этап 1.5 — WASM смарт-контракты (strangecoin-vm-wasm крейт)

### Цели

Реализовать WASM (wasmi) VM в отдельном крейте `strangecoin-vm-wasm`. Host
functions, gas metering, account model, precompiles, events/logs. Реализовать
`VmExecutor` trait в `core` (без имплементации).

### Зависимости

- Stage 1 завершён (Verkle Trie, state root, headers-first).

### Задачи

**VM trait:**
- [ ] `VmExecutor` trait в `crates/strangecoin-core/src/vm/traits.rs`.
- [ ] `ExecutionContext`, `ExecutionResult`, `VmError` типы.
- [ ] `core` зависит от trait, не от рантайма.

**WASM runtime (wasmi):**
- [ ] Создать `crates/strangecoin-vm-wasm/`.
- [ ] wasmi integration (no_std, детерминированный).
- [ ] `execute(ctx: ExecutionContext) -> ExecutionResult`.

**Account model:**
- [ ] EOA + контрактные аккаунты (`code_hash`, `storage_root`, `nonce`, `balance`).
- [ ] Storage в Verkle Trie (общий state trie).

**Tx types:**
- [ ] `transfer` / `create` / `call`.
- [ ] Payload вызова входит в подписываемые данные.

**Gas metering:**
- [ ] Instruction counting + host-call weights.
- [ ] `gas_left()` host function.
- [ ] Block gas limit: 1M gas (фиксированный, до EIP-1559 на Stage 5).

**Host functions:**
- [ ] `secp256k1_verify`, `bls_verify` (для future PoS).
- [ ] `blake3`, `keccak`, `sha256`.
- [ ] `storage_read`, `storage_write`.
- [ ] `call`, `delegatecall`, `staticcall`.
- [ ] `block_context` (height, timestamp, coinbase).
- [ ] `gas_left`.
- [ ] `emit_log`.

**Precompiles:**
- [ ] `ecrecover` (secp256k1 recovery).
- [ ] `sha256`, `keccak`, `blake3`.
- [ ] `ed25519_verify` (для legacy).
- [ ] `bls_aggregate` (для future PoS).
- [ ] `modexp`.

**ABI:**
- [ ] **Простой JSON ABI** (как в Ethereum) для MVP.
- [ ] Канонический encode/decode аргументов.
- [ ] WIT migration placeholder (через `format_version`).

**Reentrancy guard:**
- [ ] `reentrant: bool` в `ExecutionContext`.
- [ ] `nonReentrant` modifier pattern.

**Limits:**
- [ ] `max_code_size` (например, 24KB).
- [ ] `max_storage_per_contract` (например, 1MB).
- [ ] `max_call_depth` (например, 1024).
- [ ] `max_stack_size`.
- [ ] `max_memory_size`.

**Events/logs:**
- [ ] `Log { address, topics, data }` в receipt.
- [ ] `Receipt { gas_used, logs, status }`.
- [ ] Receipts неизменны (инвариант #18).

**State in Verkle Trie:**
- [ ] Contract storage в общем state trie.
- [ ] `state.apply_call` обновляет storage trie.
- [ ] `state.root_after(block) == block.state_root` (инвариант #19).

**Fuzzing harness (Stage 6 подготовка):**
- [ ] `cargo-fuzz` targets для host functions.
- [ ] Differential testing: `wasmi` vs `wasmtime` (эталонный интерпретатор).

### Definition of Done

- [ ] `crates/strangecoin-vm-wasm/` создан, wasmi интегрирован.
- [ ] Все host functions реализованы и протестированы.
- [ ] Gas metering работает (test: `gas_left()` корректно уменьшается).
- [ ] Tx types `transfer/create/call` работают.
- [ ] Precompiles реализованы и протестированы.
- [ ] JSON ABI работает (test: round-trip encode/decode).
- [ ] Reentrancy guard работает (test: malicious contract не может reenter).
- [ ] Limits enforced (test: превышение → `OutOfGas`/`OutOfMemory`).
- [ ] Events/logs работают (test: emitted log in receipt).
- [ ] Contract storage в Verkle Trie (test: `state.root_after(block) == block.state_root`).
- [ ] Differential testing проходит (wasmi vs wasmtime, same result).
- [ ] All Stage 0+1 invariants still enforced (no regressions).
- [ ] ADR-0011: "Why WASM (wasmi) instead of RISC-V" написан.
- [ ] ADR-0012: "Why JSON ABI for MVP instead of WIT" написан.
- [ ] `Changelog.md` обновлён: `1.5.0 — WASM smart contracts (basic)`.

### Риски

- **Risk: wasmi gas metering неточен.** Mitigation: differential testing с
  wasmtime; property-based tests на edge cases.
- **Risk: Host functions leak недетерминизма.** Mitigation: fuzzing; review
  каждой host function на детерминизм.
- **Risk: Reentrancy attacks.** Mitigation: `reentrant` flag; pattern enforcement.

### ADRs required

- ADR-0011: WASM (wasmi) vs RISC-V (ckb-vm)
- ADR-0012: JSON ABI for MVP vs WIT
- ADR-0013: Gas metering strategy (instruction counting + host-call weights)
- ADR-0014: Precompiles list

---

## Этап 2 — Сетевая зрелость (gossip, Noise, Erlay, tokio)

### Цели

Реализовать production-grade P2P: gossip, compact blocks (BIP 152), Erlay,
Noise Protocol Framework для transport encryption, peer discovery через seed
nodes + addr gossip, peer scoring. Перенести сеть в `crates/strangecoin-net/`.

### Зависимости

- Stage 1 завершён (headers-first, events bus, tokio).
- Stage 1.5 завершён (или параллельно — VM не влияет на сеть).

### Задачи

**Gossip protocol:**
- [ ] `announce_tx` / `announce_block` через inventory (как Bitcoin `inv`).
- [ ] `GET_DATA(hash) → TX | BLOCK`.
- [ ] broadcast новым пирам.

**Compact blocks (BIP 152):**
- [ ] Short transaction IDs (siphash-based).
- [ ] `CompactBlock` message.
- [ ] Fill-up: reconstruction из mempool.

**Erlay:**
- [ ] Reconciliation-based relay (mincut protocol).
- [ ] Эффективный relay на 10k+ узлов.

**Peer discovery:**
- [ ] Hard-coded seed IPs (не только DNS).
- [ ] DNSSEC validation.
- [ ] `ADDR` gossip (peers обмениваются списками адресов).
- [ ] Bootstrap from multiple sources.

**P2P handshake:**
- [ ] `HELLO { version, network_id, capabilities, height }`.
- [ ] Capabilities negotiation (what features this peer supports).
- [ ] Reject peers with `version < MIN_PROTO_VERSION`.

**Peer scoring (расширение Stage 1):**
- [ ] `peer_store.rs`, `peer_manager.rs`, `connection_pool.rs`.
- [ ] Scoring factors: latency, invalid blocks/txs sent, protocol violations,
      uptime.
- [ ] Ban threshold настраивается, бан временный (expire) + persistent (в
      `network.json`).

**Noise Protocol Framework:**
- [ ] Handshake XX pattern.
- [ ] Ephemeral keys (forward secrecy).
- [ ] Transport encryption (ChaCha20-Poly1305).
- [ ] Auth peers через `node_id` (Ed25519 для internal auth).

**Rate limiting:**
- [ ] Tokens per second per peer.
- [ ] Бан при превышении.

**Deprecated messages cleanup:**
- [ ] Удалить `GET_BLOCKCHAIN`, `BLOCKCHAIN` (legacy v0.8.6).

**Shadow network (Stage 6 подготовка):**
- [ ] Tool для replay mainnet-блоков на staging.
- [ ] Тестирование апгрейдов перед deployment.

### Definition of Done

- [ ] Gossip работает (test: txs распространяются за ≤1 сек на 100 узлах).
- [ ] Compact blocks работают (test: bandwidth reduction ≥50% vs naive).
- [ ] Erlay работает (test: peer reconciliation успешна).
- [ ] Peer discovery: new node находит ≥8 peers за ≤30 сек.
- [ ] Handshake работает (test: incompatible peers отвергаются).
- [ ] Peer scoring: malicious peers banned после N violations.
- [ ] Noise encryption работает (test: MITM обнаружен).
- [ ] Rate limiting работает (test: spammer banned).
- [ ] Legacy messages удалены (no backward compatibility with v0.8.6 network).
- [ ] `crates/strangecoin-net/` создан, сеть перенесена.
- [ ] ADR-0015: "Why Noise Protocol Framework" написан.
- [ ] ADR-0016: "Why Erlay" написан.
- [ ] `Changelog.md` обновлён: `2.0.0 — production-grade P2P`.

### Риски

- **Risk: Noise implementation сложна.** Mitigation: взять готовый crate
  (`snow`); thorough fuzzing.
- **Risk: Erlay reconciliation может приводить к потере txs.** Mitigation:
  fallback на full relay если reconciliation fails; monitoring.
- **Risk: Peer scoring может ban legitimate peers.** Mitigation: tunable
  thresholds; monitoring of false positive rate.

### ADRs required

- ADR-0015: Noise Protocol Framework
- ADR-0016: Erlay integration
- ADR-0017: Peer scoring algorithm
- ADR-0018: Rate limiting strategy

---

## Этап 3 — Хранение и масштабируемость (RocksDB, stateless)

### Цели

Мигрировать с `rusty_leveldb` на **RocksDB** (battle-tested, как в Bitcoin Core
и reth). Ввести stateless validation witnesses (Verkle proofs). Ввести pruning
исторического state (EIP-4444 analog). Перенести storage в
`crates/strangecoin-storage/`.

### Зависимости

- Stage 2 завершён (production-grade P2P).
- Stage 1.5 завершён (state root, contract storage).

### Задачи

**LevelDB → RocksDB миграция:**
- [ ] Создать `crates/strangecoin-storage/`.
- [ ] RocksDB integration (через `rocksdb` crate).
- [ ] Column families: `chain`, `state`, `transactions`, `nonces`, `meta`,
      `peers`, `mempool_cache`.
- [ ] `migrations.rs`: LevelDB → RocksDB миграция (отдельная миграция, не
      ручное копирование).
- [ ] `schema.rs`: `KeyPrefix` enum, versioning.
- [ ] `current_schema_version` в `m/schema_version`.

**Stateless validation:**
- [ ] `StateWitness` для Verkle proofs.
- [ ] Light-клиенты верифицируют блоки без полного state.
- [ ] `verify_block(block, witness) -> bool` в `consensus`.

**State pruning / expiry:**
- [ ] Удаление исторического state старше N блоков (EIP-4444 analog).
- [ ] State revival через Verkle witnesses.
- [ ] `state_expiry_check` в `state.apply_block`.
- [ ] Опционально: state rent (через activation height, если решено через SCIP).

**Snapshot sync:**
- [ ] Быстрая синхронизация новых узлов без скачивания всей цепочки.
- [ ] Snapshot file format.
- [ ] Snapshot generation every N blocks.

**Indices:**
- [ ] Индекс `address → UTXO/balance`.
- [ ] Индекс `txid → block_height`.
- [ ] Индекс `contract_address → storage_root`.

**Performance:**
- [ ] Пакетная валидация (batch validation).
- [ ] Кэши состояния (LRU).
- [ ] Crash-recovery без ручного удаления LOCK.

### Definition of Done

- [ ] RocksDB миграция завершена, LevelDB удалён.
- [ ] Stateless validation работает (test: light client верифицирует block без
      полного state).
- [ ] State pruning работает (test: disk usage уменьшается после N блоков).
- [ ] Snapshot sync работает (test: new node синхронизируется за разумное время
      без full chain download).
- [ ] Indices работают (test: `address → balance` lookup O(1)).
- [ ] Performance: batch validation ≥10x быстрее sequential.
- [ ] `crates/strangecoin-storage/` создан, storage перенесён.
- [ ] All Stage 0-2 invariants still enforced.
- [ ] ADR-0019: "Why RocksDB instead of redb" написан.
- [ ] ADR-0020: "State pruning strategy" написан.
- [ ] `Changelog.md` обновлён: `3.0.0 — RocksDB + stateless + pruning`.

### Риски

- **Risk: RocksDB requires CGO on some platforms.** Mitigation: pre-built
  bindings; Docker for CI.
- **Risk: Stateless validation witnesses large.** Mitigation: Verkle proofs
  instead of SMT; witness compression.
- **Risk: State pruning breaks light clients.** Mitigation: archival nodes
  keep full state; `node_mode=archival`.

### ADRs required

- ADR-0019: RocksDB vs redb
- ADR-0020: State pruning strategy (EIP-4444 analog)
- ADR-0021: Snapshot sync format

---

## Этап 4 — Dev-experience (SDK, devnet, indexer, JSON-RPC eth_*)

### Цели

Создать экосистему инструментов для разработчиков: `cargo-strangecoin` SDK
(foundry-аналог), local devnet (anvil-аналог), indexer (Subgraph/sqd-аналог),
web block explorer, testnet faucet, JSON-RPC eth_* compatibility для
MetaMask/WalletConnect.

### Зависимости

- Stage 1.5 завершён (WASM VM).
- Stage 2 завершён (network).
- Stage 3 завершён или параллельно (storage для indexer).

### Задачи

**JSON-RPC API (eth_* compatibility):**
- [ ] `eth_sendTransaction`, `eth_getBalance`, `eth_call`, `eth_getLogs`.
- [ ] `eth_blockNumber`, `eth_getBlockByNumber`, `eth_getTransactionByHash`.
- [ ] `eth_subscribe` (WebSocket subscriptions).
- [ ] Strangecoin-specific: `sc_getTip`, `sc_getValidatorSet`, `sc_getMempool`.
- [ ] CLI: `--balance`, `--send --sign`, `--mine-once`, `--change-password`.
- [ ] `crates/strangecoin-api/`.

**HD Wallet (BIP-39/44):**
- [ ] Mnemonic generation (BIP-39).
- [ ] HD derivation (BIP-44, path `m/44'/coin_type'/account'/change/address_index`).
- [ ] `coin_type` для Strangecoin (зарегистрировать в SLIP-44).
- [ ] Hardware wallet support (Ledger через Ledger Connect).

**SDK `cargo-strangecoin`:**
- [ ] `cargo strangecoin new <contract_name>` — scaffold contract (Rust +
      wasmi target).
- [ ] `cargo strangecoin test` — run tests (anvil-аналог devnet).
- [ ] `cargo strangecoin deploy <network>` — deploy contract.
- [ ] `cargo strangecoin verify <address>` — verify deployed contract source.
- [ ] `cargo strangecoin fuzz <target>` — run cargo-fuzz on contract.
- [ ] `crates/strangecoin-sdk/`.

**Local devnet (anvil-аналог):**
- [ ] Мгновенный старт: `sc devnet start`.
- [ ] Pre-funded аккаунты (10 accounts with 1000 SC each).
- [ ] Time-travel: `sc devnet mine 100` (майн 100 блоков мгновенно).
- [ ] Fork mainnet: `sc devnet fork mainnet` (replicate mainnet state).
- [ ] `tools/devnet/`.

**Block explorer (web-based):**
- [ ] Web UI для просмотра блоков/транзакций/адресов/контрактов.
- [ ] Search by address/txid/block height.
- [ ] Event logs viewer.
- [ ] `tools/explorer/`.

**Testnet faucet:**
- [ ] Web UI для запроса testnet tokens.
- [ ] Rate limiting (1 request per IP per day).
- [ ] Captcha.
- [ ] `tools/faucet/`.

**Indexer (Subgraph/sqd-аналог):**
- [ ] GraphQL API для дApps.
- [ ] Indexing of contract events.
- [ ] Custom subgraphs (declarative mapping).
- [ ] `crates/strangecoin-indexer/`.

**IDE support:**
- [ ] Rust analyzer для контрактов (Rust + WASM target).
- [ ] Formatter.
- [ ] Linter (clippy rules для contract patterns).

**Metrics (Prometheus):**
- [ ] `prometheus` exporter для ноды.
- [ ] Метрики: peers, mempool size, block time, sync status, validator
      performance (PoS-фаза).
- [ ] OpenTelemetry tracing.

**Fuzzing harness (Stage 6 подготовка):**
- [ ] `cargo-fuzz` targets для VM, host functions, serialization.
- [ ] Differential testing infrastructure.

### Definition of Done

- [ ] JSON-RPC eth_* работает с MetaMask (test: send tx from MetaMask).
- [ ] HD Wallet работает (test: BIP-39 mnemonic → derived addresses).
- [ ] SDK работает (test: `cargo strangecoin new` → deploy → call).
- [ ] Devnet работает (test: `sc devnet start` → mine 100 blocks in <1 sec).
- [ ] Explorer работает (test: browse block #1, see txs).
- [ ] Faucet работает (test: request testnet tokens).
- [ ] Indexer работает (test: GraphQL query returns contract events).
- [ ] Metrics exposed (test: `curl /metrics` returns Prometheus format).
- [ ] `crates/strangecoin-api/`, `strangecoin-sdk/`, `strangecoin-indexer/` созданы.
- [ ] `tools/devnet/`, `tools/explorer/`, `tools/faucet/` созданы.
- [ ] ADR-0022: "Why JSON-RPC eth_* compatibility" написан.
- [ ] ADR-0023: "SDK design (foundry-аналог)" написан.
- [ ] `Changelog.md` обновлён: `4.0.0 — dev-experience ecosystem`.

### Риски

- **Risk: eth_* compatibility может требовать EVM-specific behavior.**
  Mitigation: map Strangecoin concepts to EVM concepts (e.g., Strangecoin
  account nonce → eth nonce); document differences.
- **Risk: SDK complexity.** Mitigation: start with minimal commands; add
  features iteratively.
- **Risk: Indexer performance.** Mitigation: column families for event index;
  pagination; caching.

### ADRs required

- ADR-0022: JSON-RPC eth_* compatibility
- ADR-0023: SDK design (foundry-аналог)
- ADR-0024: Devnet architecture (anvil-аналог)
- ADR-0025: Indexer design (Subgraph/sqd-аналог)

---

## Этап 5 — Экономика (tail emission, EIP-1559, MEV, AA)

### Цели

Активировать tail emission (после достижения `max_supply_pre_tail`). Ввести
EIP-1559 dynamic base fee. Реализовать MEV mitigation (threshold encryption
mempool). Реализовать Account Abstraction (ERC-4337 analog). Подготовить
staking infrastructure (placeholder для Stage 7).

### Зависимости

- Stage 4 завершён (dev-experience для тестирования экономики).
- Stage 3 завершён (storage для staking state).

### Задачи

**Tail emission activation:**
- [ ] `block_reward_at_height(h, total_supply) = max(base_reward, tail_rate * total_supply)`.
- [ ] Активация на высоте `TAIL_EMISSION_ACTIVATION_HEIGHT` (когда
      `total_supply >= max_supply_pre_tail`).
- [ ] `tail_rate = 0.6%/год` (Monero value).
- [ ] SCIP с on-chain signaling.

**EIP-1559 dynamic base fee:**
- [ ] `base_fee` корректируется каждый блок в зависимости от `gas_used / block_gas_limit`.
- [ ] Часть комиссии сжигается (`fee_burned`), часть идёт майнеру (`tip`).
- [ ] Инвариант #20: `fee_burned + fee_to_miner = total_fees`.
- [ ] `min_relay_fee` deprecated (заменяется на `base_fee`).
- [ ] Multidimensional fees (EIP-7706 analog) — research, не реализация.

**MEV mitigation (threshold encryption mempool):**
- [ ] `EncryptedMempool` реализация (placeholder с Stage 1, реализация Stage 5).
- [ ] Txs зашифрованы до включения в блок.
- [ ] Расшифровываются только после finality (PoS) или после N подтверждений (PoW).
- [ ] Threshold key generation через distributed key generation (DKG) среди
      validators/miners.
- [ ] Майнер/validator не видит содержимое txs до включения → frontrunning
      невозможен.

**Account Abstraction (ERC-4337 analog):**
- [ ] `UserOperation` mempool (отдельный от обычного `Transaction`).
- [ ] `Bundler` — отдельная роль (или node), собирает UserOps в одну tx.
- [ ] `Paymaster` — контракт, который платит gas.
- [ ] `EntryPoint` — precompile, через который проходят все UserOps.
- [ ] Social recovery через multisig.
- [ ] Session keys.
- [ ] Gas в любом токене (через paymaster).

**Staking infrastructure (placeholder для Stage 7):**
- [ ] `ValidatorSet` trait в `core`.
- [ ] `staking.rs` skeleton: validator deposits, slashing conditions (не
      активны, но типы готовы).
- [ ] `ValidatorDeposit` tx type.
- [ ] Validator registration через on-chain tx (но без actual staking rewards).

**SCIP process (on-chain signaling):**
- [ ] `SCIP` struct (proposal format).
- [ ] On-chain signaling: validators/miners сигналят поддержку в блоках.
- [ ] Activation threshold: ≥75% support в течение N блоков (например, 2016).
- [ ] Activation height: SCIP активируется на определённой высоте.

### Definition of Done

- [ ] Tail emission работает (test: `block_reward_at_height` correct after
      `max_supply_pre_tail`).
- [ ] EIP-1559 работает (test: `base_fee` adjusts based on demand; fee
      invariant holds).
- [ ] Threshold encryption mempool работает (test: txs not visible to miner
      until inclusion).
- [ ] Account Abstraction работает (test: paymaster pays gas; social
      recovery succeeds).
- [ ] Staking infrastructure skeleton в `core` (types defined, not active).
- [ ] SCIP process работает (test: on-chain signaling, activation threshold).
- [ ] All Stage 0-4 invariants still enforced.
- [ ] ADR-0026: "Why tail emission (Monero-style)" написан.
- [ ] ADR-0027: "Why EIP-1559 on Stage 5" написан.
- [ ] ADR-0028: "Why threshold encryption for MEV" написан.
- [ ] ADR-0029: "Why ERC-4337 analog for AA" написан.
- [ ] `Changelog.md` обновлён: `5.0.0 — economy (tail, EIP-1559, MEV, AA)`.

### Риски

- **Risk: Threshold encryption DKG сложна.** Mitigation: start with trusted
  key holder; transition to DKG later.
- **Risk: EIP-1559 в маленькой цепи нестабилен.** Mitigation: adjustable
  parameters; monitoring; fallback to fixed fee.
- **Risk: AA attack surface (paymaster, bundler).** Mitigation: thorough
  security audit; rate limiting; whitelisting.
- **Risk: Tail emission может не понравиться hodlers.** Mitigation: education;
  clear explanation of security budget trade-off.

### ADRs required

- ADR-0026: Tail emission (Monero-style)
- ADR-0027: EIP-1559 on Stage 5
- ADR-0028: Threshold encryption mempool
- ADR-0029: ERC-4337 analog for AA
- ADR-0030: Staking infrastructure (placeholder)
- ADR-0031: SCIP process design

---

## Этап 6 — Безопасность и формальная верификация

### Цели

Завершить formal verification: TLA+ model-checking консенсуса, K-framework
спека WASM subset, formal verification критических контрактов. Внешний
аудит кода. Bug bounty program (Immunefi) активна с day-1.

### Зависимости

- Stage 5 завершён (экономика стабильна).
- Сквозные треки Security & Documentation завершены (см. ниже).

### Задачи

**TLA+ model-checking:**
- [ ] `docs/spec/consensus.tla` полностью формализован (rules, invariants).
- [ ] Safety properties: no double-spend, no inflation, no deadlock.
- [ ] Liveness properties: progress, termination.
- [ ] Model-checking passes (TLC).
- [ ] Edge cases: reorg глубины N, time-warp, selfish mining.

**K-framework WASM spec:**
- [ ] `docs/spec/vm.k` — формальная семантика WASM subset.
- [ ] Match wasmi behavior (differential testing).
- [ ] Property-based tests derived from K spec.

**Formal verification critical contracts:**
- [ ] Identify critical contracts (e.g., EntryPoint for AA, Paymaster).
- [ ] K-framework / Boogie / Coq harness.
- [ ] Prove safety properties (e.g., no funds drained).

**External audit:**
- [ ] Audit firm (e.g., Trail of Bits, Certora, Quantstamp).
- [ ] Audit report published.
- [ ] All findings fixed.

**Bug bounty (Immunefi):**
- [ ] Bug bounty program live (с Stage 0, но здесь — расширение scope).
- [ ] Reward tiers defined.
- [ ] Response SLA <48 hours.

**Fuzzing:**
- [ ] `cargo-fuzz` targets для всех critical paths (consensus, serialization,
  VM, host functions).
- [ ] Continuous fuzzing (ClusterFuzzLite или подобное).
- [ ] Differential testing (wasmi vs wasmtime).

**Reproducible builds:**
- [ ] CI gate (с Stage 0, но здесь — full verification).
- [ ] SLSA Level 3.
- [ ] cosign signatures на всех release artifacts.

**Shadow-fork testing:**
- [ ] Tool для replay mainnet-блоков на staging.
- [ ] Тестирование апгрейдов перед deployment.
- [ ] Integration tests с mainnet data.

### Definition of Done

- [ ] TLA+ model-checking passes (TLC).
- [ ] K-framework spec passes differential testing.
- [ ] Formal verification of critical contracts complete.
- [ ] External audit report published, findings fixed.
- [ ] Bug bounty program live with expanded scope.
- [ ] Fuzzing: 0 crashes in last 30 days.
- [ ] Reproducible builds: SLSA Level 3 verified.
- [ ] Shadow-fork: all mainnet blocks replay successfully.
- [ ] All Stage 0-5 invariants still enforced.
- [ ] ADR-0032: "Why TLA+ for consensus" написан.
- [ ] ADR-0033: "Why K-framework for WASM" написан.
- [ ] `Changelog.md` обновлён: `6.0.0 — formal verification + audit`.

### Риски

- **Risk: TLA+ находит bugs в правилах консенсуса.** Mitigation: это желаемый
  outcome; фиксим bugs перед mainnet freeze (если ещё не запущен) или через
  hard fork + activation height.
- **Risk: External audit expensive.** Mitigation: phased audit (consensus first,
  VM second, etc.); bug bounty program как дополнение.
- **Risk: Formal verification может занять месяцы.** Mitigation: prioritize
  critical contracts; use automated tools (Saw, Coq automation).

### ADRs required

- ADR-0032: TLA+ for consensus
- ADR-0033: K-framework for WASM
- ADR-0034: Audit firm selection
- ADR-0035: Bug bounty scope

---

## Этап 7 — PoS миграция

### Цели

Переключить консенсус с PoW на PoS через activation height. Реализовать
Casper FFG finality overlay с BLS12-381 подписями, slashing conditions,
validator set rotation.

### Зависимости

- Stage 5 завершён (staking infrastructure placeholder).
- Stage 6 завершён (formal verification, audit).

### Задачи

**Validator set:**
- [ ] Validator registration через on-chain tx (реальное staking, не placeholder).
- [ ] Min stake: 1000 SC (фиксируется в genesis, может изменяться через SCIP).
- [ ] Max stake per validator: 5% от total stake (защита от централизации).
- [ ] Delegation: holders могут делегировать validator'у (с комиссией).
- [ ] Validator set rotation каждый epoch (1 день = 144 блоков).

**BLS12-381 подписи:**
- [ ] `bls_sign(block_hash, privkey) -> Attestation`.
- [ ] `bls_aggregate(attestations) -> AggregateSignature`.
- [ ] `bls_verify(aggregate, message, pubkeys) -> bool`.
- [ ] Aggregator role (ротация по epoch).

**Casper FFG finality:**
- [ ] Validators attest proposed block.
- [ ] ≥2/3 stake voted → block finalized.
- [ ] Justification → finalization (2-epoch rule).
- [ ] Finality gadget overlay на PoW (на transition height).

**Slashing conditions:**
- [ ] Double-vote: loss of 100% stake.
- [ ] Surround-vote: loss of 50% stake.
- [ ] Downtime (больше N epochs offline): loss of 0.1% stake.
- [ ] Whistleblower rewards (50% of slashed amount).

**Inclusion lists:**
- [ ] Validators могут принуждать следующего proposer включить определенные txs.
- [ ] Защита от censoring.

**Activation height:**
- [ ] `POS_ACTIVATION_HEIGHT` в `consensus_manager`.
- [ ] До этой высоты: PoW logic active.
- [ ] После: PoS logic active (Casper FFG).
- [ ] Backward compatibility: PoW-блоки валидны до `POS_ACTIVATION_HEIGHT`.
- [ ] Hard fork: узлы с старой версией отвергают PoS-блоки (по дизайну).

**Light client update:**
- [ ] Light clients синхронизируются через validator set updates.
- [ ] Sync committee (subset of validators, rotates periodically).
- [ ] Light client proof format.

**Staking rewards:**
- [ ] Validator rewards от inflation (tail emission) + transaction fees.
- [ ] Reward distribution: validator + delegators (pro-rata).
- [ ] Compounding (optional auto-restake).

### Definition of Done

- [ ] PoS migration прошла на testnet (test: PoW → PoS transition successful).
- [ ] BLS12-381 подписи работают (test: aggregate verification correct).
- [ ] Casper FFG finality работает (test: block finalized after 2 epochs).
- [ ] Slashing работает (test: double-vote → stake slashed).
- [ ] Inclusion lists работают (test: censored tx forced inclusion).
- [ ] Light client update работает (test: light client syncs after PoS).
- [ ] Staking rewards работают (test: validator + delegators paid correctly).
- [ ] All Stage 0-6 invariants still enforced.
- [ ] ADR-0036: "Why Casper FFG" написан.
- [ ] ADR-0037: "Why BLS12-381" написан.
- [ ] ADR-0038: "Validator set rotation strategy" написан.
- [ ] `Changelog.md` обновлён: `7.0.0 — PoS migration complete`.

### Риски

- **Risk: PoS migration — крупнейший hard fork в истории цепи.** Mitigation:
  thorough testnet testing; shadow-fork replay; gradual rollout (e.g., 50%
  validators first, then 100%).
- **Risk: Validator collusion.** Mitigation: max stake per validator; random
  selection; whistleblower rewards; slashing.
- **Risk: Long-range attack.** Mitigation: weak subjectivity checkpoint
  (sync с trusted source раз в N блоков).
- **Risk: Nothing-at-stake.** Mitigation: mandatory slashing; whistleblower
  rewards.

### ADRs required

- ADR-0036: Casper FFG
- ADR-0037: BLS12-381
- ADR-0038: Validator set rotation
- ADR-0039: Slashing conditions
- ADR-0040: Light client protocol
- ADR-0041: Staking rewards distribution

---

## Сквозные треки

Эти треки ведутся параллельно со всеми этапами, не имеют своего «Stage».

### Security track

- Stage 0+: Threat model (STRIDE) maintained.
- Stage 0+: TLA+ specification.
- Stage 0+: Reproducible builds (cosign/sigstore).
- Stage 0+: Bug bounty (Immunefi).
- Stage 1+: Fuzzing (`cargo-fuzz`).
- Stage 6: External audit.
- Stage 6: Formal verification of critical contracts.

### Governance track

- Stage 0: License (MIT/Apache-2.0).
- Stage 1: SCIP process skeleton.
- Stage 1: `consensus_version` + activation height mechanism.
- Stage 5: On-chain signaling.
- Stage 7: PoS governance (validators vote on SCIPs).

### Documentation track

- Stage 0: README, CONTRIBUTING, LICENSE.
- Stage 0: `docs/security/THREAT_MODEL.md`.
- Stage 0: `docs/spec/consensus.tla` (skeleton).
- Stage 1+: ADRs for each architectural decision.
- Stage 1+: `docs/spec/wire_format.md`.
- Stage 4: API documentation (OpenAPI для JSON-RPC).
- Stage 4: SDK documentation.
- Stage 7: Operator documentation (node setup, monitoring).

### Operations track

- Stage 0: CI/CD (GitHub Actions, 3 platforms).
- Stage 0: Logging (`tracing` crate).
- Stage 4: Metrics (Prometheus/OpenTelemetry).
- Stage 4: Docker images.
- Stage 4: systemd unit files.
- Stage 7: Validator operations guide.

---

## Definition of Done — общие критерии

Каждый Stage считается завершённым только по достижении **всех** критериев:

1. **Все задачи Stage выполнены** (checkboxes в задачах).
2. **Все invariants из `ARCHITECT3.md` §5 enforced** (включая новые, добавленные
   на этом Stage).
3. **Все тесты проходят:** `cargo test` green, `cargo clippy -D warnings` green.
4. **Property-based тесты** на правила консенсуса (proptest).
5. **Integration тесты:** `two_clients`, `reorg`, `double_spend`, `pow`,
   `emission`, `time`, `network` — все green.
6. **Fuzzing:** 0 crashes в последних 7 днях.
7. **ADRs:** все architectural decisions зафиксированы в `docs/ADR/`.
8. **Documentation обновлена:** Changelog, README, relevant docs.
9. **Security:** threat model актуализирован (если Stage добавляет новые
   attack vectors).
10. **Backward compatibility:** mainnet-блоки валидны (если mainnet запущен);
    несовместимые изменения — через hard fork + activation height.
11. **Reproducible builds:** CI gate проходит.
12. **Code review:** все PRs ревьюнуты минимум одним ревьюером.
13. **Bug bounty:** новые attack vectors добавлены в scope (Immunefi).

---

## Критические развилки (выборы, требующие ADR)

Эти решения должны быть приняты в указанные моменты. Без ADR разработка не
продолжается.

### Stage 0

- **ADR-0001:** secp256k1 вместо Ed25519 (принято: см. `ARCHITECT3.md` §3.8).
- **ADR-0002:** Tail emission вместо halving + max_supply (принято: см.
  `ARCHITECT3.md` §7.1).
- **ADR-0003:** Hybrid PoW→PoS как стратегия (принято: см. `ARCHITECT3.md` §3.2).
- **ADR-0004:** License choice (рекомендация: MIT/Apache-2.0).
- **ADR-0005:** `tracing` вместо `println!`.

### Stage 1

- **ADR-0006:** Verkle Trie vs SMT (рекомендация: Verkle).
- **ADR-0007:** Tokio на Stage 1 (принято).
- **ADR-0008:** RocksDB план для Stage 3 (принято).
- **ADR-0009:** Events bus design.
- **ADR-0010:** Sync engine architecture.

### Stage 1.5

- **ADR-0011:** WASM (wasmi) vs RISC-V (ckb-vm) (принято: WASM/wasmi).
- **ADR-0012:** JSON ABI for MVP vs WIT (принято: JSON для MVP).
- **ADR-0013:** Gas metering strategy.
- **ADR-0014:** Precompiles list.

### Stage 2

- **ADR-0015:** Noise Protocol Framework.
- **ADR-0016:** Erlay integration.
- **ADR-0017:** Peer scoring algorithm.
- **ADR-0018:** Rate limiting strategy.

### Stage 3

- **ADR-0019:** RocksDB vs redb (принято: RocksDB).
- **ADR-0020:** State pruning strategy.
- **ADR-0021:** Snapshot sync format.

### Stage 4

- **ADR-0022:** JSON-RPC eth_* compatibility.
- **ADR-0023:** SDK design.
- **ADR-0024:** Devnet architecture.
- **ADR-0025:** Indexer design.

### Stage 5

- **ADR-0026:** Tail emission (Monero-style).
- **ADR-0027:** EIP-1559 on Stage 5.
- **ADR-0028:** Threshold encryption mempool.
- **ADR-0029:** ERC-4337 analog for AA.
- **ADR-0030:** Staking infrastructure (placeholder).
- **ADR-0031:** SCIP process design.

### Stage 6

- **ADR-0032:** TLA+ for consensus.
- **ADR-0033:** K-framework for WASM.
- **ADR-0034:** Audit firm selection.
- **ADR-0035:** Bug bounty scope.

### Stage 7

- **ADR-0036:** Casper FFG.
- **ADR-0037:** BLS12-381.
- **ADR-0038:** Validator set rotation.
- **ADR-0039:** Slashing conditions.
- **ADR-0040:** Light client protocol.
- **ADR-0041:** Staking rewards distribution.

---

## Что делать в первую очередь

Если разработчик только начинает путь по `ROADMAP2.md`, шаги:

1. **Прочитать `ARCHITECT3.md`** — понять целевую архитектуру.
2. **Прочитать `ANALYSIS.md`** — понять критику предыдущих документов и
   обоснование решений.
3. **Завершить Stage 0 задачи** (sanitization на монолите):
   - LICENSE файл (MIT/Apache-2.0) — первый commit.
   - Заменить Ed25519 на secp256k1.
   - Подписи транзакций (chain_id, nonce).
   - Каноническая сериализация.
   - PoW валидация + tail emission формула.
   - OOM fix + лимиты.
   - Threat model + TLA+ skeleton.
   - Тесты + CI.
4. **Не переходить к Stage 1** до завершения Stage 0 Definition of Done.

**Принцип:** без подписей транзакций, валидации difficulty и детерминированного
генезиса всё остальное бессмысленно, т.к. монета сейчас фактически крадётся
простым редактированием поля `sender`.

---

## Связь с другими документами

- `ARCHITECT.md` — предыдущая версия архитектуры (PoW-centric).
- `ARCHITECT2.md` — критика зрелости ARCHITECT.md.
- `ARCHITECT3.md` — целевая архитектура (этот документ ведёт к ней).
- `ROADMAP.md` — предыдущая дорожная карта.
- `ANALYSIS.md` — критический разбор всех четырёх исходных документов.
- `prompt.md` — 117 промптов для Stage 0 (требует обновления с учётом
  `ARCHITECT3.md` и `ROADMAP2.md`).
- `docs/ADR/` — Architecture Decision Records (детали решений, см. §«Критические
  развилки»).
- `docs/spec/consensus.tla` — TLA+ спецификация консенсуса.
- `docs/security/THREAT_MODEL.md` — детальный threat model.
