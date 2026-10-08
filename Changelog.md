# Changelog

## 1.1.1 — Stage 1.5 debt track (BUG-S0-011 + S1.5-P04)

**Status**: In progress  
**Date**: 2026-10-08  
**Prompts**: S1.5-P02 (BUG-S0-011/014/016), S1.5-P04 (BUG-S0-018/019)

### BUG-S0-011: Sparse Merkle Tree replaces flat «Verkle»

- **Breaking:** `state_root` computation changed. Historical roots from the flat 256-slot structure are invalid. Reset LevelDB or resync testnet nodes. Wire `FORMAT_VERSION` unchanged (4).
- `crates/strangecoin-core/src/state/verkle.rs` **removed** → `sparse_merkle.rs`
- `SparseMerkleTrie`: binary tree, depth 256 over `blake3(address)` (256-bit key, MSB-first); leaf = `blake3(key‖balance‖nonce)`; internal = `blake3(left‖right)`; empty subtree hashes precomputed
- Closes **BUG-S0-011** (not a Verkle — now honestly an SMT), **BUG-S0-014** (no collisions for n ≤ 2^256; sweep 256→512 test), **BUG-S0-016** (`prove(address, account)` consumes `account`; pruned `(0,0)` proven by empty slot)
- Proof: 256 sibling hashes (8 KiB), one per level; `verify_proof` walks leaf→root
- ADR-0006 amended: honest SMT decision; true Verkle/KZG deferred to Stage 3+
- API surface unchanged for callers: `compute_state_root`, `root_after`, `build_witness`, `verify_block_stateless`
- Tests: unit + proptest (1000+ accounts unique root; 256→512 sweep; prove/verify round-trip; tamper rejection); full workspace green

### S1.5-P04: blockchain facade decomposition (BUG-S0-018/019)

- `src/blockchain/blockchain_facade.rs`: **1578 → 362 lines** (КГ ≤400 met)
- Moved out of facade:
  - `block_executor.rs`: mining (`mine_block`, `mine_block_inner`), `commit_block`, genesis/grant construction
  - `state_cache.rs`: `open_blockchain` (DB open/load/migrate), LevelDB save, `add_transaction`, `validate_chain`, address migrations
  - `chain_selector.rs`: **real component** (BUG-S0-019) — `try_adopt_candidate`, `chain_has_tx`, `headers_from_height`, `blocks_by_hashes` + unit tests; pure fork-choice algorithm stays in core (strangler)
- Public `BlockchainFacade` API unchanged (behavior preserved); all workspace tests green

### Not in this change

- BUG-S0-012 (`state_root == [0;32]` opt-out) — still open, S1.5-P03
- BUG-S0-013 (`verify_block_stateless` post-root) — still open, S1.5-P03
- BUG-S0-015 (genesis key) — still open, S1.5-P01

## 1.1.0 — Stage 1 (core extracted + Verkle + headers-first)

**Status**: Complete — verified by S1-P22; tag `v1.1.0-stage1`
**Date**: 2026-10-04
**DoD evidence**: `docs/stage1/STAGE1_SUMMARY.md`

### S1-P01: consensus — grant blocks behind regtest-only flag

- Added `Config.allow_grant_blocks` (default false; true only for regtest / legacy DB migration)
- `validate_chain`: grant exception (`block.index != 1`) and magic-string sender skips run **only** when the flag is true
- `create_grant_block` returns typed `GrantBlocksDisabled` + warn when disabled
- Magic-strings removed from the normal consensus path
- Tests: `tests/grant_flag.rs` (4)

### S1-P02: core — strangecoin-core crate + serialize (+ADR-0008)

- Cargo workspace `members = [".", "crates/strangecoin-core"]`, `resolver = "2"`
- Moved `serialize.rs` (canonical blake3 codec, txid) into `strangecoin-core`; golden vectors in `crates/strangecoin-core/tests/serialize_golden.rs`
- Stub `economics/fee_market.rs` (Stage 5)
- ADR-0008: RocksDB planning instead of redb/sled

### S1-P03: core — consensus rules + economics

- Moved consensus (chain_id, U256, retarget, MTP, CURRENT_CONSENSUS_VERSION) and `economics/emission.rs` into core
- Property tests → `crates/strangecoin-core/tests/consensus_proptest.rs`
- Core stays 0 I/O (no fs/net/tokio/leveldb)

### S1-P04: core — pure state apply/unapply

- `State`, `apply_block`, `unapply_block` in `crates/strangecoin-core/src/state.rs` (pure, no I/O)
- Property tests: round-trip `unapply(apply(s,b),b) == s` (invariant #4)
- Monolith apply paths delegate to core where applicable

### S1-P05: core — governance skeleton (SCIP + consensus_version + activation height)

- `crates/strangecoin-core/src/governance/scip.rs`: ScipDocument, ConsensusRules, activation-by-height
- `consensus_version: u32` in block header; `CURRENT_CONSENSUS_VERSION = 1`
- Canonical serialization bumped (format_version); golden vectors updated
- `docs/SCIP/README.md` + `docs/SCIP/scip-0000-process.md`
- Tests: `tests/consensus_version.rs`, core scip activation tests

### S1-P06: core — Verkle Trie + block.state_root (+ADR-0006)

- `crates/strangecoin-core/src/state/verkle.rs`: deterministic state commitment
- `block.state_root` in header; invariant #19 enforced in validation
- ADR-0006: Verkle Trie vs SMT
- Tests: core `tests/state_root.rs`, e2e `tests/state_root.rs`

### S1-P07: core — StateWitness + stateless verification

- `state/witness.rs`: `build_witness`, `verify_block_stateless`, AccountProof
- Tamper / wrong-parent-root / proptest coverage in `crates/strangecoin-core/tests/witness.rs`

### S1-P08: core — merkle tx_root in block header

- `merkle_root` / `compute_tx_root` in serialize; `tx_root` field in Block
- `validate_tx_root` in consensus; golden vectors updated
- Tests: `crates/strangecoin-core/tests/merkle.rs`

### S1-P09: node — EventBus (crossbeam) (+ADR-0009)

- `src/events.rs`: multi-subscriber bus (BlockApplied, BlockReorged, TxAccepted, TxRejected, StatePersisted, …)
- ADR-0009 written before code

### S1-P10: node — tokio runtime (+ADR-0007)

- tokio introduced for new subsystems; legacy mining/network threads remain until Stage 2
- ADR-0007; `tests/two_clients.rs::node_runs_on_tokio_and_shuts_down_cleanly`

### S1-P11: blockchain — chain_selector + deterministic tie-breaking

- `src/blockchain/chain_selector.rs` + core ChainSelector: **work → earliest timestamp → lowest hash**
- `docs/spec/fork_choice.md`
- Proptests: `tests/chain_selector_proptest.rs`

### S1-P12: blockchain — block_executor + state_cache

- `block_executor.rs`: single validate+apply path (no tip selection, no storage writes)
- `state_cache.rs`: balances/nonces reader-writer + `rebuild_from_chain` (invariant #1)
- Tests: `tests/block_executor.rs`, `tests/state_cache.rs`

### S1-P13: blockchain — facade + consensus_manager (5-component split)

- `blockchain_facade.rs`: sole public entry point for chain adoption/reads
- `consensus_manager.rs`: consensus phase/version by height
- Split complete: chain_selector, block_executor, state_cache, facade, consensus_manager

### S1-P14: network — network_id in genesis + HELLO

- `network_id` in genesis and HELLO handshake; foreign networks rejected/banned
- Constants: mainnet=1, testnet=2, regtest=3

### S1-P15: core — bech32 addresses with network HRP

- `encode_address` / `decode_address` (bech32m, 33-byte compressed pubkey)
- HRP: `sc1` / `tsc1` / `rsc1`; base64 addresses eliminated from core
- Legacy base64 migration path on DB open in facade
- Tests: address unit tests + `tests/two_clients.rs::bech32_address_transfer`

### S1-P16: network — headers-first sync

- Binary wire protocol: GET_HEADERS/HEADERS/GET_BLOCKS/BLOCKS (tags 0x01..0x04)
- Header codec, cumulative work, `plan_best_branch`, download-then-adopt
- Pre-P16 peers fall back to legacy GET_BLOCKCHAIN
- Tests: `tests/sync_headers.rs`

### S1-P17: mempool — replace-by-fee

- RBF_MIN_DELTA_BPS, find_replaceable, MAX_RBF_REPLACEMENTS; deterministic rules
- Tests: `tests/rbf.rs`

### S1-P18: node — SyncEngine inbox (+ADR-0010)

- `src/network/sync_engine.rs`: single consumer of incoming chain data
- Bounded lanes, dedupe, backpressure (drop/ban)
- Network no longer calls `adopt_candidate` / `apply_tx` / `save_state` directly
- Tests: `tests/sync_engine.rs`

### S1-P19: tests — Stage 1 matrix + first fuzz target

- Integration matrix: state_root, events (3 subscribers), network_id, RBF, facade/bech32 updates
- `fuzz/fuzz_targets/canonical_decode.rs` + `fuzz/README.md`
- cargo-fuzz not runnable on Windows dev host (recorded); fallback soak found + fixed OOB panic in `deserialize_block`
- Soak: 28,744,131 inputs / 0 panics (10 min, post-fix)

### S1-P20: docs — invariants re-audit

- `docs/stage1/INVARIANTS_ENFORCED.md`: all 22 invariants with Stage 1 enforcement locations
- Invariants #19 (state root) and #21 (consensus versioning) enforced for the first time
- Network modules cleaned to facade-only coupling

### S1-P21: security — threat model update

- `docs/security/THREAT_MODEL.md` → v3.0: Stage 1 vectors V-34..V-42 (headers-first, state_root, witness, tx_root, consensus_version, network_id, RBF, inbox, HRP)
- `docs/security/INCIDENT_RESPONSE.md` → v3.0: Stage 1 alert sources + SLA
- Residual risks documented (zero state_root opt-in, cargo-fuzz host, TLA+ coverage)

### S1-P22: DoD verification + tag

- `docs/stage1/STAGE1_SUMMARY.md`: 15/15 DoD criteria with evidence
- `crates/strangecoin-core/src/vm/traits.rs`: `VmExecutor` trait declaration (Stage 1.5; no runtime)
- Version sync 1.1.0 (Cargo.toml root + core, Changelog, AGENTS.md)
- Verification: `cargo test --workspace` green; `cargo clippy --all-targets -- -D warnings` green; fuzz smoke green
- Annotated tag `v1.1.0-stage1` (Gate Stage 1.5)

---

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
- Tag `v1.0.0-stage0`
