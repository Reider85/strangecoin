---
scip: 2
title: Mandatory state_root commitment (zero-root opt-out removal)
status: Draft
consensus_version: 1 (unchanged)
activation_height: 0 (from genesis on mainnet/testnet; regtest legacy opt-in via config)
author: strangecoin team
discussions: https://github.com/Reider85/strangecoin/issues
created: 2026-10-11
---

# SCIP-0002: Mandatory state_root commitment

## Abstract

Invariant #19 requires `state_root` on every block except genesis. Stage 0/1
shipped with an unconditional opt-out: any block whose header carried
`state_root == [0u8; 32]` skipped the commitment check entirely
(`state/mod.rs::root_after`, `block_executor.rs::validate_and_apply`). A block
without a commitment was therefore indistinguishable from a valid one, so
light clients and peers could not trust `block.state_root` (BUG-S0-012,
carried as BUG-S1-002, severity Critical).

This SCIP removes the unconditional opt-out. From now on:

- **mainnet / testnet:** a non-genesis block with a zero `state_root` is
  **rejected**. No config knob exists to re-enable it
  (`Config::validate` refuses `allow_zero_state_root = true` outside regtest).
- **regtest:** a network-aware config default keeps a legacy escape hatch —
  `Config.allow_zero_state_root` resolves to `true` when unset on
  `network_id = 3` — so pre-SCIP regtest databases keep loading. Operators
  may set it to `false` to enforce commitments on fresh regtest chains.
- **node-built blocks** (mining, grant) now always commit to their real
  post-state root; the zero-root construction path is gone.

## Motivation

- **Invariant #19 / light-client trust:** a zero `state_root` meant
  "no commitment"; an attacker could serve such a block and a verifier had
  nothing to check against (THREAT_MODEL V-35 residual).
- **Consistency:** the canonical block header has carried `state_root` since
  S1-P06 (ADR-0006, amended SMT); leaving an opt-out indefinitely made the
  field advisory, not consensus-critical.
- **Pre-mainnet timing:** no mainnet chain exists yet (SCIP-0001 genesis-key
  gate still open), so the rule can land at `activation_height: 0` for the
  future mainnet without a mid-chain hard fork. A `consensus_version` bump is
  deliberately **not** taken: bumping the version would reject legacy regtest
  chains on the version check — a strictly harsher break than the state-root
  rule this SCIP fixes.

## Specification

### 1. Validation rule (all networks)

In `validate_and_apply` and core `root_after`, after applying the block:

```
computed = compute_state_root(post_state)
if block.state_root == [0; 32]:
    allowed = block.index == 0            # genesis exemption (invariant #19)
              || view.allow_zero_state_root  # regtest legacy opt-in only
    if !allowed: reject (StateRootMismatch / InvalidBlock)
else:
    if block.state_root != computed: reject (StateRootMismatch)
```

### 2. Configuration

- `Config.allow_zero_state_root: Option<bool>` (serde default `None`).
- Resolution (`Config::zero_state_root_allowed`): `None` → `true` on
  `network_id = 3` (regtest), `false` on mainnet/testnet; explicit values
  always win.
- `Config::validate` rejects `Some(true)` unless `network_id == 3` — a
  mainnet/testnet node can never accept commitment-less blocks.
- The flag reaches consensus through `Blockchain.allow_zero_state_root` →
  `BlockView.allow_zero_state_root` (same plumbing as `allow_grant_blocks`).

### 3. Block construction

- `mine_block_inner` computes the post-state root over the parent state plus
  the block's transactions and writes it into the header **before** sealing
  (PoW/hash). An unapplicable mempool transaction aborts the mining attempt.
- `create_grant_block` commits the same way.
- Genesis (`index == 0`) keeps a zero root — exempt by invariant #19;
  `EXPECTED_GENESIS_HASH` and `genesis.json` are unchanged.

### 4. Migration / backward compatibility

- **Legacy regtest chains** (zero-root blocks produced before this SCIP):
  remain valid because the regtest default resolves the opt-in to `true`.
  No destructive DB rewrite is performed — rewriting stored `state_root`
  values would change block hashes and break the `previous_hash` chain.
- **Balance reconstruction** is unaffected: `state_cache::rebuild_from_chain`
  still replays the chain; it now threads the flag into every `BlockView`.
- **Mainnet/testnet:** no legacy chain exists to migrate (mainnet not
  launched; the testnet genesis key is burned per SCIP-0001).
- **Wire format:** unchanged (`FORMAT_VERSION` untouched); the rule is
  validation-side only.

## Rationale

- Enforcing the commitment at activation height 0 closes the invariant before
  any value is at stake, instead of carrying a Critical residual into
  mainnet freeze (BUG-S1-002 was a listed mainnet blocker).
- Keeping the regtest escape hatch (rather than a hard cutover) preserves
  developer databases and the historical test workflows that craft zero-root
  fixtures, while the shipped default keeps new regtest chains fully
  committed (mining/grant paths fixed in the same change).
- No `consensus_version` bump: version checks compare the expected version
  per height (`ConsensusManager`); a bump would invalidate legacy regtest
  blocks on a second axis and is deferred until a real mainnet fork schedule
  exists (SCIP-0 process, Stage 5+ signaling).

## Reference Implementation

- `crates/strangecoin-core/src/state/mod.rs` — `root_after` (3rd parameter)
- `src/blockchain/block_executor.rs` — `BlockView.allow_zero_state_root`,
  `validate_and_apply` rule, mining/grant root population
- `src/config.rs` — `allow_zero_state_root`, `zero_state_root_allowed`,
  mainnet/testnet rejection in `validate`
- Tests: core `tests/state_root.rs` (`zero_state_root_rejected_without_opt_in`,
  `genesis_zero_state_root_is_always_tolerated`);
  `tests/block_executor.rs` (`rejects_zero_state_root_when_commitment_required`,
  `accepts_zero_state_root_with_opt_in_flag`);
  `tests/state_root.rs` (`zero_state_root_chain_rejected_without_the_opt_in_flag`,
  `grant_and_mined_blocks_commit_real_state_roots`)
- `analytics/bugfixes-stage1.md` — BUG-S1-002
