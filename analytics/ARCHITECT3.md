# ARCHITECT3.md — Целевая архитектура Strangecoin

**Версия документа:** 3.0 (альтернатива ARCHITECT.md / ARCHITECT2.md)
**Дата:** 2026-09-09
**Цель:** зафиксировать целевую архитектуру, к которой ведёт `ROADMAP2.md`.

## Введение

Документ описывает структурную цель эволюции Strangecoin от прототипа `0.8.6`
(монолит `src/main.rs` 1756 строк + `src/wallet.rs`, Ed25519, LevelDB, сырой TCP,
PoW без подписей) до production-grade криптовалюты. В отличие от `ARCHITECT.md`
(который фиксирует PoW как долгосрочный консенсус) и `ARCHITECT2.md` (который
фиксирует Ed25519 и halving), `ARCHITECT3.md` принимает следующие стратегические
решения (см. обоснование в `ANALYSIS.md`):

- **Консенсус:** Hybrid PoW→PoS (PoW bootstrapping первые ~2 года, затем PoS
  миграция через activation height, native token staking, BLS12-381).
- **Эмиссия:** Tail emission (Monero-style, 0.6%/год после первичного cap).
- **Криптография:** secp256k1 для EOA-подписей (совместимость с MetaMask/Ledger),
  BLS12-381 для консенсусных подписей (PoS-фаза, агрегация).
- **Смарт-контракты:** WASM (wasmi) в отдельном крейте, ABI через простой JSON с
  заделом на WIT migration.
- **State tree:** Verkle Trie для stateless validation witnesses.
- **Storage:** RocksDB с column families (как в reth).
- **Стратегия миграции:** Strangler pattern (greenfield `strangecoin-core` крейт,
  текущий код — референс).
- **Async runtime:** Tokio с Stage 1 (вместе с headers-first sync).
- **Безопасность:** Threat model (STRIDE) + TLA+ спецификация с Stage 0.
- **Экономика:** MEV mitigation (threshold encryption mempool) как архитектурный
  компонент с Stage 1.
- **Governance:** SCIP процесс + activation height с Stage 1.
- **Dev-experience:** SDK, devnet, indexer, explorer, faucet, JSON-RPC eth_*
  compatibility как first-class архитектурные компоненты.
- **License:** MIT/Apache-2.0 с Stage 0.

Уровень абстракции: блоки/подсистемы и их связи, без деталей каждой функции.
Соглашения по границам данных описаны на уровне «что проходит через границу».

---

## 1. Принципы архитектуры

Архитектура построена на восьми принципах. Первые пять наследованы из
`ARCHITECT.md` §1 (они корректны), последние три — новые, закрывающие пробелы
из `ANALYSIS.md`.

1. **Три консенсусные аксиомы** — база всего остального:
   - вся валидность выводится из цепочки блоков детерминированно, `balances` —
     только кэш;
   - криптографическая целостность: подпись secp256k1 на каждую транзакцию EOA,
     BLS12-381 для консенсусных подписей (PoS-фаза), хэш-цепочка блоков;
   - никакого недетерминизма: нет wall-clock времени в консенсусе, нет float,
     только `blockhash`/время блоков как источник энтропии.

2. **Ядро без побочных эффектов** — `consensus`, `state`, `serialize`, `economics`,
   `governance` — чистые функции без I/O; внешние эффекты (сеть, БД, GUI) живут в
   тонких периферийных обёртках. Это делает ядро тестируемым, аудируемым и
   переиспользуемым в альтернативных нодах, SDK, индексаторах.

3. **Один источник истины** — блокчейн. Mempool, кэш балансов, индексы —
   производные, пересчитываемые структуры. Любое расхождение = reorg или
   пересчёт.

4. **Жёсткие границы модулей** — единственная точка входа в каждую подсистему;
   приватные поля недоступны извне даже через сокеты/DB/GUI. В Rust выражается
   через `pub(crate)` и модульную инкапсуляцию.

5. **Всё, что может быть атакой, — валидируется на входе**: лимиты размеров,
   подписи, время, формат, генезис, replay protection, rate limits.

6. **Эволюционируемость через activation height** (новый принцип). Любые изменения
   правил консенсуса — через SCIP (Strangecoin Improvement Proposal) + activation
   height. Это включает PoW→PoS миграцию, изменения эмиссии, добавление новых
   типов транзакций. Без этого принципа хардфорки рискованны.

7. **Безопасность как first-class citizen** (новый принцип). Threat model (STRIDE),
   TLA+ спецификация, fuzzing harness, differential testing, formal verification
   для критических контрактов — все заложены в архитектуру с Stage 0/1, не
   откладываются на потом. Bug bounty program с day-1.

8. **Strangler pattern для миграции** (новый принцип). Новый код пишется в
   отдельных крейтах (`strangecoin-core`, `strangecoin-net`, ...), а старый
   монолит постепенно «удушается»: каждая подсистема заменяется на новую, пока
   `main.rs` не превращается в тонкий launcher. Это устраняет risk «большого
   переписывания» и позволяет постепенно вводить новые абстракции.

---

## 2. Контекстная диаграмма (границы системы)

```
                          ┌────────────────────────────────────────────────┐
                          │                  Внешний мир                   │
   другие ноды (P2P) ───▶│  TCP/Noise  JSON-RPC  CLI  GUI  Web-Explorer   │◀─ оператор/пользователь/dApp
                          │              eth_*  WalletConnect  GraphQL      │
                          └────────────────────────┬───────────────────────┘
                                                   │
                          ┌────────────────────────▼───────────────────────┐
                          │              Strangecoin Node                   │
                          │  ────────────────────────────────────────────  │
                          │  network/p2p ──▶ core ──▶ storage                │
                          │  api          ──▶ wallet (крипто)                │
                          │  vm (wasmi)   ──▶ state                          │
                          │  indexer      ──▶ GraphQL                       │
                          │  sdk/devnet   ──▶ (отдельные инструменты)       │
                          └────────────────────────┬───────────────────────┘
                                                   │
                          ┌────────────────────────▼───────────────────────┐
                          │   RocksDB + файлы                                │
                          │   chain, state (Verkle), transactions,           │
                          │   keystore, config, network.json, metrics         │
                          └─────────────────────────────────────────────────┘
```

Внешние интерфейсы:
- **P2P-протокол** (бинарный, Noise-зашифрованный с Stage 2).
- **JSON-RPC** (eth_*-совместимый для MetaMask/WalletConnect с Stage 4).
- **CLI** (оператор: `--send`, `--mine`, `--balance`, `--change-password`).
- **GUI** (egui для desktop клиента, опциональный через feature flag).
- **Web-Explorer** (отдельный сервис, читает из indexer).
- **GraphQL API** (отдельный сервис для дApps, через indexer).
- **Файловая система** (RocksDB, keystore, config, metrics).
- **SDK** (`cargo-strangecoin` — foundry-аналог для контрактов).
- **Devnet** (anvil-аналог: мгновенный старт, pre-funded аккаунты, time-travel).

---

## 3. Функциональные подсистемы

### 3.1 `serialize` — канонические байты

Один источник кодировки для всего консенсуса. Никакой `serde_json` в хэш-путях.
- детерминированная бинарная кодировка `Transaction`, `Block`, `Header`,
  `UserOperation`, `Attestation`;
- `txid(Transaction) -> [u8;32]`, `block_hash_header(Block) -> [u8;32]`;
- версия формата (`format_version`) — страховка от ломающих изменений;
- golden-векторы в тестах защищают от регрессий кодировки;
- `serde` используется только для config/api/explorer (не для consensus).

### 3.2 `consensus` — правила валидности (чистое)

Все проверки «какой блок/цепочка валидны», без состояния и I/O.

**PoW-фаза (Stage 0–6):**
- PoW: `target` в заголовке, `hash <= target`; ретаргетинг по скользящему окну;
- время: `median_time_past`, запрет timestamp из будущего;
- эмиссия: `block_reward_at_height` с tail emission (см. §8);
- константы консенсуса — в одном месте;
- replay protection: `chain_id` в каждой транзакции;
- nonce: `account.nonce` строго инкрементируется.

**PoS-фаза (Stage 7+, через activation height):**
- Casper FFG finality overlay: validators lock native token, attestations BLS12-381;
- slashing conditions: double-vote, surround-vote, downtime;
- validator set rotation: каждый epoch (например, 1 день);
- `consensus_version` в заголовке блока для activation height;
- backward compatibility: PoW-блоки валидны до высоты `POS_ACTIVATION_HEIGHT`.

### 3.3 `state` — детерминированное исполнение

Превращает цепочку блоков в состояние (балансы, контрактный storage).
- чистые `apply_block`/`unapply_block`;
- результат — Verkle root (решение зафиксировано с Stage 1, не «нерешённое»);
- `balances` — кэш, пересчитываемый из цепочки;
- `StateWitness` для stateless validation (light-клиенты верифицируют блоки без
  полного state);
- `state_expiry_check` для pruning старого state (см. §8.3);
- `ExecutionContext` с dependency tracking для будущего Block-STM (Stage 3+).

### 3.4 `blockchain` — оркестратор консенсуса

Декомпозиция на 4 компонента (наследовано из `ARCHITECT.md` §11.1):

| Компонент | Ответственность | Владеет состоянием |
|-----------|----------------|-------------------|
| `chain_selector.rs` | Tip selection по cumulative work, fork choice, reorg logic (unapply/apply), tie-breaking | `tip_height`, `tip_hash`, `total_work` |
| `block_executor.rs` | Валидация блока (`consensus.validate`), исполнение (`state.apply_block`), coinbase reward | нет (чистый) |
| `state_cache.rs` | `balances` кэш + invalidation, `nonces` кэш, пересчёт из цепочки при расхождении | `HashMap<Addr, Balance>`, `HashMap<Addr, Nonce>` |
| `blockchain_facade.rs` | Публичный API: `add_block`, `apply_tx`, `get_balance`, `get_tip`; делегирует выше | `RwLock<BlockchainInner>` (обёртка) |

Новый компонент `consensus_manager.rs` (для PoS-фазы):
- отслеживает `consensus_version` и activation height;
- переключает логику валидации (PoW → PoS) на `POS_ACTIVATION_HEIGHT`;
- управляет validator set rotation;
- expose: `current_consensus_rules(height) -> ConsensusRules`.

Правила:
- `chain_selector` не знает про state/transactions — только заголовки и work.
- `block_executor` не знает про выбор вершины — только «примени этот блок на это
  состояние».
- `state_cache` — единственное место, где читаются балансы.
- `facade` — единственная точка входа для network/mempool/api/gui.
- `consensus_manager` — единственный, кто решает, какие правила применяются на
  данной высоте.

### 3.5 `mempool` — пул неподтверждённых транзакций

Отдельная периферийная подсистема (не часть `blockchain`).
- валидация подписи/dup/nonce/chain_id на `insert` (делегирует blockchain-методу);
- эвристика приоритизации по feerate (EIP-1559 на Stage 5, до этого — fixed gas);
- лимиты размера/количества (`MAX_PENDING_TXS`);
- RBF (Replace-By-Fee): `find_replaceable`, `Replaced(Vec<TxId>)` для анонса;
- `EncryptedMempool` (placeholder с Stage 1, реализация Stage 5): threshold
  encryption для MEV mitigation (см. §8.4);
- `UserOperationMempool` (Stage 5): для Account Abstraction (ERC-4337 analog);
- `MempoolTrait` для тестирования и альтернативных реализаций.

Мемпул не хранит данные в RocksDB и не знает про сеть — только in-memory
и взаимодействие с blockchain по публичным методам.

### 3.6 `network/p2p` — общение узлов

Бинарное кадрирование поверх TCP, **Noise-шифрование с Stage 2**.

- `protocol.rs`: кодировка сообщений (`HELLO`, `GET_HEADERS`, `HEADERS`,
  `GET_BLOCKS`, `BLOCK`, `TX`, `IDENTIFY`, `ADDR`, `PING`, `USEROP`), версионирование,
  лимиты размеров, length-prefixed framing с проверкой ДО аллокации;
- `p2p.rs`: сокеты (tokio с Stage 1), таймауты, лимит соединений, баны, discovery;
- `sync.rs`: headers-first (с Stage 1), передача недостающих блоков, cumulative
  work, snapshot sync (Stage 3);
- `gossip.rs`: announce + инвентарь (как Bitcoin `inv`), compact blocks (BIP 152),
  Erlay (Stage 2);
- `noise.rs`: Noise Protocol Framework (Stage 2) — handshake XX pattern, ephemeral
  keys, forward secrecy;
- `rate_limiter.rs`: токены на сообщения/сек от одного пира;
- `peer_store.rs` / `peer_manager.rs` / `connection_pool.rs` — разделены как в
  `ARCHITECT.md` §11.6.

Network messages — backwards compatible через `format_version`. Deprecated
сообщения (`GET_BLOCKCHAIN`, `BLOCKCHAIN` из v0.8.6) удаляются на Stage 2.

### 3.7 `storage` — персистентность

Единственный модуль, знающий про **RocksDB** (с Stage 3; до этого — LevelDB с
планом миграции).

- column families: `chain`, `state`, `transactions`, `nonces`, `meta`, `peers`,
  `mempool_cache`;
- снапшоты/чекпоинты состояния (snapshot sync для быстрой синхронизации новых
  узлов), прунинг старых блоков, crash-recovery (без ручного удаления LOCK);
- индексы адрес→баланс/UTXO (Stage 3);
- `schema.rs`: `KeyPrefix` enum (`c/`, `s/`, `b/`, `n/`, `t/`, `d/`, `m/`),
  `encode_key(prefix, ...)`, `decode_key`;
- `migrations.rs`: `Migration { from_version, to_version, fn apply(db) }`,
  `current_schema_version` хранится в `m/schema_version`;
- при старте: `while current < TARGET { apply_migration(current); current++ }`;
- LevelDB → RocksDB миграция = отдельная миграция, не ручное копирование.

### 3.8 `wallet` — криптографический кошелёк

Именные ключи и подпись. `sign_transaction` подписывает канонические байты.

**Криптография:**
- secp256k1 (ECDSA) для EOA-подписей (совместимость с MetaMask/Ledger/WalletConnect);
- BLS12-381 для консенсусных подписей (PoS-фаза, агрегация);
- Ed25519 — опционально для internal node-to-node auth (не для транзакций);
- keystore AES-256-GCM + PBKDF2 (≥ 210k итераций), уникальный nonce;
- HD (BIP-39/44) с Stage 4.

**Адресация:**
- bech32 с контрольной суммой (с Stage 1);
- `address_from_public_key(pubkey) -> Address` — единая точка;
- HRP (human-readable part) различает mainnet/testnet/regtest:
  `sc1...` (mainnet), `tsc1...` (testnet), `rsc1...` (regtest).

**Кошелёк не касается сети/БД**; его эксплуатируют GUI/CLI/API.

### 3.9 `vm` — смарт-контракты (Stage 1.5)

WASM (wasmi) в отдельном крейте `strangecoin-vm-wasm`. `core` зависит от
`VmExecutor` trait, не от конкретного рантайма.

```rust
pub trait VmExecutor {
    fn execute(&self, ctx: ExecutionContext) -> Result<ExecutionResult, VmError>;
    fn gas_cost(&self, opcode: Opcode) -> u64;
}
```

**Компоненты:**
- WASM runtime: wasmi (no_std, детерминированный, готовые фаззеры);
- Host functions: `secp256k1_verify`, `bls_verify`, `blake3`/`keccak`,
  `storage_read`/`write`, `call`/`delegatecall`/`staticcall`, `block_context`,
  `gas_left`, `emit_log`;
- Account model: EOA + контрактные (`code_hash`, `storage_root`, `nonce`, `balance`);
- Tx types: `transfer` / `create` / `call`; payload вызова входит в подписываемые
  данные;
- Gas metering: instruction counting + host-call weights (не таблица опкодов вручную);
- Block gas limit: фиксированный (например, 1M gas) с Stage 1.5;
- Reentrancy guard, лимиты на размер кода/хранилища/стек/память;
- Precompiles: `ecrecover`, `sha256`, `keccak`, `blake3`, `ed25519_verify`,
  `bls_aggregate`, `modexp`;
- ABI: **простой JSON ABI** (как в Ethereum) для MVP, WIT migration на Stage 4+
  когда tooling дозреет;
- Events/logs: `Log { address, topics, data }` в receipt;
- Reentrancy guard: `reentrant: bool` в `ExecutionContext`;
- Параллельное исполнение (Block-STM) — Stage 3+ после корректной
  последовательной валидации.

**Формальная верификация (Stage 6):**
- K-framework спецификация WASM subset;
- Differential testing: эталонный интерпретатор (wasmtime) vs wasmi;
- cargo-fuzz targets: фаззинг host functions, gas metering, storage trie operations.

### 3.10 `api` — JSON-RPC + CLI

Тонкий слой-фасад поверх `blockchain`/`wallet`/`vm`.

**JSON-RPC (Stage 4):**
- Bitcoin/Ethereum-подобные методы;
- **eth_*-совместимость** (Stage 4): `eth_sendTransaction`, `eth_getBalance`,
  `eth_call`, `eth_getLogs`, `eth_blockNumber`, `eth_subscribe` — для
  MetaMask/WalletConnect integration;
- Strangecoin-специфичные: `sc_getTip`, `sc_getValidatorSet`, `sc_getMempool`;
- subscriptions (WebSocket): `eth_subscribe`-подобные для real-time events.

**CLI:**
- `--balance`, `--send --sign`, `--mine-once`, `--change-password`;
- `sc block <height>`, `sc tx <txid>` — minimal block explorer;
- `sc faucet <address>` — testnet faucet (Stage 4+);
- `sc node start --headless`, `sc node status`, `sc node stop`.

CLI/API — единый интерфейс для всех трёх режимов работы узла (full/light/archival).

### 3.11 `gui` — клиент оператора (egui)

Потребитель `api`/`blockchain`/`wallet`. Показывает адрес/баланс/майнинг,
инициирует перевод (обязательно подписанный). Никогда не имеет доступа к
внутренностям консенсуса напрямую. Feature flag `default = ["gui"]`;
серверная сборка `--no-default-features` — демон без egui.

### 3.12 `config` — параметры и генезис

- `genesis.json`: входные данные для детерминированного генезиса (timestamp,
  initial_holder, block_reward, tail_emission_rate, max_supply_pre_tail,
  target_block_time, retarget_interval, format_version, network_id);
- `EXPECTED_GENESIS_HASH` — константа в `consensus.rs`; узел при старте сверяет
  локальный генезис по хэшу и отказывается работать при расхождении;
- `config.toml` (единый формат, замена `config.json`+`config.toml`+`[wallet]` в
  `Cargo.toml`): только НЕсекретные параметры (порт, ip, name, node_mode,
  network_id, data_dir, log_level);
- `network.json`: публичные адреса пиров (seed nodes);
- `Config` struct с валидацией (serde + custom validators), no `lazy_static`/`once_cell`
  для консенсусных констант (только `pub const` в `consensus.rs`).

Параметры `genesis.json` меняются ТОЛЬКО через активацию по высоте, никогда —
редактированием файла.

### 3.13 `economics` — экономическая модель (новая подсистема)

См. §8 для деталей. Здесь — компонентная декомпозиция:
- `emission.rs`: `block_reward_at_height(h) -> u64` с tail emission формулой;
- `fee_market.rs`: EIP-1559 base fee (Stage 5), fixed gas limit до этого;
- `mev_mitigation.rs`: threshold encryption mempool (Stage 5), placeholder с
  Stage 1;
- `staking.rs`: validator deposits, slashing conditions (Stage 7+);
- `account_abstraction.rs`: UserOperation mempool, Bundler, Paymaster (Stage 5).

Все компоненты — чистые функции в `core`, состояние (например, текущий base fee)
хранится в `state`.

### 3.14 `governance` — SCIP и активация по высоте (новая подсистема)

- `scip.rs`: `ConsensusVersion`, `ActivationHeight`, `SCIP` struct (proposal format);
- `voting.rs`: on-chain signaling (Stage 5+), off-chain discussion (GitHub);
- `fork_coordinator.rs`: rules for soft/hard forks, activation threshold;
- `upgrade_path.rs`: documented path for each `consensus_version` bump;

Любое изменение правил консенсуса — через SCIP + activation height. Это
включает: PoW→PoS миграцию (Stage 7), изменения эмиссии, новые типы транзакций,
новые precompiles, изменения gas cost.

### 3.15 `security` — threat model и verification (новая подсистема)

См. §6 для threat model. Здесь — компонентная декомпозиция:
- `threat_model.rs`: STRIDE analysis, mitigations map;
- `tla_spec/`: TLA+ specification of consensus rules;
- `audit_log.rs`: tamper-evident logging of security-relevant events;
- `bounty_program.rs`: integration with Immunefi (Stage 6);
- `reproducible_builds.rs`: cosign/sigstore integration in CI (Stage 0);
- `formal_verification/`: K-framework / Boogie harnesses for critical contracts
  (Stage 6).

---

## 4. Потоки данных (главные сценарии)

### 4.1 Перевод средств (пользователь → блокчейн)

```
GUI/CLI ──send(to, amount)──▶ api
   └─ wallet.sign_transaction(tx_bytes, secp256k1) ──▶ Transaction{ sig, nonce, chain_id }
api ──▶ blockchain.apply_transaction ──▶ [mempool.insert → валидация]
mempool ──▶ network.announce_tx ──▶ пиры (gossip)
майнер: blockchain.mine_block( мемпул + coinbase ) ──▶ Block
blockchain: consensus.validate → state.apply_block → storage.save → events.publish(BlockApplied)
events ──▶ [GUI redraw, metrics, JSON-RPC subscriptions, indexer]
```

### 4.2 Синхронизация с пиром

```
network: HELLO (version, network_id, capabilities) ──▶ handshake
network: GET_HEADERS(from_height) ──▶ пир отвечает HEADERS
sync_engine: для каждого заголовка: consensus.validate_header
sync_engine: GET_BLOCKS(hashes) ──▶ пир отвечает BLOCKS
blockchain: для каждого блока: consensus.validate → state.apply → storage.store
            выбор вершины по cumulative_work; при расхождении — реорг (unapply/apply)
mempool: отброс транзакций, попавших в чужие блоки (по txid)
```

### 4.3 Майнинг (без блокировки узла)

```
api/worker: читает tip + mempool → ищет nonce под target (чистая, без лока на blockchain)
найдено: consensus.validate(block) → state.apply_block → storage.commit (write-lock кратко)
новый блок от сети: прерывает поиск → пересчитывает tip/difficulty → продолжает
events.publish(BlockApplied)
```

### 4.4 PoS-аттестация (Stage 7+)

```
validator (если в active set):
  - читает proposed_block
  - проверяет: consensus.validate, state.apply (без коммита)
  - если ок: bls_sign(block_hash) → Attestation
  - транслирует Attestation через gossip
aggregator (ротация по epoch):
  - собирает Attestations за epoch
  - агрегирует BLS подписи
  - включает aggregate в следующий блок
consensus_manager: проверяет ≥2/3 stake voted → block finalized
```

### 4.5 Смарт-контракт вызов (Stage 1.5+)

```
dApp/CLI ──call(contract, method, args, gas_limit)──▶ api
   └─ wallet.sign_transaction(tx_bytes) ──▶ Transaction{ type: Call, ... }
api ──▶ mempool.insert
майнер включает tx в блок
blockchain.execute: vm.execute(ExecutionContext { code, args, gas })
  └─ host functions: storage_read/write, call/delegatecall, emit_log
  └─ result: ExecutionResult { gas_used, logs, return_data }
state.apply: update contract storage, code, nonce
events.publish(TxExecuted { receipt })
```

### 4.6 Старт узла

```
storage.load_state(chain, state_root, mempool) → blockchain.init
   → сверка кэша балансов с пересчётом из цепочки
   → genesis: если БД пуста — создать по genesis.json; иначе валидировать генезис
   → проверка EXPECTED_GENESIS_HASH
   → проверка consensus_version и activation heights
config.load → crypto (wallet) → network.start (HELLO handshake)
   → если node_mode=light: только заголовки + verkle witnesses
   → если node_mode=full: полная цепочка + state
   → если node_mode=archival: полная история + state + receipts
```

### 4.7 Graceful shutdown (новый сценарий)

```
SIGTERM/SIGINT → shutdown_signal
   ──▶ network.stop (закрыть соединения, дождаться in-flight)
   ──▶ mining_worker.stop (прервать поиск, сохранить состояние)
   ──▶ sync_engine.flush (сохранить pending blocks)
   ──▶ blockchain.flush (state_cache → storage)
   ──▶ storage.flush (RocksDB WAL → disk)
   ──▶ wallet.lock (keystore → disk)
   ──▶ process exit (без ручного удаления LOCK)
```

---

## 5. Ключевые инварианты (нельзя нарушать)

Расширенный список по сравнению с `ARCHITECT.md` §5. Инварианты 1-9 наследованы,
10-22 — новые.

| # | Инвариант | Где enforce |
|---|-----------|-------------|
| 1 | Вся валидность — из цепочки; `balances` — кэш | `state`, `blockchain` |
| 2 | Каждая транзакция подписана, `sender == pubkey` (secp256k1) | `consensus`, `mempool` |
| 3 | `hash <= target` для каждого блока при его difficulty | `consensus` |
| 4 | `apply_block`/`unapply_block` — обратные чистые функции | `state` |
| 5 | Хэш/подпись — на канонических байтах (`serialize`), не JSON | `serialize` |
| 6 | Блок не содержит непроверимых транзакций/наград сверх эмиссии | `blockchain` |
| 7 | Размеры сообщений/блоков всегда ограничены до аллокации | `network/p2p`, `blockchain` |
| 8 | Генезис детерминирован и совпадает у всех узлов | `config/genesis` |
| 9 | Секреты не пишутся недиск и не логируются | `config`, `wallet`, `gui` |
| **10** | **Replay protection:** каждая транзакция содержит `chain_id`; mainnet/testnet/regtest несовместимы | `consensus`, `mempool` |
| **11** | **Nonce:** `account.nonce` строго инкрементируется; tx с `nonce <= account.nonce` отвергается | `consensus`, `state` |
| **12** | **Transaction hash = commitment:** `txid` выводится из канонических байтов всей транзакции (включая подпись); уникальный идентификатор для mempool, rollback, tracking | `serialize` |
| **13** | **Mempool basic rules:** на `insert` проверяются подпись, dup, nonce, chain_id, balance | `mempool` |
| **14** | **Graceful shutdown:** `Drop` для storage/wallet/network; нет «ручного удаления LOCK» | `node`, `storage`, `wallet` |
| **15** | **Block gas limit:** каждый блок имеет `gas_used <= block_gas_limit` (Stage 1.5+) | `consensus`, `vm` |
| **16** | **P2P framing:** length-prefixed, проверка размера ДО аллокации (`vec![0; length]` с cap) | `network/protocol` |
| **17** | **Rate limiting:** лимит сообщений/сек от одного пира; нарушение → бан | `network/rate_limiter` |
| **18** | **Event log:** каждый tx имеет `Receipt { gas_used, logs, status }`; receipts неизменны | `state`, `storage` |
| **19** | **State root match:** `state.root_after(block) == block.state_root`; иначе reorg | `state`, `blockchain` |
| **20** | **Fee invariant:** `fee_burned + fee_to_miner = total_fees`; `gas_used <= block_gas_limit` (Stage 5+) | `economics`, `consensus` |
| **21** | **Consensus versioning:** `block.consensus_version <= current_version`; активация по высоте | `consensus_manager` |
| **22** | **Reproducible builds:** CI публикует SLSA provenance + cosign signature для каждого release | CI/CD |

---

## 6. Threat Model (STRIDE)

Анализ векторов атак с митигациями в архитектуре. **Обязателен с Stage 0** —
не откладывается на поздние этапы.

| # | Вектор атаки | Категория STRIDE | Митигация в архитектуре |
|---|-------------|------------------|------------------------|
| 1 | **Eclipse attack** — изоляция узла от сети | Spoofing/Information Disclosure | Peer discovery через seed nodes + addr gossip; min 8 outgoing + 8 incoming connections; peer diversity (разные подсети); `peer_manager` scoring |
| 2 | **Partition attack** — разделение сети | Denial of Service | Cross-checking chain tips с multiple peers; `sync_engine` запрашивает headers у ≥3 пиров; alert при расхождении |
| 3 | **Selfish mining** — майнер скрывает блоки | Information Disclosure/Elevation | `median_time_past` + cumulative work; мониторинг stale rate; alert при аномалиях; в PoS-фазе — slashing за witholding |
| 4 | **Time-warp attack** — манипуляция timestamp | Tampering | `median_time_past` с окном 11 блоков; запрет на timestamp больше чем на 2 часа в будущем; retargeting по реальному времени блоков |
| 5 | **51% attack** (PoW) — двойная трата | Elevation of Privilege | Finality gadget (PoS-фаза); checkpointing (weak subjectivity); monitoring of large miner concentration; alert при 51% threshold |
| 6 | **Long-range attack** (PoS) — атака с старыми ключами | Spoofing | Weak subjectivity checkpoint (синхронизация с trusted source раз в N блоков); validator set rotation каждый epoch; slashing для nothing-at-stake |
| 7 | **Nothing-at-stake** (PoS) — validators подписывают оба fork | Tampering | Slashing condition: double-vote → loss of stake; mandatory slashing enforcement в consensus |
| 8 | **MEV: frontrunning** — майнер/validator вставляет tx перед жертвой | Information Disclosure/Elevation | Threshold encryption mempool (Stage 5): tx зашифрованы до включения в блок; commitment scheme |
| 9 | **MEV: censoring** — майнер/validator не включает tx | Denial of Service | `mempool.broadcast` (gossip всем пирам); inclusion lists (PoS-фаза); monitoring of censoring rate |
| 10 | **MEV: sandwich attack** — buy→victim→sell | Information Disclosure/Elevation | Threshold encryption (Stage 5); batch auctions (Stage 5+); user-side slippage protection |
| 11 | **Replay attack** (cross-chain) — tx из testnet в mainnet | Spoofing | `chain_id` в каждой tx; mainnet=1, testnet=2, regtest=3; инвариант #10 |
| 12 | **Replay attack** (intra-chain) — повтор tx | Spoofing | `account.nonce` строго инкрементируется; инвариант #11 |
| 13 | **DoS: OOM** — `vec![0; length]` с огромным length | Denial of Service | Length-prefixed framing с проверкой ДО аллокации; `MAX_MESSAGE_SIZE`, `MAX_BLOCK_SIZE`, `MAX_TX_SIZE` |
| 14 | **DoS: spam txs** — миллионы мусорных txs | Denial of Service | `mempool` rate limit per sender; `min_relay_fee` (Stage 5); `mempool.eviction` по feerate |
| 15 | **DoS: invalid blocks** — пиров атакует invalid блоками | Denial of Service | `peer_manager` scoring; ban после N invalid blocks; `chain_selector` не применяет блоки до валидации |
| 16 | **Eclipse via DNS poisoning** — seed nodes отравлены | Spoofing | Hard-coded seed IPs (not just DNS); DNSSEC validation; rotation of seeds |
| 17 | **Sybil attack** — много фейковых пиров | Spoofing | `peer_manager` scoring; limit on connections per IP range; ban on protocol violations |
| 18 | **Reentrancy** (smart contracts) — контракт вызывает сам себя | Tampering | `reentrant: bool` в `ExecutionContext`; `nonReentrant` modifier pattern; `gas_limit` enforcement |
| 19 | **Integer overflow** (smart contracts) — переполнение в арифметике | Tampering | Checked arithmetic (Rust default); `SafeMath`-style precompiles; gas cost for arithmetic ops |
| 20 | **Storage exhaustion** — контракт заполняет весь state | Denial of Service | `max_storage_per_contract`; `state_rent` (Stage 3+, опционально); gas cost for storage writes |
| 21 | **Code injection** — malicious WASM | Tampering/Elevation | wasmi sandbox (no I/O by default); host functions whitelist; gas metering prevents infinite loops |
| 22 | **Compromised build** — зловредный binary | Tampering | Reproducible builds (Stage 0); cosign/sigstore signatures; SLSA provenance |
| 23 | **Key compromise** — украден privkey | Elevation of Privilege | Account Abstraction (Stage 5): social recovery, multisig; keystore AES-256-GCM + PBKDF2 (≥210k iters); hardware wallet support (Ledger) |
| 24 | **Network MITM** — перехват трафика | Spoofing/Information Disclosure | Noise Protocol Framework (Stage 2) — handshake XX, ephemeral keys, forward secrecy |
| 25 | **Validator collusion** (PoS) — validators сговариваются | Elevation of Privilege | Max stake per validator; random validator selection; slashing conditions; whistleblower rewards |

---

## 7. Экономическая модель

### 7.1 Tail emission

Вместо Bitcoin-модели (21M cap + halving), Strangecoin использует **tail emission**
(как Monero):

- **Initial phase (Stage 0–5):** halving каждые 4 года (примерно), пока
  `block_reward > tail_emission_rate * total_supply`.
- **Tail phase (после initial cap):** `block_reward = max(base_reward, tail_rate * total_supply)`.
- `tail_rate = 0.6%/год` (Monero value).
- Это обеспечивает:
  - постоянный security budget (майнерам всегда есть что майнить);
  - защиту от дефляционного spiral risk (hodling → низкая ликвидность);
  - отсутствие проблемы «через 30+ лет майнерам невыгодно» (Bitcoin risk).
- `max_supply_pre_tail` фиксируется в genesis (например, 21M), после чего
  включается tail emission. Это не «бесконечная инфляция» (как Dogecoin), а
  controlled asymptotic growth.

```rust
// economics/emission.rs
pub fn block_reward_at_height(height: u64, total_supply: u64) -> u64 {
    let base_reward = halving_schedule(height);
    let tail_reward = (total_supply * TAIL_RATE_NUMERATOR)
        / (TAIL_RATE_DENOMINATOR * BLOCKS_PER_YEAR);
    base_reward.max(tail_reward)
}
```

### 7.2 Fee market

- **Stage 1–4:** fixed gas limit (например, 1M gas/block), fixed `min_relay_fee`.
  Простая модель, легко понять и аудировать.
- **Stage 5+:** EIP-1559 dynamic base fee. `base_fee` корректируется каждый блок
  в зависимости от `gas_used / block_gas_limit`. Часть комиссии сжигается
  (`fee_burned`), часть идёт майнеру/validator (`tip`). Инвариант #20:
  `fee_burned + fee_to_miner = total_fees`.
- **Multidimensional fees (EIP-7706 analog):** research на Stage 6+, не для MVP.
  Calldata / blob / storage writes — отдельные маркеты.

### 7.3 State rent / expiry (опционально)

- **Statelessness + pruning** (обязательно, Stage 3): stateless validation через
  Verkle witnesses; pruning исторического state старше N блоков (EIP-4444 analog).
- **State rent** (опционально, Stage 3+): плата за хранение state. Ethereum
  отказался от state rent в пользу statelessness; Strangecoin может пойти тем же
  путём. Если state rent вводится — через activation height.

### 7.4 MEV mitigation

- **Threshold encryption mempool** (Stage 5): txs зашифрованы до включения в блок,
  расшифровываются только после finality. Майнер/validator не видит содержимое
  txs до включения → frontrunning невозможен.
- **PBS (Proposer-Builder Separation)** (research, Stage 6+): разделение ролей
  proposer (validator) и builder (блок-билдер). Builder конкурируют через
  аукцион за право собрать блок.
- **Batch auctions** (опционально, Stage 5+): все txs в блоке исполняются по
  одной цене, что устраняет sandwich attacks.
- **Inclusion lists** (PoS-фаза, Stage 7+): validators могут принуждать
  следующего proposer включить определенные txs (защита от censoring).

### 7.5 Staking (Stage 7+)

- **Native token staking:** не отдельный governance token. Strangecoin (SC) сам
  по себе является стейкинг-токеном.
- **Validator set rotation:** каждый epoch (например, 1 день = 144 блоков при
  10-минутных блоках в PoW-фазе, или 1 час в PoS-фазе).
- **Slashing conditions:**
  - double-vote: loss of 100% stake;
  - surround-vote: loss of 50% stake;
  - downtime (больше N epochs offline): loss of 0.1% stake.
- **Min stake:** 1000 SC (фиксируется в genesis, может изменяться через SCIP).
- **Delegation:** holders могут делегировать validator'у (с комиссией).
- **Max stake per validator:** 5% от total stake (защита от централизации).

### 7.6 Account Abstraction (Stage 5)

ERC-4337 analog. Без AA нет:
- спонсируемых транзакций (paymaster платит gas);
- социального восстановления;
- multisig кошельков без смарт-контрактов;
- session keys;
- gas в любом токене.

**Компоненты:**
- `UserOperation` mempool (отдельный от обычного `Transaction`);
- `Bundler` — отдельная роль (или node), собирает UserOps в одну tx;
- `Paymaster` — контракт, который платит gas;
- `EntryPoint` — precompile, через который проходят все UserOps.

---

## 8. Governance

### 8.1 SCIP процесс

Strangecoin Improvement Proposals — процесс для любых изменений правил консенсуса
или протокола.

**Стадии:**
1. **Idea** — GitHub discussion.
2. **Draft** — formal SCIP document, review by community.
3. **Review** — technical analysis, security audit if needed.
4. **On-chain signaling** (Stage 5+) — validators/miners сигналят поддержку в
   блоках.
5. **Activation threshold** — ≥75% support в течение N блоков (например, 2016).
6. **Activation height** — SCIP активируется на определённой высоте.
7. **Finalized** — после активации, SCIP не может быть отменён.

**SCIP format:**
```yaml
scip: <number>
title: <title>
status: Draft|Review|Active|Finalized
consensus_version: <version>
activation_height: <height or TBD>
author: <name>
discussions: <url>
created: <date>
```

### 8.2 Activation height

- `consensus_version` в заголовке блока;
- `activation_height` для каждого SCIP хранится в consensus_rules;
- На `POS_ACTIVATION_HEIGHT`: переключение с PoW на PoS логики валидации;
- Любые изменения параметров (emission, gas limit, precompiles) — через
  activation height;
- Backward compatibility: узлы с старой `consensus_version` принимают блоки до
  activation height, после — отвергают (hard fork).

### 8.3 Fork coordination

- Soft forks: новые правила ⊆ старые (старые узлы принимают новые блоки).
- Hard forks: новые правила ⊄ старые (старые узлы отвергают новые блоки).
- Strangecoin предпочитает **hard forks** (явные, планируемые), а не soft forks
  (неявные, рискованные).
- Каждый hard fork: SCIP + activation height + upgrade guide + ample warning
  (минимум 3 месяца между Finalized и Activation).

---

## 9. Модульная карта (workspace)

Целевая структура (Stage 3+, через strangler pattern):

```
strangecoin/
├── crates/
│   ├── strangecoin-core/      # consensus, state, serialize, economics, governance (0 I/O)
│   │   └── vm/traits.rs       # VmExecutor trait (без реализации)
│   ├── strangecoin-net/       # p2p, protocol, sync, gossip, noise
│   ├── strangecoin-storage/   # RocksDB, schema, migrations
│   ├── strangecoin-wallet/    # secp256k1, BLS, keystore, HD
│   ├── strangecoin-vm-wasm/   # wasmi runtime, host functions, gas, precompiles
│   ├── strangecoin-node/      # сборка узла: блокировки, потоки, события, runtime
│   ├── strangecoin-api/       # jsonrpc (eth_* compat), cli
│   ├── strangecoin-gui/       # egui-клиент (feature flag)
│   ├── strangecoin-indexer/   # GraphQL API, subgraph analog
│   └── strangecoin-sdk/       # cargo-strangecoin (new, test, deploy, verify)
├── tools/
│   ├── devnet/                # anvil-аналог: мгновенный старт, pre-funded
│   ├── explorer/               # web-based block explorer
│   ├── faucet/                 # testnet faucet
│   └── fuzz/                   # cargo-fuzz targets
├── docs/
│   ├── ADR/                    # Architecture Decision Records
│   ├── spec/                   # протокольная спецификация
│   │   ├── consensus.tla       # TLA+ спека консенсуса
│   │   ├── vm.k                # K-framework спека WASM subset
│   │   └── wire_format.md
│   ├── security/
│   │   ├── THREAT_MODEL.md     # STRIDE analysis (см. §6)
│   │   ├── AUDIT_REPORTS/
│   │   └── BOUNTY.md           # Immunefi integration
│   └── SCIP/                   # Strangecoin Improvement Proposals
├── examples/
│   ├── transfer.rs            # пример использования SDK
│   ├── contract.rs             # пример контракта на Rust
│   └── devnet.rs               # пример запуска devnet
├── Cargo.toml                  # workspace
├── LICENSE                     # MIT/Apache-2.0 (Stage 0, first commit)
├── README.md
├── CONTRIBUTING.md
└── ARCHITECT3.md               # этот документ
```

### 9.1 Зависимости между крейтами

```
strangecoin-core ← (depends on nothing external except crypto primitives)
       ▲
       │
strangecoin-net, strangecoin-storage, strangecoin-vm-wasm, strangecoin-wallet
       ▲
       │
strangecoin-node (depends on all above)
       ▲
       │
strangecoin-api, strangecoin-gui, strangecoin-indexer (depend on node)
       ▲
       │
strangecoin-sdk, tools/devnet, tools/explorer (depend on api)
```

Правила:
- `strangecoin-core` — 0 I/O, 0 external deps (только crypto primitives).
- `strangecoin-net` зависит от `core` (для типов), не от `node`.
- `strangecoin-storage` зависит от `core` (для типов), не от `node`.
- `strangecoin-vm-wasm` зависит от `core` (для trait), не от `node`.
- `strangecoin-wallet` зависит от `core` (для типов), не от `node`.
- `strangecoin-node` собирает всё вместе: потоки, блокировки, события, runtime.
- `strangecoin-api/gui/indexer` — потребители `node`, не имеют доступа к
  внутренностям.
- `strangecoin-sdk` — public API для разработчиков контрактов (высокий уровень).

---

## 10. Стратегия миграции (Strangler Pattern)

Strangler pattern: новый код пишется в новых крейтах, старый монолит постепенно
«удушается». Каждый перенос подсистемы — это отдельный PR с полным test coverage.

### 10.1 Stage 0: Фиксация монолита (sanitization)

- Монолит `src/main.rs` (1756 строк) + `src/wallet.rs` сохраняется как есть;
- Критические фиксы: подписи (secp256k1), каноническая сериализация, PoW
  валидация, детерминированный генезис, OOM fix, лимиты, тесты;
- Цель: монолит становится «минимально безопасным», готовым к постепенной
  декомпозиции.

### 10.2 Stage 1: Создание `strangecoin-core` крейта

- Создаётся пустой `crates/strangecoin-core/`;
- В него переносится: `serialize`, `consensus`, `state`, `economics`, `governance`;
- Все они — чистые функции (0 I/O), что упрощает перенос;
- `src/main.rs` обновляется, чтобы использовать `strangecoin-core` как зависимость;
- Тесты переносятся в `crates/strangecoin-core/tests/`;
- Цель: чистое ядро в отдельном крейте, монолит работает как обёртка.

### 10.3 Stage 1.5: Перенос VM в отдельный крейт

- Создаётся `crates/strangecoin-vm-wasm/`;
- Реализуется `VmExecutor` trait в `core`;
- `wasmi` runtime, host functions, gas metering;
- Монолит `main.rs` обновляется, чтобы использовать `vm` как зависимость.

### 10.4 Stage 2: Перенос сети в отдельный крейт

- Создаётся `crates/strangecoin-net/`;
- Переносятся: `protocol`, `p2p`, `sync`, `gossip`, `noise`;
- Вводится tokio (вместо threads);
- `main.rs` обновляется, чтобы использовать `net` как зависимость.

### 10.5 Stage 3: Перенос storage, wallet

- Создаются `crates/strangecoin-storage/` и `crates/strangecoin-wallet/`;
- LevelDB → RocksDB миграция (через `migrations.rs`);
- Ed25519 → secp256k1/BLS миграция (через новый `wallet` крейт);
- `main.rs` становится тонкой обёрткой над `node`.

### 10.6 Stage 4: Перенос api, gui, indexer

- Создаются `crates/strangecoin-api/`, `crates/strangecoin-gui/`,
  `crates/strangecoin-indexer/`;
- Feature flags для GUI;
- `main.rs` превращается в launcher: config → node → (опционально) gui.

### 10.7 Stage 5+: Экономика, governance, SDK, devnet

- `crates/strangecoin-economics/` (extracted из `core`);
- `crates/strangecoin-sdk/` (новый, high-level API для контракт-разработчиков);
- `tools/devnet/`, `tools/explorer/`, `tools/faucet/` — отдельные бинарники.

### 10.8 Критерии успешного strangler

- На каждом Stage: `cargo test` проходит (no regressions);
- На каждом Stage: `clippy -D warnings` (no warnings);
- На каждом Stage: документация обновляется;
- На каждом Stage: backward compatibility с предыдущим Stage (в рамках
  mainnet freeze — после Stage 1).

---

## 11. Событийная шина (`events`)

Замена «GUI сам читает Mutex» (v0.8.6, `main.rs:1454`) и разбросанных mpsc:

```rust
// events.rs
use std::sync::{Arc, Mutex};
use crossbeam_channel::Sender;

pub struct EventBus {
    subscribers: Mutex<Vec<Sender<NodeEvent>>>,
}
impl EventBus {
    pub fn subscribe(&self) -> Receiver<NodeEvent> { ... }
    pub fn publish(&self, event: NodeEvent) { ... }  // не блокируется на медленном подписчике
}
```

- **Никаких `std::sync::mpsc`** для broadcast — только multi-producer multi-consumer
  канал;
- Публикация неблокирующая: `try_send` + drop slow subscriber (или unbounded
  channel);
- События: `BlockApplied`, `BlockReorged { old_tip, new_tip }`, `TxAccepted`,
  `TxRejected`, `TxExecuted { receipt }`, `PeerScoreChanged`, `MiningStarted/Finished`,
  `ValidatorAttested` (Stage 7+), `StatePersisted`, `ConsensusUpgraded`;
- Подписчики: GUI (перерисовка), метрики (Prometheus), JSON-RPC `eth_subscribe`
  subscriptions, indexer, тесты (детерминированная проверка без sleep).

---

## 12. Типы узлов

Один бинарник, режим — в конфиге (`node_mode`), чтобы серверы и клиенты не
собирались по-разному:

- **Full node** — полная цепочка + state; опционально майнинг (PoW) или
  staking (PoS).
- **Light client (SPV)** — только заголовки + Verkle-пробы; баланс
  проверяется через proof, а не через копию state. Не участвует в майнинге/стейкинге.
- **Archival node** — полная история + state + receipts; для индексатора/эксплорера.
- **Validator node** (Stage 7+) — full node + active validator в PoS.

`network/sync.rs` параметризуется `node_mode`; `api` — единый интерфейс для всех
четырёх режимов.

---

## 13. Headless-режим и feature flags

- Cargo features: `default = ["gui"]`; серверная сборка
  `cargo build --no-default-features` → демон без egui;
- Сигналы SIGTERM/SIGINT → корректный `save_state` (см. §4.7 graceful shutdown);
- Node как процесс-демон: systemd unit, Docker image;
- Единственная точка входа — CLI/API, GUI только потребитель;
- GUI не является архитектурным компонентом консенсуса.

---

## 14. Идентификация сетей

- `network_id` в генезисе и в `HELLO`-сообщении: mainnet=1, testnet=2, regtest=3;
- Пиры с чужим `network_id` отбрасываются до любых данных;
- Все тестовые сценарии работают на `regtest` с fake clock (детерминизм);
- `chain_id` в каждой транзакции для replay protection (инвариант #10);
- HRP в bech32: `sc1...` (mainnet), `tsc1...` (testnet), `rsc1...` (regtest).

---

## 15. Anti-goals (что мы НЕ делаем)

| Anti-goal | Причина |
|-----------|---------|
| `serde_json` в консенсусных путях | только бинарная `serialize`; JSON — только config/api/explorer |
| Глобальные `lazy_static`/`once_cell` для консенсусных констант | константы в `consensus.rs` как `pub const`; тесты подменяют через dependency injection |
| Своя VM инструкция | WASM (wasmi) — готовый интерпретатор, фаззеры, LLVM бэкенд |
| Hand-written cryptographic primitives | только audited crates (`secp256k1`, `blst`, `ed25519-dalek`) |
| `tokio` на Stage 0 | threads + mpsc достаточно для sanitization; tokio вводится на Stage 1 |
| Сохранение `rusty_leveldb` как permanent storage | миграция на RocksDB на Stage 3 |
| Ed25519 для EOA-подписей | secp256k1 для совместимости с MetaMask/Ledger; Ed25519 — только опция для internal node-to-node auth |
| Halving + max_supply без tail emission | tail emission (Monero-style 0.6%/год) для постоянного security budget |
| PoW как финальный консенсус | Hybrid PoW→PoS; PoS миграция через activation height на Stage 7 |
| `redb` как storage engine | RocksDB battle-tested (Bitcoin Core, Ethereum reth) |
| WIT/Wasm Component Model для ABI на MVP | слишком новый стандарт (W3C, 2023+); JSON ABI для MVP, WIT migration на Stage 4+ |
| Block-STM на Stage 1.5 | cutting-edge research; сначала корректная последовательная валидация (Stage 1.5), потом Block-STM (Stage 3+) |
| Account Abstraction на Stage 1.5 | слишком сложно для ранней стадии; AA на Stage 5 после VM и dev-experience |
| EIP-1559 на Stage 1 | фиксированный gas limit для MVP; EIP-1559 на Stage 5 после стабилизации экономики |
| State rent как обязательная фича | statelessness + pruning достаточно; state rent — опционально на Stage 3+ через activation height |
| Soft forks | только hard forks через SCIP + activation height (явные, планируемые) |
| Secret в config files | только в keystore (AES-256-GCM + PBKDF2); config — только несекретные параметры |
| `println!` для production логирования | `tracing` crate с уровнями; structured logging для metrics |

---

## 16. Open questions (нерешённые, для ADR)

Эти решения должны быть зафиксированы в `docs/ADR/` до того, как затронут
консенсус:

1. **POS_ACTIVATION_HEIGHT:** на какой высоте переключаться с PoW на PoS?
   Зависит от: достаточной decentralization PoW-фазы, готовности validator set,
   security audit PoS логики.
2. **Tail emission старт:** на какой высоте включается tail emission? После
   достижения `max_supply_pre_tail` (например, 21M).
3. **Validator set size:** сколько validators в active set? Большой set →
   decentralization, но медленная финализация. Маленький → быстрый, но
   централизованный.
4. **MEV mitigation:** threshold encryption vs PBS vs batch auctions? Решение
   на Stage 5 через SCIP.
5. **State rent:** включать или нет? Решение на Stage 3+ через SCIP.
6. **Multidimensional fees:** EIP-7706 analog? Research на Stage 6+.
7. **ZK-VM:** интеграция RISC Zero / SP1 для stateless validation? Research на
   Stage 6+.
8. **Finality threshold:** 2/3 stake (Casper FFG) или другой? Зависит от
   trade-off finality time vs security.

Решение по каждому фиксируется в `docs/ADR/` (Architecture Decision Records) до
того, как затронет консенсус (требование «изменения только через activation
height»).

---

## 17. Чек-лист соответствия (для PR review)

Каждый PR обязан закрывать соответствующие пункты:

```
[ ] Чистое ядро: serialize/consensus/state/economics/governance — 0 I/O
[ ] Инварианты 1-22 enforce (см. §5)
[ ] Threat model: PR затрагивает новый вектор атаки? Митигация добавлена (см. §6)
[ ] Events bus: новые события через EventBus, не через прямой Mutex
[ ] Blockchain декомпозиция: chain_selector, block_executor, state_cache, facade, consensus_manager
[ ] Sync engine: разрыв цикла network↔blockchain
[ ] VM trait в core, runtime отдельно (wasmi)
[ ] Protocol messages: GetHeaders/Headers, deprecated legacy удалён
[ ] Peer store/manager/connection_pool разделены
[ ] Tie-breaking: work → earliest timestamp → lowest hash
[ ] Mempool RBF policy implemented
[ ] Storage schema versioned + migrations
[ ] Wallet: secp256k1/BLS, public API only, path to separate crate documented
[ ] Economics: tail emission formula, fee invariant enforced
[ ] Governance: SCIP + activation height для изменений консенсуса
[ ] Security: TLA+ spec актуализирован, fuzzing targets добавлены
[ ] Anti-goals respected (см. §15)
[ ] License: MIT/Apache-2.0, LICENSE файл в корне
[ ] Reproducible builds: CI gate, cosign signature для release
[ ] Strangler pattern: перенос подсистемы в отдельный крейт (если применимо)
[ ] Tests: unit + property (proptest) + integration (two_clients, reorg, double_spend)
[ ] CI: clippy -D warnings, all platforms (Linux/macOS/Windows)
[ ] Documentation: ADR для архитектурных решений, SCIP для изменений протокола
```

---

## 18. Связь с другими документами

- `ARCHITECT.md` — предыдущая версия архитектуры (PoW-centric).
- `ARCHITECT2.md` — критика зрелости ARCHITECT.md.
- `ROADMAP.md` — предыдущая дорожная карта.
- `ROADMAP2.md` — путь к ARCHITECT3.md (см. `ROADMAP2.md`).
- `ANALYSIS.md` — критический разбор всех четырёх документов (см. `ANALYSIS.md`).
- `prompt.md` — 117 промптов для Stage 0 (для AI-агентов; требует обновления с
  учётом решений в ARCHITECT3.md).
- `docs/ADR/` — Architecture Decision Records (детали решений).
- `docs/spec/consensus.tla` — TLA+ спецификация консенсуса.
- `docs/security/THREAT_MODEL.md` — детальный threat model (расширение §6).
