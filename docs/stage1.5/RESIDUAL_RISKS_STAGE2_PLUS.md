# Stage 1.5 Residual Risks for Stage 2+ (S1.5-FBP08)

**Document**: `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md`
**Status**: Complete ✅
**Date**: 2026-10-20
**Prompt**: S1.5-FBP08 (`analytics/bugfixes-stage1.5.md`)

Regression-перечень residual-рисков Stage 1.5 после закрытия S1.5-FBP03..FBP07
(selector dispatch, constructor execution, nested value settlement, lazy
contract storage, `receipts_root` в BlockHeader). Не дублирует
`INVARIANTS_ENFORCED.md` §Residual и `THREAT_MODEL_STAGE1_5.md` §Residual —
ссылается.

## 1. Constructor economics (Stage 5)

| Risk | Описание | Fix path |
|------|----------|----------|
| Constructor gas/fee burning | `Create.gas_limit` резервируется pre-execution и meter-ится (S1.5-FBP04), но fees = 0: gas выгорает в OutOfGas, fee не сжигается и не платится майнеру — deploy-спам контрактами остаётся бесплатным (за исключением cost of PoW блока, не internalized на tx-level) | Stage 5 fee market (EIP-1559-style dynamic base fee + fee burning split); SCIP до mainnet freeze |

## 2. State pruning (Stage 3+, через SCIP)

| Risk | Описание | Fix path |
|------|----------|----------|
| Receipts prune | Receipts sub-trie (`receipts_subroot`, key = txid) только растёт — старые receipts никогда не удаляются; хранение O(все tx) у всех full-нод | Stage 3+ receipts prune через SCIP (consensus-changing: `receipts_root` в headers фиксирует полный набор — prune меняет верифицируемость старых receipts, поэтому только activation height) |
| Contract storage prune | Удаление контракта с storage не реализовано; per-contract cap `MAX_STORAGE_PER_CONTRACT`=1MB не останавливает спам множеством контрактов (каждый со своим storage) | Stage 3+ contract storage prune + опциональный state rent (activation height; ADR/SCIP до mainnet freeze) |

## 3. Смежные residuals (cross-ref, без дублирования)

| Risk | Где зафиксировано |
|------|------------------|
| Fee market stub (инвариант #20, degenerate fee=0) | `INVARIANTS_ENFORCED.md` #20 |
| State rent / global state cap | `THREAT_MODEL_STAGE1_5.md` §3 Residual |
| Verkle/KZG (вместо binary SMT depth 256) | `INVARIANTS_ENFORCED.md` #19 residual; ADR-0006 amended |
| Block-STM, AA, EIP-1559, WIT ABI, RocksDB, Noise/Erlay, K-framework, ZK-VM, BLS12-381, PoS | anti-goals Stage 1.5: `docs/stage1.5/STAGE1_5_SUMMARY.md`, `analytics/prompt-stage1.5.md` §7 |
| 2 ops-gate (BUG-S1-013 PGP-key, BUG-S1-015 offline genesis key) + SCIP-0001 (genesis key replacement) | `docs/stage1.5/STAGE1_5_SUMMARY.md`, `analytics/bugfixes-stage1.md` |

---

**Related**: `docs/stage1.5/INVARIANTS_ENFORCED.md` · `docs/stage1.5/THREAT_MODEL_STAGE1_5.md` · `docs/stage1.5/STAGE1_5_SUMMARY.md` · `analytics/bugfixes-stage1.5.md` S1.5-FBP08
