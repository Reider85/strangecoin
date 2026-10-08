# SCIP — Strangecoin Improvement Proposals

SCIP is the formal process for any change to Strangecoin consensus rules or protocol.
All consensus rule changes **must** go through SCIP + activation height.

## Process Overview

1. **Idea** — GitHub discussion
2. **Draft** — Formal SCIP document, community review
3. **Review** — Technical analysis, security audit if needed
4. **On-chain signaling** (Stage 5+) — Validators/miners signal support in blocks
5. **Activation threshold** — ≥75% support over N blocks
6. **Activation height** — SCIP activates at a specific block height
7. **Finalized** — After activation, SCIP cannot be reverted

## Document Format

```yaml
scip: <number>
title: <title>
status: Draft|Review|Active|Finalized
consensus_version: <version>
activation_height: <height or TBD>
author: <name>
discussions: <url>
created: <date>
```

## Key Rules

- `consensus_version` is embedded in every block header
- `activation_height` is enforced by `current_consensus_rules(height)`
- Backward compatibility: nodes with older `consensus_version` accept blocks up to activation height, then reject (hard fork)
- Mainnet is not launched: direct consensus edits are allowed before `v1.1.0-stage1` tag, but each edit must be logged in Changelog

## Directory Structure

- `scip-0000-process.md` — This document (process skeleton)
- `scip-0001-genesis-key-replacement.md` — Genesis key burned; offline key mandatory before mainnet freeze (BUG-S0-015 / S1.5-P01)
- Future SCIPs: `scip-NNNN-<slug>.md`
