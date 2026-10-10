# Аудит реализации Strangecoin — Stage 0 (P01–P26) и Stage 1 (D01–D03, S1-P01..S1-P22)

**Репозиторий:** https://github.com/Reider85/strangecoin
**Версия в Cargo.toml:** 1.1.0
**Дата аудита:** 2026-10-10
**Метод:** только по коду (`src/`, `crates/strangecoin-core/src/`, `tests/`, `docs/`, `.github/`, `git log`, `git tag -l`), НЕ по документационным заявлениям
**Масштаб:** ~11 074 строк Rust, 23 тест-файла, 11 ADR, 30 файлов документации, 1 коммит в git, 0 тегов

---

## Сводный вердикт

| Этап | Всего пунктов | ✅ Fully | ⚠️ Partial | ❌ Missing |
|---|---|---|---|---|
| Stage 0 (P01–P26) | 26 | 18 | 8 | 0 |
| Stage 1 (D01–D03 + S1-P01..S1-P22) | 25 | 22 | 3 | 0 |
| **Итого** | **51** | **40 (78%)** | **11 (22%)** | **0** |

**Зрелость:** кодовая — TRL 4–5 (высокая), продакшн — TRL 2 (mainnet не готов).

---

## Stage 0 — детальный аудит (P01–P26)

### P01. Подготовка репозитория: LICENSE, ADR-0004, ADR-0005, .gitignore, branch protection — ✅ Fully
- `LICENSE` (root, 1-204): MIT + Apache-2.0 ✓
- `docs/ADR/0001-template.md` (1-46): Status/Context/Decision/Consequences/Alternatives ✓
- `docs/ADR/0004-license.md` (66 строк) ✓
- `docs/ADR/0005-tracing-vs-println.md` (93 строки) ✓
- `docs/CONTRIBUTING.md` ссылается на ADR process ✓
- `Cargo.toml`: секции `[wallet]` нет ✓
- **⚠️ Minor:** `.gitignore` использует `/target` (не `target/`) и **не содержит `*.lock`** — явное требование КГ не выполнено

### P02. Скелет модулей + error.rs + thiserror — ✅ Fully
- 10 подмодулей: `src/{blockchain,consensus,network,mempool,storage,api,cli,gui,economics,governance}/mod.rs` ✓
- `mod`-блок в `src/lib.rs:17-32` (не в `main.rs`, функциональный эквивалент)
- `src/error.rs:19-73`: `StrangecoinError` enum с **25+ вариантами** (требовалось ≥6) — `InvalidSignature`, `InvalidNonce`, `InvalidChainId`, `InvalidDifficulty`, `SizeLimitExceeded`, `TimestampTooOld`, `TimestampInFuture`, `GenesisMismatch`, etc.
- `thiserror = "1"` в `Cargo.toml:52` и `crates/strangecoin-core/Cargo.toml:11` ✓

### P03. Миграция логирования на tracing — ✅ Fully
- `rg "println!"` по `src/` и `crates/strangecoin-core/src/` → **0 совпадений** ✓
- `tracing` и `tracing-subscriber` (env-filter+fmt) в `Cargo.toml:55-56` ✓
- Subscriber init в `src/lib.rs:991-994` внутри `run_async()` ✓
- Structured fields далеко за 5 точками: `info!(peer=%address, duration_secs, "Пир добавлен")`, `info!(mining_count, duration_secs, result=?result, "Майнинг завершен")`, etc.

### P04. Миграция кошелька на secp256k1 (ECDSA) — ✅ Fully (test gap)
- `docs/ADR/0001-secp256k1-vs-ed25519.md` (63 строки) ✓
- `secp256k1 = "0.29"` с features `rand, serde, global-context, recovery` (recovery для P07) ✓
- `ed25519-dalek` убран из Cargo.toml ✓
- `src/wallet.rs` переписан: `SecretKey`/`PublicKey`, `Keystore { public_key, encrypted_private_key, salt, nonce, version }`, PBKDF2+AES-256-GCM, sign возвращает `[u8;65]` recoverable ECDSA
- **⚠️ Test gap:** нет round-trip теста `Wallet::new → save → load → sign/verify`

### P05. Replay protection: chain_id, nonce, address_from_public_key — ✅ Fully
- `crates/strangecoin-core/src/types.rs:79-92`: `Transaction` имеет `nonce, chain_id, signature, is_coinbase`, без поля `id`; `#[serde(default)]` на новые поля ✓
- `CHAIN_ID_MAINNET=1`, `CHAIN_ID_TESTNET=2`, `CHAIN_ID_REGTEST=3` (`consensus.rs:5-7`) ✓
- `validate_nonce` с overflow-проверкой (`consensus.rs:300-311`) ✓
- Mempool отвергает неверный `chain_id` (`mempool/mod.rs:92-97`) и неверный nonce (`:160-163`) ✓
- `AccountState { balance: u64, nonce: u64 }` (`types.rs:73-77`) ✓
- **⚠️ Test gap:** нет явного теста `Err(InvalidChainId)` на cross-chain tx; `chain_id_validation` proptest таутологичен
- **Примечание:** `address_from_public_key` уже использует bech32 (это Stage 1 работа, сверх Stage 0 требований)

### P06. Каноническая бинарная сериализация (blake3) — ⚠️ Partial
- `crates/strangecoin-core/src/serialize.rs` (426 строк) ✓
- `serialize_transaction`: `FORMAT_VERSION` + length-prefixed strings + BE numbers, подпись исключена ✓
- `serialize_block_header` и `serialize_block` ✓
- `blake3 = "1"` в `Cargo.toml:54` и core ✓
- Golden-векторы для **3 транзакций** в `serialize_golden.rs:5-162` сравнивают с literal hex ✓
- **⚠️ Критический дефект:** golden-векторы **для блоков таутологичны** — `serialize_golden.rs:201, 243, 329` сравнивают `actual_hex == actual_hex.clone()` и никогда не падают
- **⚠️ Мёртвая зависимость:** `sha2 = "0.10"` в `crates/strangecoin-core/Cargo.toml:13` не импортируется нигде (PoW использует blake3)

### P07. txid = commitment + полная верификация подписи — ✅ Fully
- `serialize_transaction_signed`, `txid`, `block_hash` в `serialize.rs:19-28, 145-147` ✓
- Поля `tx.id` нет нигде в коде ✓
- `sign_transaction` подписывает `blake3(serialize_transaction(tx))` без подписи ✓
- `recover_pubkey_from_sig` через `RecoverableSignature::from_compact` + `recover_ecdsa` (`consensus.rs:274-295`) ✓
- `verify_transaction` в `consensus.rs:313-323` ✓
- Вызывается в `mempool::insert` и `validate_and_apply` ✓
- `merge_pending_transactions` отсутствует ✓
- **⚠️ Test gap:** нет явного теста «изменить amount после подписи → reject»

### P08. Валидация difficulty + sliding-window retarget — ✅ Fully
- `Block.target: String` (`types.rs:13`) ✓
- Константы `MEDIAN_TIME_WINDOW=11`, `MAX_FUTURE_TIME=7200`, `RETARGET_INTERVAL=2016`, `TARGET_BLOCK_TIME=600`, `MAX_TARGET_CHANGE_FACTOR=4` ✓
- `compute_target` (`consensus.rs:202-232`): sliding window last `RETARGET_INTERVAL`, clamp `[prev/4, prev*4]` ✓
- `validate_difficulty`: u256 comparison `hash <= target` ✓
- `validate_target` (`block_executor.rs:186-217`): retarget на высотах, кратных 2016 ✓
- `mine_block_inner` (`block_executor.rs:445-513`): u256 `hash <= target` loop ✓
- Тесты: `rejects_proof_of_work_above_target`, `rejects_target_changed_outside_a_retarget_height`, `accepts_the_target_computed_at_a_retarget_height`, proptest `difficulty_target_clamp` ✓
- **⚠️ Test gap:** нет теста, строящего chain длиной 2016 блоков и сравнивающего `compute_target` с эталоном

### P09. Median-time-past + запрет future timestamp — ✅ Fully
- `MEDIAN_TIME_WINDOW=11`, `MAX_FUTURE_TIME=7200` ✓
- `median_time_past` (`consensus.rs:175-186`): окно 11, медиана ✓
- `validate_timestamp` (`consensus.rs:188-200`): rejects `<= mtp` и `> now + 2h` ✓
- `mine_block_inner`: `timestamp: now.max(mtp + 1)` ✓
- Тесты: `reject_block_timestamp_too_far_future`, `accept_valid_timestamp`, `reject_block_before_mtp`, `rejects_timestamp_not_after_median_time_past` ✓

### P10. Детерминированный генезис — ✅ Fully (dormant mainnet)
- `genesis.json` (валидный, `network_id=1, chain_id=1, initial_holder_pubkey, initial_amount=1_000_000_000, tail_emission_rate=0.006, max_supply_pre_tail=21M, target_block_time=600, retarget_interval=2016, genesis_hash`) ✓
- `load_genesis` (`consensus/mod.rs:23-75`) с `GenesisNetworkMismatch` если `network_id != chain_id` ✓
- Сравнение с `EXPECTED_GENESIS_HASH` ✓
- `--print-genesis-hash` в `cli/mod.rs:11-27` ✓
- regtest генерирует свой genesis (`block_executor.rs:520-549`) ✓
- Тест `tests/genesis_key.rs` (5 тестов) ✓
- **⚠️ Критический дефект:** `current_chain_id()` **захардкожен в `CHAIN_ID_REGTEST`** (`crates/strangecoin-core/src/consensus.rs:171-173`). На дефолтном старте mainnet-ветка (с `EXPECTED_GENESIS_HASH`) — **мёртвый код**. Все тесты проходят на regtest; mainnet/testnet структурно не протестированы.

### P11. Tail emission + убрать искусственные лимиты майнинга (ADR-0002) — ✅ Fully
- `docs/ADR/0002-tail-emission-vs-halving.md` (71 строка, все секции) ✓
- `crates/strangecoin-core/src/economics/emission.rs` с halving + tail ~0.6%/год ✓
- Тесты `block_reward_at_height_for_chain(0, 0, MAINNET) == 50 * COIN`, `== 25 * COIN` на halving, `test_tail_emission_activates` ✓
- `MAX_SUPPLY_PRE_TAIL=21M * COIN` — мягкий cap, не жёсткий ✓
- Майнинг `block_executor.rs:486-512` без iter cap и без sleep ✓
- Проверка `coinbase.amount > expected` → `InvalidCoinbaseAmount` (`inner.rs:84-89`) ✓

### P12. Лимиты размеров + length-prefixed framing ДО аллокации — ⚠️ Partial
- `MAX_BLOCK_SIZE=4MB`, `MAX_TX_SIZE=256KB` (`protocol.rs:10-11`) ✓
- `read_length_prefixed` проверяет длину ДО `vec![0; len]` (`protocol.rs:250-270`, 289-311) ✓
- `parse_blocks` проверяет `len > MAX_BLOCK_SIZE` per item ✓
- 10 unit-тестов framing ✓
- **⚠️ Критический дефект:** `validate_and_apply` (`block_executor.rs:85-149`) и `commit_block` (`:366-381`) **НЕ проверяют размер блока** до применения. Проверка только на wire-уровне и в `validate_chain` (whole-chain scan). Блок, приходящий через `adopt_candidate` из `ChainSnapshot`, обходит размерный инвариант на validation-уровне.

### P13. P2P rate limiting per peer — ⚠️ Partial
- `src/network/rate_limiter.rs`: `RateLimiter::new(window, max)`, default `(10, 100)` ✓
- Бан 5 минут (`Duration::from_secs(300)`) ✓
- 4 теста: `test_rate_limit_exceeded`, `test_banned_peer_rejected`, `test_ban_expires`, `test_independent_peers` ✓
- **⚠️ Критический дефект:** `rate_limiter.check()` вызывается **один раз на TCP-коннект** в `handle_connection` (`lib.rs:571`), **НЕ на каждое сообщение** в request-loop (`lib.rs:620-732`). Long-lived peer может стримить много запросов без per-message throttling. Реальная защита от спама — bounded SyncEngine inbox (64/256 cap) + бан на overflow, не per-message token bucket.

### P14. Mempool: валидация на insert + MAX_PENDING_TXS — ✅ Fully (с RBF extension)
- `MAX_PENDING_TXS=10000` (`mod.rs:10`) ✓
- `verify_transaction` на insert (`mod.rs:90`) ✓
- `DuplicateTx`, `MempoolFull`, `InvalidNonce`, `InvalidChainId`, `InsufficientBalance` — типизированные ✓
- Block-applied txs удаляются (`block_executor.rs:373-377`) ✓
- Доп. `by_sender`, `generations` для Stage 1 RBF ✓

### P15. Унифицированный Config struct + секреты только в keystore — ✅ Fully
- `src/config.rs`: `Config { network_id, node_mode, network, storage, log_level, data_dir, allow_grant_blocks }`, TOML ✓
- `Config::validate()` rejects bad `network_id` (not in {1,2,3}), `max_peers==0`, empty paths ✓
- Миграция `config.json → config.toml` (`lib.rs:1012-1052`) ✓
- Пароль через env `STRANGECOIN_WALLET_PASSWORD` или interactive prompt ✓
- Секреты только в `keystore/*.json` (PBKDF2+AES-256-GCM) ✓
- **⚠️ Test gap:** нет unit-тестов `Config::load`/`Config::validate`

### P16. RwLock вместо Mutex<Blockchain> — ✅ Fully
- `Arc<RwLock<Blockchain>>` в `BlockchainFacade` (`blockchain_facade.rs:71`) ✓
- `rg "Mutex<Blockchain>" src/` → 0 ✓
- `rg "blockchain\.lock\(\)" src/` → 0 ✓
- `tests/concurrency.rs::deadlock_test_blockchain_wallet_lock_order` (100 потоков) ✓
- Lock ordering документирован в `src/error.rs:1-15` ✓
- **Примечание:** `Storage` всё ещё `Arc<Mutex<DB>>` — вне scope P16

### P17. Graceful shutdown — ✅ Fully
- `tokio::signal::ctrl_c()` (`lib.rs:1209-1226`) + `ctrlc::set_handler` fallback (`:1231-1246`) ✓
- `Arc<AtomicBool>` shutdown flag, 30s budget (`SHUTDOWN_TIMEOUT`) ✓
- Майнинг проверяет флаг каждый nonce iteration ✓
- `Drop for Storage` flushes LevelDB; manual LOCK removal не нужен ✓
- `Drop for Node` aborts accept_task + sync_task ✓
- **⚠️ Девиация:** вместо `panic!` на timeout — `error!` + `std::process::exit(0)` (эквивалент для оператора, но не буквально по спеке)
- **⚠️ Minor:** `secp256k1` не включает `zeroize` feature — приватный ключ не zerolized в Drop

### P18. Property-based тесты (proptest) — ✅ Fully
- `proptest = "1"` в `[dev-dependencies]` ✓
- 9 proptest в `consensus_proptest.rs` (txid_deterministic, signature_verification_roundtrip, block_reward_bounded_by_schedule, block_reward_at_halving, serialize_deserialize_roundtrip, nonce_reject, difficulty_target_clamp, chain_id_validation, u256_arithmetic_roundtrip) ✓
- Доп. 13 в других файлах = **22 property-теста** всего (chain_selector, state_roundtrip, witness) ✓
- Сохранены regression-семена в `.proptest-regressions` ✓

### P19. Интеграционные тесты: 7 сценариев — ⚠️ Partial
- 7 файлов + доп.: `two_clients.rs`, `reorg.rs`, `double_spend.rs`, `pow.rs`, `emission.rs`, `time.rs`, `network.rs`, `concurrency.rs` ✓
- `TestDir` с `Drop` (cleanup) ✓
- `sleep ≤ 300ms` (нет `> 1s`) ✓
- `main.rs` 4 строки — inline `#[test]` нет ✓
- **⚠️ `two_clients.rs` использует 5 кошельков** (промпт просил 2 — есть `bech32_address_transfer` с 2)
- **⚠️ `pow.rs` assert `elapsed < 5s`** (промпт просил <1s — в 5x слабее)
- **⚠️ `network.rs::three_instances_receive_transfer`** использует `adopt_from` (прямой коп chain), не реальный gossip

### P20. CI: GitHub Actions matrix + clippy -D warnings — ✅ Fully
- `.github/workflows/ci.yml`: matrix `[ubuntu-latest, macos-latest, windows-latest] × [stable, beta]`, `fail-fast: false` ✓
- `cargo fmt --check` ✓
- `cargo clippy -- -D warnings` ✓
- `Swatinem/rust-cache@v2` ✓
- Доп. coverage job (cargo-tarpaulin) ✓
- Доп. fuzz 600s job (`fuzz-canonical-decode`) ✓

### P21. Устранение 20 warnings + dead code — ⚠️ Partial
- Crate-level `#![allow]` нет ✓
- `mining_thread`, `[wallet]` секции, дубликат `config.toml` — отсутствуют ✓
- **⚠️ 6 `#[allow(dead_code)]` бандажей** в `tests/common/mod.rs` (5, 29, 40, 51, 68) и `crates/strangecoin-core/tests/state_roundtrip.rs:6` — **без TODO-комментариев**, что P21 явно запрещал
- **⚠️ 6 `#[allow(clippy::await_holding_lock)]`** в `tests/network_id.rs:28`, `tests/sync_headers.rs:24,86,133`, `tests/network.rs:69,169`, `tests/events.rs:18`

### P22. Threat Model (STRIDE) — ✅ Fully (произведён в D02, не Stage 0)
- `docs/security/THREAT_MODEL.md` 1057 строк, **42 вектора** (V-01..V-42), все 25 из ARCHITECT3 §6 + Stage 1 surface ✓
- Каждый вектор: ID, Name, STRIDE, Description, Mitigation, Stage, Residual Risk, Monitoring ✓
- §6.1 Prompt → Vector mapping ✓
- `docs/security/INCIDENT_RESPONSE.md` (285 строк) ✓
- **⚠️ Честное признание:** `Changelog.md:235` явно пишет «P22: Threat Model (STRIDE) — NOT COMPLETED IN STAGE 0... P22 is executed in D02 (debt prompt)» — формально Stage 0 DoD пункт выполнен пост-Stage 0

### P23. TLA+ спецификация — ✅ Fully
- `docs/spec/consensus.tla` (209 строк): `EXTENDS Naturals, Sequences`, 14 CONSTANTS, VARIABLES, Init, Next, Spec, THEOREM ✓
- Invariants: `TypeInvariant`, `SupplyConsistency`, `NoDoubleSpend`, `NoInflation`, `AllTxSigned`, `NonceMonotonic`, `ChainContinuity`, `PowValidity`, `ChainIdConsistency`, `Liveness` ✓
- `docs/spec/consensus.cfg` (значения констант) ✓
- `docs/spec/README.md`: **реально model-checkeded TLC 2.19**: 35 207 states, 9/9 invariants PASS, 1/1 temporal property PASS, ~6s, 2026-10-09 ✓
- Предыдущая «TLC PASS 2026-09-28 v1.8.0» честно помечена как superseded/невоспроизводимая

### P24. Reproducible builds: cosign + SLSA — ⚠️ Partial
- `.github/workflows/release.yml`: triggers on `tags: ['v*']`, 6 matrix targets (linux/macOS × x86_64/aarch64 + windows) ✓
- SHA256 checksums ✓
- SLSA3 provenance: `slsa-framework/slsa-github-generator@v2.1.0` ✓
- cosign keyless OIDC: `sigstore/cosign-installer@v3.7.0` + `cosign sign-blob --yes` ✓
- `docs/security/REPRODUCIBLE_BUILDS.md` с verify-инструкциями ✓
- **⚠️ Критический дефект:** pipeline **НИКОГДА не запускался на реальном `v*` теге** — `git tag -l` пуст. THREAT_MODEL V-33 явно это признаёт. Cosign+SLSA artifacts существуют только на бумаге.

### P25. Bug bounty (Immunefi) + ADR-0003 — ⚠️ Partial
- `docs/ADR/0003-hybrid-pow-pos.md` (131 строка, 3 альтернативы) ✓
- `docs/security/BOUNTY.md` (127 строк): Scope, Reward Tiers, Disclosure Policy (90 дней) ✓
- `docs/security/SECURITY.md` (129 строк): Contacts, Response SLA (48h ack) ✓
- **⚠️ PGP-ключ — placeholder-текст** (`SECURITY.md:25`), файла `pgp_key.asc` нет
- **⚠️ 4 tier'а наград** ($500/$1k/$10k/$100k) — промпт просил 3 ($1k/$10k/$100k)
- **⚠️ Immunefi не настроен** (`BOUNTY.md:93` явно признаёт)

### P26. DoD verification Stage 0 — ⚠️ Partial
- `docs/stage0/STAGE0_SUMMARY.md` (109 строк) ✓
- `docs/stage0/INVARIANTS_ENFORCED.md` (66 строк) ✓
- `docs/stage0/CRITICAL_ISSUES_CLOSED.md` (42 строки) ✓
- **⚠️ Внутренние арифметические расхождения** в сводках:
  - INVARIANTS: header «4 Deferred, 1 N/A» → фактически **5 Deferred, 0 N/A**
  - CRITICAL_ISSUES: header «10 closed, 3 partially» → фактически **11+1 closed, 3 partial**
  - STAGE0_SUMMARY: «9/13 fully, 3 partially, 1 deferred» — расходится с CRITICAL_ISSUES_CLOSED
- **⚠️ P22 формально закрыт пост-Stage 0** (D02 debt)
- **⚠️ P24 release pipeline** не верифицирован
- **⚠️ PGP-ключ** не сгенерирован
- **⚠️ Immunefi** не настроен

---

## Stage 1 — детальный аудит (D01–D03, S1-P01..S1-P22)

### D01. Закрытие долга P19: tests/ каталог + перенос тестов из main.rs — ✅ Fully
- 8 файлов (требовалось 7): `two_clients.rs`(188), `reorg.rs`(168), `double_spend.rs`(58), `pow.rs`(49), `emission.rs`(115), `time.rs`(112), `network.rs`(281), `concurrency.rs`(43) ✓
- `src/main.rs` — 4 строки, inline `#[test]` нет ✓
- `tests/common/mod.rs` (97 строк): `TestDir`, `wait_for_event`, `walk_state`, `coinbase_tx`, `craft_child` ✓
- `sleep ≤ 300ms` ✓
- `TestDir::Drop` cleanup ✓

### D02. Закрытие долга P22: THREAT_MODEL + INCIDENT_RESPONSE + правда в Changelog — ✅ Fully
- THREAT_MODEL 1057 строк, 42 вектора (V-01..V-42), каждый с Mitigation/Residual/Monitoring ✓
- 4+ retro-векторов: V-26 (testnet low-diff), V-31 (genesis key), V-32 (grant blocks), V-33 (release pipeline) ✓
- Changelog честно помечает «P22 NOT COMPLETED IN STAGE 0» ✓
- Предыдущая «TLC PASS 2026-09-28» честно помечена как superseded (BUG-S0-026) ✓
- `INCIDENT_RESPONSE.md` (285 строк): 4h triage SLA, 48h ack, 90-day disclosure ✓

### D03. Закрытие долга P26: DoD-verification Stage 0 + тег v1.0.0-stage0 — ⚠️ Partial
- Артефакты в `docs/stage0/` присутствуют ✓
- 13 critical issues table, 22 invariants table ✓
- STAGE0_SUMMARY.md §"Explicit Obligations for Stage 1" ✓
- **❌ КРИТИЧНО:** `git tag -l` → **пусто.** Тега `v1.0.0-stage0` **не существует**
- STAGE1_SUMMARY.md:42 ссылается на `v1.0.0-stage0 @ 3dd37ef` — этот коммит **не в HEAD**

### S1-P01. Консенсусная санация: grant-механизм за regtest-флагом — ✅ Fully
- `BlockView.allow_grant_blocks` + `Config.allow_grant_blocks` (default false, `config.rs:24`) ✓
- `block_executor.rs:127`: `let is_opt_in_grant_block = view.allow_grant_blocks && block.index == GRANT_BLOCK_INDEX;` — цикл подписей выполняется для всех tx когда не opt-in ✓
- `grant_initial_balance_to_first_wallet` возвращает `Err(GrantBlocksDisabled)` (`block_executor.rs:342-345`) ✓
- 4 теста в `tests/grant_flag.rs` ✓

### S1-P02. Cargo workspace + crates/strangecoin-core: serialize + ADR-0008 — ✅ Fully
- `[workspace] members = [".", "crates/strangecoin-core"]`, `resolver = "2"` ✓
- `serialize.rs` перенесён в core; в root `src/serialize.rs` НЕТ ✓
- `rg "tokio|std::net|std::fs|leveldb" crates/strangecoin-core/src` → **0** (0 I/O в core) ✓
- `docs/ADR/0008-rocksdb-plan.md` с 3 альтернативами (redb, sled, keep leveldb) ✓
- `economics/fee_market.rs` stub для Stage 5 ✓

### S1-P03. Перенос consensus + economics в core — ✅ Fully
- Все `pub const` в одном модуле `consensus.rs:5-26` ✓
- 0 I/O в core ✓
- Proptest перенесены в `crates/strangecoin-core/tests/consensus_proptest.rs` ✓
- Re-exports в root `src/consensus/mod.rs` и `src/economics/mod.rs` ✓

### S1-P04. state.rs: чистое apply_block/unapply_block — ✅ Fully
- `crates/strangecoin-core/src/state/inner.rs:54` `apply_block` (pure, no RwLock/DB) ✓
- `:136` `unapply_block` ✓
- 0 I/O ✓
- Proptest `apply_unapply_roundtrip` в `state_roundtrip.rs` ✓
- `prune_if_empty` для точности round-trip ✓

### S1-P05. governance: SCIP skeleton + consensus_version + activation height — ✅ Fully
- `crates/strangecoin-core/src/governance/scip.rs` (116 строк): `ScipDocument`, `ConsensusRules { consensus_version, activations: BTreeMap<Height, u32> }`, `current_consensus_rules` ✓
- 0 I/O ✓
- `Block.consensus_version: u32` ✓
- `FORMAT_VERSION = 4` ✓
- `tests/consensus_version.rs`: stale_consensus_version_rejected, future_consensus_version_rejected, correct_consensus_version_accepted ✓
- 5 unit-тестов scip на activation boundaries ✓
- `docs/SCIP/` (3 документа: README, scip-0000-process, scip-0001-genesis-key-replacement) ✓

### S1-P06. ADR-0006 + Verkle Trie + state_root в заголовке — ⚠️ Partial
- `Block.state_root: [u8;32]` ✓
- `serialize.rs:49` пишет state_root; `FORMAT_VERSION = 4`; golden vectors обновлены ✓
- `tests/state_root.rs:67` `tampered_state_root_is_rejected_by_the_second_node` ✓
- Proptest `proptest_root_consistent` ✓
- **⚠️ КРИТИЧНО:** исходный `state/verkle.rs` был **плоской `[256]`-slot array** (BUG-S0-011), не trie. Файл **удалён**, заменён на `state/sparse_merkle.rs` (бинарный SMT, depth 256, blake3(address), 8 KiB proofs). ADR-0006 **честно исправлен** 2026-10-08. **Verkle Trie как такового нет** (KZG/BLS12-381 отложены на Stage 3+). Stage 1 DoD пункт «Verkle Trie» **технически не выполнен**.
- **⚠️ BUG-S0-012:** `block_executor.rs:136` и `state/mod.rs:20` **пропускают проверку** при `block.state_root == [0; 32]` — opt-in residual

### S1-P07. StateWitness + stateless validation API — ✅ Fully (с residual)
- `crates/strangecoin-core/src/state/witness.rs` (118 строк): `StateWitness { pre_state_root, proofs: HashMap<String, AccountProof> }`, `build_witness`, `verify_block_stateless` ✓
- 0 I/O ✓
- Proptest `proptest_tamper_detected`, `wrong_parent_root_rejected`, `witness_contains_only_touched_addresses`, `tampered_balance_rejected` ✓
- **⚠️ BUG-S0-013 (residual):** `verify_block_stateless` НЕ пересчитывает post-state root из witness — только проверяет pre-state proofs и применяет блок. Честно документировано в `witness.rs:81-86` comment.

### S1-P08. Merkle root транзакций — ✅ Fully
- `serialize.rs:107-135` `merkle_root`: odd-node duplication ✓
- `validate_tx_root` в `consensus.rs:330` возвращает `TxRootMismatch` ✓
- Тесты `merkle.rs`: `empty_txids_returns_zero`, `single_txid_duplicated`, `two_txids_pair_hash`, `proptest_deterministic_root` ✓
- Тест `rejects_wrong_tx_root` ✓

### S1-P09. ADR-0009 + EventBus — ✅ Fully
- `docs/ADR/0009-events-bus.md`: crossbeam_channel::unbounded выбран, 3 альтернативы ✓
- `src/events.rs` (217 строк): crossbeam `unbounded` + `try_send` (slow subscriber не блокирует) ✓
- 8 вариантов `NodeEvent`: BlockApplied, BlockReorged, TxAccepted, TxRejected, MiningStarted, MiningFinished, PeerScoreChanged, StatePersisted ✓
- Тесты `three_subscribers_receive_all_events`, `slow_subscriber_does_not_block_publish`, `reorg_generates_block_reorged` ✓
- Live-node тест `three_subscribers_each_receive_live_node_events` ✓

### S1-P10. ADR-0007 + tokio (постепенная миграция) — ✅ Fully
- `docs/ADR/0007-tokio-on-stage-1.md`: hybrid model, 4 альтернативы ✓
- `src/main.rs:1` `#[tokio::main]` ✓
- `tokio = { version = "1", features = ["rt-multi-thread","macros","sync","time","net","signal","io-util"] }` ✓
- `tests/two_clients.rs:80` `node_runs_on_tokio_and_shuts_down_cleanly` ✓
- `tests/concurrency.rs` sync paths preserved ✓
- **Примечание:** ADR-0011 (Stage 1.5) позже полностью миггрировал mining/P2P на tokio (BUG-S0-021)

### S1-P11. chain_selector.rs: tip selection + tie-breaking — ✅ Fully
- `crates/strangecoin-core/src/chain_selector.rs` — чистый fork-choice (cumulative work → tip timestamp → tip hash), без import state/transactions ✓
- `ChainInfo { tip_height, tip_hash, total_work, tip_timestamp }`, `is_better` (lexicographic), `select_best` (reduce) ✓
- Proptest `select_best_permutation_invariant`, `transitivity` ✓
- Интеграционные тесты `chain_selector_prefers_higher_work`, `chain_selector_tiebreaks_by_timestamp_then_hash`, `real_network_fast_registration_race` ✓
- `docs/spec/fork_choice.md` (68 строк) ✓

### S1-P12. block_executor.rs + state_cache.rs — ✅ Fully
- 5 компонентов в `src/blockchain/`: chain_selector (285), block_executor (571), state_cache (586), blockchain_facade (361), consensus_manager (132), mod.rs (9) ✓
- `block_executor.rs:85-149` не выбирает tip, не пишет в DB ✓
- `state_cache.rs` — единственный читатель `balances` ✓
- Тест `rebuild_from_chain_repairs_a_tampered_cache` ✓
- **Исполняемый rg-gate** `tests/balances_gate.rs` (154 строк): `RAW_MUTATION_WHITELIST = ["state_cache.rs","block_executor.rs"]` — ходит по `src/` и проверяет, что `.balances.(insert|remove|get_mut)` нет вне whitelist ✓

### S1-P13. blockchain_facade.rs + consensus_manager.rs — ✅ Fully
- 5 компонентов в `src/blockchain/` ✓
- `blockchain_facade.rs` — 361 строка (КГ ≤ 400) ✓
- `consensus_manager.rs` — 132 (тонкий wrapper) ✓
- `BlockView.expected_consensus_version` от `ConsensusManager::expected_version(height)` ✓
- `src/network/sync_engine.rs` импортирует только `crate::blockchain::BlockchainFacade` (public API) ✓
- `src/lib.rs:34` ре-экспортирует `Blockchain, BlockchainFacade, ConsensusManager` ✓

### S1-P14. network_id в генезисе и HELLO — ✅ Fully
- `genesis.json:3` `"network_id": 1`, `:4` `"chain_id": 1` ✓
- `load_genesis` rejects if `network_id != chain_id` (`consensus/mod.rs:28-33`, `GenesisNetworkMismatch`) ✓
- Сравнение с `EXPECTED_GENESIS_HASH` (`:85`) ✓
- `encode_hello`/`parse_hello` в `protocol.rs:231, 241` ✓
- `tests/network_id.rs:30` `foreign_network_id_is_rejected_and_banned` — реальная TCP-проверка + бан через `rate_limiter` ✓

### S1-P15. bech32-адреса (HRP sc1/tsc1/rsc1) — ✅ Fully
- `crates/strangecoin-core/src/address.rs` использует **Bech32m** (BIP-173 update, сильнее Bech32) ✓
- `hrp_for_network`: 1→"sc", 2→"tsc", 3→"rsc" ✓
- Тесты `test_round_trip` (3 сети), `test_checksum_error` (`InvalidAddressChecksum`), `test_network_id_hrp_mapping` ✓
- `tests/two_clients.rs:160` `assert!(addrs[0].starts_with("rsc1"))` ✓
- `tests/two_clients.rs:153` `bech32_address_transfer` — grant + 1000 transfer + balance check ✓
- `tests/address_migration.rs` (263 строки) — миграция legacy base64 → bech32, идемпотентная ✓
- `rg "base64::encode" src/` → только в `wallet.rs:69,71,164` (keystore storage, не адреса) ✓

### S1-P16. Headers-first sync — ✅ Fully
- `src/network/sync.rs` (640 строк): `GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS` ✓
- `MAX_HEADERS_BATCH=2000`, `MAX_BLOCKS_BATCH=128` ✓
- Count > `(bytes.len() - offset) / 4` отвергается до аллокации (invariant #7) ✓
- `validate_header_pow` — первый bad PoW дропает всю партию ✓
- `tests/sync_headers.rs:26` `new_node_syncs_20_blocks_via_headers_first` — 21 заголовок, 20 блоков, `adopt_candidate` succeeds ✓
- `HeaderCache`, `plan_best_branch`, `sync_headers_first` ✓

### S1-P17. Mempool RBF (Replace-By-Fee) — ✅ Fully
- `RBF_MIN_DELTA_BPS=3000` (30%), `MAX_RBF_REPLACEMENTS=10` ✓
- `feerate_bump_ok` чистая целочисленная арифметика (без f64): `signed_len(old) * RBF_BPS_DENOMINATOR >= signed_len(new) * (RBF_BPS_DENOMINATOR + RBF_MIN_DELTA_BPS)` ✓
- `by_sender: HashMap<String, BTreeMap<u64, TxId>>` (индекс по nonce), `generations: HashMap<TxId, u32>` (replacement depth) ✓
- `InsertOutcome::Replaced(Vec<TxId>)` ✓
- 5 тестов в `tests/rbf.rs`: `rbf_replacement_emits_tx_rejected` (через EventBus), `rbf_replacement_chain_is_limited` ✓
- `double_spend.rs` regression не сломан ✓

### S1-P18. ADR-0010 + SyncEngine — ✅ Fully
- `docs/ADR/0010-sync-engine.md` с 5 альтернативами ✓
- `src/network/sync_engine.rs` (543 строки): `HEADERS_INBOX_CAP=64`, `BLOCKS_INBOX_CAP=256`, `SEEN_CAP=4096` ✓
- Single consumer: validate → apply → announce (publish on EventBus + send to sync_tx) ✓
- `rg "adopt_candidate|apply_tx|save_state" src/network/` → только в `sync_engine.rs` ✓
- `tests/sync_engine.rs:25` `concurrent_candidates_race_through_one_engine` — 3 фазы через Barrier, детерминированный race ✓

### S1-P19. Stage 1 интеграционные тесты + первый fuzz-target — ✅ Fully
- Все D01 тесты сохранены ✓
- 6 новых: `tests/state_root.rs`, `tests/events.rs`, `tests/rbf.rs`, `tests/network_id.rs`, `tests/sync_headers.rs`, `tests/sync_engine.rs` ✓
- `fuzz/fuzz_targets/canonical_decode.rs` ✓
- `fuzz/Cargo.toml` с libfuzzer-sys ✓
- **Измеренный soak 600s: 25 947 107 inputs / 0 panics** (`fuzz/README.md`, 2026-10-09) ✓
- CI job `fuzz-canonical-decode` 600s на ubuntu-latest ✓

### S1-P20. Регрессия инвариантов Stage 0 + регистрация новых — ✅ Fully
- `docs/stage1/INVARIANTS_ENFORCED.md` (90 строк) — таблица 22 инвариантов с 1:1 нумерацией ARCHITECT3 §5 ✓
- Колонки: #, Invariant, Stage 0 location, Stage 1 location, Test(s), Status ✓
- #19 (state_root) и #21 (consensus_version) перенесены из deferred ✓
- **#19 🟡 residual** (zero opt-in BUG-S0-012/013)
- **#21 ✅** enforced
- rg audits clean: 0 I/O в core, 0 прямых mutations balances вне state_cache/block_executor ✓

### S1-P21. Threat model актуализация Stage 1 — ✅ Fully
- V-34..V-42 (9 новых векторов): headers-first poisoning, state_root manipulation, witness spoofing, tx_root manipulation, consensus_version downgrade, network confusion, RBF fee-war DoS, SyncEngine flooding, HRP confusion ✓
- Каждый с Mitigation (код-ссылка), Test reference, Status ✓
- §Mitigations Map (lines 891-899): каждый V-XX → конкретный test ✓
- Self-review §17 checklist (15 пунктов) ✓

### S1-P22. DoD-verification Stage 1 + тег v1.1.0-stage1 — ⚠️ Partial
- `docs/stage1/STAGE1_SUMMARY.md` (162 строки), 15 DoD-критериев с Evidence/Status/Residual/Predecessor-verified ✓
- 11 ✅ + 4 🟡 residual (#2 state root, #11 TLA+, #12 invariants, #13 ADRs/run, #15 tag) ✓
- §5 Deferred to later stages, §6 Open obligations (7 пунктов) ✓
- `VmExecutor` trait объявлен в `crates/strangecoin-core/src/vm/traits.rs:7` (без impl) ✓
- Version sync: Cargo.toml=1.1.0, core=1.1.0, Changelog `## 1.1.0 — Stage 1`, AGENTS.md "v1.1.0" ✓
- **❌ КРИТИЧНО:** `git tag -l` → **пусто.** Тега `v1.1.0-stage1` **не существует**
- **❌ КРИТИЧНО:** `git log --oneline` → **1 коммит** (`53930ea` — Stage 1.5 docfix). `STAGE1_SUMMARY.md §2` ссылается на 21 атомарный коммит (cb0ac1a, d5d55af, … 68e358f), **которых нет в git**. История squashed, audit-trail утерян — нарушение retro §8.1 («1 промпт = 1 атомарный коммит»)

---

## Топ-10 критических дефектов (по коду)

1. **Отсутствие git-тегов и реальной истории коммитов** — `git log` содержит 1 коммит, `git tag -l` пуст. `STAGE1_SUMMARY.md §2` перечисляет 21 атомарный коммит, которых нет в репо. **Нарушение retro §8.1** (правило «1 промпт = 1 атомарный коммит»). История squashed, теги утеряны, audit-trail исчез.

2. **`current_chain_id()` захардкожен в `CHAIN_ID_REGTEST`** (`crates/strangecoin-core/src/consensus.rs:171-173`) — mainnet-ветка `EXPECTED_GENESIS_HASH` валидации — **мёртвый код**. Все тесты проходят на regtest; mainnet/testnet структурно не протестированы.

3. **S1-P06: «Verkle Trie» не существует** — исходный `state/verkle.rs` был плоской `[256]`-slot array (BUG-S0-011). Заменён на SMT (`sparse_merkle.rs`). ADR-0006 честно исправлен. Реальный Verkle (KZG+BLS12-381) отложен. Stage 1 DoD пункт «Verkle Trie ✅» **технически не выполнен**.

4. **State root enforcement opt-in** (BUG-S0-012/013) — `block_executor.rs:136` и `state/mod.rs:20` пропускают проверку при `block.state_root == [0; 32]`. `verify_block_stateless` не пересчитывает post-state root из witness. Invariant #19 — 🟡 residual.

5. **Пер-сообщенный rate-limit не enforced** — `rate_limiter.check()` вызывается один раз на TCP-коннект, не на каждое сообщение в request-loop (`lib.rs:620-732`).

6. **Single-block size check отсутствует** — `validate_and_apply`/`commit_block` не проверяют размер блока до применения. Проверка только на wire-уровне и в `validate_chain` (whole-chain scan).

7. **PGP-ключ — placeholder-текст** (`SECURITY.md:25`), файла `pgp_key.asc` нет.

8. **`#[allow(dead_code)]` бандажи** — 6 штук в `tests/common/mod.rs` и `state_roundtrip.rs` без TODO-комментариев (P21 явно запрещал такой подход).

9. **Golden-векторы блоков таутологичны** (`serialize_golden.rs:201, 243, 329`) — сравнивают `actual_hex == actual_hex.clone()`, никогда не падают.

10. **Release pipeline никогда не запускался** — `release.yml` написан корректно, но `v*` тегов нет → cosign+SLSA не верифицированы на практике.

---

## Оценка зрелости по измерениям

| Измерение | Оценка | Обоснование по коду |
|---|---|---|
| Качество кода | 🟢 8/10 | Rust edition 2021, типизированные ошибки (thiserror), structured logging, zero-I/O ядро, чистая modular декомпозиция (5 компонентов blockchain/), Arc<RwLock<Blockchain>> с deadlock-тестом |
| Безопасность консенсуса | 🟢 7/10 | secp256k1 с recovery, blake3, MTP+2h future ban, retarget с clamp factor 4, RBF с anti-DoS, network_id ban. Слабые места: per-message rate-limit, state_root opt-in zero, no mainnet genesis |
| Архитектура | 🟢 8/10 | Workspace с чистым core (0 I/O), 11 ADR, SCIP-процесс governance, 5-компонентная декомпозиция blockchain, EventBus, SyncEngine разрывает network↔blockchain цикл |
| Тестирование | 🟢 7/10 | 22 property-теста, 23 интеграционных теста, fuzz 600s/25M inputs clean, golden-векторы tx. Слабые места: таутологические golden-векторы блоков, нет тестов Config::load, нет Wallet round-trip, нет InvalidChainId теста, retarget на высоте 2016 не построен |
| Документация | 🟢 8/10 | ADR-0001..0011, THREAT_MODEL 42 вектора, TLA+ с реальным TLC PASS, SECURITY/BOUNTY/INCIDENT_RESPONSE. Честные ретро-поправки в Changelog |
| Экономика | 🟢 7/10 | Tail emission Monero-style 0.6%/год, halving-график, проверка coinbase amount в state/inner.rs. Fee market — stub (Stage 5) |
| Network/P2P | 🟡 5/10 | Headers-first sync, SyncEngine, network_id gate, bech32m с 3 HRP, RBF. Нет: per-message rate-limit, real-world soak test, Erlay, encrypted transport (отложено Stage 2) |
| Governance | 🟡 4/10 | SCIP-процесс описан, consensus_version + activation height, но только 1 SCIP (genesis key replacement). Нет on-chain голосования, нет PoS (Stage 7) |
| VM / Smart contracts | 🔴 1/10 | VmExecutor trait объявлен в vm/traits.rs, реализации нет. WASM отложен на Stage 1.5 |
| Операционная готовность | 🔴 2/10 | Нет git-тегов, 1 squashed commit (нарушает retro §8.1), release pipeline никогда не запускался, PGP-ключ placeholder, Immunefi не настроен, mainnet genesis key ещё не сгенерирован оффлайн (SCIP-0001 — ops obligation), genesis testnet-key BURNED, но replacement ещё не создан |
| Mainnet-готовность | 🔴 0/10 | current_chain_id() == CHAIN_ID_REGTEST, mainnet-ветка валидации мёртвая, 0 пользователей, 0 нод на mainnet, 0 exchange listings, 0 реальных транзакций |

---

## Итоговый вердикт

**Реализация пунктов промптов:** из 51 требования — **40 полностью реализовано** (~78%), **11 частично** (~22%), **0 отсутствуют**. Это **очень высокий** уровень соответствия спецификации для кода, написанного ИИ-агентами.

**Зрелость криптовалюты: прототип уровня pre-mainnet (TRL 4–5).**

- **Кодовая зрелость — высокая** (~7/10): инженерное качество, тестирование, архитектура, документация соответствуют серьёзному open-source проекту.
- **Продакшн-зрелость — низкая** (~3/10): нет git-тегов, нет release-артефактов, нет mainnet, нет пользователей, genesis-key ещё должен быть сгенерирован оффлайн, pipeline не верифицирован.
- **Зрелость как криптовалюты — нулевая**: это не запущенная криптовалюта, а **хорошо спроектированный testnet-стейдж код** с credible engineering practices. До публичного mainnet нужно закрыть как минимум:
  1. Поставить git-теги `v1.0.0-stage0` и `v1.1.0-stage1`
  2. Сгенерировать оффлайн genesis key и обновить `EXPECTED_GENESIS_HASH`
  3. Впервые прогнать `release.yml` на теге
  4. Перевернуть `current_chain_id()` на mainnet
  5. Запустить публичный testnet с внешними валидаторами

**Главный риск:** между документацией (STAGE1_SUMMARY.md с 21 «атомарным коммитом») и фактическим git (1 squashed commit, 0 тегов) — **audit-trail разрыв**, прямо противоречащий собственному ретро §8.1 проекта. Документация серии Stage 1 выглядит как **aspirational, не reflective**. Это подрывает доверие к любым будущим DoD-заявлениям, пока теги не будут поставлены и история не восстановлена (или честно не зафиксировано, что squash произошёл).
