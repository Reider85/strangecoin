# Stage 1 Invariants Enforced (S1-P20, rev. BUG-S0-024 / BUG-S0-025)

**Document**: `docs/stage1/INVARIANTS_ENFORCED.md`  
**Status**: Complete ✅  
**Original audit**: 2026-10-04 (S1-P20)  
**Revision**: 2026-10-09 — BUG-S0-024/BUG-S0-031: нумерация 1:1 к `ARCHITECT3.md §5`, честные residual-статусы, битые ссылки устранены; BUG-S0-025: nonce-reject proptest закрыт; 2026-10-11 — BUG-S1-002: №19 closed (SCIP-0002), residual count обновлён  
**Prompt**: S1-P20 — Regression audit of 22 Stage 0 invariants after strangler migration  

**Нумерация**: 1:1 с `analytics/ARCHITECT3.md` §5 (22 инварианта; эталон — строки 484-507).  
**Stage 0 baseline**: `docs/stage0/INVARIANTS_ENFORCED.md` (исторический снимок на теге `v1.0.0-stage0`).

## Status Summary

| Status | Count | Invariants |
|--------|-------|------------|
| ✅ Enforced | 19 | 1-14, 16, 17, 19, 21, 22 |
| 🟡 Residual (enforced with known gaps) | 0 | — |
| Deferred Stage 1.5 | 2 | 15, 18 |
| Deferred Stage 5 | 1 | 20 |

## Invariant Audit Table

| # | Invariant (ARCHITECT3 §5) | Stage 0 location (v1.0.0-stage0) | Stage 1 location | Test(s) | Status |
|---|---------------------------|----------------------------------|------------------|---------|--------|
| 1 | Вся валидность — из цепочки; `balances` — кэш | `validate_chain` in `src/main.rs` — reconstructs balances from chain | `src/blockchain/state_cache.rs::validate_chain` + `rebuild_from_chain`; gate `tests/balances_gate.rs` (нет прямых мутаций вне state_cache) | `tests/state_cache.rs`, `tests/reorg.rs` | ✅ |
| 2 | Каждая транзакция подписана, `sender == pubkey` (secp256k1) | `verify_transaction` in `src/main.rs`; mempool `add_transaction` | core `consensus::verify_transaction` (`crates/strangecoin-core/src/consensus.rs:297`, recovery pubkey); вызывается в `block_executor.rs:130` и `mempool/mod.rs:90` | core `tests/consensus_proptest.rs::signature_verification_roundtrip`; `tests/block_executor.rs::rejects_unsigned_transfer`; `tests/two_clients.rs::hundred_transactions_five_wallets` | ✅ |
| 3 | `hash <= target` для каждого блока при его difficulty | `validate_difficulty` in `src/consensus/` (U256) | core `consensus::validate_difficulty:234` + `block_executor.rs::validate_target` (высота > 0) | `tests/pow.rs`, `tests/block_executor.rs::rejects_proof_of_work_above_target`, `tests/sync_headers.rs` (PoW-остров) | ✅ |
| 4 | `apply_block`/`unapply_block` — обратные чистые функции | Implicit in `validate_chain` reconstruction | core `state/inner.rs::apply_block:54` / `unapply_block:136` (чистые, `State → State`) | core `tests/state_roundtrip.rs` (4 proptest: single/multi/chain/nonce) | ✅ |
| 5 | Хэш/подпись — на канонических байтах (`serialize`), не JSON | `src/serialize.rs` — canonical binary, blake3; 19 golden-vector tests | core `serialize.rs` (canonical encode/decode, FORMAT_VERSION) | core `tests/serialize_golden.rs`, proptest `serialize_deserialize_roundtrip` | ✅ |
| 6 | Блок не содержит непроверимых транзакций/наград сверх эмиссии | `validate_chain`: coinbase ≤ `block_reward_at_height` | core `state/inner.rs:75-87` — coinbase.amount ≤ `block_reward_at_height(index, total_supply)`; grant-gate `allow_grant_blocks` (`S1-P01`) | `tests/emission.rs` (3: mining-loop supply_before + `mainnet_tail_phase_reward_matches_total_supply` + `tail_phase_transition_at_fifth_halving`), `tests/grant_flag.rs`, `tests/block_executor.rs::rejects_coinbase_above_the_block_reward` | ✅ |
| 7 | Размеры сообщений/блоков всегда ограничены до аллокации | `MAX_MESSAGE_SIZE`, `MAX_BLOCK_SIZE`, `MAX_TX_SIZE` в `validate_chain` + `network/protocol.rs` | `src/network/protocol.rs:9-10` (`MAX_MESSAGE_SIZE`=32MiB, `MAX_BLOCK_SIZE`=4MiB); `state_cache.rs:394`; core serialize length-checks | `src/network/protocol.rs` framing tests (10); `tests/network.rs` | ✅ |
| 8 | Генезис детерминирован и совпадает у всех узлов | `genesis.json` ↔ `EXPECTED_GENESIS_HASH`; panic on mismatch | core `consensus.rs:309 EXPECTED_GENESIS_HASH`; `blockchain_facade` open-path + `block_executor::validate_position` (genesis на пустой цепи) | `tests/genesis_key.rs` (5), `tests/block_executor.rs::genesis_is_applied_to_an_empty_chain` | ✅ |
| 9 | Секреты не пишутся на диск и не логируются | `config.toml` без секретов; keystore AES-256-GCM + PBKDF2 | `src/wallet.rs` (PBKDF2+AES-GCM, Drop-flush); `src/config.rs` — нет секретов; genesis seed удалён (BUG-S0-015 / SCIP-0001) | `tests/genesis_key.rs::consensus_source_has_no_genesis_seed`; code audit | ✅ |
| 10 | **Replay protection:** каждая tx содержит `chain_id`; mainnet/testnet/regtest несовместимы | `chain_id` в `verify_transaction` + `validate_chain`; mainnet=1, testnet=2, regtest=3 | core `verify_transaction` + `mempool/mod.rs:92` (insert); HELLO `network_id` (`S1-P14`) | `tests/network_id.rs::foreign_network_id_is_rejected_and_banned`; proptest | ✅ |
| 11 | **Nonce:** `account.nonce` строго инкрементируется; tx с `nonce <= account.nonce` отвергается | `validate_chain` nonce check; `add_transaction` mempool | core `consensus::validate_nonce` (BUG-S0-025, единое правило); `state/inner.rs` (apply инкремент); `mempool/mod.rs` (expected_nonce с RBF-цепочками через core validate_nonce) | proptest `nonce_reject` (core); `tests/double_spend.rs`; `tests/two_clients.rs` | ✅ |
| 12 | **Transaction hash = commitment:** `txid` из канонических байт всей tx (включая подпись) | `txid` from signed canonical bytes in `src/serialize.rs` | core `serialize.rs::txid:26` | core golden-vector tests; proptest roundtrip | ✅ |
| 13 | **Mempool basic rules:** на `insert` — подпись, dup, nonce, chain_id, balance | `mempool/insert()` validation; `MAX_PENDING_TXS` | `src/mempool/mod.rs::insert:80` — verify_transaction, chain_id, nonce-цепочка, balance, RBF limits (`S1-P17`) | `tests/rbf.rs` (5), `tests/two_clients.rs` | ✅ |
| 14 | **Graceful shutdown:** `Drop` для storage/wallet/network; нет «ручного удаления LOCK» | `Drop` for `Node`, `Blockchain`, `Wallet`; ctrlc; `AtomicBool` mining loop | `Drop` for `Node` (`src/lib.rs:526`), `Wallet` (`src/wallet.rs:245`), `Storage` (`src/storage/mod.rs:28`); tokio graceful (`S1-P10`/ADR-0011) | `tests/concurrency.rs::deadlock_test_blockchain_wallet_lock_order`; `tests/two_clients.rs::node_runs_on_tokio_and_shuts_down_cleanly` | ✅ |
| 15 | **Block gas limit:** `gas_used <= block_gas_limit` | — (deferred Stage 1.5) | core `vm/traits.rs` — `VmExecutor` trait only; wasmi/gas — Stage 1.5 | — | Deferred Stage 1.5 |
| 16 | **P2P framing:** length-prefixed, проверка размера ДО аллокации | `network/protocol.rs`: read length, validate, then `vec![0; length]` | `src/network/protocol.rs` — `read_length_prefixed:250` / async variant `:289`; count/len caps перед аллокацией | `src/network/protocol.rs` framing tests; `tests/sync_headers.rs` | ✅ |
| 17 | **Rate limiting:** лимит сообщений/сек от одного пира; нарушение → бан | `RateLimiter` in `src/network/rate_limiter.rs` | `src/network/rate_limiter.rs::check:79`, `ban:107`, `is_banned:116` | `rate_limiter.rs` unit tests (4); `tests/network_id.rs` (ban path) | ✅ |
| 18 | **Event log:** каждый tx имеет `Receipt { gas_used, logs, status }`; receipts неизменны | — (deferred Stage 1.5) | — (требует VM execution; EventBus `src/events.rs` — node-события, не receipts) | — | Deferred Stage 1.5 |
| 19 | **State root match:** `state.root_after(block) == block.state_root`; иначе reject | — (deferred Stage 1, S1-P06) | core `state/mod.rs::root_after` (SMT depth 256, ADR-0006 amended; SCIP-0002: 3-й параметр `allow_zero_state_root`); `block_executor.rs::validate_and_apply` — zero-root reject вне genesis/regtest-opt-in + compare с computed root. Node-блоки (mine/grant) коммитят реальный корень. Stateless: `verify_block_stateless` пересчитывает post-root из parent root + witness + блока (`sparse_merkle.rs::root_after_updates`, mulproof update) и сверяет с `block.state_root` → `PostStateRootMismatch`. **closed 2026-10-11 (BUG-S1-002 + BUG-S1-003)** | core `tests/state_root.rs` (9: tamper→reject, zero-root reject без opt-in, genesis exempt, determinism, proptest); e2e `tests/state_root.rs` (6: strict reject, opt-in tolerate, mine/grant commit); `tests/block_executor.rs` (3: mismatch, zero-reject, zero-opt-in); core `sparse_merkle.rs` unit (8 + proptest `root_after_updates`); core `tests/witness.rs` (14: forged root, untouched tamper, zero-root, missing address, genesis exempt, proptest) | ✅ (BUG-S1-002 + BUG-S1-003, 2026-10-11; SCIP-0002) |
| 20 | **Fee invariant:** `fee_burned + fee_to_miner = total_fees`; `gas_used <= block_gas_limit` | — (deferred Stage 5) | core `economics/fee_market.rs` — stub (fee=0, RBF-feerate прокси S1-P17) | — | Deferred Stage 5 |
| 21 | **Consensus versioning:** `block.consensus_version <= current_version`; активация по высоте | — (deferred Stage 1, S1-P05) | core `governance/scip.rs` (SCIP + activation height); `src/blockchain/consensus_manager.rs` (expected version by height); `block_executor.rs:109` reject stale/future | `tests/consensus_version.rs` (3); core scip activation tests (5); `tests/block_executor.rs::rejects_stale_consensus_version` | ✅ |
| 22 | **Reproducible builds:** CI публикует SLSA provenance + cosign signature для каждого release | `.github/workflows/release.yml` — triggers on `push: tags: ['v*']` | `.github/workflows/release.yml` (LTO, single codegen unit, `--remap-path-prefix`, cosign keyless, SLSA L3). First `v*` run executed **2026-10-10**: tag `v0.0.1-rc1` → run `38071514479` green on all 6 targets + provenance + sign + release | run 38071514479 (all jobs success); `docs/security/REPRODUCIBLE_BUILDS.md` §Verified Release Runs: 6 SHA256s, `cosign verify-blob` = `Verified OK`, `slsa-verifier` = `PASSED` @ commit `a830c51` | ✅ (BUG-S1-001, 2026-10-10) |

### Registering notes (S1-P20)

- **№19, №21** — впервые enforce-нуты в Stage 1 (deferred → enforced): №19 через SMT + block_executor, zero-root opt-out закрыт SCIP-0002 (BUG-S1-002, 2026-10-11), stateless post-root gap закрыт BUG-S1-003 (2026-10-11, mulproof-update пересчёт в `verify_block_stateless`), №21 через SCIP/consensus_manager. Это **не новые** инварианты сверх ARCHITECT3 §5 — нумерация соответствует §5.
- **№4** — enforce-место переехало: implicit-in-validate_chain → явные чистые функции core `state/inner.rs` + property-тесты.
- **№1** — enforce-место переехало: `main.rs::validate_chain` → `state_cache::validate_chain` + `rebuild_from_chain`; balances-mutation gate — `tests/balances_gate.rs` (BUG-S0-022).

## Residual Coupling Cleaned (S1-P20 evidence)

**Before**: Network modules directly imported `Blockchain` struct  
**After**: Network modules use `BlockchainFacade` and `ChainSnapshot` DTOs  
**Files Updated**:
- `src/network/sync_engine.rs`: Uses `ChainSnapshot` instead of `Blockchain`
- `src/lib.rs`: Node struct uses `ChainSnapshot` for sync channel
- All test files updated to use `ChainSnapshot` in channels

## Audit Results (S1-P20, rev. 2026-10-09)

- **Compilation**: ✅ workspace suite green (`cargo test --workspace`)
- **Type Safety**: ✅ `cargo clippy --all-targets -- -D warnings` exit 0
- **0 I/O in core**: ✅ `rg "std::fs|std::net|tokio|leveldb" crates/strangecoin-core/src` → 0 matches
- **Network isolation**: ✅ network modules only use facade APIs
- **Sync coupling**: ✅ ADR-0010 — `adopt_candidate`/`apply_tx`/`save_state` only in `sync_engine.rs`
- **Balances mutations**: ✅ 0 прямых мутаций вне state_cache-слоя (gate `tests/balances_gate.rs` + CI `source-gates`)
- **Invariant numbering**: ✅ 1:1 с ARCHITECT3 §5 (BUG-S0-024, 2026-10-09)
- **Enforcement links**: ✅ все пути существуют (BUG-S0-031, 2026-10-09)

## Known residuals / deferred (перекрытие со STAGE1_SUMMARY §6)

| # | Residual | Bug | Fix path |
|---|----------|-----|----------|
| 19 | ~~Zero `state_root` opt-in + stateless verify без post-root~~ | ~~BUG-S0-012~~, ~~BUG-S0-013~~ | **closed 2026-10-11:** zero-root opt-out удалён (BUG-S1-002 / SCIP-0002, activation_height 0; regtest legacy — network-aware `Config.allow_zero_state_root`; mainnet/testnet — reject); stateless post-root пересчитывается и сверяется с `block.state_root` (BUG-S1-003, `root_after_updates`) |
| ~~22~~ | ~~Release pipeline не прогонялся на GitHub Actions~~ | ~~BUG-S0-005~~ | **closed 2026-10-10 (BUG-S1-001):** tag `v0.0.1-rc1`, run `38071514479` green, evidence в REPRODUCIBLE_BUILDS.md |

## Historical Context

- **Stage 0**: `docs/stage0/INVARIANTS_ENFORCED.md` (historical snapshot; numbering was already 1:1)
- **Stage 1**: this document (living document; numbering 1:1 since BUG-S0-024 rev.)
- **Migration**: Strangler migration completed; residual coupling cleanup (S1-P20)
- **Superseded**: rev. 2026-10-04 версия этой таблицы использовала собственную нумерацию (inconsistent with ARCHITECT3 §5) — заменена 2026-10-09

---

**Related**: STAGE1_SUMMARY §6 (open obligations) · `analytics/bugfixes-stage0.md` (BUG-S0-024, BUG-S0-031) · S1.5-P03, S1.5-P06
