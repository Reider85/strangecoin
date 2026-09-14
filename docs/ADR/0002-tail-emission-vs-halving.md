# ADR-0002: Tail Emission vs Halving + Max Supply

## Context

Bitcoin's emission model uses halving every 210,000 blocks (~4 years) with a hard cap of 21M BTC. This creates a predictable supply schedule but has a fundamental long-term issue: the block reward (security budget) trends to zero. After ~30+ years (6-7 halvings), the block subsidy becomes negligible, and network security must rely entirely on transaction fees.

This creates several risks:
1. **Security budget collapse**: Miners' revenue drops exponentially while hardware/energy costs remain. If fee market doesn't compensate, hashrate drops → chain becomes vulnerable to 51% attacks.
2. **Deflationary spiral**: Fixed supply + growing economy = increasing purchasing power → hoarding → reduced velocity → economic stagnation.
3. **Fee market uncertainty**: No guarantee that fees will sustain security. EIP-1559 style markets help but don't guarantee minimum revenue.

## Decision

Strangecoin adopts a **tail emission** model (Monero-style):

- **Primary emission**: Halving every 210,000 blocks, starting at 50 SC/block, until `MAX_SUPPLY_PRE_TAIL = 21,000,000 SC` is reached
- **Tail emission**: After `MAX_SUPPLY_PRE_TAIL`, a fixed annual inflation of **0.6%** of total supply is distributed as block rewards
- **Formula**: `block_reward = max(halving_schedule(height), tail_reward)`
  - `tail_reward = (total_supply * 6) / (1000 * BLOCKS_PER_YEAR)`
  - `BLOCKS_PER_YEAR = 52,560` (10-minute blocks: 365 × 24 × 6)

This ensures:
- **Permanent security budget**: Miners always receive at least the tail reward
- **Predictable inflation**: 0.6%/year asymptotically, decreasing in real terms as economy grows
- **No hard cap**: Supply grows unbounded but at diminishing rate

Parameters fixed in `genesis.json` before mainnet freeze:
- `tail_emission_rate`: 0.006 (0.6%)
- `max_supply_pre_tail`: 21,000,000
- `halving_interval`: 210,000
- `initial_reward`: 50 SC (5,000,000,000 satoshis)

## Consequences

### Positive
- **Sustainable security**: Permanent miner revenue floor prevents hashrate collapse
- **Economic stability**: Mild inflation counters deflationary hoarding, encourages velocity
- **Predictability**: Formula is deterministic, no governance decisions needed post-freeze
- **Compatibility**: Similar to Monero (proven since 2014), familiar to miners

### Negative
- **No "digital gold" narrative**: Unbounded supply may deter store-of-value maximalists
- **Complexity**: More complex than fixed cap; requires tracking total supply
- **Regulatory**: Some jurisdictions treat tail emission differently for tax/accounting

### Implementation Notes
- Implemented in `src/economics/emission.rs`
- Coinbase transaction in each block carries the reward
- `validate_chain` verifies coinbase amount ≤ expected reward (miners may underpay voluntarily)
- Genesis block uses `initial_amount` from `genesis.json` (separate from emission schedule)
- All values in satoshis (1 SC = 100,000,000 satoshis) for integer arithmetic

## Alternatives Considered

### 1. Bitcoin-style: Halving + Hard Cap (21M)
- **Rejected**: Security budget → 0; deflationary spiral risk; fee market uncertainty

### 2. Dogecoin-style: Fixed Inflation (5B/year forever)
- **Rejected**: High perpetual inflation (~3-5%/year); no supply cap narrative; less predictable long-term

### 3. Ethereum-style: EIP-1559 + Issuance Reduction
- **Rejected**: Requires fee market machinery (EIP-1559 deferred to Stage 5 per ROADMAP3); issuance changes require governance (SCIP process post-freeze)

### 4. Hybrid: Halving to Minimum + Tail
- **Selected**: This is what we implemented — halving until tail takes over, then constant % inflation

## References
- Monero tail emission design: https://www.getmonero.org/2014/07/21/disinflation.html
- Bitcoin security budget analysis: https://arxiv.org/abs/1810.02837
- ARCHITECT3.md §7.1 (emission), §15 (anti-goals)
- ROADMAP3.md Stage 0 (consensus freeze after P11)