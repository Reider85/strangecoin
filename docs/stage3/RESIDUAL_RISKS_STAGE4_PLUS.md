# Stage 3 Residual Risks for Stage 4+ (S3-P14)

**Document**: `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md`
**Status**: Complete ✅
**Date**: 2026-11-16
**Prompt**: S3-P14 (`analytics/prompt-stage3.md`)

Regression-перечень residual-рисков Stage 3 после закрытия S3-P01..S3-P13
(ADRs, storage crate + schema/backend, monolith-миграция + авто-миграция
LevelDB→RocksDB, индексы + perf/cache, witness wire codec, pruning
height-window, snapshot export/import). Не дублирует
`docs/stage3/INVARIANTS_ENFORCED.md` §Residual и
`docs/stage3/THREAT_MODEL_STAGE3.md` §4 — ссылается. Продолжает
`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` (carry-over §3 ниже).

## 1. Stage 4+ attack surface (новые риски / отложенный wire-work)

| Risk | Описание | Fix path |
|------|----------|----------|
| Wire snapshot sync (GET_SNAPSHOT P2P) | Snapshot сейчас file-based (ADR-0026): дистрибуция вне протокола → withholding/подмена файла на транспортном слое не детектируется протоколом; P2P-transport потребует нового wire-сообщения, rate-limit class, capability gating, взаимодействия с Erlay/compact (новый attack surface в `strangecoin-net`) | Stage 4+ (ADR-0026 §Transport residual); до этого — operational mirrors + off-band распространение |
| Wire witness integration (gossip/sync relay) | S3-P11 дал node-API (`verify_incoming_block_with_witness`) и canonical codec, но relay-путь gossip/sync пока не грузит/не проверяет witnesses — блоки с отсутствующим/подделанным witness на wire-пути не эксплуатируются только потому, что witness ещё не ходит по сети | Stage 4+ (prompt §6 anti-goals); fuzz witness decoder — там же |
| Snapshot freshness/rollback beyond формата | `verify_file` гарантирует целостность/принадлежность сети, но не «свежесть» относительно честной цепочки (старый валидный snapshot другой высоты принимается; защита — tip linkage + выбор оператором; eclipse-style подмена mirror'а на старый файл) | Stage 4+ (snapshot versioning/height gossip); Split-brain-детект — monitoring |
| Witness decoder fuzzing | Wire codec отрицательно покрыт unit/proptest/golden, но coverage-guided fuzz отсутствует (Stage 3 не добавлял таргетов) | Stage 4+: fuzz target `state_witness_decode` в CI matrix (precedent S2-P21) |

## 2. Stage 3-specific residuals (storage layer)

| Risk | Описание | Fix path |
|------|----------|----------|
| Persistent SMT (trie-узлы в DB) | State root пересчитывается in-memory из state-записей; SMT-узлы не персистятся — каждый cold-open/rebuild платит recompute (performance, не correctness) | Residual ADR-0024 (§Alternatives); не Stage 3 (invariant #1 rebuild сохраняется); пересмотр — SCIP если станет consensus-relevant |
| Pruned `state_history` ограничивает deep re-org/replay-верификацию | Sweep удаляет исторические дельты старше window: глубокий re-org за пределами окна вынужден полным rebuild-from-chain; boundary #26 гарантирует, что current state/blocks целы | Осознанный trade-off ADR-0025 (retention_blocks default 5000 ≫ обычной глубины re-org); archival-режим — полный no-op; tuning — config, не SCIP |
| Snapshot withholding (операционный) | Формат не решает availability: узел без честного файла падает на полный replay (correctness ок, скорость потеряна) | Stage 4 ops: множественные независимые mirrors (ADR-0026); wire-transport — §1 |
| RocksDB C++ toolchain / platform drift | librocksdb-sys (bindgen, C++17) чувствителен к host toolchain; cross-сборка 3 платформ + CI-time bench ratio — не верифицируемы локально (Windows host) | Pin `rocksdb = "=0.25.0"` (SCIP-only update); CI matrix + release pipeline (S3-P15); «не верифицировано + причина» помечено в audit-секциях |
| One-shot миграция — legacy только читается | `rusty-leveldb = "=4.0.0"` остаётся в tree (optional feature `legacy-migrate`) ради единственного read-пути; двойная поддержка форматов — временная | Stage 4+: выпиливание feature + dep после окна миграции (AGENTS.md pin policy: «умирающий деп») |
| LRU cache invalidation surface | Точечный stale-read возможен при будущем коде, пишущем мимо `write_batch` (обход invalidate) | Контракт: все записи — через `Storage::write_batch`; state CF не кэшируется; code-review invariant (S3-P10) |

## 3. Carry-over SCIP-gated (consensus-changing, отложено)

| Risk | Stage | Комментарий |
|------|-------|-------------|
| Verkle/KZG замена SMT | Stage 3+ (SCIP; ADR-0006 amendment) | Stateless scaling; замена SMT — consensus-breaking; Stage 3 = witness-over-SMT (S3-P11) |
| State expiry (dormant accounts из current state) | Stage 3+ (SCIP; ADR-0025) | Stage 3 — только touch-tracking маркеры в `state_history` (данные для будущего SCIP); удаление current state требует wire-witness revival semantics |
| Receipts prune | Stage 3+ (SCIP) | Receipts — derived, не персистятся в Stage 3; prune появится только если receipts станут persisted (carry-over Stage 1.5/2) |
| Contract storage prune | Stage 3+ (SCIP) | Удаление контракта с storage не реализовано (carry-over Stage 1.5/2) |
| State rent / global state cap | Stage 3+ (SCIP) | Глобальный рост state не стягивается; per-contract 1MB cap не закрывает спам множеством контрактов (carry-over) |
| Erlay minisketch full impl (impl_id=1) | SCIP-gated (carry-over Stage 2) | Активная impl — documented stub (`impl_id=0`); minisketch-rs optional default-off (ADR-0021 escape hatch) |
| Fee market / constructor gas burning (инвариант #20) | Stage 5 (SCIP) | fee=0 → degenerate; EIP-1559-style dynamic base fee + burning split (carry-over) |

## 4. Carry-over non-SCIP (Stage 3+ / mainnet track)

| Risk | Stage | Комментарий |
|------|-------|-------------|
| Block-STM parallel execution | Stage 3+ | Serial execution; deterministic parallelISM design + differential testing serial-vs-parallel (carry-over Stage 2 §1; anti-goal Stage 3) |
| TLA+ формальная верификация network protocol | Stage 6 | `docs/spec/consensus.tla` покрывает только консенсус; Noise/gossip/Erlay не формализованы (carry-over Stage 2) |
| External audit | Stage 6 | Security audit track (carry-over Stage 1.5/2) |
| wasmi interpreter 0-days | continuous | Fuzz + differential wasmi vs wasmtime; K-framework — Stage 6 (carry-over Stage 1.5) |
| Контракт-level бизнес-логика | Stage 4+ (SDK, audits) | Sandbox не защищает от «легитимно злонамеренного» кода (carry-over) |
| SCIP-0001 (genesis key replacement) + ops-gates BUG-S1-013 (PGP-key offline keygen) / BUG-S1-015 (offline genesis key air-gapped keygen) | mainnet freeze | Human ops gates; НЕ блокируют Stage 3/4 (regtest/devnet scope); carry-over Stage 1.5/2 |
| Peer scoring weights / seed rotation / TOFU pinning | Stage 4+/6 | Network-операционные residuals (carry-over Stage 2 §2) |

## 5. Anti-goals Stage 4+ (cross-ref)

Полный anti-goal список — `analytics/prompt-stage3.md` §6 + AGENTS.md
§Key Constraints. Не дублируется; ключевые: SDK/devnet/indexer/JSON-RPC
faucet (Stage 4), Account Abstraction + EIP-1559 + threshold encryption
mempool (Stage 5), WIT ABI (Stage 4+), K-framework/ZK-VM/TLA+ network
(Stage 6+), BLS12-381 + bls_aggregate/PoS (Stage 7), metrics/Docker
(Stage 4 operations), multidimensional fees/PBS (Stage 5+/6+ research).

---

**Related**: `docs/stage3/INVARIANTS_ENFORCED.md` · `docs/stage3/THREAT_MODEL_STAGE3.md` · `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` · `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` · `docs/security/THREAT_MODEL.md` v3.3 §7.1d · `analytics/prompt-stage3.md` S3-P14 · ADR-0024..0026
