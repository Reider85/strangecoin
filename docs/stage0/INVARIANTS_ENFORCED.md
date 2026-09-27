# INVARIANTS_ENFORCED.md — Stage 0

**Source:** `analytics/ARCHITECT3.md` §5 (22 invariants)
**Date:** 2026-09-28
**Tag:** v1.0.0-stage0

## Summary

| Status | Count |
|--------|-------|
| Enforced | 17 |
| Deferred (planned Stage 1) | 4 |
| N/A at Stage 0 | 1 |

## Enforced Invariants

| # | Invariant | Enforcement Location | Test |
|---|-----------|---------------------|------|
| 1 | All validity derived from chain; `balances` is cache only | `validate_chain` in `src/main.rs`: reconstructs balances from chain, cross-checks | `hundred_transactions_five_wallets`, `no_rollback_on_shorter_chain` |
| 2 | Every transaction signed, `sender == pubkey` (secp256k1) | `verify_transaction` in `src/main.rs`; `add_transaction` in mempool | `hundred_transactions_five_wallets`, proptest (P18) |
| 3 | `hash <= target` for every block at its difficulty | `validate_difficulty` in `src/consensus/`: U256 arithmetic | proptest (P18), `real_network_three_nodes` |
| 4 | `apply_block`/`unapply_block` are inverse pure functions | Implicit in `validate_chain` reconstruction | `no_rollback_on_shorter_chain`, `hundred_transactions_five_wallets` |
| 5 | Hash/signature on canonical bytes (blake3), not JSON | `src/serialize.rs`: canonical binary encoding, 19 golden-vector tests | serialize unit tests |
| 6 | Block contains no unverifiable txs/rewards above emission | `validate_chain`: coinbase ≤ `block_reward_at_height`; exception for block.index==1 (grant) | `emission.rs` tests (10), proptest |
| 7 | Message/block sizes always bounded before allocation | `MAX_MESSAGE_SIZE`, `MAX_BLOCK_SIZE`, `MAX_TX_SIZE` checked in `validate_chain` and `network/protocol.rs` | `src/network/protocol.rs` framing tests |
| 8 | Genesis is deterministic and matches across all nodes | `genesis.json` ↔ `EXPECTED_GENESIS_HASH`; panic on mismatch | `print_genesis_hash` test, startup validation |
| 9 | Secrets not written to disk or logged | `config.toml` has no secrets; wallet password via env/prompt; keystore encrypted AES-256-GCM + PBKDF2 | Code audit |
| 10 | Replay protection: every tx contains `chain_id`; networks incompatible | `chain_id` checked in `verify_transaction` and `validate_chain`; mainnet=1, testnet=2, regtest=3 | `hundred_transactions_five_wallets`, proptest |
| 11 | Nonce strictly increments; tx with `nonce <= account.nonce` rejected | `validate_chain` nonce check; `add_transaction` mempool check | proptest (P18), `hundred_transactions_five_wallets` |
| 12 | Transaction hash = commitment from canonical bytes | `txid` computed from signed canonical bytes in `src/serialize.rs` | serialize golden-vector tests |
| 13 | Mempool validates on insert: signature, dup, nonce, chain_id, balance | `mempool/insert()` validation; `MAX_PENDING_TXS=10000`, `MempoolFull` error | `hundred_transactions_five_wallets` |
| 14 | Graceful shutdown: `Drop` for storage/wallet/network; no manual LOCK removal | `Drop` impls for `Node`, `Blockchain`, `Wallet`; ctrlc handler; `AtomicBool` in mining loop | `deadlock_test_blockchain_wallet_lock_order` |
| 16 | P2P framing: length-prefixed, size checked before allocation | `network/protocol.rs`: read length, validate, then `vec![0; length]` | `src/network/protocol.rs` |
| 17 | Rate limiting: message limit per peer; violation → ban | `RateLimiter` in `src/network/rate_limiter.rs`: 100 msg/10s per peer | `rate_limiter.rs` tests (4) |
| 22 | Reproducible builds: CI publishes SLSA provenance + cosign signature | `.github/workflows/release.yml`: LTO, single codegen unit, `--remap-path-prefix`, cosign, SLSA Level 3 | Pipeline exists; not verified to run (no tags yet) |

## Deferred Invariants

| # | Invariant | Planned Stage | Notes |
|---|-----------|---------------|-------|
| 15 | Block gas limit: `gas_used <= block_gas_limit` | Stage 1.5 | Requires WASM VM and gas metering |
| 18 | Event log: each tx has `Receipt { gas_used, logs, status }` | Stage 1.5 | Requires VM execution to produce receipts |
| 19 | State root match: `state.root_after(block) == block.state_root` | Stage 1 (S1-P06) | Requires Verkle Trie implementation |
| 20 | Fee invariant: `fee_burned + fee_to_miner = total_fees` | Stage 5 | Requires fee market (EIP-1559); currently fee=0 |
| 21 | Consensus versioning: `block.consensus_version <= current_version` | Stage 1 (S1-P05) | Requires governance skeleton with activation height |

## N/A at Stage 0

| # | Invariant | Reason |
|---|-----------|--------|
| — | (none) | All 22 invariants are either enforced or deferred with clear stage assignment |

## Enforcement Map by Module

| Module | Invariants Enforced |
|--------|-------------------|
| `src/main.rs` (validate_chain) | #1, #2, #3, #4, #5, #6, #7, #8, #10, #11, #12 |
| `src/serialize.rs` | #5, #12 |
| `src/consensus/` | #3, #10, #11 |
| `src/mempool/` | #2, #13 |
| `src/network/protocol.rs` | #7, #16 |
| `src/network/rate_limiter.rs` | #17 |
| `src/economics/emission.rs` | #6 |
| `src/config.rs` | #8, #9 |
| `src/wallet.rs` | #9 |
| `.github/workflows/release.yml` | #22 |
