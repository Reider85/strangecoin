# Fork Choice Rule (Strangecoin)

**Status:** Implemented (S1-P11)
**Module:** `src/blockchain/chain_selector.rs`

## Overview

Strangecoin uses a three-tier lexicographic rule to select the best chain when multiple valid forks exist. The rule is **deterministic**: given the same set of candidate chains, all nodes converge on the same tip.

## Rule

Given two candidate chains A and B, chain A is strictly better than chain B if and only if:

1. **Cumulative work:** A has greater total work than B, OR
2. **Tip timestamp:** A and B have equal total work, but A's tip has an earlier (lower) timestamp than B's, OR
3. **Hash bytes:** A and B have equal total work and equal tip timestamps, but A's tip hash is lexicographically lower (byte-by-byte comparison) than B's.

If all three tiers are equal, the chains are identical for selection purposes.

## Definitions

- **Cumulative work:** Sum of `work(target)` for each block in the chain, where `work(target) = (2^256 - 1) / target`. Lower target = higher work = harder to produce.
- **Tip timestamp:** The `timestamp` field of the last block in the chain.
- **Tip hash:** The `hash` field of the last block in the chain (hex-encoded).

## Rationale

| Tier | Purpose | Security property |
|------|---------|-------------------|
| Cumulative work | Nakamoto consensus — longest chain by work | Attacker must outpace honest hashpower |
| Tip timestamp | Break ties deterministically without relying on network timing | Nodes agree on fork ordering without communication |
| Hash bytes | Final tiebreaker — deterministic, cheap, unpredictable | Prevents racing attacks on timestamp manipulation |

## Implementation

```rust
// src/blockchain/chain_selector.rs
pub fn is_better(a: &ChainInfo, b: &ChainInfo) -> bool {
    // Tier 1: more cumulative work
    if u256_gt(a.total_work, b.total_work) { return true; }
    if u256_gt(b.total_work, a.total_work) { return false; }
    // Tier 2: earlier tip timestamp
    if a.tip_timestamp < b.tip_timestamp { return true; }
    if b.tip_timestamp < a.tip_timestamp { return false; }
    // Tier 3: lower hash bytes
    a.tip_hash < b.tip_hash
}
```

## Properties

- **Transitivity:** If A > B and B > C, then A > C (follows from lexicographic ordering).
- **Totality:** For any two distinct chains, exactly one is better (hash comparison is total on hex strings).
- **Commutativity of selection:** `select_best([A, B]) == select_best([B, A])`.
- **Idempotence:** `select_best([A]) == Some(A)`.

## Edge Cases

- **Empty chain:** `chain_info(&[])` returns `None`; not a valid candidate.
- **Single block (genesis only):** Works normally; work is computed from genesis target.
- **Identical chains:** Same work, timestamp, and hash — `is_better` returns `false` for both directions; `select_best` returns the first encountered.

## Integration Points

- `Blockchain.total_work` is maintained incrementally (updated on add_block, reorg, genesis creation).
- `ChainSelector::chain_info(chain)` computes on-demand from `Vec<Block>` for network sync paths.
- `adopt_from()` and `sync_to_longest()` use `ChainSelector::is_better()` for fork resolution.
- Network sync (`UPDATE_BLOCKCHAIN`, `sync_blockchain`) compares by work via `ChainSelector`.
