# Stage 2 Residual Risks for Stage 3+ (S2-P23)

**Document**: `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md`
**Status**: Complete ✅
**Date**: 2026-10-29
**Prompt**: S2-P23 (`analytics/prompt-stage2.md`)

Regression-перечень residual-рисков Stage 2 после закрытия S2-P01..S2-P22
(ADRs, crate scaffolding + миграция, Noise, peer management, gossip/compact/
Erlay, rate limiting + scoring, cleanup + async, shadow tooling, fuzz, интеграционные
тесты). Не дублирует `docs/stage2/INVARIANTS_ENFORCED.md` §Residual и
`docs/stage2/THREAT_MODEL_STAGE2.md` §4 — ссылается. Продолжает
`docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` (carry-over §3 ниже).

## 1. Stage 3 attack surface (новые риски, вводимые Stage 3)

| Risk | Описание | Fix path |
|------|----------|----------|
| Snapshot sync | Быстрый bootstrap через полный state-snapshot вместо replay-верификации всей цепочки: узел доверяет снимку → новые векторы (forged snapshot, snapshot withholding, split-brain между snapshot- и replay-нодами) | Stage 3; требует отдельного threat-моделинга + верификации snapshot- commitment (state root + height) перед применением |
| RocksDB migration | LevelDB → RocksDB: crash-recovery-semantics меняются (WAL, compaction, column families); возможны расхождения derived-state rebuild vs disk state | Stage 3; crash-recovery test suite + invariant #1 (rebuild from chain) regression на новом backend |
| Stateless validation witnesses (wire) | Witness API есть в ядре (S1-P07, V-36 mitigated core-side), но не в протоколе: переход к wire- witnesses добавляет вектор witness forgery / omission на relay-пути | Stage 3; proof verification на границе приёма, fuzz witness decoder |
| Block-STM parallel execution | Параллельное исполнение транзакций: interleaving-баги (read/write set conflicts, deterministic ordering) могут изменить gas/receipts vs serial → consensus fork | Stage 3+; deterministic parallelISM design + differential testing serial-vs-parallel |
| State pruning / expiry | Удаление старых state/receipts ослабляет replay-verifiability у полных нод (EIP-4444 analog) | Stage 3+ через SCIP (consensus-changing) |

## 2. Stage 2-specific residuals (network layer)

| Risk | Описание | Fix path |
|------|----------|----------|
| Erlay minisketch full impl | Активная impl — documented capacity-bounded stub (`impl_id=0`); реальный PinSketch (minisketch-rs =0.1.9, optional default-off) не включён (нет libclang на Windows-хосте, bindgen) | SCIP-gated включение `impl_id=1`; CI-верификация minisketch-rs сборки; ADR-0021 escape hatch |
| TLA+ формальная верификация network protocol | `docs/spec/consensus.tla` покрывает только консенсус (35,207 состояний PASS); Noise/gossip/Erlay/state-machine протоколы не формализованы | Stage 6 (external audit + formal verification track) |
| Seed rotation (автоматическая) | Ротация hard-coded seed IPs — ручная через network.json; автоматическая rotation (V-16 Stage 0 residual) не реализована | Stage 6+ или SCIP, если ротация станет consensus-relevant |
| TOFU unpinned first contact | Первая связь к unpinned peer принимается как unverified (MITM на первой встрече возможен); pinning — per-node_id | Monitoring + pinning hardening (Stage 4+ operational guidance) |
| Ephemeral из long-term seed | HKDF-вывод эфемерных ключей даёт reproducibility (чек-лист детерминизма), но компрометация `keystore/node` seed раскрывает записанные сессии узла | Осознанный trade-off (S2-P09); rotation node seed — ops-procedure; formal review — Stage 6 audit |
| Plaintext-клиенты Stage 1.5 несовместимы | Единый Noise-транспорт (S2-P19) ломает Stage 1.5 plaintext-пиров | Допустимо до тега `v2.0.0-stage2` (mainnet не запущен); Changelog-документирование — S2-P24 |
| Peer scoring weights | Начальные веса/пороги (ADR-0022) не tuning'ались на реальной сети (нет mainnet telemetry) | Stage 4+ metrics (Prometheus) → re-tune via SCIP если станет protocol-relevant |

## 3. Carry-over из Stage 1.5 (`docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md`)

| Risk | Stage | Комментарий |
|------|-------|-------------|
| Constructor gas/fee burning | Stage 5 | `Create.gas_limit` резервируется, но fees=0 — deploy-спам бесплатен (кроме PoW cost); fee market + burning split (SCIP до mainnet freeze) |
| Receipts prune | Stage 3+ (SCIP) | Receipts sub-trie только растёт; prune меняет верифицируемость старых receipts |
| Contract storage prune | Stage 3+ (SCIP) | Удаление контракта с storage не реализовано; per-contract 1MB cap не останавливает спам множеством контрактов |
| State rent / global state cap | Stage 3+ | Глобальный рост state не стягивается |
| Fee market stub (инвариант #20) | Stage 5 | fee=0 → degenerate; EIP-1559-style dynamic base fee (SCIP) |
| Verkle/KZG (вместо binary SMT depth 256) | Stage 3+ (ADR-0006 amended) | Stateless scaling; замена SMT — consensus-breaking |
| wasmi interpreter 0-days | continuous | Fuzz (S1.5-P16) + differential wasmi vs wasmtime; K-framework — Stage 6 |
| Контракт-level бизнес-логика | Stage 4+ (SDK, audits) | Sandbox не защищает от «легитимно злонамеренного» кода |
| 2 ops-gate (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) + SCIP-0001 (genesis key replacement) | mainnet freeze | Human ops gates; НЕ блокируют Stage 2/3/4 (regtest/devnet scope) |

## 4. Anti-goals Stage 3+ (cross-ref)

Полный anti-goal список — `analytics/prompt-stage2.md` §7 + AGENTS.md
§Key Constraints. Не дублируется; ключевые: Account Abstraction (Stage 5),
EIP-1559 (Stage 5), threshold encryption mempool (Stage 5), WIT ABI (Stage 4+),
BLS12-381 + bls_aggregate (Stage 7), PoS migration (Stage 7), ZK-VM (Stage 6+),
multidimensional fees / PBS (Stage 6+ research).

---

**Related**: `docs/stage2/INVARIANTS_ENFORCED.md` · `docs/stage2/THREAT_MODEL_STAGE2.md` · `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` · `docs/security/THREAT_MODEL.md` v3.2 §7.1c · `analytics/prompt-stage2.md` S2-P23 · ADR-0020..0023
