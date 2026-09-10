# prompt-stage0.md — Промпты для реализации Stage 0 (Санация прототипа)

**Версия:** 1.0
**Дата:** 2026-09-10
**Источник:** `ROADMAP3.md` (Stage 0), `ARCHITECT3.md` (§1–§17)
**Цель:** превратить прототип `0.8.6` в минимально безопасный монолит, готовый к декомпозиции (Stage 1). Каждый промпт — самостоятельное задание для ИИ-агента, завершающееся конкретными артефактами и чек-листом.

---

## 0. Общий контекст (вставлять в начало каждого промпта)

```
Проект: Strangecoin, Rust, edition 2021, версия 0.8.6.
Монолит: src/main.rs (~2418 строк) + src/wallet.rs (~204 строки).
Зависимости (Cargo.toml): rusty-leveldb, ed25519-dalek, sha2, pbkdf2, aes-gcm,
  eframe, ctrlc, toml, serde, serde_json, uuid, rand, base64.
Текущие типы:
  Block { index, timestamp, transactions, previous_hash, hash, nonce } (main.rs:41-49)
  Transaction { id, sender, receiver, amount } (main.rs:52-58)
  Blockchain { chain, balances, difficulty, pending_transactions, db: Arc<Mutex<DB>> }
Кошелёк: Ed25519, keystore зашифрован PBKDF2 + AES-256-GCM.
Целевая архитектура (ARCHITECT3.md):
  - secp256k1 (ECDSA) для EOA-подписей; Ed25519 — только опционально для internal auth.
  - Tail emission (Monero-style, 0.6%/год).
  - Каноническая бинарная сериализация; serde_json — только для config/api.
  - 22 инварианта (см. ARCHITECT3.md §5) — все enforce с Stage 0.
  - 25 векторов атак STRIDE (см. ARCHITECT3.md §6) — митигированы с Stage 0.
  - Strangler pattern: монолит остаётся; новые подсистемы выделяются в крейты с Stage 1.
Принципы:
  - Чистое ядро (consensus/state/serialize/economics/governance — 0 I/O).
  - Валидация на входе: размеры, подписи, время, формат, генезис, replay, rate.
  - Любое изменение консенсуса — через SCIP + activation height (после Stage 0 freeze).
  - Anti-goal: tokio на Stage 0 (только с Stage 1), state rent как обязательство,
    PoS до Stage 7, EIP-1559 до Stage 5, AA до Stage 5, софт-форки.
Стиль кода:
  - Не добавлять комментарии без необходимости.
  - После каждого промпта: cargo check и cargo test обязаны проходить.
  - Ничего не коммитить без явной просьбы.
  - Логирование — через tracing (не println!) начиная с P03.
Сокращения: КГ — критерии готовности.
```

---

## 1. Карта зависимостей промптов

```
P01 (LICENSE/ADRs) ──▶ P02 (module skeleton) ──▶ P03 (tracing)
                                                    │
              ┌─────────────────────────────────────┤
              ▼                                     ▼
P04 (secp256k1) ──▶ P05 (chain_id/nonce/address) ──▶ P06 (serialize.rs) ──▶ P07 (txid+verify)
              │
              ├─▶ P08 (difficulty/retarget) ──▶ P09 (MTP/time) ──▶ P10 (genesis) ──▶ P11 (emission)
              │
              └─▶ P12 (size limits/framing) ──▶ P13 (rate limit) ──▶ P14 (mempool)

P03 ──▶ P15 (Config/secrets) ──▶ P16 (RwLock) ──▶ P17 (graceful shutdown)

P03 + P04 + P07 + P08 + P11 ──▶ P18 (proptest) ──▶ P19 (integration tests)
P18 ──▶ P20 (CI matrix) ──▶ P21 (warnings cleanup)

P01 + P22 (threat model) + P23 (TLA+) + P24 (reproducible builds) + P25 (bug bounty + ADR-0003)

Все промпты ──▶ P26 (DoD verification)
```

Параллельные треки (можно делать независимо):
- P22/P23/P24 (security track) — после P01.
- P15/P16/P17 (config/concurrency/shutdown) — после P03.

---

## 2. Список промптов

---

### P01. Подготовка репозитория: LICENSE, ADR-0004, ADR-0005, .gitignore, branch protection

**Цель:** заложить юридическую и архитектурную базу до любых изменений кода. ADR пишутся до того, как решение затронет код (принцип ROADMAP3 §8).

**Контекст:** В `ARCHITECT3.md §15` зафиксировано: license = MIT/Apache-2.0, tracing вместо println!. В `ROADMAP3 §Stage 0 ADRs required` — ADR-0004 (license) и ADR-0005 (tracing). В корне репо нет `LICENSE`, нет `docs/ADR/`.

**Задачи:**

1. Создать `LICENSE` в корне — dual license MIT OR Apache-2.0 (текст обеих лицензий).
2. Создать `docs/ADR/0001-template.md` — шаблон ADR (Context, Decision, Consequences, Alternatives).
3. Создать `docs/ADR/0004-license.md`:
   - Context: выбор между MIT, Apache-2.0, GPL, dual.
   - Decision: dual MIT OR Apache-2.0 (как Rust ecosystem convention).
   - Consequences: совместимость с зависимостями, простота adoption.
4. Создать `docs/ADR/0005-tracing-vs-println.md`:
   - Context: текущий код использует `println!` в ~30 местах main.rs и wallet.rs.
   - Decision: migrate на `tracing` crate с structured fields.
   - Consequences: уровни логирования, фильтрация, integration с metrics.
   - Alternatives: `log` + `env_logger` (отвергнут — нет structured fields).
5. Обновить `.gitignore`: добавить `blockchain_db_*`, `*.lock`, `target/`, `keystore/*.json` (если есть).
6. Обновить `Cargo.toml`: убрать секцию `[wallet]` (она не должна быть в Cargo.toml, это config-уровень).
7. Создать `docs/CONTRIBUTING.md` со ссылкой на ADR process.

**Артефакты:**
- `LICENSE`
- `docs/ADR/0001-template.md`
- `docs/ADR/0004-license.md`
- `docs/ADR/0005-tracing-vs-println.md`
- `.gitignore` (обновлён)
- `Cargo.toml` (без секции `[wallet]`)
- `docs/CONTRIBUTING.md`

**КГ (чек-лист):**
- [ ] `LICENSE` существует и содержит оба текста (MIT, Apache-2.0)
- [ ] `docs/ADR/0001-template.md` содержит Context/Decision/Consequences/Alternatives секции
- [ ] ADR-0004 и ADR-0005 написаны, каждая ~50–150 строк
- [ ] `Cargo.toml` не содержит секции `[wallet]`
- [ ] `cargo check` проходит
- [ ] `.gitignore` содержит `blockchain_db_*` и `*.lock`

---

### P02. Скелет модулей: blockchain/, consensus/, network/, mempool/, storage/, api/, cli/, gui/, error.rs

**Цель:** подготовить модульную структуру монолита для последующего наполнения. Сам код не переносится — только объявляются модули с заглушками. Это precondition для P03–P17 (каждая подсистема получает «дом»).

**Контекст:** `ARCHITECT3.md §3` описывает 15 подсистем. На Stage 0 все они живут внутри `src/main.rs` как подмодули; выделение в отдельные крейты — Stage 1+.

**Задачи:**

1. Создать в `src/` подмодули (пустые `mod.rs` с `// TODO: P0X наполнит`):
   - `src/blockchain/mod.rs` — оркестратор (сейчас `Blockchain` в main.rs).
   - `src/consensus/mod.rs` — правила валидности.
   - `src/network/mod.rs` — p2p, protocol, sync.
   - `src/mempool/mod.rs` — пул неподтверждённых tx.
   - `src/storage/mod.rs` — LevelDB-обёртка (RocksDB будет в Stage 3).
   - `src/api/mod.rs` — JSON-RPC + CLI (позже).
   - `src/cli/mod.rs` — отдельный модуль CLI-команд.
   - `src/gui/mod.rs` — egui-клиент (feature flag `gui`).
   - `src/economics/mod.rs` — placeholder (emission.rs, fee_market.rs — пустые).
   - `src/governance/mod.rs` — placeholder (scip.rs, consensus_version — пустые).
2. Создать `src/error.rs` — типизированные ошибки:
   ```rust
   #[derive(Debug, thiserror::Error)]
   pub enum StrangecoinError {
       #[error("invalid signature")]
       InvalidSignature,
       #[error("invalid nonce: expected {expected}, got {got}")]
       InvalidNonce { expected: u64, got: u64 },
       #[error("invalid chain_id: expected {expected}, got {got}")]
       InvalidChainId { expected: u32, got: u32 },
       #[error("block difficulty mismatch")]
       InvalidDifficulty,
       #[error("size limit exceeded: {0}")]
       SizeLimitExceeded(&'static str),
       // ... расширять в последующих промптах
   }
   ```
3. Добавить `thiserror = "1"` в `Cargo.toml`.
4. В `src/main.rs` заменить прямые объявления на `mod blockchain; mod consensus; ...`.
5. Существующий код `Blockchain`, `Block`, `Transaction` временно оставить в `main.rs` (перенос — следующий шаг после P03).

**Артефакты:**
- `src/error.rs`
- `src/{blockchain,consensus,network,mempool,storage,api,cli,gui,economics,governance}/mod.rs` (10 файлов-заглушек)
- `Cargo.toml` (добавлен `thiserror`)
- `src/main.rs` (обновлённый `mod`-блок)

**КГ (чек-лист):**
- [ ] Все 10 подмодулей созданы, `mod`-блок в main.rs подключает их
- [ ] `error.rs` содержит `StrangecoinError` enum с ≥6 вариантами
- [ ] `cargo check` проходит (заглушки не ломают сборку)
- [ ] `cargo test` проходит (существующие тесты не сломаны)
- [ ] В main.rs нет дублирования: `mod wallet;` и новые `mod`-объявления корректны

---

### P03. Миграция логирования на `tracing` crate (замена println!)

**Цель:** выполнить ADR-0005. Ввести structured logging, подготовить базу для metrics и integration tests (последние смогут читать логи вместо sleep).

**Контекст:** `ARCHITECT3.md §15` явно запрещает `println!` для production. `ROADMAP3 §Stage 0` требует `tracing` с Stage 0.

**Задачи:**

1. Добавить в `Cargo.toml`:
   ```toml
   tracing = "0.1"
   tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
   ```
2. В `src/main.rs` инициализировать subscriber в `main()`:
   ```rust
   tracing_subscriber::fmt()
       .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
       .with_target(false)
       .init();
   ```
3. Заменить ВСЕ `println!` в `src/main.rs` и `src/wallet.rs` на `tracing::info!` / `tracing::warn!` / `tracing::error!` / `tracing::debug!`:
   - ошибки и паники → `error!`
   - важные события (block applied, peer connected, mining started) → `info!`
   - диагностика (waiting for lock, retrying) → `debug!`
   - подозрительные ситуации (peer sent invalid block, nonce mismatch) → `warn!`
4. Использовать structured fields где возможно: `info!(height = block.index, txs = block.transactions.len(), "block applied")`.
5. Уровень по умолчанию `info`, переопределяется через `RUST_LOG=strangecoin=debug`.
6. Не трогать `eframe::egui` UI-текст (это не логирование).

**Артефакты:**
- `Cargo.toml` (добавлены tracing зависимости)
- `src/main.rs` (все `println!` заменены)
- `src/wallet.rs` (все `println!` заменены)

**КГ (чек-лист):**
- [ ] `rg "println!" src/` возвращает 0 совпадений (кроме комментариев, если есть)
- [ ] `RUST_LOG=strangecoin=debug cargo run` показывает debug-сообщения
- [ ] `cargo run` без `RUST_LOG` показывает только `info`/`warn`/`error`
- [ ] Логи содержат structured fields (минимум в 5 точках: block apply, tx receive, peer connect, mining start, mining finish)
- [ ] `cargo check` и `cargo test` проходят

---

### P04. Миграция кошелька на secp256k1 (ECDSA) — ADR-0001

**Цель:** выполнить ADR-0001 (secp256k1 вместо Ed25519) из `ROADMAP3 §Stage 0`. Совместимость с MetaMask/Ledger/WalletConnect — long-term goal. Ed25519 полностью убирается из `src/wallet.rs` (опциональный return — Stage 2 для internal auth).

**Контекст:** `ARCHITECT3.md §3.8` требует secp256k1 для EOA-подписей. `ARCHITECT3.md §15` anti-goal: «Ed25519 для EOA-подписей».

**Задачи:**

1. Написать `docs/ADR/0001-secp256k1-vs-ed25519.md`:
   - Context: текущий `wallet.rs` использует ed25519-dalek 2.2.
   - Decision: migrate на secp256k1 (rust-secp256k1 crate).
   - Consequences: совместимость с MetaMask/Ledger, BIP-340 Schnorr в будущем.
   - Alternatives: Ed25519 (отвергнут — несовместимость с Ethereum tooling).
2. Добавить в `Cargo.toml`:
   ```toml
   secp256k1 = { version = "0.29", features = ["rand", "serde", "global-context"] }
   ```
   Убрать `ed25519-dalek` из зависимостей (если не используется в других местах — иначе оставить до P05).
3. Переписать `src/wallet.rs`:
   - `SigningKey` = `secp256k1::SecretKey`.
   - `VerifyingKey` = `secp256k1::PublicKey`.
   - `Keystore` структура: `public_key` (hex), `encrypted_private_key` (hex), `salt`, `nonce`, `version`.
   - `Wallet::new(password, path)` — генерация secp256k1 ключа, шифрование PBKDF2 + AES-256-GCM.
   - `Wallet::load(password, path)` — расшифровка.
   - `Wallet::sign(message: &[u8]) -> [u8; 64]` — ECDSA подпись.
   - `Wallet::verify(sig: &[u8; 64], message: &[u8], pk: &PublicKey) -> bool`.
4. НЕ трогать `address_from_public_key` (пока base64 → будет bech32 в Stage 1). P05 добавит правильный формат.
5. НЕ трогать `sign_transaction` (он будет переписан в P07 на канонические байты).
6. Сохранить существующий keystore-формат (PBKDF2 + AES-256-GCM + nonce), обновить только ключи.
7. Обновить тесты: создание кошелька, sign/verify round-trip.

**Артефакты:**
- `docs/ADR/0001-secp256k1-vs-ed25519.md`
- `Cargo.toml` (secp256k1 добавлен, ed25519-dalek убран если возможно)
- `src/wallet.rs` (полностью переписан под secp256k1)

**КГ (чек-лист):**
- [ ] ADR-0001 написан, ~80–150 строк
- [ ] `cargo check` проходит
- [ ] Тест: создали кошелёк → сохранили keystore → загрузили → sign/verify round-trip работает
- [ ] Тест: чужой публичный ключ не верифицирует подпись
- [ ] `ed25519-dalek` отсутствует в `Cargo.toml` (если только не оставлен для internal auth с пометкой TODO)
- [ ] Keystore зашифрован (проверка: открыли файл — там base64, не plaintext)

---

### P05. Replay protection: chain_id, nonce, address_from_public_key

**Цель:** реализовать инварианты #10 (chain_id), #11 (nonce), #12 (txid из канонических байтов) из `ARCHITECT3.md §5`. Защита от cross-chain replay и tx дублирования.

**Контекст:** `ARCHITECT3.md §3.2` требует `chain_id` в каждой tx. `ARCHITECT3.md §14` фиксирует: mainnet=1, testnet=2, regtest=3.

**Задачи:**

1. Расширить `Transaction` в `src/main.rs`:
   ```rust
   struct Transaction {
       sender: String,
       receiver: String,
       amount: u64,
       nonce: u64,           // NEW
       chain_id: u32,        // NEW
       signature: Vec<u8>,   // NEW (пустой для coinbase)
       is_coinbase: bool,    // NEW
   }
   ```
   Убрать поле `id` — оно будет вычисляться (P07). Старые JSON-транзакции из LevelDB — `#[serde(default)]` на новые поля.
2. В `src/consensus/mod.rs` объявить константы:
   ```rust
   pub const CHAIN_ID_MAINNET: u32 = 1;
   pub const CHAIN_ID_TESTNET: u32 = 2;
   pub const CHAIN_ID_REGTEST: u32 = 3;
   pub fn current_chain_id() -> u32 { /* из Config в P15 */ }
   ```
3. Создать `src/address.rs` (или в `consensus`):
   ```rust
   pub fn address_from_public_key(pk: &secp256k1::PublicKey) -> String {
       // Временно: hex(pubkey). Stage 1 → bech32 (sc1...).
       hex::encode(pk.serialize())
   }
   ```
   Добавить `hex = "0.4"` в Cargo.toml.
4. Обновить места создания `Transaction` (main.rs genesis, GUI send, тесты): всегда проставлять `chain_id = current_chain_id()` и `nonce = account_nonce + 1`.
5. В `Blockchain::add_transaction` (main.rs:548):
   - Reject если `tx.chain_id != current_chain_id()`.
   - Reject если `tx.nonce != account_nonce(sender) + 1`.
   - Reject если `tx.sender != address_from_public_key(pubkey из подписи)`.
6. Добавить поле `nonce` в `balances: HashMap<String, AccountState>` где `AccountState { balance: u64, nonce: u64 }`. Обновить все чтения (`balances.get(&addr)` → `.balance`).

**Артефакты:**
- `src/main.rs` (Transaction расширен, add_transaction валидирует)
- `src/consensus/mod.rs` (chain_id константы)
- `src/address.rs` (новый файл)
- `Cargo.toml` (hex добавлен)

**КГ (чек-лист):**
- [ ] `Transaction` содержит `nonce`, `chain_id`, `signature`, `is_coinbase`
- [ ] Старые JSON-транзакции из LevelDB десериализуются (через `#[serde(default)]`)
- [ ] Тест: tx с `chain_id=2` отвергается на mainnet-узле
- [ ] Тест: tx с `nonce <= account.nonce` отвергается
- [ ] Тест: tx с `nonce = account.nonce + 1` принимается
- [ ] Тест: `address_from_public_key` round-trip с sign/verify
- [ ] `cargo check` и `cargo test` проходят

---

### P06. Каноническая бинарная сериализация (serialize.rs)

**Цель:** реализовать инвариант #5 (хэш/подпись — на канонических байтах, не JSON) и #12 (txid = commitment). `ARCHITECT3.md §3.1` требует единую бинарную кодировку для всего консенсуса.

**Контекст:** Текущий код использует `serde_json::to_string` для подписи (`wallet.rs:191`). Это неканонично: JSON key order, escapes, whitespaces не детерминированы. Должна быть детерминированная бинарная кодировка.

**Задачи:**

1. Создать `src/serialize.rs`:
   ```rust
   pub const FORMAT_VERSION: u8 = 1;

   pub fn serialize_transaction(tx: &Transaction) -> Vec<u8> {
       let mut out = Vec::new();
       out.push(FORMAT_VERSION);
       out.extend_from_slice(tx.sender.as_bytes());     // length-prefixed
       out.extend_from_slice(&(tx.sender.len() as u32).to_be_bytes());
       // ... то же для receiver, amount (u64 BE), nonce (u64 BE), chain_id (u32 BE)
       // signature НЕ включается (она по каноническим байтам БЕЗ подписи)
       out.push(tx.is_coinbase as u8);
       out
   }

   pub fn serialize_block_header(b: &Block) -> Vec<u8> { ... }
   pub fn serialize_block(b: &Block) -> Vec<u8> { ... }
   ```
   Правила: все строки — length-prefixed (u32 BE + bytes); все числа — big-endian; поля в фиксированном порядке; `format_version` в начале.
2. Использовать `blake3` (быстрее SHA-256) для хэшей. Добавить `blake3 = "1"` в Cargo.toml.
3. НЕ удалять SHA-256 из `Cargo.toml` (используется в PoW). PoW hash остаётся SHA-256 (или меняется на blake3 — решить в P08).
4. Тестовые golden-векторы: зафиксировать сериализацию 3 эталонных транзакций и 2 блоков. Любое изменение кодировки = сломает тест.
5. В `src/wallet.rs::sign_transaction` — подписывать `serialize_transaction(tx)` (не JSON).
6. В `src/main.rs` — `block.hash` = `blake3(serialize_block_header(block))`.

**Артефакты:**
- `src/serialize.rs` (новый модуль)
- `Cargo.toml` (добавлен blake3)
- `src/wallet.rs` (sign_transaction использует канонические байты)
- `src/main.rs` (block.hash через blake3)

**КГ (чек-лист):**
- [ ] `serialize.rs` содержит `serialize_transaction`, `serialize_block`, `serialize_block_header`
- [ ] Golden-вектор: сериализация test-vector-1 = зафиксироранные байты (hex)
- [ ] Тест: изменение любого поля tx ломает сериализацию (сравнение с golden)
- [ ] Тест: `serialize_transaction(tx_a) == serialize_transaction(tx_a)` (детерминизм)
- [ ] Тест: `serialize_transaction(tx_a) != serialize_transaction(tx_b)` если tx_a ≠ tx_b
- [ ] `sign_transaction` подписывает канонические байты (см. P04 тесты)
- [ ] `cargo check` и `cargo test` проходят

---

### P07. txid = commitment + полная верификация подписи

**Цель:** реализовать инвариант #12 (`txid` из канонических байтов всей транзакции, включая подпись) и инвариант #2 (sender == pubkey подписанта). Унифицировать идентификатор tx для mempool, rollback, tracking.

**Контекст:** `ARCHITECT3.md §3.1` фиксирует `txid(Transaction) -> [u8;32]`. `ARCHITECT3.md §5` инвариант #12: «`txid` выводится из канонических байтов всей транзакции (включая подпись)».

**Задачи:**

1. В `src/serialize.rs` добавить:
   ```rust
   pub fn serialize_transaction_signed(tx: &Transaction) -> Vec<u8> {
       let mut out = serialize_transaction(tx);
       out.extend_from_slice(&(tx.signature.len() as u32).to_be_bytes());
       out.extend_from_slice(&tx.signature);
       out
   }
   pub fn txid(tx: &Transaction) -> [u8; 32] {
       blake3::hash(&serialize_transaction_signed(tx)).into()
   }
   pub fn block_hash(b: &Block) -> [u8; 32] {
       blake3::hash(&serialize_block_header(b)).into()
   }
   ```
2. Везде убрать использование `tx.id` (строкового) → заменить на `hex::encode(txid(tx))`. Legacy `id` поле удалить из `Transaction`.
3. В `src/wallet.rs::sign_transaction`:
   ```rust
   pub fn sign_transaction(&mut self, tx: &mut Transaction) -> Result<(), StrangecoinError> {
       let bytes = serialize_transaction(tx);  // БЕЗ подписи
       let sig = self.sign(&bytes);
       tx.signature = sig.to_vec();
       Ok(())
   }
   ```
4. В `src/consensus/mod.rs` добавить `verify_transaction`:
   ```rust
   pub fn verify_transaction(tx: &Transaction) -> Result<(), StrangecoinError> {
       if tx.is_coinbase { return Ok(()); }
       let pk = recover_pubkey_from_sig(&tx.signature, &serialize_transaction(tx))?;
       if address_from_public_key(&pk) != tx.sender {
           return Err(StrangecoinError::InvalidSignature);
       }
       Ok(())
   }
   ```
   Использовать `secp256k1::ecdsa::recoverable_signature` (или verify если recover не нужен).
5. В `Blockchain::add_transaction` — обязательно вызывать `verify_transaction(tx)?`. Никаких веток без верификации.
6. В `validate_chain` — для каждого блока валидировать все tx. Coinbase tx пропускается (нет подписи).
7. Убрать `merge_pending_transactions` (если он не валидирует подписи) — оставить только `add_transaction`.

**Артефакты:**
- `src/serialize.rs` (добавлены `serialize_transaction_signed`, `txid`, `block_hash`)
- `src/consensus/mod.rs` (добавлен `verify_transaction`)
- `src/wallet.rs` (`sign_transaction` использует канонические байты)
- `src/main.rs` (все `tx.id` удалены, валидация везде)

**КГ (чек-лист):**
- [ ] Тест: tx с поддельной подписью отклоняется в `add_transaction`
- [ ] Тест: tx с изменённым amount после подписи отклоняется
- [ ] Тест: tx с `sender != address_from_public_key(pubkey)` отклоняется
- [ ] Тест: coinbase tx проходит без подписи (в validate_chain, не в add_transaction)
- [ ] Тест: `txid(tx)` детерминирован (одинаковые байты → одинаковый txid)
- [ ] Тест: цепочка с одной неверной подписью → `validate_chain() == false`
- [ ] `rg "tx\.id" src/` возвращает 0 совпадений (кроме комментариев)
- [ ] `cargo check` и `cargo test` проходят

---

### P08. Валидация difficulty + алгоритм ретаргетинга (sliding window)

**Цель:** закрыть критическую проблему «нет проверки difficulty» (ROADMAP3 §Stage 0 Консенсус). Реализовать инвариант #3 (`hash <= target`).

**Контекст:** `ARCHITECT3.md §3.2`: «PoW: `target` в заголовке, `hash <= target`; ретаргетинг по скользящему окну». Текущий код: `hash.starts_with("0".repeat(difficulty))` — упрощённо, без ретаргетинга по реальному времени.

**Задачи:**

1. В `Block` добавить поле `target: [u8; 32]` (или `bits: u32` как в Bitcoin). Закодировать как compact representation (Bitcoin-style).
2. В `src/consensus/mod.rs`:
   ```rust
   pub const RETARGET_INTERVAL: u64 = 2016;  // блоков
   pub const TARGET_BLOCK_TIME: u64 = 600;   // секунд (10 минут)
   pub const MEDIAN_TIME_WINDOW: usize = 11;

   pub fn compute_target(prev_blocks: &[Block]) -> [u8; 32] {
       // Скользящее окно: real_time / expected_time * prev_target
       // clamp чтобы не менялся слишком резко (factor 4 max)
   }

   pub fn validate_difficulty(block: &Block) -> Result<(), StrangecoinError> {
       let hash = block_hash(block);
       if hash.iter().take_while(|&&b| b == 0).count() < block.difficulty as usize {
           // или: u256(hash) <= u256(target)
           return Err(StrangecoinError::InvalidDifficulty);
       }
       Ok(())
   }
   ```
3. В `validate_chain`:
   - На каждом блоке проверять `validate_difficulty(block)?`.
   - На высоте, кратной `RETARGET_INTERVAL` — проверять, что `target` пересчитан по sliding window.
   - Запрет снижения `difficulty` иначе как по правилу ретаргетинга (если `block.target != expected_target` → reject).
4. Майнинг: `mine_block` ищет `nonce` такой, что `block_hash(block) <= target` (как u256, не как строковый prefix).
5. Убрать упрощённую проверку `hash.starts_with("0".repeat(difficulty))`.

**Артефакты:**
- `src/consensus/mod.rs` (функции `compute_target`, `validate_difficulty`, константы)
- `src/main.rs` (Block расширен, validate_chain обновлён, mine_block обновлён)

**КГ (чек-лист):**
- [ ] Тест: блок с `hash > target` отвергается в `validate_chain`
- [ ] Тест: блок с `target != expected_target` (не на retarget height) отвергается
- [ ] Тест: на retarget height `target` пересчитан корректно (сравнение с эталоном)
- [ ] Тест: ретаргетинг clamp-ится (фактор 4 — нельзя изменить target в 4 раза за один retarget)
- [ ] `cargo check` и `cargo test` проходят

---

### P09. Median-time-past + запрет future timestamp

**Цель:** закрыть уязвимость time-warp attack (ARCHITECT3 §6 вектор #4). Реализовать правила валидации timestamp.

**Контекст:** `ARCHITECT3.md §3.2`: «время: `median_time_past`, запрет timestamp из будущего». `ARCHITECT3.md §6` вектор #4: «Time-warp attack — манипуляция timestamp → Mitigations: MTP с окном 11 блоков; запрет на timestamp > now + 2 часа».

**Задачи:**

1. В `src/consensus/mod.rs`:
   ```rust
   pub const MEDIAN_TIME_WINDOW: usize = 11;
   pub const MAX_FUTURE_TIME: u64 = 2 * 60 * 60;  // 2 часа

   pub fn median_time_past(blocks: &[Block], current_height: u64) -> u64 {
       let start = current_height.saturating_sub(MEDIAN_TIME_WINDOW as u64);
       let mut times: Vec<u64> = blocks[start..].iter().map(|b| b.timestamp).collect();
       times.sort();
       times[times.len() / 2]
   }

   pub fn validate_timestamp(block: &Block, prev_blocks: &[Block], now: u64) -> Result<(), StrangecoinError> {
       let mtp = median_time_past(prev_blocks, block.index);
       if block.timestamp <= mtp {
           return Err(StrangecoinError::TimestampTooOld);
       }
       if block.timestamp > now + MAX_FUTURE_TIME {
           return Err(StrangecoinError::TimestampInFuture);
       }
       Ok(())
   }
   ```
2. Добавить варианты ошибок `TimestampTooOld`, `TimestampInFuture` в `StrangecoinError`.
3. В `validate_chain` — для каждого блока вызывать `validate_timestamp(block, &prev_blocks, now)?`.
4. В `mine_block` — `timestamp = max(now, mtp + 1)`.

**Артефакты:**
- `src/consensus/mod.rs` (добавлены MTP функции, константы, `validate_timestamp`)
- `src/error.rs` (новые варианты ошибок)
- `src/main.rs` (validate_chain, mine_block обновлены)

**КГ (чек-лист):**
- [ ] Тест: блок с `timestamp <= mtp` отвергается
- [ ] Тест: блок с `timestamp > now + 2h` отвергается
- [ ] Тест: блок с `mtp < timestamp <= now + 2h` принимается
- [ ] Тест: MTP корректно вычисляется для 11 блоков (median)
- [ ] `cargo check` и `cargo test` проходят

---

### P10. Детерминированный генезис: genesis.json + EXPECTED_GENESIS_HASH

**Цель:** реализовать инвариант #8 (генезис детерминирован, совпадает у всех узлов). Закрыть критическую проблему «недетерминированный genesis» (ROADMAP3 §Stage 0).

**Контекст:** `ARCHITECT3.md §3.12`: «`genesis.json` как входные данные, `EXPECTED_GENESIS_HASH` в `consensus.rs` как якорь». `ARCHITECT3.md §5` инвариант #8.

**Задачи:**

1. Создать `genesis.json` в корне репо:
   ```json
   {
     "format_version": 1,
     "network_id": 1,
     "chain_id": 1,
     "timestamp": 0,
     "initial_holder": "0x<secp256k1_pubkey_hex>",
     "initial_amount": 1000000000,
     "block_reward": 50,
     "tail_emission_rate": 0.006,
     "max_supply_pre_tail": 21000000,
     "target_block_time": 600,
     "retarget_interval": 2016,
     "genesis_hash": "0x<placeholder>"
   }
   ```
2. В `src/consensus/mod.rs`:
   ```rust
   pub const EXPECTED_GENESIS_HASH: [u8; 32] = [/* вычислить после фиксации genesis.json */];
   pub fn load_genesis(path: &str) -> Result<Block, StrangecoinError> { ... }
   pub fn validate_genesis(block: &Block) -> Result<(), StrangecoinError> {
       let h = block_hash(block);
       if h != EXPECTED_GENESIS_HASH {
           return Err(StrangecoinError::GenesisMismatch { expected: EXPECTED_GENESIS_HASH, got: h });
       }
       Ok(())
   }
   ```
3. При старте узла: если БД пуста — создать genesis-блок из `genesis.json`, проверить `validate_genesis`. Если БД не пуста — проверить, что первый блок в БД совпадает с `EXPECTED_GENESIS_HASH`.
4. Узел отказывается стартовать при mismatch (panic с понятным сообщением).
5. Поддержать regtest: regtest-узел генерирует свой genesis (без проверки `EXPECTED_GENESIS_HASH`), но `chain_id=3`.
6. Добавить CLI-флаг `--print-genesis-hash` для вычисления хэша genesis.json (для обновления константы).

**Артефакты:**
- `genesis.json` (в корне)
- `src/consensus/mod.rs` (константа `EXPECTED_GENESIS_HASH`, функции `load_genesis`, `validate_genesis`)
- `src/error.rs` (вариант `GenesisMismatch`)
- `src/main.rs` (старт узла с валидацией genesis)
- `src/cli/mod.rs` (команда `--print-genesis-hash`)

**КГ (чек-лист):**
- [ ] `genesis.json` существует и валиден против JSON-схемы
- [ ] Тест: пустая БД → узел создаёт genesis из `genesis.json`, валиден
- [ ] Тест: БД с чужим genesis → узел отказывается стартовать
- [ ] Тест: `--print-genesis-hash` выводит хэш, совпадающий с `EXPECTED_GENESIS_HASH`
- [ ] Тест: regtest-узел генерирует свой genesis (chain_id=3, без EXPECTED_HASH)
- [ ] `cargo check` и `cargo test` проходят

---

### P11. Эмиссия с tail emission + убрать искусственные лимиты майнинга — ADR-0002

**Цель:** реализовать инвариант #6 (блок не содержит наград сверх эмиссии) и tail emission (Monero-style). ADR-0002 — «Why tail emission instead of halving + max_supply».

**Контекст:** `ARCHITECT3.md §7.1`: «tail emission: 0.6%/год после первичного cap». `ARCHITECT3.md §15` anti-goal: «Halving + max_supply без tail emission». Текущий код: искусственные лимиты `1000 итераций / 5 сек` (main.rs mining thread) — убрать.

**Задачи:**

1. Написать `docs/ADR/0002-tail-emission-vs-halving.md`:
   - Context: Bitcoin halving + 21M cap → через 30+ лет security budget → 0.
   - Decision: tail emission 0.6%/год (Monero-style) после max_supply_pre_tail.
   - Consequences: постоянный security budget, защита от дефляционной spiral.
   - Alternatives: halving без tail (Bitcoin), fixed inflation (Dogecoin).
2. Создать `src/economics/emission.rs`:
   ```rust
   pub const TAIL_RATE_NUMERATOR: u64 = 6;
   pub const TAIL_RATE_DENOMINATOR: u64 = 1000;  // 0.6%
   pub const BLOCKS_PER_YEAR: u64 = 365 * 24 * 6;  // 10-минутные блоки
   pub const HALVING_INTERVAL: u64 = 210_000;  // ~4 года
   pub const MAX_SUPPLY_PRE_TAIL: u64 = 21_000_000;

   pub fn block_reward_at_height(height: u64, total_supply: u64) -> u64 {
       let base_reward = halving_schedule(height);
       let tail_reward = (total_supply * TAIL_RATE_NUMERATOR)
           / (TAIL_RATE_DENOMINATOR * BLOCKS_PER_YEAR);
       base_reward.max(tail_reward)
   }

   fn halving_schedule(height: u64) -> u64 {
       let initial = 50 * 100_000_000;  // 50 SC в сатоши
       let halvings = height / HALVING_INTERVAL;
       if halvings >= 64 { return 0; }
       initial >> halvings
   }
   ```
3. Убрать искусственные лимиты в майнинге: удалить «1000 итераций» и «5 секунд sleep» (`main.rs mining_thread`). Майнинг ищет nonce пока не найдёт решение (с возможностью cancellation через channel).
4. В `mine_block` — `coinbase.amount = block_reward_at_height(height, total_supply)`.
5. В `validate_chain` — для каждого блока проверять, что coinbase.amount == `block_reward_at_height(block.index, total_supply_before_block)`. Иначе — inflation, reject.
6. Поле `max_supply_pre_tail` и `tail_emission_rate` — в `genesis.json` (фиксируются до mainnet freeze).

**Артефакты:**
- `docs/ADR/0002-tail-emission-vs-halving.md`
- `src/economics/emission.rs`
- `src/main.rs` (mine_block, validate_chain, убраны искусственные лимиты)

**КГ (чек-лист):**
- [ ] ADR-0002 написан, ~80–150 строк
- [ ] Тест: `block_reward_at_height(0, 0) == 50 * COIN`
- [ ] Тест: `block_reward_at_height(HALVING_INTERVAL, 0) == 25 * COIN`
- [ ] Тест: `block_reward_at_height(very_large, MAX_SUPPLY_PRE_TAIL) >= tail_reward` (tail включается)
- [ ] Тест: блок с coinbase.amount > expected reward → reject (inflation protection)
- [ ] Тест: блок с coinbase.amount < expected reward → accept (майнер добровольно недополучает)
- [ ] В `rg "1000\|5 sec\|mining_thread.*sleep" src/main.rs` нет искусственных лимитов
- [ ] `cargo check` и `cargo test` проходят

---

### P12. Лимиты размеров + length-prefixed framing с проверкой ДО аллокации

**Цель:** закрыть критическую OOM-уязвимость (`vec![0; length]` в `src/main.rs:961`). Реализовать инварианты #7, #16. Митигировать STRIDE вектор #13 (DoS OOM).

**Контекст:** `ARCHITECT3.md §5` инвариант #16: «length-prefixed, проверка размера ДО аллокации (`vec![0; length]` с cap)». `ARCHITECT3.md §6` вектор #13: «DoS: OOM — `vec![0; length]` с огромным length».

**Задачи:**

1. В `src/network/protocol.rs`:
   ```rust
   pub const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;  // 32 MB
   pub const MAX_BLOCK_SIZE: usize = 4 * 1024 * 1024;     // 4 MB
   pub const MAX_TX_SIZE: usize = 256 * 1024;             // 256 KB

   pub fn read_length_prefixed<R: Read>(reader: &mut R) -> Result<Vec<u8>, StrangecoinError> {
       let mut len_buf = [0u8; 4];
       reader.read_exact(&mut len_buf).map_err(|_| StrangecoinError::IoError)?;
       let len = u32::from_be_bytes(len_buf) as usize;
       if len > MAX_MESSAGE_SIZE {
           return Err(StrangecoinError::SizeLimitExceeded("message"));
       }
       let mut buf = vec![0u8; len];  // теперь безопасно
       reader.read_exact(&mut buf).map_err(|_| StrangecoinError::IoError)?;
       Ok(buf)
   }
   ```
2. Заменить все `vec![0; length]` в `src/main.rs` (особенно ~line 961) на `read_length_prefixed`.
3. В `validate_block` — проверка `block.size() <= MAX_BLOCK_SIZE` ДО любых других проверок.
4. В `add_transaction` — проверка `tx.size() <= MAX_TX_SIZE` ДО верификации подписи.
5. Добавить варианты `SizeLimitExceeded(&'static str)`, `IoError` в `StrangecoinError`.
6. В логах при reject: указать размер и лимит.

**Артефакты:**
- `src/network/protocol.rs` (или `src/network/mod.rs` — пока)
- `src/error.rs` (новые варианты)
- `src/main.rs` (все `vec![0; length]` заменены, валидация размеров добавлена)

**КГ (чек-лист):**
- [ ] Тест: приём сообщения с length=10GB → reject без аллокации
- [ ] Тест: блок 5MB → reject (MAX_BLOCK_SIZE)
- [ ] Тест: tx 300KB → reject (MAX_TX_SIZE)
- [ ] Тест: блок 3MB → accept (если валиден по другим правилам)
- [ ] `rg "vec!\[0; .*length\]" src/` возвращает 0 (после замены)
- [ ] `cargo check` и `cargo test` проходят

---

### P13. P2P rate limiting per peer

**Цель:** реализовать инвариант #17 (rate limit per peer). Митигировать STRIDE векторы #14 (spam txs), #15 (invalid blocks), #17 (sybil).

**Контекст:** `ARCHITECT3.md §5` инвариант #17: «лимит сообщений/сек от одного пира; нарушение → бан». `ARCHITECT3.md §6` вектор #14: «DoS: spam txs».

**Задачи:**

1. Создать `src/network/rate_limiter.rs`:
   ```rust
   pub struct RateLimiter {
       window: Duration,
       max_messages_per_window: usize,
       peers: Mutex<HashMap<SocketAddr, PeerCounter>>,
   }
   struct PeerCounter {
       count: usize,
       window_start: Instant,
       banned_until: Option<Instant>,
   }
   impl RateLimiter {
       pub fn new(window_secs: u64, max: usize) -> Self { ... }
       pub fn check(&self, addr: SocketAddr) -> Result<(), StrangecoinError> {
           // инкремент; если > max → бан на N секунд
           // если banned → Err(PeerBanned)
       }
   }
   ```
2. В `Node::start_server` — при приёме сообщения от пира вызывать `rate_limiter.check(peer_addr)?`. При reject — закрыть соединение.
3. Параметры: 100 messages/10 sec per peer (default), бан на 5 минут при превышении.
4. Логировать баны через `tracing::warn!`.
5. В `Node::sync_blockchain` — учитывать rate limit при запросах (не слать больше N запросов в секунду).

**Артефакты:**
- `src/network/rate_limiter.rs`
- `src/network/mod.rs` (export RateLimiter)
- `src/main.rs` (Node использует rate_limiter)
- `src/error.rs` (вариант `PeerBanned`)

**КГ (чек-лист):**
- [ ] Тест: пир отправляет 200 сообщений за 1 сек → бан после 100-го
- [ ] Тест: забаненный пир не может отправить сообщение (reject сразу)
- [ ] Тест: через 5 минут бан снимается
- [ ] Тест: разные пиры имеют независимые счётчики
- [ ] `cargo check` и `cargo test` проходят

---

### P14. Mempool: валидация на insert + MAX_PENDING_TXS

**Цель:** реализовать инвариант #13 (`mempool.insert` проверяет подпись, dup, nonce, chain_id, balance) и #14 (DoS spam txs лимит).

**Контекст:** `ARCHITECT3.md §3.5`: «валидация подписи/dup/nonce/chain_id на insert (делегирует blockchain-методу); лимиты размера/количества (`MAX_PENDING_TXS`)». `ARCHITECT3.md §5` инвариант #13.

**Задачи:**

1. Создать `src/mempool/mod.rs`:
   ```rust
   pub const MAX_PENDING_TXS: usize = 10_000;

   pub struct Mempool {
       txs: HashMap<TxId, Transaction>,  // TxId = [u8; 32]
       by_sender: HashMap<String, BTreeMap<u64, TxId>>,  // sender → (nonce → txid)
   }
   impl Mempool {
       pub fn insert(&mut self, tx: Transaction, account_state: &AccountState) -> Result<(), StrangecoinError> {
           if self.txs.len() >= MAX_PENDING_TXS { return Err(StrangecoinError::MempoolFull); }
           let txid = compute_txid(&tx);
           if self.txs.contains_key(&txid) { return Err(StrangecoinError::DuplicateTx); }
           verify_transaction(&tx)?;
           if tx.chain_id != current_chain_id() { return Err(StrangecoinError::InvalidChainId); }
           if tx.nonce != account_state.nonce + 1 { return Err(StrangecoinError::InvalidNonce); }
           if tx.amount > account_state.balance { return Err(StrangecoinError::InsufficientBalance); }
           self.txs.insert(txid, tx.clone());
           self.by_sender.entry(tx.sender).or_default().insert(tx.nonce, txid);
           Ok(())
       }
       pub fn remove(&mut self, txid: &TxId) { ... }
       pub fn get_pending(&self, max_count: usize) -> Vec<Transaction> { ... }
   }
   ```
2. Заменить `pending_transactions: Vec<Transaction>` на `Mempool` в `Blockchain`.
3. После применения блока — удалять из mempool все tx, попавшие в блок (по txid).
4. `mempool.broadcast` через gossip (если есть сеть) — пока заглушка, реализация в Stage 2.
5. В `Node::start_server` — при получении TX от пира: `mempool.insert`, при успехе — ретранслировать.

**Артефакты:**
- `src/mempool/mod.rs`
- `src/main.rs` (Blockchain использует Mempool, не Vec)

**КГ (чек-лист):**
- [ ] Тест: tx с поддельной подписью → `Err(InvalidSignature)` при insert
- [ ] Тест: tx с дублирующим txid → `Err(DuplicateTx)`
- [ ] Тест: tx с `nonce != account.nonce + 1` → `Err(InvalidNonce)`
- [ ] Тест: tx с `chain_id != current_chain_id` → `Err(InvalidChainId)`
- [ ] Тест: tx с `amount > balance` → `Err(InsufficientBalance)`
- [ ] Тест: при `txs.len() == MAX_PENDING_TXS` → `Err(MempoolFull)`
- [ ] Тест: после применения блока все включённые tx удаляются из mempool
- [ ] `cargo check` и `cargo test` проходят

---

### P15. Унифицированный Config struct + секреты только в keystore

**Цель:** убрать рассеяние конфигурации (config.json + config.toml + секция `[wallet]` в Cargo.toml + `password` в config.json:4). Реализовать инвариант #9 (секреты не пишутся на диск в config).

**Контекст:** `ARCHITECT3.md §3.12`: «`config.toml` (единый формат, замена `config.json`+`config.toml`+`[wallet]` в `Cargo.toml`): только НЕсекретные параметры (порт, ip, name, node_mode, network_id, data_dir, log_level)». `ARCHITECT3.md §15` anti-goal: «Secret в config files».

**Задачи:**

1. Создать `src/config.rs`:
   ```rust
   #[derive(Deserialize, Serialize)]
   pub struct Config {
       pub network_id: u32,        // 1=mainnet, 2=testnet, 3=regtest
       pub node_mode: NodeMode,    // Full/Light/Archival
       pub network: NetworkConfig,
       pub storage: StorageConfig,
       pub log_level: String,
       pub data_dir: PathBuf,
   }
   #[derive(Deserialize, Serialize)]
   pub struct NetworkConfig {
       pub listen_addr: SocketAddr,
       pub seeds: Vec<SocketAddr>,
       pub max_peers: usize,
   }
   #[derive(Deserialize, Serialize)]
   pub struct StorageConfig {
       pub path: PathBuf,
   }
   impl Config {
       pub fn load(path: &Path) -> Result<Self, StrangecoinError> { ... }
       pub fn validate(&self) -> Result<(), StrangecoinError> { ... }
   }
   ```
2. Создать `config.toml` в корне (пример):
   ```toml
   network_id = 3            # regtest по умолчанию для разработки
   node_mode = "Full"
   log_level = "info"
   data_dir = "./data"

   [network]
   listen_addr = "127.0.0.1:8081"
   seeds = []
   max_peers = 50

   [storage]
   path = "./data/leveldb"
   ```
3. Удалить `config.json` (миграция: при первом старте конвертировать config.json → config.toml).
4. Удалить секцию `[wallet]` из `Cargo.toml` (если ещё не удалена в P01).
5. Пароль — только через env var `STRANGECOIN_WALLET_PASSWORD` или интерактивный prompt. Никогда в config.
6. Keystore secrets (private key) — только в `keystore/*.json` (зашифрованном). Config не ссылается на пароль.

**Артефакты:**
- `src/config.rs`
- `config.toml` (новый пример)
- `config.json` (удалён, при старте миграция)
- `Cargo.toml` (без секции `[wallet]`)
- `src/main.rs` (использует новый Config)

**КГ (чек-лист):**
- [ ] Тест: `Config::load("config.toml")` работает, поля корректно десериализуются
- [ ] Тест: `Config::validate()` отвергает кривой config (порт=0, network_id=99, etc.)
- [ ] Тест: `rg "password" config.toml` → 0 совпадений
- [ ] Тест: при наличии старого `config.json` — конвертация в `config.toml` и удаление `config.json`
- [ ] Тест: пароль через env var работает; без env var → интерактивный prompt
- [ ] `cargo check` и `cargo test` проходят

---

### P16. RwLock вместо Mutex<Blockchain> (где нужно чтение)

**Цель:** убрать bottleneck — `Mutex<Blockchain>` блокирует всё, включая чтение баланса. Реализовать scoped locks (короткие критические секции).

**Контекст:** `ROADMAP3 §Stage 0 Модуляризация`: «`RwLock` вместо `Mutex<Blockchain>` (где нужно чтение)». `ARCHITECT3.md §3.4`: `blockchain_facade.rs` владеет `RwLock<BlockchainInner>`.

**Задачи:**

1. В `src/main.rs` заменить:
   ```rust
   // было:
   blockchain: Arc<Mutex<Blockchain>>
   // стало:
   blockchain: Arc<RwLock<Blockchain>>
   ```
2. Все места `blockchain.lock().unwrap()` — заменить на:
   - `blockchain.read().unwrap()` для `get_balance`, `get_tip`, `get_block` (только чтение).
   - `blockchain.write().unwrap()` для `add_block`, `add_transaction`, `mine_block`.
3. Минимизировать критические секции: не держать write lock во время I/O (network, disk). Паттерн:
   ```rust
   let block = { let b = blockchain.read().unwrap(); b.get_tip()? };
   // network I/O без лока
   blockchain.write().unwrap().add_block(block)?;
   ```
4. Если в критической секции нужен и blockchain, и wallet — всегда одинаковый порядок блокировки (предотвратить deadlock). Документировать порядок в `error.rs` или в комментарии.
5. Никаких `Mutex<Blockchain>` в production paths. `Arc<RwLock<Blockchain>>` — единственный способ владения.

**Артефакты:**
- `src/main.rs` (Mutex → RwLock)
- `src/wallet.rs` (если нужно)
- Все `lock()` вызовы обновлены на `read()`/`write()`

**КГ (чек-лист):**
- [ ] `rg "Mutex<Blockchain>" src/` → 0 совпадений
- [ ] `rg "blockchain\.lock\(\)" src/` → 0 (только `read()`/`write()`)
- [ ] Тест: 100 параллельных читателей `get_balance` не блокируют друг друга
- [ ] Тест: writer эксклюзивен (параллельные writers сериализуются)
- [ ] Тест: нет deadlock (тест с 100 итераций: read blockchain + read wallet)
- [ ] `cargo check` и `cargo test` проходят

---

### P17. Graceful shutdown: Drop impls, SIGTERM/SIGINT, убрать ручное удаление LOCK

**Цель:** реализовать инвариант #14 (graceful shutdown, Drop для storage/wallet/network, нет «ручного удаления LOCK»). Реализовать сценарий `ARCHITECT3.md §4.7`.

**Контекст:** `ARCHITECT3.md §4.7`: «SIGTERM/SIGINT → shutdown_signal → network.stop → mining_worker.stop → sync_engine.flush → blockchain.flush → storage.flush → wallet.lock → process exit (без ручного удаления LOCK)». Текущий код: «ручное удаление LOCK» в `main.rs:188`.

**Задачи:**

1. Реализовать `Drop` для `Storage` (флаш LevelDB), `Wallet` (lock keystore), `Node` (закрыть все TCP-соединения).
2. Использовать `ctrlc` crate (уже в зависимостях) — установить handler, который выставляет `shutdown: Arc<AtomicBool>`.
3. Главный цикл: `while !shutdown.load() { ... }`. При сигнале — выйти из цикла, вызвать `drop` всех ресурсов, выйти чисто.
4. Удалить код «ручного удаления LOCK» (`main.rs:188`). LevelDB-LOCK должен удалиться автоматически через `Drop` для `DB`.
5. В mining thread — проверить `shutdown` в каждой итерации поиска nonce. При `shutdown=true` — выйти без save (или с save последнего состояния).
6. В sync thread — проверить `shutdown`, корректно закрыть TCP streams.
7. Добавить tracing-логи: `info!("shutdown signal received")`, `info!("mining stopped")`, `info!("storage flushed")`, `info!("bye")`.
8. Таймаут shutdown: 30 секунд. Если за 30 сек не вышло — panic с diagnostic.

**Артефакты:**
- `src/storage/mod.rs` (Drop impl для Storage)
- `src/wallet.rs` (Drop impl для Wallet — если нужно)
- `src/network/mod.rs` (Drop impl для Node)
- `src/main.rs` (ctrlc handler, main loop, удалён ручной LOCK)

**КГ (чек-лист):**
- [ ] Тест: SIGINT → узел корректно завершает работу за < 30 сек
- [ ] Тест: SIGTERM → аналогично
- [ ] Тест: после shutdown — нет orphan LOCK файлов (проверка в temp dir)
- [ ] Тест: после shutdown — все TCP-соединения закрыты (нет TIME_WAIT)
- [ ] Тест: mining thread выходит в течение 5 сек после shutdown
- [ ] `rg "remove.*LOCK\|delete.*LOCK\|fs::remove" src/main.rs` → 0 (кроме cleanup temp)
- [ ] Логи показывают порядок: shutdown → mining stopped → storage flushed → bye
- [ ] `cargo check` и `cargo test` проходят

---

### P18. Property-based тесты (proptest) для правил консенсуса

**Цель:** покрыть консенсусные правила property-based тестами. `ROADMAP3 §Stage 0 Тесты`: «Юнит-тесты (property-based: proptest) для правил консенсуса».

**Контекст:** Обычные юнит-тесты проверяют конкретные случаи. Property-based — генерируют сотни случаев автоматически, выявляют edge cases (overflow, граничные значения).

**Задачи:**

1. Добавить `proptest = "1"` в `Cargo.toml` [dev-dependencies].
2. Создать `src/consensus/proptest.rs` (или tests/):
   ```rust
   proptest! {
       #[test]
       fn txid_deterministic(tx in arbitrary_transaction()) {
           prop_assert_eq!(txid(&tx), txid(&tx));
       }
       #[test]
       fn signature_verification_roundtrip(seed in any::<u64>(), tx in arbitrary_transaction()) {
           let wallet = Wallet::random(seed);
           let mut tx = tx;
           wallet.sign_transaction(&mut tx).unwrap();
           prop_assert!(verify_transaction(&tx).is_ok());
       }
       #[test]
       fn block_reward_never_negative(height in 0u64..1_000_000, supply in 0u64..MAX_SUPPLY_PRE_TAIL) {
           prop_assert!(block_reward_at_height(height, supply) >= 0);
       }
       #[test]
       fn block_reward_at_halving(height in 0u64..1_000_000) {
           let r1 = block_reward_at_height(height, 0);
           let r2 = block_reward_at_height(height + HALVING_INTERVAL, 0);
           if r1 > 1 { prop_assert!(r2 == r1 / 2); }
       }
       #[test]
       fn serialize_deserialize_roundtrip(tx in arbitrary_transaction()) {
           let bytes = serialize_transaction(&tx);
           let tx2 = deserialize_transaction(&bytes).unwrap();
           prop_assert_eq!(tx, tx2);
       }
       #[test]
       fn nonce_reject(tx_nonce in 0u64..1_000_000, account_nonce in 0u64..1_000_000) {
           let result = validate_nonce(tx_nonce, account_nonce);
           prop_assert_eq!(result.is_ok(), tx_nonce == account_nonce + 1);
       }
       #[test]
       fn difficulty_target_clamp(prev_target in arbitrary_target(), timespan in 1u64..1_000_000) {
           let new_target = compute_target_after_timespan(prev_target, timespan);
           // factor 4 clamp
           prop_assert!(new_target <= prev_target * 4);
           prop_assert!(new_target >= prev_target / 4);
       }
   }
   ```
3. Вспомогательные `arbitrary_*` функции — генераторы для proptest.
4. Запустить `cargo test` — все property tests должны пройти (минимум 256 cases каждый по умолчанию).

**Артефакты:**
- `Cargo.toml` (proptest в dev-dependencies)
- `src/consensus/proptest.rs` или `tests/consensus_proptest.rs`

**КГ (чек-лист):**
- [ ] ≥7 property-тестов покрывают: txid determinism, sign/verify, emission formula, serialize roundtrip, nonce, difficulty clamp, chain_id
- [ ] Каждый property-тест генерирует ≥256 cases
- [ ] `cargo test` проходит без shrinking failures
- [ ] Если найден failing case — он воспроизводим (seed сохранён)
- [ ] `cargo check` и `cargo test` проходят

---

### P19. Интеграционные тесты: two_clients, reorg, double_spend, pow, emission, time, network

**Цель:** покрыть end-to-end сценарии. `ROADMAP3 §Stage 0 Тесты`: «Интеграционные тесты: `two_clients`, `reorg`, `double_spend`, `pow`, `emission`, `time`, `network`».

**Контекст:** Юнит-тесты проверяют модули. Интеграционные — взаимодействие нескольких компонентов (network + blockchain + wallet + storage).

**Задачи:**

Создать в `tests/`:

1. `tests/two_clients.rs` — два узла на regtest, один майнит блок → второй синхронизируется → балансы совпадают.
2. `tests/reorg.rs` — две цепочки A (height 10) и B (height 11), узел переключается на B при получении более длинной цепочки, корректно unapply блоки A и apply B.
3. `tests/double_spend.rs` — отправитель пытается дважды потратить один UTXO (один nonce, разные receiver) → вторая tx reject.
4. `tests/pow.rs` — майнинг на regtest с low difficulty (target = 0xFF...FF), блок находится за <1 сек, валиден.
5. `tests/emission.rs` — майним 10 блоков, проверяем что coinbase.amount = `block_reward_at_height(h, total_supply_before)`.
6. `tests/time.rs` — блок с `timestamp > now + 2h` reject; блок с `timestamp <= mtp` reject; валидный блок accept.
7. `tests/network.rs` — 3 узла, gossip TX от первого → второй и третий получают за <5 сек; TCP-соединения корректно закрываются при shutdown.

Для каждого теста:
- Использовать regtest (chain_id=3, fake difficulty).
- Использовать temp dir для storage (cleanup после теста).
- Использовать random ports (не конфликтуют с другими тестами).
- Никаких `sleep` > 1 сек (использовать `tracing::info!` + event channel для детерминированных тестов).

**Артефакты:**
- `tests/two_clients.rs`
- `tests/reorg.rs`
- `tests/double_spend.rs`
- `tests/pow.rs`
- `tests/emission.rs`
- `tests/time.rs`
- `tests/network.rs`

**КГ (чек-лист):**
- [ ] Все 7 интеграционных тестов проходят локально за <60 сек суммарно
- [ ] Каждый тест не оставляет orphan файлов (temp dir cleanup)
- [ ] Каждый тест не оставляет listening ports после завершения
- [ ] `cargo test --test two_clients` и пр. — каждый запускается изолированно
- [ ] `cargo test` (без фильтра) проходит полностью
- [ ] В тестах нет `sleep > 1 сек`

---

### P20. CI: GitHub Actions matrix (Linux/macOS/Windows) + clippy -D warnings

**Цель:** `ROADMAP3 §Stage 0 Тесты`: «CI: GitHub Actions, 3 платформы (Linux/macOS/Windows), `clippy -D warnings`».

**Контекст:** До этого момента CI нет. Strangecoin — desktop-приложение с GUI (eframe), должно собираться на всех трёх платформах.

**Задачи:**

1. Создать `.github/workflows/ci.yml`:
   ```yaml
   name: CI
   on: [push, pull_request]
   jobs:
     test:
       strategy:
         fail-fast: false
         matrix:
           os: [ubuntu-latest, macos-latest, windows-latest]
           rust: [stable, beta]
       runs-on: ${{ matrix.os }}
       steps:
         - uses: actions/checkout@v4
         - uses: dtolnay/rust-toolchain@stable
           with:
             components: clippy, rustfmt
         - run: cargo fmt -- --check
         - run: cargo clippy -- -D warnings
         - run: cargo test --all
         - run: cargo build --release
   ```
2. Добавить `clippy` lint rules в `Cargo.toml` или `clippy.toml` (если нужно).
3. Убедиться, что `cargo fmt --check` проходит (отформатировать весь код `cargo fmt`).
4. Добавить cache (Swatinem/rust-cache@v2) для ускорения.
5. Добавить job для coverage (необязательно: cargo-tarpaulin).

**Артефакты:**
- `.github/workflows/ci.yml`
- `.clippy.toml` (если нужно)
- `rustfmt.toml` (если нужно)

**КГ (чек-лист):**
- [ ] На push в PR запускается CI matrix 3 OS × 2 toolchain = 6 jobs
- [ ] Все jobs зелёные (после исправлений из P21)
- [ ] `cargo fmt --check` проходит
- [ ] `cargo clippy -- -D warnings` проходит (без warning)
- [ ] Cache работает (второй запуск быстрее первого)
- [ ] Локально `cargo fmt && cargo clippy -- -D warnings && cargo test` проходит

---

### P21. Устранение 20 warning'ов + мёртвого кода

**Цель:** `ROADMAP3 §Stage 0 Тесты`: «Устранение 20 warning'ов и мёртвого кода (`mining_thread`, `config.toml`, секция `[wallet]` в `Cargo.toml`)».

**Контекст:** `clippy -D warnings` требует 0 warnings. Это precondition для P20.

**Задачи:**

1. Запустить `cargo build 2>&1 | grep warning` — получить список всех warning'ов.
2. Классифицировать:
   - unused imports → удалить.
   - unused variables → переименовать в `_var` или удалить.
   - dead code → удалить (если не используется) или пометить `#[allow(dead_code)]` с TODO (если планируется Stage 1+).
   - `mining_thread` leftover → удалить (если P11 уже убрал, если нет — убрать здесь).
   - дублирование `config.toml` → удалить один из файлов.
3. Запустить `cargo clippy -- -D warnings` — исправить все clippy lint'ы.
4. Запустить `cargo fmt` — отформатировать весь код.
5. Проверить `cargo build --release` — нет warning'ов.

**Артефакты:**
- `src/main.rs` (очищен)
- `src/wallet.rs` (очищен)
- Все остальные .rs файлы (если нужно)
- Удалены: дубликат `config.toml`, секция `[wallet]` в Cargo.toml (если ещё)

**КГ (чек-лист):**
- [ ] `cargo build 2>&1 | grep -c warning` → 0
- [ ] `cargo clippy -- -D warnings` exit code 0
- [ ] `cargo build --release 2>&1 | grep -c warning` → 0
- [ ] `cargo fmt -- --check` exit code 0
- [ ] Все 6 CI jobs (из P20) зелёные

---

### P22. Threat Model (STRIDE) документ

**Цель:** `ROADMAP3 §Stage 0 Security`: «Threat model (STRIDE): `docs/security/THREAT_MODEL.md` с 25 векторами атак (см. `ARCHITECT3.md §6`)».

**Контекст:** `ARCHITECT3.md §6` уже содержит таблицу из 25 векторов. Нужно расширить её до полноценного документа с mitigations, residual risk, monitoring plan.

**Задачи:**

1. Создать `docs/security/THREAT_MODEL.md`:
   - Введение: scope, assumptions, trust boundaries.
   - STRIDE категории (Spoofing, Tampering, Repudiation, Information Disclosure, Denial of Service, Elevation of Privilege).
   - Таблица 25 векторов из `ARCHITECT3.md §6` (перенести + расширить):
     - ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk, Monitoring.
   - Дополнительно: векторы, специфичные для Stage 0 (например, «тестовая цепочка с low difficulty → 51% attack на testnet»).
   - Mitigations map: какие промпты Stage 0 (P0X) закрывают какие векторы.
   - Residual risks: что НЕ закрыто на Stage 0 (например, MEV — откладывается на Stage 5).
2. Создать `docs/security/INCIDENT_RESPONSE.md` — короткий план реагирования на инциденты (откуда получать alerts, кто responder, как disclosed publicly).
3. Все mitigation references должны указывать на конкретные промпты (P04, P07, P12 и т.д.) или ADR.

**Артефакты:**
- `docs/security/THREAT_MODEL.md` (25+ векторов, ~1500–2500 строк)
- `docs/security/INCIDENT_RESPONSE.md` (~100–200 строк)

**КГ (чек-лист):**
- [ ] Документ покрывает все 25 векторов из `ARCHITECT3.md §6`
- [ ] Каждый вектор имеет: ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk
- [ ] Mitigations ссылаются на конкретные промпты (P0X) или ADR
- [ ] Residual risks явно отмечены (что НЕ закрыто на Stage 0)
- [ ] Документ review'нут (минимум: self-review по чек-листу в §17 ARCHITECT3)
- [ ] Incident response план содержит: alerts source, responder role, disclosure timeline

---

### P23. TLA+ спецификация консенсуса (skeleton)

**Цель:** `ROADMAP3 §Stage 0 Security`: «TLA+ спецификация: `docs/spec/consensus.tla` (skeleton) — safety properties (no double-spend, no inflation), liveness (no deadlock)».

**Контекст:** `ARCHITECT3.md §3.15`: «`tla_spec/`: TLA+ specification of consensus rules». TLA+ skeleton на Stage 0 — это не полная formal verification, а фиксация свойств, которые обязаны выполняться.

**Задачи:**

1. Создать `docs/spec/consensus.tla`:
   ```tla
   ---- MODULE strangecoin_consensus ----
   EXTENDS Naturals, Sequences, FiniteSets

   CONSTANTS
       MaxSupplyPreTail,
       TailRate,
       HalvingInterval,
       MaxFutureTime,
       MedianTimeWindow

   VARIABLES
       chain,        \* Sequence of blocks
       balances,     \* Function: Address -> Balance
       nonces,       \* Function: Address -> Nonce
       mempool,
       time

   TypeInvariant ==
       /\ chain \in Seq(Block)
       /\ balances \in [Address -> Nat]
       /\ nonces \in [Address -> Nat]
       /\ mempool \subseteq Transaction

   \* Safety: no double-spend
   NoDoubleSpend ==
       \A tx \in UNION {b.txs \in chain}:
           (tx.sender, tx.nonce) \in processed_nonce

   \* Safety: no inflation
   NoInflation ==
       \A b \in chain:
           coinbase(b) = block_reward_at_height(b.index, total_supply_before(b))

   \* Safety: every tx signed and verified
   AllTxSigned ==
       \A b \in chain:
           \A tx \in b.txs:
               tx.is_coinbase \/ VerifySignature(tx)

   \* Safety: nonce strictly increasing per account
   NonceMonotonic ==
       \A b \in chain:
           \A tx \in b.txs:
               nonces[tx.sender] = tx.nonce - 1

   \* Safety: chain continuity (prev_hash)
   ChainContinuity ==
       \A i \in 1..Len(chain)-1:
           chain[i].prev_hash = hash(chain[i+1])

   \* Liveness: mining progresses (no deadlock if mempool non-empty)
   Liveness ==
       [](mempool # {} ~> <>(Len(chain) increases))

   Spec == Init /\ [][Next]_vars
   ==================================================================
   ```
2. Создать `docs/spec/consensus.cfg` — TLA+ config (constants, invariants).
3. Документировать в `docs/spec/README.md`:
   - Какие свойства проверены (safety + liveness).
   - Какие НЕ проверены (например, PoS-финализация — Stage 7+).
   - Инструкция запуска TLC model checker.
4. Опционально: запустить TLC на маленькой модели (3 узла, 10 блоков) — должны пройти.

**Артефакты:**
- `docs/spec/consensus.tla`
- `docs/spec/consensus.cfg`
- `docs/spec/README.md`

**КГ (чек-лист):**
- [ ] `consensus.tla` компилируется (TLA+ parser без ошибок)
- [ ] Spec содержит TypeInvariant, NoDoubleSpend, NoInflation, AllTxSigned, NonceMonotonic, ChainContinuity, Liveness
- [ ] Константы определены (MaxSupplyPreTail, HalvingInterval и др.)
- [ ] Variables соответствуют состоянию узла (chain, balances, nonces, mempool, time)
- [ ] `consensus.cfg` задаёт значения констант (можно запустить TLC)
- [ ] README объясняет как запустить TLC, какие свойства проверены/не проверены
- [ ] (Опционально) TLC passes на маленькой модели

---

### P24. Reproducible builds: cosign + SLSA provenance в CI

**Цель:** `ROADMAP3 §Stage 0 Security`: «Reproducible builds: CI gate с cosign/sigstore signatures, SLSA provenance». Реализовать инвариант #22.

**Контекст:** `ARCHITECT3.md §5` инвариант #22: «CI публикует SLSA provenance + cosign signature для каждого release». `ARCHITECT3.md §6` вектор #22: «Compromised build — Mitigations: reproducible builds (Stage 0); cosign/sigstore signatures; SLSA provenance».

**Задачи:**

1. Зафиксировать deterministic build:
   - Удалить пути с timestamps в бинарнике (если есть).
   - Зафиксировать `RUSTFLAGS="--remap-path-prefix $PWD=."`.
   - Зафиксировать version Cargo.lock (не обновлять транзитивные зависимости случайно).
2. Создать `.github/workflows/release.yml`:
   - На push tag `v*`:
     - Build release binaries (Linux/macOS/Windows × x86_64/aarch64).
     - Вычислить SHA256 каждого.
     - Загрузить binaries + checksums как release assets.
     - Запустить SLSA provenance generator (slsa-github-generator).
     - Подписать binaries с cosign (sigstore, keyless, OIDC).
     - Опубликовать provenance + signature как release assets.
3. Создать `docs/security/REPRODUCIBLE_BUILDS.md`:
   - Инструкция: как верифицировать бинарник локально.
   - `cosign verify-blob --certificate-identity=... --certificate-oidc-issuer=...`
4. Тестировать reproducibility: две сборки на разных машинах → одинаковые SHA256.

**Артефакты:**
- `.github/workflows/release.yml`
- `docs/security/REPRODUCIBLE_BUILDS.md`
- `Cargo.lock` (если нужно обновить)

**КГ (чек-лист):**
- [ ] При push tag `v0.9.0-test` запускается release pipeline
- [ ] Pipeline собирает binaries для 3 OS × 2 arch = 6 артефактов
- [ ] Каждый артефакт имеет SHA256 checksum
- [ ] SLSA provenance генерируется и публикуется
- [ ] cosign signature генерируется и публикуется (keyless, OIDC)
- [ ] Документ описывает как verifier локально: `cosign verify-blob ...`
- [ ] (Опционально) Две сборки на разных runners дают одинаковый SHA256

---

### P25. Bug bounty (Immunefi) + ADR-0003 (Hybrid PoW→PoS)

**Цель:** `ROADMAP3 §Stage 0 Security`: «Bug bounty program: Immunefi integration с day-1». ADR-0003: «Why hybrid PoW→PoS».

**Контекст:** `ARCHITECT3.md §3.15`: «`bounty_program.rs`: integration with Immunefi (Stage 6)» — но Stage 0 — это объявление программы. `ARCHITECT3.md §15` anti-goal: «PoW как финальный консенсус».

**Задачи:**

1. Написать `docs/ADR/0003-hybrid-pow-pos.md`:
   - Context: pure PoW (Bitcoin) vs pure PoS (Ethereum post-merge) vs hybrid.
   - Decision: hybrid PoW (first ~2 years) → PoS migration via activation height (Stage 7).
   - Consequences: PoW bootstraps distribution; PoS energy efficiency; PoS requires validator set + slashing.
   - Alternatives: pure PoW (отвергнут — energy concerns, 51% attack risk long-term), pure PoS from day-1 (отвергнут — distribution problem, nothing-at-stake bootstrapping).
2. Создать `docs/security/BOUNTY.md`:
   - Scope: что считается vulnerability (consensus bugs, fund loss, OOM, RCE).
   - Out of scope: social engineering, low-info reports.
   - Rewards: 3 tiers (low $1k, medium $10k, critical $100k).
   - Disclosure policy: 90-day coordinated disclosure.
   - Immunefi setup: ссылка на программу, или local setup если Immunefi ещё не подключён.
3. Создать `docs/security/SECURITY.md`:
   - Контакты для security reports (PGP key, email).
   - PGP public key (сгенерировать, добавить в repo).
   - SLA: response в течение 48 часов.
4. (Опционально) Настроить Immunefi программу — это требует человеческих действий, для AI-агента — только подготовить документацию.

**Артефакты:**
- `docs/ADR/0003-hybrid-pow-pos.md`
- `docs/security/BOUNTY.md`
- `docs/security/SECURITY.md`
- `docs/security/pgp_key.asc` (если генерируется)

**КГ (чек-лист):**
- [ ] ADR-0003 написан, ~100–200 строк, содержит Context/Decision/Consequences/Alternatives
- [ ] BOUNTY.md описывает scope, rewards, disclosure policy
- [ ] SECURITY.md содержит контакты, PGP key, SLA
- [ ] ADR-0003 объясняет почему не pure PoW и не pure PoS
- [ ] Все 5 ADR (0001–0005) теперь существуют и завершены

---

### P26. DoD verification: закрытие всех задач Stage 0

**Цель:** `ROADMAP3 §Stage 0 Definition of Done` — проверить что все критерии достигнуты. Это финальный промпт, он не пишет новый код, а верифицирует.

**Контекст:** `ROADMAP3 §Stage 0 Definition of Done` содержит 12 критериев. Все предыдущие промпты (P01–P25) закрывают подмножество. P26 — финальная проверка.

**Задачи:**

1. **Все 13 критических проблем из `ARCHITECT2.md` §1.1 закрыты:**
   - Сверить с `analytics/ARCHITECT2.md` §1.1 (если есть) — иначе с `analytics/ANALYSIS.md`.
   - Создать `docs/stage0/CRITICAL_ISSUES_CLOSED.md` — таблица: проблема → промпт → статус.
2. **Все 22 инварианта из `ARCHITECT3.md` §5 enforce:**
   - Создать `docs/stage0/INVARIANTS_ENFORCED.md` — таблица: инвариант → где enforce (модуль/функция) → тест, проверяющий.
   - Если инвариант НЕ enforce — создай follow-up issue.
3. **Threat model (STRIDE) документ написан и ревьюнут** — из P22.
4. **TLA+ skeleton спецификация консенсуса написана** — из P23.
5. **Reproducible builds в CI** — из P24.
6. **License файл в корне** — из P01.
7. **Все тесты проходят: `cargo test` green** — запустить, убедиться.
8. **`clippy -D warnings` green на 3 платформах** — из P20/P21.
9. **Bug bounty program активна на Immunefi** (или локально подготовлена) — из P25.
10. **ADR-0001–0005 написаны** — проверить существование всех файлов.
11. **`Changelog.md` обновлён:** `1.0.0 — sanitized prototype`.
12. **Создать `docs/stage0/STAGE0_SUMMARY.md`** — summary: что сделано, что осталось (Stage 1+).

**Артефакты:**
- `docs/stage0/CRITICAL_ISSUES_CLOSED.md`
- `docs/stage0/INVARIANTS_ENFORCED.md`
- `docs/stage0/STAGE0_SUMMARY.md`
- `Changelog.md` (обновлён: `1.0.0 — sanitized prototype`)

**КГ (чек-лист):**
- [ ] Все 13 критических проблем отмечены как closed (со ссылкой на промпт P0X)
- [ ] Все 22 инварианта отмечены как enforced (со ссылкой на модуль + тест)
- [ ] Если какие-то инварианты НЕ enforce — заведён issue с описанием gap
- [ ] `cargo test` проходит на 100%
- [ ] `cargo clippy -- -D warnings` проходит
- [ ] Все 5 ADR (0001–0005) существуют
- [ ] `Changelog.md` содержит `1.0.0 — sanitized prototype` запись
- [ ] STAGE0_SUMMARY.md описывает что готово и что переходит в Stage 1
- [ ] Repo можно tag'нуть как `v1.0.0-stage0` и переходить к Stage 1

---

## 3. Сводная таблица промптов

| ID  | Заголовок                                                       | Артефакты                                                 | Зависимости |
|-----|-----------------------------------------------------------------|-----------------------------------------------------------|-------------|
| P01 | LICENSE + ADR-0004/0005 + .gitignore                            | LICENSE, docs/ADR/, .gitignore, Cargo.toml               | —           |
| P02 | Скелет модулей + error.rs                                       | src/{blockchain,consensus,...}/mod.rs, src/error.rs       | P01         |
| P03 | Миграция на tracing                                             | src/main.rs, src/wallet.rs, Cargo.toml                   | P02         |
| P04 | secp256k1 wallet + ADR-0001                                      | docs/ADR/0001-*.md, src/wallet.rs, Cargo.toml            | P03         |
| P05 | chain_id + nonce + address_from_public_key                      | src/main.rs, src/consensus/mod.rs, src/address.rs        | P04         |
| P06 | Каноническая бинарная сериализация                              | src/serialize.rs, Cargo.toml                             | P05         |
| P07 | txid = commitment + verify_transaction                          | src/serialize.rs, src/consensus/mod.rs, src/main.rs      | P06         |
| P08 | difficulty validation + retargeting                              | src/consensus/mod.rs, src/main.rs                        | P07         |
| P09 | median-time-past + future timestamp                             | src/consensus/mod.rs, src/error.rs, src/main.rs         | P08         |
| P10 | Детерминированный genesis                                       | genesis.json, src/consensus/mod.rs, src/cli/mod.rs       | P09         |
| P11 | Tail emission + убрать лимиты майнинга + ADR-0002                | docs/ADR/0002-*.md, src/economics/emission.rs, src/main.rs | P10         |
| P12 | Лимиты размеров + length-prefixed framing                       | src/network/protocol.rs, src/error.rs, src/main.rs      | P11         |
| P13 | P2P rate limiting per peer                                      | src/network/rate_limiter.rs, src/main.rs                 | P12         |
| P14 | Mempool: insert с валидацией + MAX_PENDING_TXS                  | src/mempool/mod.rs, src/main.rs                          | P13         |
| P15 | Унифицированный Config + секреты только в keystore               | src/config.rs, config.toml, src/main.rs                  | P03         |
| P16 | RwLock вместо Mutex<Blockchain>                                 | src/main.rs                                              | P15         |
| P17 | Graceful shutdown + Drop impls                                  | src/{storage,wallet,network}/mod.rs, src/main.rs        | P16         |
| P18 | Property-based тесты (proptest)                                | tests/consensus_proptest.rs, Cargo.toml                  | P11, P07    |
| P19 | Интеграционные тесты (7 сценариев)                              | tests/{two_clients,reorg,double_spend,pow,emission,time,network}.rs | P18         |
| P20 | CI: GitHub Actions matrix (3 OS) + clippy -D warnings           | .github/workflows/ci.yml                                 | P19, P21    |
| P21 | Устранение 20 warning'ов + мёртвый код                         | src/*.rs (cleanup)                                       | P03         |
| P22 | Threat Model (STRIDE) документ                                  | docs/security/THREAT_MODEL.md, docs/security/INCIDENT_RESPONSE.md | P01         |
| P23 | TLA+ skeleton спецификация                                      | docs/spec/{consensus.tla,consensus.cfg,README.md}        | P01         |
| P24 | Reproducible builds (cosign + SLSA)                            | .github/workflows/release.yml, docs/security/REPRODUCIBLE_BUILDS.md | P20         |
| P25 | Bug bounty + ADR-0003 (hybrid PoW→PoS)                          | docs/ADR/0003-*.md, docs/security/{BOUNTY,SECURITY}.md   | P01         |
| P26 | DoD verification: закрытие всех задач Stage 0                   | docs/stage0/{CRITICAL_ISSUES_CLOSED,INVARIANTS_ENFORCED,STAGE0_SUMMARY}.md, Changelog.md | P01–P25     |

---

## 4. Покрытие задач Stage 0 (ROADMAP3.md)

| Категория из ROADMAP3 §Stage 0 | Покрыто промптами |
|-------------------------------|-------------------|
| Криптография                  | P04, P05, P07     |
| Сериализация                  | P06, P07          |
| Консенсус                     | P08, P09, P10     |
| Эмиссия                       | P11               |
| Лимиты и DoS-защита           | P12, P13          |
| Модуляризация (на монолите)   | P02, P03, P16     |
| Configuration management      | P15               |
| Mempool (basic)               | P14               |
| Graceful shutdown             | P17               |
| Тесты и CI                    | P18, P19, P20, P21 |
| Security & Documentation      | P01, P22, P23, P24, P25 |
| Definition of Done            | P26               |
| ADRs required (0001–0005)     | P01 (0004, 0005), P04 (0001), P11 (0002), P25 (0003) |

## 5. Покрытие 22 инвариантов (ARCHITECT3.md §5)

| # | Инвариант                                | Enforce в промпте | Тест в промпте |
|---|------------------------------------------|-------------------|----------------|
| 1  | Вся валидность — из цепочки              | P10, P11          | P19 (two_clients) |
| 2  | Каждая tx подписана, sender==pubkey      | P04, P07          | P07, P18       |
| 3  | hash <= target                           | P08               | P08, P18       |
| 4  | apply_block/unapply_block — обратные      | P17 (Drop)        | P19 (reorg)    |
| 5  | Хэш/подпись на канонических байтах        | P06, P07          | P06, P18       |
| 6  | Блок не содержит наград сверх эмиссии    | P11               | P11, P19 (emission) |
| 7  | Размеры ограничены до аллокации          | P12               | P12            |
| 8  | Генезис детерминирован                   | P10               | P10            |
| 9  | Секреты не пишутся на диск               | P15               | P15            |
| 10 | Replay protection (chain_id)             | P05               | P05            |
| 11 | Nonce строго инкрементируется            | P05               | P05, P18       |
| 12 | txid = commitment                         | P07               | P07, P18       |
| 13 | Mempool basic rules                      | P14               | P14            |
| 14 | Graceful shutdown                        | P17               | P17            |
| 15 | Block gas limit                          | (Stage 1.5+)      | —              |
| 16 | P2P framing с проверкой до аллокации     | P12               | P12            |
| 17 | Rate limiting per peer                   | P13               | P13            |
| 18 | Event log / Receipt                      | (Stage 1.5+)      | —              |
| 19 | State root match                         | (Stage 1)         | —              |
| 20 | Fee invariant                            | (Stage 5)         | —              |
| 21 | Consensus versioning                    | (Stage 1)         | —              |
| 22 | Reproducible builds                      | P24               | P24            |

Инварианты 15, 18, 19, 20, 21 — относятся к подсистемам, которые появляются на Stage 1+, и явно отмечены как deferred. На Stage 0 они N/A.

---

## 6. Примечания по использованию

1. **Порядок выполнения:** строго по карте зависимостей (§1). Параллельно можно запускать: security track (P22–P25) после P01; config track (P15–P17) после P03.
2. **Размер промпта:** каждый промпт спроектирован так, чтобы:
   - Контекст + задача + КГ помещались в ≤200 строк markdown.
   - Реализация (код + тесты) помещалась в контексте 32k-token агента (большинство современных: Claude 3.5, GPT-4o, Gemini 1.5).
   - Если промпт кажется слишком большим — разделить на a/b подзадачи (например, P19a = первые 3 теста, P19b = остальные 4).
3. **Контекст:** вставлять блок «Общий контекст» (§0) в начало каждого промпта при отправке агенту.
4. **Артефакты:** каждый промпт производит конкретные файлы — это делает прогресс измеримым. Прогресс = число созданных файлов / общее число ожидаемых.
5. **КГ обязательны:** нельзя закрывать промпт без прохождения всех пунктов КГ. Если КГ не проходит — итерировать промпт, не переходить к следующему.
6. **Связь с ARCHITECT3.md §17 чек-листом:** после P26 можно прогнать чек-лист из `ARCHITECT3.md §17` и убедиться, что все применимые к Stage 0 пункты отмечены.

---

## 7. Что НЕ входит в Stage 0 (явно отложено)

Следующие задачи появляются в `ROADMAP3.md` на Stage 1+, и их НЕ следует делать в рамках Stage 0:

- **Verkle Trie / state root** — Stage 1.
- **WASM смарт-контракты** — Stage 1.5.
- **Tokio / async runtime** — Stage 1 (на Stage 0 — threads + mpsc).
- **Bech32 addresses** — Stage 1 (на Stage 0 — `address_from_public_key` в hex).
- **Headers-first sync** — Stage 1 (на Stage 0 — current sync).
- **RocksDB migration** — Stage 3 (на Stage 0 — LevelDB).
- **Noise Protocol Framework** — Stage 2 (на Stage 0 — plaintext TCP).
- **Erlay / compact blocks** — Stage 2.
- **EIP-1559 fee market** — Stage 5 (на Stage 0 — fixed gas).
- **Account Abstraction** — Stage 5.
- **PoS миграция** — Stage 7.
- **TLA+ full formal verification** — Stage 6 (на Stage 0 — skeleton).
- **Fuzzing harnesses** — Stage 6 (на Stage 0 — только объявление в THREAT_MODEL.md).

Если промпт случайно затрагивает эти задачи — остановить и переориентировать на Stage 0 scope.

---

**Конец `prompt-stage0.md`.**
