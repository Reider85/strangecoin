Я в режиме планирования (read-only), поэтому не могу записать файл. Вот полный анализ — вы можете сохранить его как `architect2.md`:

---

# Анализ зрелости Strangecoin: чего не хватает до зрелой криптовалюты

## 1. Чего не хватает проекту до зрелой криптовалюты

### 1.1 Критические уязвимости (блокируют любую «зрелость»)

| # | Проблема | Где в коде | Последствие |
|---|----------|------------|-------------|
| 1 | **Нет подписей транзакций** | `main.rs:1605-1610` — `Transaction` создаётся без подписи; `wallet.rs:191` `sign_transaction` нигде не вызывается | Любой может потратить чужой баланс, подменив `sender` |
| 2 | **`validate_chain` не проверяет PoW** | `main.rs:725-836` — проверяет хэш, но не `hash.starts_with("0".repeat(difficulty))` | Блоки с нулевой сложностью принимаются как валидные |
| 3 | **Тавтологичная валидация балансов** | `main.rs:747` — `expected_balances` инициализируется из `self.balances` (копией), а не из генезиса | Манипуляции с `balances` в БД не обнаруживаются |
| 4 | **Нет награды за блок / эмиссии** | `main.rs:305-343` генезис даёт 10000 фиктивному адресу; нет `coinbase` | Майнинг невыгоден, нет расписания эмиссии, нет `max_supply` |
| 5 | **Недетерминированный генезис** | `main.rs:309-313` — `receiver: "initial_wallet_address"` (строка), UUID для txid | Разные узлы ? разные генезис-хэши ? невозможен консенсус |
| 6 | **Адреса без контрольной суммы** | `wallet.rs:65` — base64 публичного ключа | Опечатка в адресе = потеря средств |
| 7 | **`find_wallet_by_ip` возвращает случайный баланс** | `main.rs:1029-1039` — итерирует по `balances`, возвращает первый | Полная неработоспособность поиска кошелька |
| 8 | **Регистрация мутирует `balances` напрямую** | `main.rs:1518-1530` — минуя блокчейн/мемпул | Узлы рассинхронизируются при регистрации |
| 9 | **Пароль в открытом виде** | `config.json:4` — `"password": "password"` | Компрометация ключей при утечке конфига |
|10| **Ad-hoc синхронизация, гонки, OOM** | `main.rs:1057-1058` — `vec![0; length]` без лимита; `Mutex<Blockchain>` глобальный | DoS вектор, блокировка майнингом всего узла, расхождения цепей |
|11| **Неканоническая сериализация** | `main.rs:425-432` — `serde_json::to_string` для хэша | Недетерминизм хэшей, нельзя верифицировать независимо |
|12| **Нет валидации времени блоков** | `main.rs:735-744` — только проверка генезиса; нет median-time-past | Блоки из будущего, манипуляция difficulty |
|13| **Нет тестов / CI** | ROADMAP.md:40 — тест `test_two_clients` упомянут, но отсутствует | Нет гарантий корректности после изменений |

### 1.2 Фундаментальные компоненты, отсутствующие полностью

Mempool, P2P протокол (только 2 сообщения), State root/Merkle, Fee market, Finality gadget, Checkpoint sync, JSON-RPC/CLI, HD Wallet, Indexer/Explorer.

### 1.3 Архитектурные долги

Монолит `main.rs` (1756 строк), глобальный `Mutex<Blockchain>`, нет событийной шины, `storage` не изолирован, `wallet` привязан к конфигу, `Transaction` без `nonce`, `gas`, `signature`, `pubkey`.

---

## 2. Целевая архитектура (ARCHITECT.md) — зрелая ли она?

### 2.1 Что сделано правильно ?

- Чистое ядро (`consensus`, `state`, `serialize` без I/O)
- Жёсткие границы модулей + dependency rules
- Один источник истины — блокчейн, балансы — кэш
- Events bus — multi-subscriber broadcast
- Headers-first sync + cumulative work через `SyncEngine`
- Peer scoring / ban / connection pool
- VM trait в core, runtime отдельно
- Workspace structure (`strangecoin-core`, `wallet`, `node`, `api`, `gui`)
- Network ID (mainnet/testnet/regtest)
- Genesis как консенсусный якорь (`EXPECTED_GENESIS_HASH`)
- Anti-goals (no tokio в Этапе 0, no custom VM, no PoS prematurely)

### 2.2 Где НЕ доходит до зрелой криптовалюты (пробелы)

| Пробел | Почему критично | Что добавить |
|--------|-----------------|--------------|
| **Нет формальной спецификации консенсуса (TLA+)** | Без model-checking — неизвестны граничные случаи реоргов, time-warp, selfish mining | `docs/spec/consensus.tla`, CI gate |
| **Нет threat model** | Нет анализа векторов: eclipse, partition, selfish mining, MEV | `docs/security/THREAT_MODEL.md` |
| **MEV mitigation — только Этап 5** | MEV — системный риск (фронтраннинг, цензура) | Запроектировать PBS / threshold encryption **до** Этапа 2 |
| **Fee market / EIP-1559 — только Этап 5** | Без динамической базовой комиссии — спам, нестабильные комиссии | Перенести на Этап 1 |
| **Account Abstraction — только Этап 1.5** | Без AA — нет спонсируемых транзакций, социального восстановления | Запроектировать `UserOperation` mempool заранее |
| **State rent / expiry — только Этап 1.5** | Без pruning — неограниченный рост диска | Запроектировать как часть state trie design на Этапе 1 |
| **Stateless validation (Verkle) — только Этап 3** | Без witness — light клиенты не верифицируют блоки | Выбор SMT vs Verkle нужен **до** Этапа 1 |
| **Parallel execution (Block-STM) — только Этап 1.5** | Последовательное исполнение — узкое горлышко TPS | Запроектировать `ExecutionContext` с dependency tracking |
| **Upgrade governance (SCIP) — только Этап 5** | Без активации по высоте — хардфорки рискованны | `consensus_version` + activation height на Этапе 1 |
| **Reproducible builds / cosign — только Этап 6** | Без воспроизводимых сборок — нельзя верифицировать бинарник | Добавить в CI на Этапе 0 |

### 2.3 Нерешённые решения (§9) — нужны до Этапа 1

- **VM runtime**: рекомендую **WASM (wasmi)** — mature toolchain, no-std, детерминированный
- **State tree**: рекомендую **Verkle Trie** — stateless validation witnesses критичны
- **Storage**: рекомендую **redb** (pure Rust, ACID, no CGO)
- **Finality**: **PoW + Casper FFG** без стейкинга (стейкинг = экономическая сложность)

---

## 3. Улучшения целевой архитектуры для зрелости

### Добавить в ARCHITECT.md новые разделы:

**13. Формальная спецификация консенсуса (TLA+)** — safety/liveness properties, model-checking scope

**14. Threat Model (STRIDE)** — таблица 9 векторов атак с митигациями в архитектуре

**15. MEV Mitigation Architecture** — Threshold Encryption Mempool + PBS design (интеграция в `mempool`, `protocol`, `consensus`)

**16. Account Abstraction (ERC-4337 analog)** — UserOperation mempool, Bundler, Paymaster, EntryPoint precompile

**17. State Rent & Expiry (EIP-4444)** — history pruning, state expiry с revival через Verkle witnesses, optional rent

### Изменения в существующих разделах:

- §3.3 `state`: добавить Verkle root, `StateWitness`, `state_expiry_check`
- §3.5 `mempool`: `EncryptedMempool` + `UserOperationMempool`, `MempoolTrait`
- §3.6 `network`: `MEVRelay` subnet, `Dandelion++`, `Portal Network`
- §5 Инварианты: добавить `state_root` match, `fee_burned + fee_to_miner = total_fees`, `gas_used ? block_gas_limit`
- §8 Этапы: сдвинуть Fee market, AA design, State expiry, MEV design ? Этап 1; TLA+ spec ? Этап 0

---

## 4. Анализ смарт-контрактов: чего не хватает

### 4.1 Текущее состояние: смарт-контрактов НЕТ

В v0.8.6 нет VM, нет host functions, нет gas metering, нет контрактных аккаунтов, нет ABI, нет событий.

### 4.2 Минимальный набор для MVP (Этап 1.5)

VM Runtime (WASM/wasmi), Host Functions (ed25519_verify, blake3/keccak, storage_read/write, call/delegatecall/staticcall, block context), Account Model (nonce, balance, code_hash, storage_root), Tx Types (Transfer/Create/Call), Gas Metering, Verkle Trie, Precompiles (ecrecover, sha256, keccak, blake3, ed25519, modexp), ABI (WIT/Wasm Component Model), Reentrancy Guard, Limits (max_code_size, max_storage, max_call_depth).

### 4.3 Чего не хватает для **зрелых** смарт-контрактов

| Категория | Не хватает | Критичность |
|-----------|------------|-------------|
| **Формальная верификация VM** | K-спека wasmi/ckb-vm, proof-carrying code | ? Высокая |
| **Fuzzing harness** | cargo-fuzz targets, differential testing | ? Высокая |
| **Parallel Execution (Block-STM)** | Dependency detection, optimistic execution | ? Средняя |
| **Account Abstraction** | UserOperation mempool, Bundler, Paymaster, EntryPoint | ? Средняя |
| **State Rent / Expiry** | EIP-4444, state expiry с revival | ? Средняя |
| **Fee Abstraction** | Paymaster платит любым токеном | ? Низкая |
| **Upgradeability Standard** | Proxy pattern (EIP-1967) + governance через SCIP | ? Низкая |
| **Token Standards** | SCIP-20/721/1155 | ? Низкая |
| **ZK-VM Integration** | RISC Zero / SP1 / ckb-vm zkVM | ? Будущее |

### 4.4 Архитектурные интерфейсы (добавить в ARCHITECT.md)

- `VmExecutor` trait с `ExecutionContext` / `ExecutionResult`
- `GasCosts` константы в `consensus.rs`
- `StateTrie` trait с `prove` / `verify_proof` для Verkle/SMT
- SDK `cargo-strangecoin` (new, test, deploy, verify, fuzz) + local devnet (anvil-аналог)

---

## 5. Critical Path — что сделать в первую очередь

### Этап 0 (Санация) — MUST HAVE
1. Подписи транзакций Ed25519
2. Каноническая бинарная сериализация
3. Проверка PoW + retargeting algorithm
4. Median-time-past timestamp validation
5. Детерминированная валидация балансов из генезиса
6. Детерминированный генезис + `EXPECTED_GENESIS_HASH`
7. Награда за блок + эмиссия (halving, max_supply, tail)
8. Лимиты размеров (закрыть OOM)
9. Модульное разделение
10. Тесты + CI (clippy -D warnings)

### Этап 1 (Криптографическое ядро)
Bech32 адреса, Merkle + **State Root (Verkle)**, Headers-first sync, Комиссии + mempool RBF, Майнинг без блокировки (RwLock), **Finality gadget (Casper FFG)**, Checkpoint sync, Rolling checkpoints.

### Этап 1.5 (Смарт-контракты) — параллельно
WASM (wasmi) — зафиксировать в ADR, Аккаунт-модель, Host functions + gas, Verkle Trie, Block-STM, WIT ABI, **Формальная VM спека + fuzzing** параллельно.

---

## 6. Definition of Done для «Зрелой криптовалюты»

(Полный чек-лист из 60+ пунктов по слоям: Consensus, Network, Storage, VM, Wallet, API, Security, Governance — см. полный текст выше)

---

## 7. Резюме

**Strangecoin v0.8.6 — прототип уровня "hello world blockchain"**, уязвимый ко всем атакам.

**Архитектура ARCHITECT.md — сильный фундамент**, близкая к зрелой, но с критическими пробелами: нет TLA+ спеки, threat model, MEV mitigation, AA, state rent, upgrade governance.

**Смарт-контрактов не существует**. Для MVP нужен WASM, аккаунты, gas, Verkle, precompiles. Для зрелости — формальная верификация VM, Block-STM, AA, state expiry, fuzzing.

**Рекомендация**: не переходить к Этапу 1.5 до полного завершения Этапа 0 и 1. Смарт-контракты на сломанном консенсусе = потеря средств пользователей.

---

Хотите, чтобы я создал план реализации этого анализа или записал его в файл когда вы переключитесь из режима планирования?