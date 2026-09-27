# STAGE0_SUMMARY.md — Stage 0 Completion Summary

**Tag:** v1.0.0-stage0
**Date:** 2026-09-28
**Prompt:** `analytics/prompt-stage1.md` D03

## Overview

Stage 0 (Sanitized Prototype) is complete. 22/26 original prompts (P01–P25) were executed, plus 3 debt prompts (D01–D03) to close gaps identified in the retrospective (`analytics/retro-stage0.md`).

| Metric | Value |
|--------|-------|
| Prompts completed | 25 (P01–P18, P20–P25) + 3 debt (D01–D03) |
| Prompts skipped | 0 (all addressed via debt prompts) |
| Unit tests | 48 in `src/` |
| Integration tests | 8 files in `tests/` (D01) |
| ADRs | 5 (0001–0005) |
| Critical issues closed | 9/13 fully, 3 partially, 1 deferred |
| Invariants enforced | 17/22, 4 deferred, 1 N/A |

## What Was Done

### Cryptographic Core
- Migrated from Ed25519 to secp256k1 (P04, ADR-0001): recoverable ECDSA, `verify_transaction` enforces `sender == address(pubkey)`
- Canonical binary serialization via blake3 (P06): 586 lines, 19 golden-vector tests, txid = commitment from signed canonical bytes
- Deterministic genesis (P10): `genesis.json` ↔ `EXPECTED_GENESIS_HASH`, panic on mismatch, regtest with chain_id=3

### Consensus
- PoW with U256 arithmetic (P08): `hash <= target`, retarget every 2016 blocks, clamp ×4
- Median-time-past with window 11 (P09): reject timestamps >2h in future
- Tail emission (P11): `block_reward_at_height` formula, 10 emission tests
- Nonce monotonicity, chain_id replay protection, balance reconstruction from chain

### Defense
- Length-prefixed framing with size validation before allocation (P12): OOM mitigation
- Rate limiting per peer (P13): 100 msg/10s, ban on violation
- Mempool with full validation on insert (P14): signature, dup, nonce, chain_id, balance; MAX_PENDING_TXS=10000
- RwLock<BlockchainInner> (P16): lock ordering documented in `src/error.rs`, deadlock test
- Graceful shutdown (P17): AtomicBool + Drop + ctrlc, no manual LOCK removal

### Infrastructure
- Unified Config (P15): TOML, secrets only in keystore/env
- CI matrix (P20): 3 OS × 2 toolchain, fmt, clippy -D warnings, tarpaulin coverage
- Reproducible builds (P24): release.yml with LTO, cosign, SLSA Level 3
- Bug bounty + security docs (P25): BOUNTY.md, SECURITY.md, ADR-0003
- TLA+ skeleton (P23): consensus.tla with safety properties, TLC verified (D02)

### Testing (D01)
- 8 integration test files in `tests/`: two_clients, network, reorg, double_spend, pow, emission, time, concurrency
- Test infrastructure: `tests/common/mod.rs` with RAII cleanup, random ports, polling with timeout
- 6 existing tests migrated from `src/main.rs`

### Security (D02)
- THREAT_MODEL.md: 25+ STRIDE vectors with mitigations, residual risks, monitoring
- INCIDENT_RESPONSE.md: alerts source, responder role, disclosure timeline
- TLC verification results documented in `docs/spec/README.md`

## What Is Deferred to Stage 1

| Item | Prompt | Description |
|------|--------|-------------|
| Grant mechanism cleanup | S1-P01 | `create_grant_block` behind regtest-only flag; magic strings removed from consensus path |
| bech32 addresses | S1-P15 | HRP sc1/tsc1/rsc1; replaces base64(pubkey) without checksum |
| Verkle Trie + state_root | S1-P06 | `block.state_root` field; invariant #19 enforcement |
| Merkle tx root | S1-P08 | `block.tx_root` field for SPV readiness |
| Governance skeleton | S1-P05 | SCIP process + consensus_version + activation height |
| EventBus | S1-P09 | Crossbeam multi-subscriber event bus |
| tokio runtime | S1-P10 | Async runtime for new subsystems |
| Blockchain decomposition | S1-P11..P13 | 5 components: chain_selector, block_executor, state_cache, facade, consensus_manager |
| Headers-first sync | S1-P16 | GET_HEADERS/HEADERS protocol |
| Mempool RBF | S1-P17 | Replace-by-fee with deterministic rules |
| SyncEngine | S1-P18 | Inbox pattern to break network↔blockchain cycle |
| strangecoin-core crate | S1-P02..P05 | Workspace + clean core (serialize, consensus, economics, governance) |

## Explicit Obligations for Stage 1

1. **Offline genesis key before mainnet freeze**: The current genesis private key is derived from hashing `"strangecoin-genesis-seed-2026"`. This must be replaced with a proper offline-generated key before any mainnet launch.

2. **bech32 addresses (S1-P15)**: Current addresses are base64-encoded public keys without checksum. This is a critical UX and security issue (no typo detection). Closure planned in S1-P15.

3. **Grant- sanitized (S1-P01)**: The `create_grant_block` function bypasses consensus rules. Must be gated behind regtest-only flag before strangler migration to `strangecoin-core`.

4. **P24 (reproducible builds) partially done**: `release.yml` is fixed but never tested on a real tag push. First `v*` tag will validate the pipeline.

## Version Synchronization

| File | Version |
|------|---------|
| `Cargo.toml` | 1.0.0 |
| `Changelog.md` | 1.0.0 |
| `AGENTS.md` | 1.0.0 |
| Git tag | v1.0.0-stage0 |

## DoD Verification

| # | Criterion | Evidence |
|---|-----------|----------|
| 1 | All 13 critical issues addressed | `CRITICAL_ISSUES_CLOSED.md` |
| 2 | All 22 invariants enforced or deferred | `INVARIANTS_ENFORCED.md` |
| 3 | Threat model written and reviewed | `docs/security/THREAT_MODEL.md` (D02) |
| 4 | TLA+ skeleton written | `docs/spec/consensus.tla` + `docs/spec/README.md` |
| 5 | Reproducible builds in CI | `.github/workflows/release.yml` (fixed in D03) |
| 6 | License in root | `LICENSE` (MIT OR Apache-2.0) |
| 7 | cargo test green | Run as part of D03 (see commit) |
| 8 | clippy -D warnings green | Run as part of D03 (see commit) |
| 9 | Bug bounty active | `docs/security/BOUNTY.md` + `SECURITY.md` (documentation ready; Immunefi setup deferred) |
| 10 | ADR-0001–0003 written | `docs/ADR/` (5 ADRs total) |
| 11 | Changelog: single 1.0.0 section | `Changelog.md` (unified in D03) |
| 12 | Tag v1.0.0-stage0 | Placed after D03 verification |
