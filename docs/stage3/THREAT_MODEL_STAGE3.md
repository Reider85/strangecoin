# THREAT_MODEL_STAGE3.md — Stage 3 Storage-Layer Threats (Summary)

**Document**: `docs/stage3/THREAT_MODEL_STAGE3.md`
**Status**: Complete ✅
**Date**: 2026-11-16 (S3-P14)
**Prompt**: S3-P14 (`analytics/prompt-stage3.md`)
**Full model**: `docs/security/THREAT_MODEL.md` v3.3 (§5b — V-43..V-47; §1.1/§6.2d/§6.4 — coverage; §7.1d — residual)

Краткое резюме threat-векторов, актуализированных появлением storage-слоя
Stage 3 (RocksDB backend + миграция, O(1)-индексы + LRU-кэш, witness wire
codec, height-window pruning, file-based snapshot sync — S3-P04..S3-P13 в
`crates/strangecoin-storage/` + monolith edge). Нумерация векторов — общий
threat model; инварианты — `docs/stage3/INVARIANTS_ENFORCED.md` (26/26).

## 1. Закрытые/усиленные векторы (Stage 3 Amendment)

| # | Вектор | STRIDE | Митигация (Stage 3) | Ключевой код | Ключевой тест |
|---|--------|--------|---------------------|--------------|---------------|
| V-43 | Snapshot forging | Tampering / Spoofing | `verify_file` 4-step fail-closed: blake3 checksum → network_id → state-root recompute из accounts → tip hash + window linkage (S3-P13, ADR-0026); `import` только после verify + `NotFreshDatabase` (не-fresh БД → reject); canonical byte-identical export (детерминизм) | `crates/strangecoin-storage/src/snapshot.rs::verify_file:290`, `::import:362` | `tests/snapshot_sync.rs::forged_body_is_rejected`, `::corrupt_checksum_is_rejected`, `::wrong_network_id_is_rejected`, `::import_requires_fresh_db`; unit `snapshot.rs::forged_state_root_is_rejected`, `::bad_magic_is_rejected` |
| V-44 | Snapshot withholding | DoS (availability) | На уровне формата не решаемо: узел без файла падает на полный replay (correctness сохраняется, скорость теряется) — ADR-0026 §Threat mitigations; operational mitigation — множественные независимые snapshot-mirrors (Stage 4 ops); wire-transport для дистрибуции — Stage 4+ residual | `snapshot.rs` (file-based path; нет P2P-transport по дизайну) | Fallback-поведение покрыто `tests/snapshot_sync.rs::snapshot_node_catches_up_via_direct_blocks` (replay-путь работает без snapshot) |
| V-45 | Witness forgery / wire omission | Tampering / Spoofing | `verify_incoming_block_with_witness` → `verify_block_stateless`: parent state root сверка, touched-address completeness, SMT-proof verify (S3-P11); wire codec fail-closed: unknown version / unsorted / truncated / trailing / absurd proof count → reject; golden vector пинит формат | `src/blockchain/blockchain_facade.rs::verify_incoming_block_with_witness:605`, `crates/strangecoin-core/src/state/witness.rs::verify_block_stateless:279`, `::deserialize_state_witness:116` | core `tests/witness.rs` (22: `tampered_balance_rejected`, `forged_state_root_rejected`, `missing_touched_address_rejected`, `witness_codec_rejects_*`, `proptest_tamper_detected`); `tests/stateless_light_client.rs` (3: `corrupted_witness_proof_byte_is_rejected`, ...) |
| V-46 | RocksDB crash divergence | Tampering / DoS | Атомарный WriteBatch (все op'ы или ни одного) + WAL restore; schema-version guard fail-closed (mismatch → `StorageError::SchemaVersion`, без auto-upgrade); kill-sweep k∈{0,1,5,50} + stale-LOCK auto-release; invariant #1 — rebuild from chain остаётся авторитетом при любом disk-состоянии (S3-P06/S3-P10) | `crates/strangecoin-storage/src/rocks.rs::open:84` (guard `:110-129`), `schema.rs::SCHEMA_VERSION:57` | `crates/strangecoin-storage/tests/crash_recovery.rs` (5), `tests/rocks_engine.rs::schema_version_guard_fails_closed_on_mismatch_and_corruption`, `::crash_smoke_wal_restores_after_child_process_abort`, `::write_batch_applies_all_100_ops_atomically_across_reopen` |
| V-47 | Pruning replay weakness | DoS / Tampering | Boundary-invariant #26: `sweep` удаляет только `state_history` старше height-window — current state/blocks/meta не прюнятся; `[node] mode = archival` → полный no-op; expiry-маркеры bounded (1/адрес) в том же history-подпространстве; state expiry (replay ослабляющий удаление current state) — НЕ реализован, SCIP-gated (ADR-0025) | `crates/strangecoin-storage/src/pruning.rs::sweep:180`, `::should_sweep:163` | `tests/pruning.rs::sweep_deletes_history_below_window_and_spares_state_blocks_meta`, `::archival_node_mode_never_prunes_history`, `::expiry_marker_written_for_dormant_address_without_deleting_state`; unit `pruning.rs::identical_inputs_produce_identical_reports_and_state` |

Связанные инварианты (Stage 3 — `docs/stage3/INVARIANTS_ENFORCED.md`):
- **#23** (schema_version guard) — V-46: mismatched version → fail-closed open
- **#24** (crash atomicity) — V-46: kill во время write → reopen консистентен
- **#25** (snapshot commitment) — V-43/V-44: import без верификации невозможен
- **#26** (pruning boundary) — V-47: current state/blocks не прюнятся
- **#1/#14** (rebuild-from-chain; graceful shutdown) — V-46: backend-смена без ослабления

## 1b. Статус Stage 2 residual §1 (cross-ref)

Полный перечень Stage 2 attack surface — `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1 (S2-P23). Статус после S3-P01..S3-P13:

| Stage 2 §1 risk | Stage 3 status | Вектор / evidence |
|-----------------|----------------|-------------------|
| Snapshot sync (forged / withholding / split-brain) | **Mitigated (file-based)**: commitment-верификация до import (V-43), fallback на replay (V-44); wire-transport + split-brain-эксплуатация через P2P — residual Stage 4+ | `tests/snapshot_sync.rs`, ADR-0026 |
| RocksDB migration (crash-recovery divergence) | **Mitigated**: crash-suite + schema guard + atomic batches + rebuild-regression (V-46; инварианты #23/#24) | `crates/strangecoin-storage/tests/crash_recovery.rs`, `tests/rocks_engine.rs` |
| Stateless validation witnesses (wire) | **Partially mitigated**: node-side API + canonical wire-codec + light-client e2e (V-45); интеграция в gossip/sync relay-путь — residual Stage 4+ | core `tests/witness.rs`, `tests/stateless_light_client.rs` |
| State pruning / expiry | **Partially mitigated**: height-window history pruning + boundary (V-47; инвариант #26); удаление current state (expiry) — SCIP-gated residual (ADR-0025) | `tests/pruning.rs` |
| Block-STM parallel execution | **Не адресован (anti-goal Stage 3)**: serial execution; deterministic parallelISM + differential testing — residual Stage 3+ | `analytics/prompt-stage3.md` §6 |

## 2. Новый attack surface (Stage 3)

| Surface | Описание | Митигация / Status |
|---------|----------|---------------------|
| Snapshot file transport (file-based) | Файл передаётся вне протокола (USB/HTTP-mirror/etc.): подмена, порча, replay старого файла на другую сеть | V-43 fail-closed verify (checksum/network_id/state-root/tip); `NotFreshDatabase`; freshness-политика beyond формата — Stage 4+ (wire transport + versioning) |
| Witness wire codec | Новый байтовый формат (`WITNESS_FORMAT_VERSION=1`) на границе приёма: malformed input → OOM/panic/обход verify | Fail-closed decode (version/sort/truncation/trailing/absurd-count reject); size-bounded proofs; golden vector; fuzz-таргет witness — Stage 4+ residual (Stage 3 — proptest + negative unit) |
| RocksDB C++ (librocksdb-sys, bindgen) | C++-зависимость расширяет supply chain (cc-rs, clang headers); API/data-layout churn между версиями | Pin `rocksdb = "=0.25.0"` (ADR-0024, SCIP-only update после mainnet freeze); rebuild-from-chain — защита от порчи data-layout; reproducible builds (#22) покрывают binary |
| LRU block/tx cache (`CachedEngine`) | Stale-read если invalidate пропущен; cache poisoning через crafted keys | Invalidate на write_batch (reorg-удаления покрываются), capacity 0 = off, state CF не кэшируется; unit-тесты hit/invalidate/evict/state-bypass |
| Pruning sweep (height-window) | Ошибочный cutoff (слишком агрессивный) → потеря нужных history; повторный sweep неидемпотентен | ADR-0025 boundary (только `state_history`); детерминированный height-based триггер (без wall-clock); identical-inputs → identical reports (unit); archival no-op |
| One-shot LevelDB→RocksDB миграция | Corrupt legacy → порча при конвертации; divergent chains; прерванная миграция | Parse-all fail-closed → temp + atomic rename → `.leveldb.bak` (rollback); divergent-chain refusal; crash-window resume; `tests/leveldb_migration.rs` (4) |

## 3. Determinism (V-21 extension для storage layer)

Чек-лист детерминизма (`analytics/prompt-stage3.md` §0 + AGENTS.md
§Stage 3 Determinism Checklist) применён к S3-P05 (key codec), S3-P11
(witness codec), S3-P12 (pruning sweep), S3-P13 (snapshot export);
ревью — обязательный пункт КГ каждого промпта.

Ключевые свойства:
- Нет `SystemTime`/`Instant`/`thread_rng`/`OsRng` в encoding/pruning/snapshot-путях; триггеры — по block height (паттерн `tick_at` Stage 2)
- Checksums — blake3 (детерминированный); witness/snapshot кодеки — pure functions
- Key encoding — pure: BE `u64` для heights/nonces; lexicographic order == numeric order; sorted `iterate_prefix`
- Snapshot content детерминирован: accounts сортированы lexicographically; двойной export → byte-identical (`export_is_byte_identical_across_runs`)
- Witness proofs — sorted-by-address, canonical order; golden vector пинит формат
- `cargo test` воспроизводим: round-trip/codec/pruning тесты не зависят от wall clock или HashMap-порядка

## 4. Residual Risks Stage 4+

Полный перечень — `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` (S3-P14).
Ключевые строки: wire snapshot sync (GET_SNAPSHOT, Stage 4+), wire witness
integration в gossip/sync (Stage 4+), Verkle SCIP (ADR-0006 amendment),
state expiry SCIP (ADR-0025), receipts/contract-storage prune SCIP,
Block-STM (Stage 3+), state rent SCIP, TLA+ network (Stage 6), Erlay
minisketch SCIP (carry-over Stage 2), SCIP-0001 + ops-gates (mainnet).

## 5. Tests + Coverage (S3-P04..S3-P13)

| Suite | Покрытие |
|-------|----------|
| `crates/strangecoin-storage/tests/rocks_engine.rs` (6) | Schema guard fail-closed, roundtrip per CF, batch atomicity, prefix order, crash smoke (WAL restore) |
| `crates/strangecoin-storage/tests/crash_recovery.rs` (5) | Kill-sweep k∈{0,1,5,50} атомарность, stale-LOCK auto-release |
| `crates/strangecoin-storage/tests/write_batch_bench.rs` (1) | Batch vs sequential microbench (ratio 719.7x local; CI-time не верифицируемо) |
| `crates/strangecoin-storage/src/{schema,engine,pruning,snapshot,cache}.rs` unit (~43) | Key codec + proptest, MemoryEngine, pruning sweep/marker/determinism, snapshot verify/negative, LRU invalidate |
| `tests/snapshot_sync.rs` (6) | Export/import roundtrip, catch-up, forged/corrupt/wrong-network rejects, fresh-db guard |
| `tests/pruning.rs` (6) | Window delete + spares state/blocks/meta, no-op matrix, markers, disk smoke, adopt-path trigger, archival no-prune |
| `tests/indices.rs` (5) | O(1) lookups, reorg consistency, storage-root cache, backfill |
| `tests/leveldb_migration.rs` (4) | Конвертация, auto-run + .bak + идемпотентность, corrupt fail-closed, resume + divergent refusal |
| `tests/stateless_light_client.rs` (3) + core `tests/witness.rs` (22) | Light verify, corrupt proof / missing address reject, codec golden/negative/proptest |
| `tests/per_block_writes.rs` (1), `tests/mempool_persistence.rs` (2), `tests/lru_cache.rs` (3) | O(changed) adopt, mempool CF delete-on-mine, cache invalidation |

cargo-fuzz локально недоступен (Windows host) — Stage 3 fuzz-таргетов не
добавлял; regression покрытие — CI matrix Stage 1.5/2 targets (S2-P21).
Счётчики workspace-прогона S3-P14 — в `docs/stage3/INVARIANTS_ENFORCED.md`
§Regression Audit Results.

---

**Related**: `docs/security/THREAT_MODEL.md` v3.3 · `docs/stage3/INVARIANTS_ENFORCED.md` · `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` · `docs/stage2/THREAT_MODEL_STAGE2.md` · `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1 · `docs/ADR/0024-storage-schema-and-engine.md` · `docs/ADR/0025-state-pruning-strategy.md` · `docs/ADR/0026-snapshot-sync-format.md` · `analytics/prompt-stage3.md` S3-P14
