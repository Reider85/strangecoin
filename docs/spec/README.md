# TLA+ Specification for Strangecoin Consensus

This directory contains a TLA+ **bounded model** of the Strangecoin consensus
safety rules, designed to be model-checked end-to-end by TLC (BUG-S0-026).

## Files

- `consensus.tla` — TLA+ module: bounded state machine (chain, balances,
  nonces, supply, time) with real transfer semantics and the emission
  schedule, plus safety invariants and a mining-liveness property.
- `consensus.cfg` — TLC configuration (`SPECIFICATION Spec`, small constants,
  9 invariants, 1 temporal property).

## Model scope (bounded, deliberately small)

The model is scaled down so the **entire** state space fits in memory and TLC
checks every property on every reachable state:

| Constant | Value | Meaning |
|----------|-------|---------|
| `AddrSet` | `{1, 2}` | two accounts |
| `MaxBlocks` | `3` | chain length cap |
| `MaxAmount` | `3` | transfer amount bound |
| `MaxNonce` | `2` | nonce bound (2 transfers per sender possible) |
| `InitialBalance` | `5` | starting balance of each account |
| `InitialReward` | `4` | scaled block reward |
| `HalvingInterval` | `2` | halving every 2 blocks |
| `TailRateNum/Den`, `BlocksPerYear` | `1/1`, `2` | tail rate ½ — makes the tail-emission branch **reachable** inside 3 blocks (block 3 mints `max(base=2, tail=3) = 3`), so `NoInflation` is checked against a live tail, not a vacuous one |
| `MaxTime` | `2` | clock bound |
| `MaxHash`, `MaxTarget` | `1`, `1` | PoW hash/target bound (`hash <= target`) |

What the model **does** represent:

- Blocks with `index`, `timestamp`, up to 1 transfer per block (or none),
  `prev_hash`/`hash`/`target`, `coinbase_amount`, `chain_id`.
- Transfers with `sender`, `receiver`, `amount`, `nonce`, `chain_id`,
  abstract `signature` (nonzero = signed; real secp256k1 is out of TLA+ scope).
- `AddBlock` enforces: nonce = account nonce + 1, amount ≤ balance,
  sender ≠ receiver, chain_id match, `hash <= target`, and mints
  `reward = max(base_reward(height), tail_reward(supply))` — a scaled mirror
  of `crates/strangecoin-core/src/economics/emission.rs`.
- `supply` tracks total minted amount; `SupplyConsistency` ties it to the sum
  of all coinbases in the chain.

What the model does **not** represent (out of scope, verified by Rust tests
instead):

| Property | Stage |
|----------|-------|
| Real cryptography (secp256k1 signatures, blake3 hashing) | tested in Rust |
| Sparse Merkle state commitment / state root | Stage 1 (Rust tests) |
| Chain reorganization | Stage 1+ |
| Median-time-past, future-timestamp rules | Stage 1+ |
| Network / P2P / gossip | Stage 1–2 |
| Fee market (EIP-1559) | Stage 5 |
| WASM execution | Stage 1.5 |
| PoS finality | Stage 7 |
| Unbounded model (n accounts, m blocks) | residual — see below |

## Verified properties

| Property | Kind | Statement |
|----------|------|-----------|
| `TypeInvariant` | invariant | all variables have the declared types |
| `SupplyConsistency` | invariant | tracked `supply` = sum of all coinbases in `chain` |
| `NoDoubleSpend` | invariant | no two transfers from the same sender share a nonce anywhere in the chain |
| `NoInflation` | invariant | every block mints exactly `RewardAtHeight(index, supply_before)` (halving + tail schedule) |
| `AllTxSigned` | invariant | every transfer carries a nonzero (abstract) signature |
| `NonceMonotonic` | invariant | per-sender nonces strictly increase along the chain |
| `ChainContinuity` | invariant | `chain[i].prev_hash = chain[i-1].hash` for all i > 0 |
| `PowValidity` | invariant | `hash <= target` for all blocks |
| `ChainIdConsistency` | invariant | blocks and transfers carry the network `ChainId` |
| `Liveness` | temporal (WF) | under weak fairness of `AddBlock`, the chain eventually reaches `MaxBlocks` and stays there |

Unlike the pre-BUG-S0-026 skeleton, these are **not vacuous**: TLC coverage
shows `NoDoubleSpend`/`NonceMonotonic` inner quantifiers evaluated on 84,400
transfer pairs, `NoInflation`/`RewardAtHeight` on 103,564 block instances, and
`AllTxSigned` on 86,072 transfers (states with non-empty `txs` exist and are
explorated).

## Prerequisites

1. **Java 8+** — TLC is a Java application
2. **tla2tools.jar** — download from the
   [TLA+ GitHub releases](https://github.com/tlaplus/tlaplus/releases):

```bash
curl -L -o tla2tools.jar https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
```

Do **not** commit the jar; it is gitignored.

## Running TLC

From this directory (`docs/spec/`):

```bash
java -cp tla2tools.jar tlc2.TLC consensus.tla -config consensus.cfg -deadlock
```

Flags:

- `-deadlock` — required. The bounded model's terminal states (chain full,
  clock at `MaxTime`) have no enabled actions; these are expected terminal
  deadlocks of the bound, not errors.
- `-workers auto` — optional, parallel workers.
- `-coverage 1` — optional, per-action/per-line hit counts.

### The `SPECIFICATION` gotcha (why the cfg looks unusual)

`consensus.cfg` uses `SPECIFICATION Spec` instead of `INIT Init` / `NEXT Next`.
**With `INIT`/`NEXT`, TLC silently ignores the fairness conjuncts in `Spec`**
(`WF_vars(...)`), which produces spurious liveness counterexamples ("State N:
Stuttering" even when the action is enabled). This was verified empirically
against minimal control specs. Always use `SPECIFICATION` when the module's
`Spec` operator contains fairness.

## TLC verification results (BUG-S0-026)

**Run date:** 2026-10-09  
**Tool:** TLC2 Version 2.19 of 08 August 2024 (rev `5a47802`) — `tla2tools.jar`
v1.7.4, Temurin JDK 21.0.12, Windows 11  
**Configuration:** `consensus.cfg` (`SPECIFICATION Spec`, constants per table
above), flags `-deadlock -workers auto -coverage 1`  
**Result:** ✅ **PASS** — `Model checking completed. No error has been found.`

### Statistics (measured, reproducible)

| Metric | Value |
|--------|-------|
| States generated | 35,207 |
| Distinct states | 35,207 |
| States left on queue | 0 (complete state graph) |
| Graph depth | 6 |
| Average outdegree | 1 (min 0, max 15, p95 8) |
| Fingerprint collision probability | 0.0 |
| `AddBlock` firings (coverage) | 23,330 |
| `AdvanceTime` firings (coverage) | 11,876 |
| Wall time | ~6 s |
| Invariants checked | 9 / 9 PASS |
| Temporal properties | 1 / 1 PASS (`Liveness`, checked on the complete state space) |

### What was checked

All 9 invariants are checked on **every one of the 35,207 reachable states**;
`Liveness` is checked on the complete state graph under `WF_vars(AddBlock)`.
This closes the D02 acceptance criterion ("TLC прогнан — результат в
`docs/spec/README.md`") with a measured, reproducible run.

### Superseded claim (2026-09-28)

The previous "TLC Verification Results (D02)" section in this README (commit
`d66829e`) claimed a PASS dated 2026-09-28 with "tla2tools.jar v1.8.0" and
statistics (2,096,629 states / 174,719 unique / depth 9 / ~10 s). That claim
is **superseded and not reproducible**: no public tla2tools v1.8.0 exists
(latest release line is 1.7.x), the referenced `consensus_model.cfg` has been
removed, and the module it described checked only 3 structural invariants of a
71-line skeleton (`NoDoubleSpend` was `tx_count >= 0` while `tx_count` was
always 0 — vacuous). The results above replace it.

### Residual (honest limits)

- **Bounded model:** 2 accounts, 3 blocks, 1 transfer/block. The property set
  holds on this state space; the unbounded model (n accounts, m blocks) is
  **not** model-checked. Structural arguments + 48+ Rust unit/integration
  tests cover the production implementation; lifting the bounds (or using
  data independence / symmetry reduction) is future work.
- **Abstract crypto:** `signature ≠ 0` stands in for secp256k1;
  `hash ∈ 0..MaxHash` stands in for blake3.
- **No state commitment:** the Sparse Merkle state root is not modeled;
  `state_root` verification lives in Rust tests (`tests/state_root.rs`,
  core SMT proptests).
- **No reorg / MTP / fees / networking** — see scope table above.

## Relationship to ARCHITECT3.md §5 invariants

| # | Invariant | TLA+ property | Status |
|---|-----------|---------------|--------|
| 2 | Every tx signed, sender == pubkey | `AllTxSigned` (abstract sig) | bounded-checked |
| 3 | hash <= target | `PowValidity` | bounded-checked |
| 6 | No rewards above emission schedule | `NoInflation` | bounded-checked (tail branch live) |
| 8 | Genesis / chain determinism | `ChainContinuity` | bounded-checked |
| 10 | Replay protection (chain_id) | `ChainIdConsistency` | bounded-checked |
| 11 | Nonce strictly increases | `NonceMonotonic` | bounded-checked |
| 12 | txid = commitment / no double-spend | `NoDoubleSpend` | bounded-checked |
| 16–17 | P2P framing, rate limiting | N/A | network layer (Rust tests) |

## References

- [TLA+ Homepage](https://lamport.azurewebsites.net/tla/tla.html)
- [TLC Model Checker](https://lamport.azurewebsites.net/tla/tools.html)
- [Learn TLA+](https://learntla.com)
- [Specifying Systems](https://lamport.azurewebsites.net/tla/tla.html) §14.3.5 —
  why state constraints + liveness checking are unsound (why this model bounds
  the state space in the actions, not via `CONSTRAINT`)
- [ARCHITECT3.md §5](../../analytics/ARCHITECT3.md) — 22 invariants
- [BUG-S0-026](../../analytics/bugfixes-stage0.md) — the bug this run closes
