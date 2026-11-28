# Stage 3 DoD Verification (S3-P15)

**Document**: `docs/stage3/STAGE3_SUMMARY.md`
**Status**: Complete ✅ (annotated tag `v3.0.0-stage3` — ставится на этот коммит; release run — §7)
**Date**: 2026-11-16
**Prompt**: S3-P15 — Definition of Done verification for Stage 3 (RocksDB storage + stateless + pruning + snapshot)
**Prompt refs**: S3-P01..S3-P15 (`analytics/prompt-stage3.md`)
**Gate**: Tag `v3.0.0-stage3` (Gate Stage 4 dev-experience / Stage 5+)
**Predecessor**: Tag `v2.0.0-stage2` (`346e3c0`, S2-P24)

---

## 1. DoD Criteria — 11 items (ROADMAP3 Этап 3 §Definition of Done)

Нумерация 1:1 с `analytics/ROADMAP.md` §Definition of Done (§Этап 3; файла `ROADMAP3.md` нет — переименован). ROADMAP-план ADR-0020/0021/0022 → фактические ADR-0008+0024 / 0025 / 0026 (реnumbering-прецедент Stage 2).

| # | Criterion | Evidence | Status |
|---|-----------|----------|--------|
| 1 | RocksDB миграция завершена, LevelDB удалён | monolith на `strangecoin-storage` (`src/storage/`, S3-P07 `9c4c5de`); one-shot авто-миграция `src/storage/migrations.rs` + `.leveldb.bak` rollback (S3-P08 `d11221f`); `storage.engine` config-key / leveldb engine / `legacy.rs` удалены; runtime-движок = RocksDB. BF-S3-P01 (post-tag follow-up): `rusty-leveldb` переведён в dev-deps, `legacy-migrate` opt-in (`--features legacy-migrate`); reader в `tests/common/legacy_reader.rs` + пример `migrate-leveldb`; `cargo tree --edges no-dev -i rusty-leveldb` пуст; авто-миграция в узле заменена fail-closed проверкой. Tests: `tests/leveldb_migration.rs` (4), `tests/address_migration.rs` (feature-gated) | ✅ (deviation closed by BF-S3-P01) |
| 2 | Stateless validation работает (test: light client верифицирует block без полного state) | `tests/stateless_light_client.rs` (S3-P11 `872e42e`, 3 теста: `light_client_verifies_block_without_full_state`, `corrupted_witness_proof_byte_is_rejected`, `witness_missing_touched_address_is_rejected`) + core `crates/strangecoin-core/tests/witness.rs` (codec round-trip/golden/negative/proptest) + facade `verify_incoming_block_with_witness` (`src/blockchain/blockchain_facade.rs`) | ✅ |
| 3 | State pruning работает (test: disk usage уменьшается после N блоков) | `tests/pruning.rs` (S3-P12 `93d67c6`, 6 тестов: `sweep_deletes_history_below_window_and_spares_state_blocks_meta`, no-op matrix (archival/disabled/zero-window), expiry-marker lifecycle, `disk_usage_smoke_pruned_key_count_below_control` — keys-count smoke (deviation: bytes из-за async compaction), adopt-path trigger e2e, `archival_node_mode_never_prunes_history`) + unit `crates/strangecoin-storage/src/pruning.rs` | ✅ |
| 4 | Snapshot sync работает (test: new node синхронизируется без full chain download) | `tests/snapshot_sync.rs` (S3-P13 `43f3d34`, 6 тестов: `export_import_restores_state_and_tip`, `snapshot_node_catches_up_via_direct_blocks`, `forged_body_is_rejected`, `corrupt_checksum_is_rejected`, `wrong_network_id_is_rejected`, `import_requires_fresh_db`) — commitment-verified import, ADR-0026 | ✅ |
| 5 | Indices работают (test: O(1) lookups) | `tests/indices.rs` (S3-P09 `215123f`: `indices_written_on_adopt_and_lookups_are_o1` — txid→(height,index)+raw, hash→height через point-get; `reorg_keeps_indices_consistent`; `contract_storage_root_cache_tracks_flushes`; `backfills_indices_for_pre_index_databases`; `#[ignore] bench_10k_tx_lookups`). **Deviation (FBP, S3-P09)**: литеральный «address → balance» отдельным индексом не создаётся — state CF point-get уже O(1) | ✅ (deviation documented) |
| 6 | Performance: batch validation ≥10x быстрее sequential | `crates/strangecoin-storage/tests/write_batch_bench.rs` (S3-P10 `009be0e`) — 1000 account-upsert: sequential put+flush **37.824s** vs один write_batch+flush **52.557ms** = **ratio 719.7x** (≥10x). Свежий прогон S3-P15: 37.65s sequential-leg, 1 passed. **Deviation (FBP)**: test-based microbench вместо criterion; CI-time ratio не верифицируем локально (прецедент FBP) | ✅ (local) |
| 7 | `crates/strangecoin-storage/` создан, storage перенесён | workspace member `crates/strangecoin-storage/` (S3-P04 `6a32ff3`: scaffolding + `StorageEngine` trait + pin rocksdb `=0.25.0` + vendor minisketch-rs); monolith wiring (S3-P07: `src/storage/{mod,codec}.rs` re-export + state_cache на движке) | ✅ |
| 8 | All Stage 0-2 invariants still enforced | `docs/stage3/INVARIANTS_ENFORCED.md` (S3-P14 `697d1c8`) — **26/26**: 22 baseline Stage 0-2 regression green + #23 schema-version guard, #24 crash atomicity, #25 snapshot commitment, #26 pruning boundary (+ Stage 3 extensions #1/#14) | ✅ |
| 9 | ADR «Why RocksDB instead of redb» написан | `docs/ADR/0008-rocksdb-plan.md` (Accepted, Stage 0) + аменда `docs/ADR/0024-storage-schema-and-engine.md` (S3-P01 `4912d75` — schema + StorageEngine trait) | ✅ (renumbered: ROADMAP-план ADR-0020 → 0008/0024) |
| 10 | ADR «State pruning strategy» написан | `docs/ADR/0025-state-pruning-strategy.md` (S3-P02 `aa33edc` — height-window + archival + SCIP-path) + `docs/ADR/0026-snapshot-sync-format.md` (S3-P03 `766dd73` — snapshot format; ROADMAP «ADRs required») | ✅ (renumbered: ROADMAP ADR-0021/0022 → 0025/0026) |
| 11 | `Changelog.md` обновлён: `3.0.0 — RocksDB + stateless + pruning` | секция `## 3.0.0 — RocksDB storage + stateless validation + pruning + snapshot sync` (этот коммит) + annotated tag `v3.0.0-stage3` + версии root/core 3.0.0 | ✅ (в этом коммите) |

**Итог: 11/11 ✅** (2 с documented deviations — #1 и #5; #6 локально, CI-side не верифицируем).

Плюс: тег `v3.0.0-stage3` — **Gate Stage 4 (dev-experience + AI agents) / Stage 5+**; dependency «indexer/JSON-RPC требуют storage crate» закрыта.

---

## 2. What was delivered (S3-P01 → S3-P14)

| Prompt | Commit | Date | Summary |
|--------|--------|------|---------|
| S3-P01 | `4912d75` + `f5c80e3` | 2026-10-30 | ADR-0024: storage schema + StorageEngine trait (аменда ADR-0008) + githooks prompt-id |
| S3-P02 | `aa33edc` | 2026-10-30 | ADR-0025: state pruning (height-window, archival mode, SCIP-path для expiry) |
| S3-P03 | `766dd73` | 2026-10-30 | ADR-0026: snapshot sync format (commitment-verified, file-based) |
| S3-P04 | `6a32ff3` | 2026-10-30 | Crate scaffolding `crates/strangecoin-storage/` + workspace member + pin rocksdb `=0.25.0` + vendor minisketch-rs |
| S3-P05 | `50e52f9` | 2026-11-07 | `schema.rs`: KeyPrefix index-subspace tags + BE key constructors + `SCHEMA_VERSION=1` + proptests |
| S3-P06 | `a9ea7ee` | 2026-11-07 | `rocks.rs` RocksDbEngine: 7 CF open, schema-guard fail-closed, атомарный WriteBatch, prefix-iter, all-CF flush, subprocess crash smoke |
| S3-P07 | `9c4c5de` | 2026-11-08 | Monolith → storage: per-block incremental saves, mempool CF, `state_history` pre-apply, `[storage] path` wiring |
| S3-P08 | `d11221f` | 2026-11-08 | One-shot LevelDB→RocksDB авто-миграция (fail-closed, temp+atomic rename, `.leveldb.bak`); `rusty-leveldb` → optional `legacy-migrate` |
| S3-P09 | `215123f` | 2026-11-14 | O(1) индексы: txid→(height,index)+raw, hash→height, contract storage-root кэш; reorg Phase A/B + backfill |
| S3-P10 | `009be0e` + `bc468f1` | 2026-11-14 | Batch microbench 719.7x + crash-recovery kill-sweep + stale-LOCK + `CachedEngine` LRU; rustfmt drift cleanup |
| S3-P11 | `872e42e` + `074896d` | 2026-11-14 | Witness wire codec `WITNESS_FORMAT_VERSION=1` + light-client e2e; style cleanup |
| S3-P12 | `93d67c6` | 2026-11-15 | Pruning: height-window sweep `state_history` + archival no-op + expiry markers + `[pruning]`/`[node] mode` |
| S3-P13 | `43f3d34` | 2026-11-15 | Snapshot: export/import + 4-step commitment verify fail-closed + sync e2e |
| S3-P14 | `697d1c8` | 2026-11-16 | Invariants 26/26 + THREAT_MODEL v3.3 (V-43..V-47) + RESIDUAL_RISKS_STAGE4_PLUS |

Замечания:
- Стиль-коммиты `bc468f1` (fmt drift S3-P10) и `074896d` (fmt drift S3-P11) — вне промптового scope (§Common Gotchas).
- Параллельность: ADRs S3-P01..S3-P03 ∥; crate S3-P04→P05→P06 и migration S3-P07..P10 — sequential; feature S3-P11∥P12∥P13 после S3-P10; verification S3-P14→S3-P15 строго после всех (гейты §3 prompt — соблюдены).

---

## 3. Verification runs (2026-11-16, S3-P15)

| Run | Command | Result |
|-----|---------|--------|
| Storage crate | `cargo test -p strangecoin-storage` | **green** — 4 suites + doc: 43 lib + 5 crash_recovery + 6 rocks_engine + 1 write_batch_bench = **55 passed, 0 failed** (bench sequential-leg 37.65s) |
| Root integration | `cargo test -p strangecoin -j 2` | **green** — **49 suites, 240 passed, 0 failed** (`EXITCODE=0`, `target/s3p15_root.log`); `-j 2` — лечение parallel-link gotcha (§Common Gotchas, прецедент S3-P08) |
| Core | `cargo test -p strangecoin-core -j 2` | **green** — **15 suites, 270 passed, 0 failed** (`CORE_EXIT=0`, `target/s3p15_core.log`) |
| VM | `cargo test -p strangecoin-vm-wasm -j 2` | **green** — **10 suites, 155 passed, 0 failed** (`VM_EXIT=0`, `target/s3p15_vm.log`; первый на хосте сбор wasmtime 50.0.1 — dev-dep differential oracle, dependabot-бамп) |
| Network crate | `cargo test -p strangecoin-net -j 2` | **green** — **18 suites, 192 passed, 0 failed** (`NET_EXIT=0`, `target/s3p15_net.log`) |
| Doc tests | `cargo test --workspace --doc` | **green** — 5 suites, 0 doctests (`DOC_EXIT=0`, `target/s3p15_doc.log`) |
| Lint | `cargo clippy --all-targets -- -D warnings` | **green** — exit 0, 0 warnings (`CLIPPY_EXIT=0`, `target/s3p15_clippy.log`, 57.4s) |

**Итог S3-P15**: 6 чанков + clippy — все **exit 0**, суммарно **101 suite / 912 passed / 0 failed** (55+240+270+155+192+0; совпадает с регресс-базой S3-P14).

Тестовое окружение: Windows, `stable-x86_64-pc-windows-gnu` (gcc linker, `.cargo/config.toml`), Windows host preamble (`C:\msys64\mingw64\bin` + `CARGO_HOME=C:\rt\cargo_home`), чанки через `cmd /c "... > target\s3p15_*.log 2>&1"` + detached `Start-Process` (харнесс убивает pipe-команды на ~5 мин — §Common Gotchas; root-чанк пережил первый kill через bat-повтор с `-j 2`).

Регресс-база: S3-P14 (тот же HEAD до bump'а версий) — 102 suites / 912 passed / 0 failed чанками; S3-P15 подтверждает на версиях 3.0.0.

---

## 4. Residual obligations for Stage 4+ (зафиксировано S3-P15)

| # | Obligation | Stage | Source |
|---|-----------|-------|--------|
| 1 | Wire snapshot sync (GET_SNAPSHOT P2P; file-based S3-P13 — baseline) | Stage 4+ | `prompt-stage3.md` §6; `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` |
| 2 | Wire witness integration в gossip/sync (S3-P11 — node API только) | Stage 4+ | `prompt-stage3.md` §6; S3-P11 deviation |
| 3 | Verkle trie замена SMT (stateless через Verkle) | **SCIP** (ADR-0006 amendment) | ADR-0024 §Decision; anti-goals §6 |
| 4 | State expiry (dormant account removal из current state) | **SCIP** (ADR-0025) | S3-P12 markers-only; anti-goals §6 |
| 5 | Receipts prune / contract storage prune | **SCIP** | Stage 1.5/2 residuals; anti-goals §6 |
| 6 | State rent | **SCIP** (optional) | prompt §6; ROADMAP §Задачи |
| 7 | Block-STM (parallel execution) | Stage 3+/6 | ARCHITECT §15; RESIDUAL_RISKS |
| 8 | Erlay full minisketch (`impl_id=1`, minisketch-rs) | **SCIP** (carry-over Stage 2) | ADR-0021 escape hatch |
| 9 | TLA+ формальная верификация network protocol | Stage 6 | S2-P23 residual |
| 10 | Выпиливание `legacy-migrate` reader (`rusty-leveldb`) | ✅ закрыто BF-S3-P01 (dev-deps + tooling example; авто-миграции в узле нет) | `analytics/BUGFIXES-stage3.md` BF-S3-P01 |
| 11 | SDK, devnet, indexer, explorer, JSON-RPC eth_*, faucet, cargo-strangecoin, HD wallets, Ledger + AI/UI extension (MCP/ACP/ADR-0028..0031) | Stage 4 | `analytics/ROADMAP.md` §Этап 4; prompt §6 |
| 12 | Account Abstraction, EIP-1559, threshold encryption mempool, fee market (инвариант #20) | Stage 5 | ARCHITECT §15; INVARIANTS #20 |
| 13 | External audit, K-framework, ZK-VM, multidimensional fees, PBS | Stage 6+ | prompt §6; RESIDUAL_RISKS |
| 14 | PoS/Casper, BLS12-381 + `bls_aggregate`, inclusion lists, validator ops | Stage 7 | prompt §6 |
| 15 | **BUG-S1-013** (PGP-key offline keygen), **BUG-S1-015** (offline genesis key air-gapped keygen) | **mainnet freeze** (human ops; НЕ блокируют Stage 4 — regtest/devnet scope) | `docs/stage1.5/STAGE1_5_SUMMARY.md` §4 |
| 16 | **SCIP-0001** (genesis key replacement) | отдельный ops+code трек; единственный блокер mainnet | Stage 2 carry-over |

---

## 5. Anti-goals Stage 3 — подтверждение «не реализовано» (prompt-stage3 §6)

Все явно отложенные пункты **не реализованы** в Stage 3 (сверено против кода на теге): Verkle trie (✗ — witness-over-SMT, SCIP), state expiry из current state (✗ — только markers S3-P12, SCIP), receipts/contract-storage prune (✗), state rent (✗), Block-STM (✗ — serial execution), wire snapshot GET_SNAPSHOT (✗ — file-based S3-P13), wire witness в gossip/sync (✗ — node API), SDK/devnet/indexer/explorer/JSON-RPC/faucet/cargo-strangecoin/HD/Ledger (✗ — Stage 4), AA/EIP-1559/threshold encryption mempool (✗ — Stage 5), WIT Component Model (✗ — `format_version: "wit-v2"` placeholder), K-framework/ZK-VM/external audit/TLA+ network (✗ — Stage 6), BLS12-381 + `bls_aggregate` impl (✗ — NotImplemented, Stage 7), PoS/Casper (✗ — Stage 7), multidimensional fees/PBS/batch auctions (✗ — Stage 5+/6+), metrics/Docker/systemd (✗ — Stage 4 operations), wallet в отдельный крейт (✗ — Stage 4+; Stage 3 scope = storage), персистентный SMT trie-узлов в DB (✗ — residual ADR-0024; invariant #1 rebuild сохраняется), Erlay full minisketch `impl_id=1` (✗ — SCIP carry-over), SCIP-0001 + BUG-S1-013/015 (✗ — ops, вне Stage 3 scope).

Полный список — `analytics/prompt-stage3.md` §6 + `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md`.

---

## 6. Process notes

1. **ARCHITECT §17 checklist** — прогнан в рамках S3-P15 self-review (см. §6.1).
2. **«Done» только через этот DoD-промпт** (retro §8.1): тег `v3.0.0-stage3` ставится после прохождения всех КГ S3-P15; до этого момента Stage 3 считался in progress (AGENTS.md §Active Stage).
3. **Consensus-boundary / breaking**: storage — implementation detail (не consensus) до mainnet; storage format v1, LevelDB removal, `storage.engine` removal, witness wire codec зафиксированы в Changelog 3.0.0 как breaking (pre-mainnet); после тега — только SCIP + activation height. Pruning current-state / state expiry / receipts prune / Verkle — SCIP-path в ADR-0024/0025.
4. **Determinism** (V-21 extension): ревью выполнено в status-блоках S3-P05/P11/P12/P13 (AGENTS §Stage 3 Determinism Checklist): key-encoding pure, prune/snapshot триггеры height-based, witness codec sorted; `cargo test` воспроизводим (нет wall-clock/RNG в storage-путях).
5. **Pin policy Stage 3**: `rocksdb = "=0.25.0"`, `rusty-leveldb = "=4.0.0"` (BF-S3-P01: только dev-deps), `lru = "=0.19.0"`, `proptest = "=1.11.0"` — обновления через SCIP после mainnet freeze (AGENTS §Stage 3 Pin Policy).
6. **Отклонения от literal спеки** (FBP, зафиксированы явно): (a) DoD #1 «LevelDB удалён» → optional reader остался (S3-P08 КГ-отклонение) — закрыто BF-S3-P01 (reader в dev-deps, авто-миграция в узле убрана, конверсия через tooling example); (b) DoD #5 «address → balance index» → state CF point-get уже O(1), отдельный индекс не создаётся (S3-P09); (c) DoD #6 — test-based microbench, ratio **719.7x** локально, CI-time не верифицируем (S3-P10); (d) ROADMAP ADR-0020/0021/0022 → фактически ADR-0008+0024/0025/0026 (реnumbering-прецедент Stage 2); (e) `analytics/ROADMAP3.md`/`ARCHITECT3.md` не существуют — переименованы в `ROADMAP.md`/`ARCHITECT.md` (H1 сохранён); ссылки в AGENTS.md актуализированы этим коммитом; (f) root-чанк прогонялся с `-j 2` (parallel-link gotcha S3-P08); (g) release-pipeline verification — отдельный коммит после тега (прецедент S2-P24 deviation (d)); `gh` CLI отсутствует → REST unauthenticated (прецедент S1.5-FBP11/S2-P24); (h) AGENTS.md NOTE/секции — формулировка «Stage 3 complete (tag `v3.0.0-stage3`)» без «pending S3-P15» (тег создаётся в этом же коммите).
7. **Cross-reference — Stage 2 closure**: тег `v2.0.0-stage2` → `346e3c0` verified; precondition Stage 3 (§0.1 prompt) сохранялся весь Stage 3.

### 6.1 ARCHITECT §17 checklist (прогнан после S3-P15)

| Checklist item | Status | Evidence |
|----------------|--------|----------|
| Чистое ядро: serialize/consensus/state/economics/governance — 0 I/O | ✅ regression | core не получал I/O; witness codec — pure (`state/witness.rs`) |
| Инварианты 1-22 enforce | ✅ | `docs/stage3/INVARIANTS_ENFORCED.md` — 26/26 (22 baseline + #23..#26) |
| Threat model: новые векторы → митигации | ✅ | V-43..V-47 (`docs/stage3/THREAT_MODEL_STAGE3.md`) + `THREAT_MODEL.md` v3.3 (S3-P14) |
| Events bus: через EventBus | ✅ regression | Stage 1/2 код не тронут Stage 3 |
| Blockchain декомпозиция (5 компонентов) | ✅ regression + strangler | facade/executor/state_cache сохранены; persistence вынесена в `strangecoin-storage` |
| Sync engine: разрыв цикла network↔blockchain | ✅ regression | trait `BlockchainView` (S2-P06) — Stage 3 не трогает network |
| VM trait в core, runtime отдельно | ✅ regression | stateless baseline — transfer-only verify (documented S3-P11), VM-путь не менялся |
| Protocol messages: deprecated legacy удалён | ✅ regression | Stage 2 S2-P18 — не тронуто |
| Peer store/manager/connection_pool разделены | ✅ regression | Stage 2 — не тронуто |
| Tie-breaking: work → timestamp → hash | ✅ regression | `chain_selector.rs` — не тронут |
| Mempool RBF policy | ✅ regression | `mempool/` — не тронут (mempool CF — только persistence) |
| **Storage schema versioned + migrations** | ✅ **new** | `SCHEMA_VERSION=1` guard fail-closed (S3-P05/P06) + `migrations.rs` авто-миграция (S3-P08) |
| Wallet: public API only, path to separate crate documented | ✅ regression | wallet не тронут; вынос — Stage 4+ |
| Economics: tail emission, fee invariant | ✅ regression / #20 deferred | emission Stage 1; fee-split → Stage 5 |
| Governance: SCIP + activation height | ✅ | SCIP-path для Verkle/expiry/prune в ADR-0024/0025 |
| Security: TLA+ актуализирован, fuzzing targets | ✅ regression | `consensus.tla` unchanged (35,207 states PASS); fuzz targets Stage 1.5/2 сохранены; Stage 3 — crash-suite/kill-sweep как storage-fuzz замена (wire-input фаззинг не затронут) |
| Anti-goals respected (§15) | ✅ | §5 выше |
| License: Apache-2.0, LICENSE в корне | ✅ regression | Stage 0 |
| Reproducible builds: CI gate, cosign signature | ✅ regression + v3 run | `release.yml` verified на v2.0.0-stage2; v3.0.0-stage3 run — §7 |
| Strangler pattern: перенос подсистемы в отдельный крейт | ✅ | `crates/strangecoin-storage/` (S3-P04..S3-P07) |
| Tests: unit + property (proptest) + integration | ✅ | schema/witness proptests + feature-e2e (`stateless_light_client`, `pruning`, `snapshot_sync`, `indices`, `leveldb_migration`, `per_block_writes`) + regression Stage 0-2 |
| CI: clippy -D warnings, all platforms | ✅ regression + local | `ci.yml` (clippy job) + локальный clippy §3 |
| Documentation: ADR для решений, SCIP для протокола | ✅ | ADR-0024/0025/0026 (S3-P01..P03) + ADR-0008/0012..0023 (Stage 0-2) |

---

## 7. Release (S3-P15)

- **Tag**: annotated `v3.0.0-stage3` — ставится на коммит `[S3-P15] release: stage3 dod verification + summary + tag v3.0.0-stage3` (этот коммит); message: Stage 3 complete, prompts S3-P01..S3-P15, DoD 11/11, gate Stage 4/5. Push коммита + тега подтверждается `git ls-remote --tags origin` (запись — follow-up коммит §7.1, прецедент S2-P24 `db116f9`).
- **Versions**: root `3.0.0`, `strangecoin-core` `3.0.0`, `strangecoin-storage` `0.1.0` (первый release — unchanged), `strangecoin-vm-wasm` `0.2.0`, `strangecoin-net` `0.1.0` (unchanged); `strangecoin-net` path-dep на core → `version = "3.0.0"` (прецедент S2-P24 deviation (c)); `Cargo.lock` + `fuzz/Cargo.lock` синхронизированы.
- **Release pipeline** (`release.yml`, 6 matrix + cosign + SLSA L3): trigger'ится тегом; статус/run id — **§7.1 (follow-up после push)**; `gh`/`cosign`/`slsa-verifier` не установлены на Windows-хосте → verification через GitHub REST API unauthenticated (прецедент FBP11/S1.5-FBP11/S2-P24); до записи §7.1 — «не верифицировано локально + причина».
- **Platform builds rocksdb (3 ОС)**: cross-build CI-side; локально верифицируется только `x86_64-pc-windows-gnu` (§3 green) — «не верифицируемо локально + причина: хост один, CI matrix» (прецедент FBP08).

### 7.1 Verification follow-up

*(заполняется отдельным коммитом после push тега: tag object hash, ls-remote confirmation, release run id/conclusion, assets — прецедент S2-P24 `db116f9`.)*

---

**Related**: `docs/stage3/INVARIANTS_ENFORCED.md` · `docs/stage3/THREAT_MODEL_STAGE3.md` ·
`docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` · `docs/ADR/0024..0026` · `docs/ADR/0008` ·
`analytics/prompt-stage3.md` S3-P15 · `docs/security/THREAT_MODEL.md` v3.3 ·
`docs/stage2/STAGE2_SUMMARY.md` · `docs/security/REPRODUCIBLE_BUILDS.md`
