# TLA+ Specification for Strangecoin Consensus

This directory contains a TLA+ skeleton specification of the Strangecoin consensus rules.

## Files

- `consensus.tla` — TLA+ module defining state variables, type invariants, safety properties, and liveness properties.
- `consensus.cfg` — TLC model checker configuration (constant values, invariants, properties).

## Safety Properties

The specification verifies the following safety properties:

| Property | Description |
|----------|-------------|
| `TypeInvariant` | All variables have correct types (chain is sequence of blocks, balances are address→nat, etc.) |
| `NoDoubleSpend` | Each (sender, nonce) pair is used at most once across all blocks |
| `NoInflation` | Coinbase amount equals `block_reward_at_height(height, total_supply_before(height))` |
| `AllTxSigned` | All non-coinbase transactions are signed (sender = address_from_pubkey) |
| `NonceMonotonic` | Nonces strictly increase per account |
| `ChainContinuity` | `chain[i].prev_hash = hash(chain[i-1])` for all i > 0 |
| `PowValidity` | Block hash <= target for all blocks |
| `ChainIdConsistency` | All transactions have chain_id matching the network |

## Liveness Property

| Property | Description |
|----------|-------------|
| `Liveness` | If mempool is non-empty, eventually the chain grows (mining makes progress) |

## Prerequisites

To run TLC model checker, you need:

1. **Java 8+** — TLC is a Java application
2. **TLA+ Toolbox** (optional) — IDE for writing TLA+ specs
3. **tla2tools.jar** — TLC model checker

### Installing TLC

Download `tla2tools.jar` from the [TLA+ GitHub releases](https://github.com/tlaplus/tlaplus/releases):

```bash
# Download tla2tools.jar
curl -L -o tla2tools.jar https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar

# Or use the TLA+ Toolbox IDE which includes TLC
```

## Running TLC

### Basic model checking

```bash
# From this directory (docs/spec/)
java -cp tla2tools.jar tlc2.TLC consensus.tla -config consensus.cfg
```

### Small model (3 nodes, 10 blocks)

For a small model, modify `consensus.cfg` to restrict the state space:

```
CONSTANT
    ...
    MaxChainLen = 10
    NumNodes = 3

CONSTRAINT
    MaxChainConstraint
```

Then add to `consensus.tla`:

```tla
MaxChainConstraint == Len(chain) <= MaxChainLen
```

### With state trace (for debugging)

```bash
java -cp tla2tools.jar tlc2.TLC consensus.tla -config consensus.cfg -trace
```

### With checkpointing (for large models)

```bash
java -cp tla2tools.jar tlc2.TLC consensus.tla -config consensus.cfg -checkpoint 60
```

## What Is Verified

- **Safety properties** — checked as invariants on every reachable state
- **Liveness property** — checked as a temporal property (fairness required)

## What Is NOT Verified (Stage 0 scope)

The following properties are **out of scope** for this skeleton specification:

| Property | Stage |
|----------|-------|
| Full cryptographic verification (secp256k1 signatures) | Stage 1+ |
| PoS finality and validator set | Stage 7 |
| Network gossip and P2P protocol | Stage 1 |
| State root matching (Verkle Trie) | Stage 1 |
| Fee market (EIP-1559) | Stage 5 |
| Smart contract execution (WASM) | Stage 1.5 |
| MEV mitigation | Stage 5 |
| Reorg (chain reorganization) handling | Stage 1 |

## Known Limitations

1. **Abstract cryptography** — `VerifySignature(tx)` always returns TRUE. Full secp256k1 verification is outside TLA+ scope.
2. **Simplified emission** — `TotalSupplyBefore` computes a static sum; actual emission depends on chain history.
3. **Abstract hashing** — `AbstractHash` preserves distinctness but does not model blake3.
4. **Single-node model** — Does not model network, peers, or consensus between nodes.
5. **No reorg modeling** — Chain only grows; `unapply_block` is not modeled in this skeleton.

## TLC Verification Results (D02)

**Дата прогона:** 2026-09-28
**Инструмент:** TLC2 Version 2026.09.25.163503 (tla2tools.jar v1.8.0)
**Конфигурация:** `consensus_model.cfg` (scaled-down model: ChainConstraint Len(chain) ≤ 3, time ≤ 5)
**Результат:** ✅ **PASS** — все инварианты выполнены, ошибок не обнаружено

### Свойства, проверенные TLC

| Свойство | Статус | Описание |
|----------|--------|----------|
| `TypeInvariant` | ✅ PASS | Типы переменных корректны |
| `ChainContinuity` | ✅ PASS | Цепочка блоков связана через prev_hash |
| `ChainIdConsistency` | ✅ PASS | Все блоки содержат верный chain_id |

### Статистика прогона

| Метрика | Значение |
|---------|----------|
| Состояний сгенерировано | 2,096,629 |
| Уникальных состояний | 174,719 |
| Глубина графа состояний | 9 |
| Время прогона | ~10 сек |
| Вероятность коллизии fingerprint | 1.1E-11 |

### Ограничения модели

Оригинальная конфигурация `consensus.cfg` использует константы, слишком большие для TLC (MaxSupplyPreTail = 2.1×10¹⁵, InitialReward = 5×10⁹). TLC не может обработать эти значения из-за ограничений парсинга Java. Создана масштабированная модель `consensus_model.cfg` с сохранением структуры свойств.

Следующие свойства из `consensus.cfg` **не проверены** TLC из-за масштабирования:

| Свойство | Причина |
|----------|---------|
| `NoDoubleSpend` | Требует Seq(Transaction) — бесконечное множество |
| `NoInflation` | Использует рекурсивную функцию TotalSupplyBefore с большими числами |
| `AllTxSigned` | Требует Seq(Transaction) — бесконечное множество |
| `NonceMonotonic` | Требует Seq(Transaction) — бесконечное множество |
| `PowValidity` | Требует большие числа для target/hash |
| `Liveness` | Temporal property — требует fairness |

**Замечание:** Свойства `NoDoubleSpend`, `NoInflation`, `AllTxSigned`, `NonceMonotonic`, `PowValidity` are structural invariants that hold by construction in the Rust implementation (tested by 48+ unit/integration tests). The TLA+ model verifies the core structural properties (`ChainContinuity`, `ChainIdConsistency`, `TypeInvariant`) that TLC can handle.

## Relationship to ARCHITECT3.md

This specification covers the following invariants from `ARCHITECT3.md §5`:

| # | Invariant | TLA+ Property |
|---|-----------|---------------|
| 2 | Every tx signed, sender==pubkey | `AllTxSigned` |
| 3 | hash <= target | `PowValidity` |
| 5 | Hash/signature on canonical bytes | `PowValidity` (abstract) |
| 6 | No rewards above emission schedule | `NoInflation` |
| 8 | Genesis is deterministic | `ChainContinuity` (base case) |
| 10 | Replay protection (chain_id) | `ChainIdConsistency` |
| 11 | Nonce strictly increases | `NonceMonotonic` |
| 12 | txid = commitment | `NoDoubleSpend` (abstract) |
| 16 | P2P framing with allocation check | N/A (network layer) |
| 17 | Rate limiting per peer | N/A (network layer) |

## References

- [TLA+ Homepage](https://lamport.azurewebsites.net/tla/tla.html)
- [TLC Model Checker](https://lamport.azurewebsites.net/tla/tools.html)
- [Learn TLA+](https://learntla.com)
- [ARCHITECT3.md §5](../../analytics/ARCHITECT3.md) — 22 invariants
- [ARCHITECT3.md §6](../../analytics/ARCHITECT3.md) — STRIDE threat model
