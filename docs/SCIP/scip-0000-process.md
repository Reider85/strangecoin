---
scip: 0
title: SCIP Process Definition
status: Active
consensus_version: 1
activation_height: 0
author: strangecoin team
discussions: https://github.com/anomalyco/strangecoin/issues
created: 2026-09-28
---

# SCIP-0: Process Definition

## Abstract

This SCIP defines the Strangecoin Improvement Proposal (SCIP) process itself.
It establishes the governance framework for all future consensus and protocol changes.

## Motivation

Strangecoin requires a formal, transparent process for proposing, reviewing, and
activating changes to consensus rules. Without such a process, coordination between
节点 operators becomes ad-hoc, and hard forks risk chain splits.

## Specification

### Stages

1. **Idea** — Open a GitHub issue or discussion. Community feedback collected.
2. **Draft** — Formal SCIP document created with all required fields. Assigned a number.
3. **Review** — Technical review by maintainers. Security audit if consensus-affecting.
4. **On-chain signaling** (Stage 5+) — Miners/validators embed signal bits in blocks.
5. **Activation threshold** — ≥75% of blocks signal support over a 2016-block window.
6. **Activation height** — SCIP activates at a predetermined block height.
7. **Finalized** — Post-activation, the SCIP is permanent and cannot be reverted.

### Document Format

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

### Consensus Versioning

- Each block header contains a `consensus_version: u32` field.
- `CURRENT_CONSENSUS_VERSION` is defined in `strangecoin-core/src/consensus.rs`.
- When a SCIP activates, it bumps `consensus_version` and adds an entry to the
  activation map: `BTreeMap<Height, u32>`.
- `validate_chain` rejects blocks whose `consensus_version` does not match the
  expected version for their height.

### Hard Forks vs Soft Forks

- Strangecoin prefers **hard forks** (explicit, planned) over soft forks.
- Each hard fork requires: SCIP + activation height + upgrade guide + ≥3 months warning.

## Rationale

This process mirrors Bitcoin's BIP and Ethereum's EIP, adapted for Strangecoin's
simpler governance model (no on-chain voting in Stage 0–4).

## Backward Compatibility

SCIP-0 is the genesis of the governance process. No prior blocks exist that would
conflict with this specification.

## Reference Implementation

- `crates/strangecoin-core/src/governance/scip.rs` — ConsensusRules, activation logic
- `crates/strangecoin-core/src/consensus.rs` — CURRENT_CONSENSUS_VERSION constant
- `src/lib.rs` — validate_chain consensus_version check
