# CRITICAL_ISSUES_CLOSED.md — Stage 0

**Source:** `analytics/ARCHITECT2.md` §1.1 (13 critical issues)
**Date:** 2026-09-28
**Status:** 9 closed, 3 partially closed, 1 deferred to Stage 1

## Summary

| Status | Count |
|--------|-------|
| Closed | 9 |
| Partially closed | 3 |
| Deferred | 1 |

## Detailed Status

| # | Issue | Status | Prompt | Evidence | Notes |
|---|-------|--------|--------|----------|-------|
| 1 | No transaction signatures | **Closed** | P04, P05, P07 | `verify_transaction` enforces `sender == address(pubkey)` via secp256k1 recoverable ECDSA | ed25519 completely removed; wallet.rs rewritten |
| 2 | `validate_chain` does not check PoW | **Closed** | P08 | `validate_difficulty` in `src/consensus/`: `hash <= target` via U256 arithmetic + retarget every 2016 blocks | |
| 3 | Tautological balance validation | **Closed** | P07, P11 | Balance reconstruction from chain history + cross-check against stored balances | `validate_chain` rebuilds from scratch |
| 4 | No block rewards / emission | **Closed** | P11 | `src/economics/emission.rs`: `block_reward_at_height` + tail emission formula | Grant block exception in `validate_chain` — closed by D01 test coverage, full cleanup in S1-P01 |
| 5 | Non-deterministic genesis | **Closed** | P10 | `genesis.json` + `EXPECTED_GENESIS_HASH` + panic on mismatch; regtest has own genesis (chain_id=3) | |
| 6 | Addresses without checksum | **Deferred** | — | `address_from_public_key` returns base64(pubkey) without checksum | Deferred to Stage 1: bech32 addresses (S1-P15) |
| 7 | `find_wallet_by_ip` returns random balance | **Closed** | P04 | Function removed from codebase | |
| 8 | Registration mutates `balances` directly | **Partially closed** | P14 | Registration goes through `mempool.insert` with validation | Grant mechanism still mutates balances outside blockchain rules — closed by S1-P01 |
| 9 | Password in plaintext in config | **Closed** | P15 | `config.json` deleted; password only via env var / interactive prompt | |
| 10 | Ad-hoc synchronization, races, OOM | **Closed** | P12, P13, P16, P17 | Length-prefixed framing with size check before allocation; rate limiting per peer (100 msg/10s + ban); `RwLock<BlockchainInner>`; graceful shutdown (AtomicBool + Drop + ctrlc) | |
| 11 | Non-canonical serialization | **Closed** | P06, P07 | `src/serialize.rs` (586 lines): canonical binary encoding via blake3; txid = commitment from signed canonical bytes; 19 golden-vector tests | serde_json only for config, not consensus |
| 12 | No block time validation | **Closed** | P09 | Median-time-past (window 11) + reject future timestamps (+2h) in `validate_chain` | |
| 13 | No tests / CI | **Partially closed** | P19 (D01), P20 | 48 unit/property tests in `src/`; 8 integration test files in `tests/` (D01); CI matrix `ci.yml` (3 OS × 2 toolchain) | Pipeline execution not independently verified; integration tests created in D01 |
| — | Release pipeline never ran | **Partially closed** | P24 (D03) | `release.yml` fixed with outputs.hashes + 6 targets | Pipeline execution not verified (no GitHub Actions access at time of D03) |
| — | Changelog claims non-existent P22 work | **Closed** | D02 | Entry corrected in Changelog.md with honest note | |

## Deferred to Stage 1

| Issue | Closure Prompt | Description |
|-------|----------------|-------------|
| #6 — Addresses without checksum | S1-P15 | Migrate to bech32 addresses (HRP sc1/tsc1/rsc1) |

## Residual Risks

1. **Grant mechanism** (issue #8 partial): `create_grant_block` bypasses consensus rules. Closure planned in S1-P01 (grant behind regtest-only flag).
2. **Release pipeline** (issue #13 partial): `release.yml` fixed but never tested. Will be verified on first `v*` tag push.
3. **Genesis key derived from public string**: `genesis_keypair()` hashes `"strangecoin-genesis-seed-2026"` to produce the private key. Residual risk documented in THREAT_MODEL.md (D02); offline key required before mainnet freeze.
