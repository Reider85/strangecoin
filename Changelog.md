# Changelog

## 1.0.0 — Stage 0 (Sanitized Prototype)

### P25: Bug bounty + ADR-0003 (hybrid PoW→PoS) (2026-09-17)

- Created `docs/ADR/0003-hybrid-pow-pos.md` — hybrid PoW (Stage 0–6) → PoS (Stage 7+) migration decision
- Created `docs/security/BOUNTY.md` — bug bounty program: scope, 3 reward tiers ($1k/$10k/$100k), 90-day disclosure, Immunefi setup
- Created `docs/security/SECURITY.md` — security contacts, PGP key placeholder, 48h SLA, safe harbor policy

### P24: Reproducible builds (2026-09-17)

- Added `[profile.release]` with LTO, single codegen unit, symbol stripping for deterministic builds
- Created `.github/workflows/release.yml` — release pipeline triggered on `v*` tags
  - Builds 6 targets: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64/aarch64
  - `RUSTFLAGS="--remap-path-prefix"` strips build paths for reproducibility
  - SHA256 checksums for each artifact
  - SLSA Level 3 provenance via `slsa-github-generator`
  - Keyless cosign signatures (Sigstore/OIDC)
- Created `docs/security/REPRODUCIBLE_BUILDS.md` — verification instructions

### P23: TLA+ consensus spec skeleton

- Created `docs/spec/consensus.tla` with safety properties (NoDoubleSpend, NoInflation, AllTxSigned, NonceMonotonic, ChainContinuity) and liveness
- Created `docs/spec/consensus.cfg` for TLC model checker
- Created `docs/spec/README.md` with verification instructions

### ~~P22: Threat Model (STRIDE)~~ — NOT COMPLETED IN STAGE 0

- **Entry corrected:** P22 was NOT completed during Stage 0. This changelog entry was added prematurely (commit "docs: add CHANGELOG.md" predates any THREAT_MODEL.md file).
- P22 is executed in **D02** (debt prompt, see `analytics/prompt-stage1.md`).
- Files `docs/security/THREAT_MODEL.md` and `docs/security/INCIDENT_RESPONSE.md` are created in D02 commit.

### D03: DoD-verification + version sync + tag (2026-09-28)

- Fixed `.github/workflows/release.yml`: added `aggregate-hashes` job for SLSA provenance, added 6th target (`aarch64-pc-windows-msvc`)
- Cleaned repository: removed `test.md`, `.idea/`, `.codebuddy/`, `.opencodeignore` from tracking; `Cargo.lock` now tracked
- Synchronized version to `1.0.0` across `Cargo.toml`, `Changelog.md`, `AGENTS.md`
- Created `docs/stage0/CRITICAL_ISSUES_CLOSED.md` — 13 issues from ARCHITECT2 §1.1 with honest statuses
- Created `docs/stage0/INVARIANTS_ENFORCED.md` — 22 invariants from ARCHITECT3 §5 with enforcement locations
- Created `docs/stage0/STAGE0_SUMMARY.md` — Stage 0 completion summary with Stage 1 obligations

### S1-P05: Governance skeleton — SCIP + consensus_version + activation height

- Created `crates/strangecoin-core/src/governance/` module: `scip.rs` with `ScipDocument`, `ScipStatus`, `ConsensusRules`, `current_consensus_version()` for activation-by-height logic
- Added `consensus_version: u32` field to `Block` struct (`#[serde(default)]` for JSON backward compat)
- Added `CURRENT_CONSENSUS_VERSION: u32 = 1` constant to `crates/strangecoin-core/src/consensus.rs`
- Bumped `FORMAT_VERSION` from 1 to 2 in `crates/strangecoin-core/src/serialize.rs` — `serialize_block_header` now includes `consensus_version` in canonical binary encoding
- Updated `deserialize_transaction` to accept both format versions 1 and 2
- Added consensus_version validation to `validate_chain()` — rejects blocks with mismatched version
- Updated all 20 Block struct literal sites across the codebase
- Created `docs/SCIP/README.md` and `docs/SCIP/scip-0000-process.md` — SCIP process skeleton
- Created `tests/consensus_version.rs` — rejection tests for stale/future consensus_version
- All golden vector tests updated for new serialization format

### S1-P08: Merkle tx root in block header

- Added public `merkle_root(txids: &[[u8; 32]]) -> [u8; 32]` function to `crates/strangecoin-core/src/serialize.rs` — blake3-merkle tree with odd-node duplication
- Added `compute_tx_root(transactions: &[Transaction]) -> [u8; 32]` helper that computes txids and calls `merkle_root`
- Added `tx_root: [u8; 32]` field to `Block` struct (`#[serde(default)]` for JSON backward compat)
- Bumped `FORMAT_VERSION` from 3 to 4 — `serialize_block_header` now includes stored `tx_root` instead of recomputing it
- Added `TxRootMismatch { expected, got }` variant to `CoreError`
- Added `validate_tx_root(block: &Block) -> Result<(), CoreError>` to consensus module
- Integrated tx_root computation in block creation paths: regtest genesis, `create_grant_block`, `mine_block_inner`, `load_genesis_block`
- Added tx_root validation loop in `validate_chain()`
- Updated all 29 Block literal sites across codebase
- Created `crates/strangecoin-core/tests/merkle.rs` — 10 unit tests + 2 proptests: empty, single, pair, odd, 7-tx, deterministic, different inputs, compute_tx_root round-trip, proptest determinism, proptest non-zero
- All golden vector tests updated for new serialization format

### S1-P12: block_executor + state_cache

- Created `src/blockchain/block_executor.rs` — `validate_and_apply(parent_state, block, view)`: the single "validate + apply" path for one block (position/header hash, `consensus_version`, `tx_root`, timestamp MTP + future bound, PoW + retarget schedule for `block.target`, transaction signatures with the grant-block exemption, `core::state::apply_block`, `state_root`); it never selects a tip and never writes to storage
- Added `BlockView { chain, now, allow_grant_blocks }` (`new`/`next`) and `now_secs()` — everything a block is validated against besides its parent state
- Created `src/blockchain/state_cache.rs` — `StateCache` is the only reader of balances/nonces: read API (`balance`, `nonce`, `keys`, `iter`, `nonzero_balances`, …), write API (`commit`, `replace`, `invalidate`, `unapply_block`, `credit`, `ensure_account`) and `rebuild_from_chain` (reconstruction from the chain, mechanism lifted out of `validate_chain`)
- Added `Blockchain::rebuild_state_cache()`; `validate_chain` now delegates validation to the executor and compares against the reconstructed balances for invariant #1 (the chain wins over the cache)
- Switched the monolith apply paths to the executor: `create_genesis_block`, `create_grant_block`, `mine_block` all run through `validate_and_apply` + `StateCache::commit`
- `Blockchain.balances` is now a `StateCache`; `BlockchainDeserialize` and the chain-adoption/sync paths build it with `StateCache::from_accounts`. Direct `balances` mutations outside `state_cache` removed (rg control: only a commented-out legacy block in `Blockchain::new` remains)
- Added `StrangecoinError::InvalidBlock(String)` carrying `block N: <reason>`
- Created `tests/block_executor.rs` — 20 component tests: valid block applied to parent state + rejection matrix (index, previous_hash, header hash, `consensus_version`, `tx_root`, MTP/future timestamp, PoW above target, target change outside a retarget height, unsigned transfer, spending without funds, coinbase inflation, missing coinbase, wrong `state_root`, matching `state_root`, grant-block opt-in flag, genesis on a non-empty chain)
- Created `tests/state_cache.rs` — 5 component tests: rebuild reproduces the chain state, rebuild repairs a tampered cache (invariant #1), `unapply_block` rolls the cache back on reorg, `invalidate`, tampered chain rejected (header hash and `tx_root` paths)
- Fixed two clippy deny-level lints that stopped `cargo clippy` from compiling (`consensus_proptest` reward bound, `first_wallet` loop); the workspace now clippies without errors — 89 pre-existing warnings in untouched files remain as debt
- `cargo test --workspace` green (strangecoin 15 + 24 integration targets, strangecoin-core 38 + 7 test targets)
