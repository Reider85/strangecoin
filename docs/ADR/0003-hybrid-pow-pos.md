# ADR-0003: Hybrid PoW → PoS Migration

## Status

Accepted

## Context

Strangecoin must choose a long-term consensus mechanism. Three primary options exist:

**Pure PoW (Bitcoin model):**
- Proven security model (Bitcoin, 15+ years)
- Decentralized mining (anyone with hardware can participate)
- No "nothing-at-stake" problem
- Cons: energy consumption concerns, 51% attack risk as hashrate concentrates, security budget trends to zero without tail emission (addressed by ADR-0002), ASIC centralization risk

**Pure PoS from day-1 (Ethereum post-merge model):**
- Energy efficient (~99.95% less than PoW)
- No hardware arms race
- Cons: distribution bootstrapping problem (how to fairly distribute initial stake without PoW mining), nothing-at-stake requires slashing from genesis, weak subjectivity checkpoints add sync complexity, validator set must be bootstrapped from zero

**Hybrid PoW → PoS (Strangecoin approach):**
- PoW bootstraps distribution in first ~2 years (anyone can mine, fair launch)
- After sufficient distribution and network maturity, migrate to PoS via activation height
- Cons: two consensus code paths to maintain, migration complexity, governance decisions at transition point

Forces at play:
- **ARCHITECT3.md §15** explicitly lists "PoW как финальный консенсус" as an anti-goal
- **Energy concerns**: PoW alone faces regulatory and environmental pressure long-term
- **Distribution fairness**: PoW mining is the most battle-tested way to achieve fair token distribution
- **Security budget**: Tail emission (ADR-0002) ensures PoW security budget; PoS achieves same via staking rewards
- **Governance**: Migration must happen via SCIP + activation height (post Stage 0 freeze)

## Decision

Strangecoin adopts a **hybrid PoW → PoS** consensus model with the following phases:

### Phase 1: PoW Bootstrapping (Stage 0–6, ~first 2 years)

- **Algorithm:** PoW with secp256k1-based PoW (ASIC-resistant via memory-hard function consideration for future)
- **Block validation:** `hash <= target` (u256 comparison), retargeting every 2016 blocks
- **Emission:** Tail emission (ADR-0002) — halving schedule + 0.6%/year tail after MAX_SUPPLY_PRE_TAIL
- **Purpose:** Fair distribution, network bootstrapping, hashrate decentralization

### Phase 2: PoS Migration (Stage 7+, via activation height)

- **Trigger:** `POS_ACTIVATION_HEIGHT` set via SCIP governance proposal, approved by community
- **Finality gadget:** Casper FFG overlay on top of PoW chain
- **Signatures:** BLS12-381 for consensus signatures (aggregation support)
- **Validator set:** Validators lock native token as stake; active set rotated each epoch (~1 day)
- **Slashing conditions:**
  - Double-vote: validator signs two different blocks at same height → full stake slashing
  - Surround-vote: validator vote contradicts finalized checkpoint → full stake slashing
  - Downtime: extended absence from attestation → minor penalty (soft slashing)
- **Validator economics:**
  - Staking rewards: portion of block reward (replacing miner reward)
  - Delegation: holders can delegate to validators (with commission)
  - Max stake per validator: 5% of total stake (centralization prevention)
- **Weak subjectivity:** Checkpoint sync required; nodes syncing from scratch must trust a recent checkpoint

### Migration Mechanism

- `consensus_manager.rs` module (Stage 7) manages the transition
- At `POS_ACTIVATION_HEIGHT`: switches validation logic from PoW to PoS rules
- Blocks before activation: validated under PoW rules
- Blocks after activation: validated under PoS rules (attestations + finality)
- No chain split — single canonical chain through transition

## Consequences

### Positive

- **Fair distribution:** PoW mining bootstraps token distribution without pre-mine or ICO
- **Energy efficiency:** PoS phase uses ~99.95% less energy than perpetual PoW
- **Security:** PoW provides initial security; PoS provides finality and long-term security
- **Ecosystem alignment:** Follows Ethereum's proven migration path (Eth1 → Eth2)
- **Flexibility:** Activation height can be delayed if PoS readiness is insufficient
- **Tail emission compatibility:** Both PoW miners and PoS validators receive block rewards from tail emission

### Negative

- **Complexity:** Two consensus code paths during transition period
- **Governance risk:** Activation height requires community consensus (SCIP process)
- **Migration risk:** Bugs during transition could cause chain halt or split
- **Validator bootstrapping:** Must build validator set before activation (staking campaign)

### Neutral

- **Block time:** Remains 10 minutes during PoW phase; may adjust in PoS phase
- **Emission schedule:** Unchanged — tail emission applies to both phases
- **Address format:** Bech32 (Stage 1) applies to both phases

## Alternatives Considered

### Alternative 1: Pure PoW Forever

- **Pros:** Simplest, proven, no migration risk
- **Cons:** Energy concerns, ASIC centralization, security budget relies entirely on fee market (even with tail emission, miner revenue is lower than validator revenue in PoS), violates ARCHITECT3.md §15 anti-goal
- **Why not chosen:** Strategic incompatibility with long-term sustainability goals

### Alternative 2: Pure PoS from Day-1

- **Pros:** No migration, energy efficient from start
- **Cons:** Distribution bootstrapping requires pre-mine or ICO (centralization risk), nothing-at-stake from genesis adds complexity, no proven "fair launch" mechanism for PoS, validator set bootstrapping from zero is untested at scale
- **Why not chosen:** Distribution fairness is a core value; PoW mining is the proven mechanism for fair launch

### Alternative 3: PoW with Checkpoint Finality (No Full PoS)

- **Pros:** Adds finality without full PoS migration, simpler than hybrid
- **Cons:** Still requires PoW mining (energy), no validator staking economics, doesn't address long-term energy concerns
- **Why not chosen:** Doesn't fully address ARCHITECT3.md anti-goal of PoW as final consensus

## Implementation Plan

| Stage | Component | Description |
|-------|-----------|-------------|
| 0–6 | PoW consensus | Current implementation (main.rs + consensus/) |
| 7 | `consensus_manager.rs` | PoW→PoS switch logic, activation height |
| 7 | `staking.rs` | Validator deposits, delegation, slashing |
| 7 | BLS12-381 signatures | Consensus signature aggregation |
| 7 | Casper FFG | Finality overlay, attestation handling |
| 7 | Validator rotation | Epoch-based active set management |

## Related

- ARCHITECT3.md §15 (Anti-goals: "PoW как финальный консенсус")
- ARCHITECT3.md §4.4 (PoS attestation, Stage 7+)
- ARCHITECT3.md §7.5 (Staking, Stage 7+)
- ROADMAP3.md Stage 7 (PoS migration)
- ADR-0002: Tail emission (compatible with both phases)
- ADR-0001: secp256k1 (used in PoW phase; BLS12-381 added for PoS)
