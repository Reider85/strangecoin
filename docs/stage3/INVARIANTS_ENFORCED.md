# Stage 3 Invariants Enforced (S3-P14 regression audit)

**Document**: `docs/stage3/INVARIANTS_ENFORCED.md`
**Status**: Complete ✅
**Date**: 2026-11-16 (S3-P14)
**Prompt**: S3-P14 — Regression 22 инвариантов + threat model update после Stage 3 proper work (S3-P01..S3-P13)
**Supersedes (for Stage 3 status)**: `docs/stage2/INVARIANTS_ENFORCED.md` (22/22 на теге `v2.0.0-stage2`, re-audit S2-P23)

**Нумерация**: baseline 1:1 с `analytics/ARCHITECT.md` (H1 «ARCHITECT3.md») §5 (22 инварианта; эталон — таблица lines 493-514); **#23–#26 введены `analytics/prompt-stage3.md` S3-P14** (Stage 3 storage-расширения) → итого 26.
**Stage 0 baseline**: `docs/stage0/INVARIANTS_ENFORCED.md`.
**Stage 1 baseline**: `docs/stage1/INVARIANTS_ENFORCED.md`.
**Stage 1.5 baseline**: `docs/stage1.5/INVARIANTS_ENFORCED.md`.
**Stage 2 baseline**: `docs/stage2/INVARIANTS_ENFORCED.md` (полные enforcement-локации #16/#17 и промежуточные extension'ы — там же; в этой таблице ячейки Stage 0-2 для самых длинных строк сохранены, Stage 3 — актуализация).

## Status Summary

| Status | Count | Invariants |
|--------|-------|------------|
| ✅ Enforced | 26 | 1-26 |
| 🟡 Residual (enforced with known gaps) | 0 | — |

**S3-P14 regression audit (2026-11-16): 26/26 enforced — no regressions**
после Stage 3 proper work (S3-P04..S3-P13: crate `crates/strangecoin-storage/`
+ schema v1 + RocksDbEngine, monolith-миграция per-block keys/mempool CF/
`state_history`, one-shot LevelDB→RocksDB миграция, O(1)-индексы, batch
microbench + crash-recovery suite + LRU cache, witness wire codec + light
client, pruning height-window + archival, snapshot export/import +
commitment verification).

**Расширение coverage (Stage 3):**
- **#1** (валидность из цепочки; кэш — не источник истины) — extension: persistence переехала на RocksDB backend (`strangecoin-storage`), но `rebuild_from_chain` остаётся авторитетной; per-block adopt пишет только diff (`tests/per_block_writes.rs`), crash-suite доказывает атомарность (`crash_recovery.rs`) — S3-P06/S3-P07/S3-P10
- **#14** (graceful shutdown; нет ручного удаления LOCK) — extension: `Drop for RocksDbEngine` → flush всех CF (`rocks.rs`); stale-LOCK auto-release после child-kill (`crash_recovery.rs::stale_lock_is_auto_released_after_child_kill`) — S3-P06/S3-P10
- **#23 schema_version guard** — новый: open с mismatched `SCHEMA_VERSION` fail-closed (`rocks.rs:110-129`, `SCHEMA_VERSION=1` в `schema.rs:57`)
- **#24 crash atomicity** — новый: kill во время write → reopen консистентен (WAL + атомарный WriteBatch; kill-sweep k∈{0,1,5,50})
- **#25 snapshot commitment** — новый: import без верификации невозможен (`snapshot.rs::verify_file:290` → `import:362`, fail-closed `NotFreshDatabase`)
- **#26 pruning boundary** — новый: current state/blocks/meta не прюнятся; только `state_history` старше height-window (`pruning.rs::sweep:180`)

**Остальные 20 baseline** — regression green (места enforcement не ослаблены;
часть storage-локаций переехала из LevelDB-блобов в per-block RocksDB-схему —
см. таблицу). **#16/#17** — net-инварианты, Stage 3 не трогает network-слой
(Stage 2 extensions остаются в силе). **#20** — по-прежнему degenerate (fee=0);
fee market — Stage 5 (SCIP до mainnet freeze). **#22** — release-pipeline
v3.0.0-stage3 приходит с S3-P15 (до тега regression на верифицированном
v2.0.0-stage2 run).

**Итог Stage 3**: 26/26 enforced (4 новых: #23-#26; 2 Stage 3 extensions: #1, #14; 1 still deferred до Stage 5: #20 degenerate).

## Invariant Audit Table

| # | Invariant (ARCHITECT3 §5) | Stage 0 location | Stage 1 location | Stage 1.5 location | Stage 2 location | Stage 3 location | Test(s) | Status |
|---|---------------------------|------------------|------------------|--------------------|------------------|------------------|---------|--------|
| 1 | Вся валидность — из цепочки; `balances` — кэш | `validate_chain` в `src/main.rs` | `src/blockchain/state_cache.rs::validate_chain` + `rebuild_from_chain` | без изменений (+ contract_storage/receipts как derived) | без изменений (blockchain-слой Stage 2 не тронут) | **extension (S3-P06/P07/P10):** persistence — RocksDB через `strangecoin-storage` (per-block keys, `Storage::save_state_engine`); `rebuild_from_chain` остаётся авторитетом (invariant #1), state-cache пересобирается из chain на adopt; атомарные batch'и + crash-suite | `tests/state_cache.rs`, `tests/reorg.rs`; Stage 3: `tests/per_block_writes.rs::adopt_writes_only_changed_records_not_the_whole_chain`, `crates/strangecoin-storage/tests/crash_recovery.rs` (5), `tests/indices.rs` (reorg consistency) | ✅ Stage 3 ext |
| 2 | Каждая транзакция подписана, `sender == pubkey` (secp256k1) | `verify_transaction` в `src/main.rs` | core `consensus::verify_transaction` (`consensus.rs:343`) | без изменений | без изменений | без изменений (core не тронут; storage — implementation detail) | core `tests/consensus_proptest.rs`; `tests/block_executor.rs::rejects_unsigned_transfer`; `tests/two_clients.rs` | ✅ regression |
| 3 | `hash <= target` для каждого блока при его difficulty | `validate_difficulty` (U256) | core `consensus::validate_difficulty` + `block_executor.rs::validate_target` | без изменений | без изменений | без изменений | `tests/pow.rs`, `tests/block_executor.rs::rejects_proof_of_work_above_target`, `tests/sync_headers.rs`, core `tests/u256.rs` | ✅ regression |
| 4 | `apply_block`/`unapply_block` — обратные чистые функции | Implicit in `validate_chain` | core `state/inner.rs::apply_block:54` / `unapply_block:136` | extension: contract_storage + value_transfers_undo + constructor undo (полная деталь — `docs/stage1.5/INVARIANTS_ENFORCED.md` #4) | без изменений (VM/state-слой не тронут Stage 2) | без изменений (core не тронут; state-персистентность — detail под #1) | core `tests/state_roundtrip.rs` (4 proptest); core `tests/contract_storage_smt.rs`; vm-wasm `tests/contract_storage.rs`; e2e `tests/vm_transfer_to_contract.rs`, `tests/vm_deploy_call.rs` | ✅ regression |
| 5 | Хэш/подпись — на канонических байтах (`serialize`), не JSON | canonical binary, blake3; 19 golden vectors | core `serialize.rs` (FORMAT_VERSION) | extension: Receipt/Create/Call per-variant; header v7 (`receipts_root`) | без изменений (core не тронут; wire-кодеки net — собственные байтовые форматы) | без изменений (core не тронут; новые wire-форматы Stage 3 — witness codec `WITNESS_FORMAT_VERSION=1` и snapshot `SCSNAP01` — свои детерминированные форматы, НЕ consensus serialization; witness — не consensus rule) | core `tests/serialize_golden.rs` (19/19); proptest roundtrip; core `tests/receipts.rs`; Stage 3: core `tests/witness.rs::witness_codec_golden_vector`, `crates/strangecoin-storage/src/snapshot.rs::export_is_byte_identical_across_runs` | ✅ regression |
| 6 | Блок не содержит непроверимых транзакций/наград сверх эмиссии | `validate_chain`: coinbase ≤ reward | core `state/inner.rs` + grant-gate | без изменений | без изменений | без изменений | `tests/emission.rs`, `tests/grant_flag.rs`, `tests/block_executor.rs`, `tests/chain_id.rs` | ✅ regression |
| 7 | Размеры сообщений/блоков всегда ограничены до аллокации | `MAX_MESSAGE_SIZE`/`MAX_BLOCK_SIZE`/`MAX_TX_SIZE` | те же лимиты | без изменений wire-лимитов | лимиты в `crates/strangecoin-net/src/protocol.rs::MAX_MESSAGE_SIZE:25`; block/tx caps в `block_executor.rs`/`state_cache.rs` | без изменений (net не тронут; snapshot-формат — file-based, size-контролируется при decode header `SNAPSHOT_HEADER_LEN`, отдельный не-consensus путь) | net framing tests; `tests/network.rs`; `tests/block_executor.rs` (5MB block / 300KB tx reject); Stage 3: `crates/strangecoin-storage/src/snapshot.rs::bad_magic_is_rejected` | ✅ regression |
| 8 | Генезис детерминирован и совпадает у всех узлов | Per-network genesis ↔ expected hash | core `consensus.rs::EXPECTED_*_GENESIS_HASH`; `src/consensus/mod.rs::validate_genesis` | extension: receipts_root=[0;32] в genesis, hashes пересчитаны | без изменений | без изменений (snapshot import сверяет tip/network_id, genesis-правила не менялись) | `tests/genesis_key.rs` (9), `tests/block_executor.rs::genesis_is_applied_to_an_empty_chain` | ✅ regression |
| 9 | Секреты не пишутся на диск и не логируются | keystore AES-256-GCM + PBKDF2 | `src/wallet.rs`, `src/config.rs` — нет секретов | без изменений; VM без I/O | extension: Ed25519 node keypair в `keystore/node/`; ephemeral из long-term seed | без ослабления: snapshot-файлы содержат только state/tip (не keys/secrets); schema-версия и meta — не секреты; keystore не тронут | `tests/genesis_key.rs::consensus_source_has_no_genesis_seed`; code audit; Stage 3: `tests/snapshot_sync.rs` (snapshot payload = accounts/blocks) | ✅ regression |
| 10 | **Replay protection:** каждая tx содержит `chain_id`; mainnet/testnet/regtest несовместимы | `chain_id` в verify + mempool + block validation | core verify + mempool + block_executor; HELLO `network_id` | без изменений | HELLO-валидация в `protocol/hello.rs::validate_hello:148`; Noise nonce anti-replay | без изменений (consensus/mempool/net); **дополнение:** snapshot verify сверяет `network_id` файла с ожидаемым (`snapshot.rs::verify_file`, чужая сеть → reject) — тот же принцип на новом surface | `tests/network_id.rs::foreign_network_id_is_rejected_and_banned`; `tests/chain_id.rs` (5); Stage 3: `tests/snapshot_sync.rs::wrong_network_id_is_rejected` | ✅ regression |
| 11 | **Nonce:** `account.nonce` строго инкрементируется; tx с `nonce <= account.nonce` отвергается | `validate_chain` nonce check; mempool | core `validate_nonce`; `state/inner.rs`; `mempool/mod.rs` | extension: constructor nonce++ | без изменений | без изменений (state/mempool validation не тронуты; nonce персистится в per-block state-записях) | proptest `nonce_reject` (core); `tests/double_spend.rs`; `tests/two_clients.rs`; `tests/vm_deploy_call.rs` | ✅ regression |
| 12 | **Transaction hash = commitment:** `txid` из канонических байт всей tx | `txid` from signed bytes | core `serialize.rs::txid:26` | extension: per-variant serialization, receipts key = txid | без изменений | без изменений; индекс `txid→(height,index)` (S3-P09) читает тот же canonical txid — O(1)-lookup не меняет commitment-семантику | core golden-vector tests; proptest roundtrip; core `tests/receipts.rs`; Stage 3: `tests/indices.rs::indices_written_on_adopt_and_lookups_are_o1` | ✅ regression |
| 13 | **Mempool basic rules:** на `insert` — подпись, dup, nonce, chain_id, balance | `mempool/insert()` validation | `src/mempool/mod.rs::insert:80` | без изменений | без изменений; gossip-announced txs идут тот же insert | validation без изменений; **локация персистентности:** mempool-ключи живут в отдельной CF (`mempool`), delete-on-mine (S3-P07) — stale tx больше не копятся (без ослабления insert-правил) | `tests/rbf.rs` (5), `tests/two_clients.rs`; `tests/net_gossip.rs`; Stage 3: `tests/mempool_persistence.rs::mempool_prefix_loads_and_deletes_on_mine` | ✅ regression |
| 14 | **Graceful shutdown:** `Drop` для storage/wallet/network; нет «ручного удаления LOCK» | `Drop` for Node/Wallet/Storage | `Drop` for `Node` (`src/lib.rs:526`) | без изменений; tokio graceful | network workers — `tokio::spawn`; `Drop for Node` | **extension (S3-P06/S3-P10):** `Drop for RocksDbEngine` → flush всех CF (`rocks.rs`); stale-LOCK file auto-released после child-kill (нет ручного удаления — система сама снимает осиротевший lock) | `tests/concurrency.rs::deadlock_test_blockchain_wallet_lock_order`; `tests/two_clients.rs::node_runs_on_tokio_and_shuts_down_cleanly`; Stage 3: `crates/strangecoin-storage/tests/crash_recovery.rs::stale_lock_is_auto_released_after_child_kill` | ✅ Stage 3 ext |
| 15 | **Block gas limit:** `gas_used <= block_gas_limit` | deferred Stage 1.5 | deferred Stage 1.5 | core `vm/gas.rs::BLOCK_GAS_LIMIT` = 1M; `block_executor.rs::enforce_block_gas_limit`/`enforce_receipt_gas_limit` | без изменений (VM/gas не тронут) | без изменений | `tests/vm_gas_exhaustion.rs` (3); `tests/vm_deploy_call.rs`; block_executor unit; vm-wasm `tests/gas_metering.rs` | ✅ regression |
| 16 | **P2P framing:** length-prefixed, проверка размера ДО аллокации | `network/protocol.rs`: read length, validate, then alloc | `src/network/protocol.rs::read_length_prefixed:250` / async `:289` | без изменений | **extension (S2-P08): Noise encrypted framing** — `crates/strangecoin-net/src/protocol.rs::encrypt_frame:427` / `decrypt_frame:471`; `MAX_MESSAGE_SIZE` до аллокации; monotonic nonces + `NoiseError::Replay` | без изменений (network-слой Stage 3 не тронут — witness/snapshot живут в core/storage, не в net) | net framing unit tests; `tests/net_noise_handshake.rs`; fuzz `noise_handshake.rs` | ✅ Stage 2 ext (regression Stage 3) |
| 17 | **Rate limiting:** лимит сообщений/сек от одного пира и per-message; нарушение → бан | `RateLimiter` sliding window | `rate_limiter.rs::check:111`, `check_class:126`, `ban:180` | без изменений | **extension (S2-P16/S2-P17):** token bucket per peer + per-message caps + violation window + ban threshold (`crates/strangecoin-net/src/rate_limiter.rs`, ADR-0023); production wiring в `peer/connection_loop.rs` | без изменений (network-слой Stage 3 не тронут) | `rate_limiter.rs` unit; `tests/rate_limit.rs`; `tests/net_rate_limit_integration.rs` | ✅ Stage 2 ext (regression Stage 3) |
| 18 | **Event log:** каждый tx имеет `Receipt`; receipts неизменны | deferred Stage 1.5 | deferred Stage 1.5 | core `vm/result.rs::Receipt` + sub-trie + `BlockHeader.receipts_root` (полная деталь — stage1.5 doc #18) | без изменений | без изменений (receipts/VM не тронуты; receipts — derived, не персистятся — ADR-0025) | core `tests/receipts.rs` (13); vm-wasm `tests/events_and_receipts.rs` (10); e2e `tests/receipts.rs` (4); TLA+ `ReceiptsRootMatch` | ✅ regression |
| 19 | **State root match:** `state.root_after(block) == block.state_root`; иначе reject | deferred Stage 1 | core `state/mod.rs::root_after`; `block_executor.rs::validate_and_apply` | extension: contract storage в глобальном root + `receipts_root` validation (полная деталь — stage1.5 doc #19) | без изменений (gossip/relay применяют блоки через тот же `validate_and_apply`) | без изменений (core не тронут); **дополнение:** snapshot verify пересчитывает state root из accounts перед import (`snapshot.rs::verify_file`) — тот же commitment-принцип на новом surface | core `tests/contract_storage_smt.rs` (9); vm-wasm `tests/contract_storage.rs` (11); core `tests/state_root.rs`; `tests/state_root.rs`; Stage 3: `tests/snapshot_sync.rs::forged_body_is_rejected`, `crates/strangecoin-storage/src/snapshot.rs::forged_state_root_is_rejected` | ✅ regression |
| 20 | **Fee invariant:** `fee_burned + fee_to_miner = total_fees`; `gas_used <= block_gas_limit` | deferred Stage 5 | fee_market stub | degenerate: fee=0 → тривиально; gas-половина через #15 | без изменений (degenerate) | без изменений (degenerate; Stage 3 не трогает economics) | gas-половина через `tests/vm_gas_exhaustion.rs` + block_executor unit; fee-split trivial при fee=0 | ✅ degenerate (fee market — Stage 5) |
| 21 | **Consensus versioning:** `block.consensus_version <= current_version`; активация по высоте | deferred Stage 1 | core `governance/scip.rs`; `consensus_manager.rs`; `block_executor.rs:109` | без изменений | без изменений (network-layer; consensus-правила не менялись) | без изменений (Stage 3 — storage/implementation detail; consensus-правила не менялись; SCIP-gated фичи — state expiry/receipts-prune/Verkle — НЕ реализованы) | `tests/consensus_version.rs` (3); core scip tests (5); `tests/block_executor.rs::rejects_stale_consensus_version` | ✅ regression |
| 22 | **Reproducible builds:** CI публикует SLSA provenance + cosign signature | `.github/workflows/release.yml` | release.yml (LTO, remap-path-prefix, cosign, SLSA L3) | без изменений; vm-wasm входит в тот же release | CI fuzz matrix + 4 network fuzz targets (S2-P21); release-pipeline v2.0.0-stage2 (S2-P24) | без изменений (code); v3.0.0-stage3 release-pipeline extension приходит с S3-P15 — до тега regression остаётся на верифицированном v2.0.0-stage2 run | run 38071514479 (v0.0.1-rc1); run 39110855701 (v1.5.0-stage1.5, 13/13); `docs/security/REPRODUCIBLE_BUILDS.md` | ✅ regression (S3-P15 extension pending) |
| 23 | **Schema version guard:** открытая БД с другим `SCHEMA_VERSION` → fail-closed (не молча downgrade/upgrade) | — (нет schema versioning) | — | — | — | **new (S3-P05/S3-P06):** `SCHEMA_VERSION=1` (`schema.rs:57`); fresh → init, mismatch → `StorageError::SchemaVersion`, bad-len → `Corrupt` (`rocks.rs:110-129`); без auto-upgrade | `crates/strangecoin-storage/tests/rocks_engine.rs::schema_version_guard_fails_closed_on_mismatch_and_corruption`, `::open_initializes_schema_version_on_a_fresh_db`; unit `schema.rs` guard test | ✅ new (S3-P14) |
| 24 | **Crash atomicity:** kill во время write → reopen консистентен (никаких half-applied batch'ей) | — (LevelDB, crash-suite отсутствовал) | — | — | — | **new (S3-P06/S3-P10):** атомарный `WriteBatch` (одна batch-запись = все op'ы или ни одного); WAL restore; kill-sweep детерминированный k∈{0,1,5,50}; stale-LOCK auto-release | `crates/strangecoin-storage/tests/crash_recovery.rs` (5: `crash_kill_after_{0,1,5,50}_reopens_atomically`, `stale_lock_is_auto_released_after_child_kill`); `tests/rocks_engine.rs::crash_smoke_wal_restores_after_child_process_abort`, `::write_batch_applies_all_100_ops_atomically_across_reopen` | ✅ new (S3-P14) |
| 25 | **Snapshot commitment:** snapshot с неверным checksum/network_id/state-root/tip → import невозможен (fail-closed) | — (snapshot не существовал) | — | — | — | **new (S3-P13, ADR-0026):** `verify_file` (`snapshot.rs:290`): blake3 checksum → network_id → state-root recompute → tip + window linkage; `import` (`:362`) только после verify + `NotFreshDatabase` (не-fresh → reject) | `tests/snapshot_sync.rs` (6: `forged_body_is_rejected`, `corrupt_checksum_is_rejected`, `wrong_network_id_is_rejected`, `import_requires_fresh_db`, ...); unit `snapshot.rs` (7: `forged_state_root_is_rejected`, `bad_magic_is_rejected`, `export_verify_import_round_trip`, `export_is_byte_identical_across_runs`, ...) | ✅ new (S3-P14) |
| 26 | **Pruning boundary:** current state/blocks/meta никогда не прюнятся; прюнится только `state_history` старше height-window (или ничего при archival) | — (pruning не существовал) | — | — | — | **new (S3-P12, ADR-0025):** `sweep` (`pruning.rs:180`) удаляет только `state_history`-записи < tip−retention; `state`/`blocks`/`meta` — нет; `[node] mode = archival` → no-op; expiry-маркеры — только в `state_history`, bounded 1/адрес | `tests/pruning.rs` (6: `sweep_deletes_history_below_window_and_spares_state_blocks_meta`, `archival_node_mode_never_prunes_history`, `expiry_marker_written_for_dormant_address_without_deleting_state`, ...); unit `pruning.rs` (7: `identical_inputs_produce_identical_reports_and_state`, `should_sweep_is_height_based_and_gated`, ...) | ✅ new (S3-P14) |

### Registering notes (S3-P14)

- **#1** — extension: смена backend (LevelDB-блобы → per-block RocksDB-схема) не меняет направление «валидность из цепочки»: state-cache по-прежнему пересобирается из chain, batch-атомарность держит disk-состояние консистентным между crash'ами (evidence — crash-suite + counting-mock `per_block_writes`).
- **#14** — extension: `Drop` flush'ит все CF; LOCK-файл снимается автоматически (stale-LOCK test), ручное удаление по-прежнему не требуется/не предусмотрено.
- **#16/#17** — net-инварианты: Stage 3 не меняет `strangecoin-net` (witness/snapshot — core/storage API, wire-integration в gossip — Stage 4+ residual). Stage 2 extensions остаются в силе, regression green.
- **#23** — guard без auto-upgrade: расхождение версий схемы = отказ открыть (молчаливый downgrade мог бы порча данные). `rusty-leveldb`-чтение legacy — только в рамках one-shot миграции (S3-P08); BF-S3-P01: dev-only tooling, не runtime-путь.
- **#24** — atomicity на уровне WriteBatch (не « fsync per key»); kill-sweep детерминированный (determinism checklist: без wall-clock в триггерах, k — параметр).
- **#25** — fail-closed в обе стороны: подделанный контент отвергается; «свежесть» БД (`NotFreshDatabase`) защищает от перезаписи существующей цепочки snapshot'ом.
- **#26** — boundary — прямое следствие ADR-0025: state expiry (удаление current state) — SCIP-gated и НЕ реализован; Stage 3 трогает только исторические `state_history`-дельты.
- **#20** — degenerate (fee=0) — без изменений; fee market Stage 5.
- **#22** — v3.0.0-stage3 release-pipeline extension приходит с S3-P15; на момент S3-P14 regression опирается на верифицированные runs Stage 1.5/Stage 2.
- Нумерация: baseline 1:1 с ARCHITECT3 §5; #23-#26 — Stage 3 additions из `prompt-stage3.md` S3-P14 (не входят в ARCHITECT3 §5 baseline-таблицу).

## Regression Audit Results (S3-P14, 2026-11-16)

- **Compilation / Type Safety**: ✅ `cargo clippy --all-targets -- -D warnings` exit 0 (в `target/s3p14_clippy.log`)
- **Workspace suite** (чанками, Windows host preamble + `cmd /c` redirect, см. AGENTS.md gotchas): ✅ **все 6 чанков green, 0 failed, 102 suites / 912 passed**:
  - `cargo test -p strangecoin-storage` — exit 0, **5 suites ok, 55 passed** (schema/engine/pruning/snapshot/cache unit + rocks_engine + crash_recovery + write_batch_bench — инварианты #1/#14/#23/#24/#25/#26)
  - `cargo test -p strangecoin` — exit 0, **49 suites ok, 240 passed** (root integration: snapshot_sync/pruning/indices/leveldb_migration/stateless_light_client/mempool_persistence/per_block_writes/lru_cache + regression network/sync/two_clients/rbf/reorg/concurrency/block_executor/config)
  - `cargo test -p strangecoin-core` — exit 0, **15 suites ok, 270 passed** (core consensus/state/receipts/proptest + witness suite — инварианты #2..#6, #11..#13, #15, #18..#21 + witness codec 22)
  - `cargo test -p strangecoin-vm-wasm` — exit 0, **10 suites ok, 155 passed** (VM regression — инварианты #15/#18/#19/#20 gas-половина)
  - `cargo test -p strangecoin-net` — exit 0, **18 suites ok, 192 passed** (network regression — инварианты #16/#17 не тронуты)
  - `cargo test --workspace --doc` — exit 0, **5 suites ok**
  - Счётчики per-chunk — `target/s3p14_summary.txt`; логи — `target/s3p14_{storage,root,core,vmwasm,net,doc}.log`
- **Fuzz**: ⚠️ **не верифицировано локально + причина** — хост Windows, cargo-fuzz недоступен (libFuzzer C++ требует clang/MSVC; ASan unsupported на `x86_64-pc-windows-gnu`); CI-side (9 targets matrix, S2-P21 `ci.yml`). Stage 3 не добавлял fuzz-таргетов. Прецедент FBP08/FBP11.
- **No regressions**: ✅ 26/26 enforced; Stage 3 extensions #1/#14, новые #23-#26 покрыты тестами S3-P06..S3-P13; regression-тесты Stage 0-2 остались green (912 passed, 0 failed).

## Residual / Deferred

| # | Residual | Fix path |
|---|----------|----------|
| 20 | Fee market stub: fee=0, `fee_burned + fee_to_miner` trivially | Stage 5 (EIP-1559-style dynamic base fee; SCIP до mainnet freeze) |
| 19 | True Verkle/KZG (вместо binary SMT depth 256); wire witnesses над Verkle | Stage 3+ (ADR-0006 amended; SCIP) |
| — | State expiry (удаление dormant accounts из current state) | Stage 3+ через SCIP (ADR-0025; Stage 3 — только markers) |
| — | Receipts prune / contract storage prune | Stage 3+ через SCIP |
| — | Wire snapshot sync (GET_SNAPSHOT P2P) | Stage 4+ (`docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md`; ADR-0026 file-based only) |
| — | Wire witness integration в gossip/sync | Stage 4+ (S3-P11 node API only) |
| — | Block-STM parallel execution | Stage 3+ (deterministic parallelISM + differential testing) |
| — | State rent / global state cap | Stage 3+ через SCIP |
| — | TLA+ формальная верификация network protocol | Stage 6 (`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`) |
| — | Erlay minisketch full impl (impl_id=1) | SCIP-gated (carry-over Stage 2) |
| — | Constructor gas/fee burning | Stage 5 fee market |
| — | 2 ops-gate (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) + SCIP-0001 | mainnet freeze (human ops) |

## Historical Context

- **Stage 0**: `docs/stage0/INVARIANTS_ENFORCED.md` (17/5/0)
- **Stage 1**: `docs/stage1/INVARIANTS_ENFORCED.md` (19/0/2/1)
- **Stage 1.5**: `docs/stage1.5/INVARIANTS_ENFORCED.md` (22/0/0 после S1.5-FBP08)
- **Stage 2**: `docs/stage2/INVARIANTS_ENFORCED.md` (22/0/0 после S2-P23: #16/#17 extensions, 20 regression, #20 degenerate)
- **Stage 3**: this document (26/0/0 после S3-P14: #23-#26 новые, #1/#14 Stage 3 extensions, #20 degenerate)
- **Migration**: Strangler — core в `strangecoin-core`, network runtime в `strangecoin-net` (Stage 2), storage runtime в `strangecoin-storage` (Stage 3), монолит — orchestrator

---

**Related**: `docs/stage2/INVARIANTS_ENFORCED.md` · `docs/stage3/THREAT_MODEL_STAGE3.md` · `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` · `docs/security/THREAT_MODEL.md` v3.3 · `analytics/prompt-stage3.md` S3-P14 · ADR-0024..0026 · `docs/ADR/0008-rocksdb-plan.md`
