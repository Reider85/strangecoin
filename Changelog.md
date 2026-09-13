# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.8] - 2026-09-13

### Added
- **P09: Median-time-past + future timestamp protection**
  - `consensus::median_time_past()` — computes median timestamp of last 11 blocks (MTP)
  - `consensus::validate_timestamp()` — enforces timestamp > MTP and timestamp ≤ now + 2 hours
  - `consensus::MEDIAN_TIME_WINDOW = 11` — Bitcoin-style MTP window
  - `consensus::MAX_FUTURE_TIME = 7200` — 2-hour future tolerance (per ARCHITECT3.md §6 vector #4)
  - New error variants: `TimestampTooOld`, `TimestampInFuture` in `StrangecoinError`

### Changed
- `mine_block_inner()` now sets `timestamp = max(now, mtp + 1)` ensuring mined blocks always pass timestamp validation
- `validate_chain()` validates timestamp for every block (genesis block with timestamp=0 is allowed)

### Security
- Mitigates time-warp attack (STRIDE vector #4): prevents miners from manipulating timestamps to lower difficulty

### Tests
- All 24 integration tests pass including multi-node sync, concurrent transfers, and chain adoption

## [0.8.7] - 2026-09-12

### Added
- **P07: txid commitment + full signature verification**
  - `consensus::verify_transaction()` — centralized transaction signature verification using canonical binary serialization
  - `consensus::recover_pubkey_from_sig()` — helper to recover public key from ECDSA recoverable signature
  - `validate_chain()` now verifies all transaction signatures in every block (coinbase transactions skipped)
  - `Blockchain::add_transaction()` delegates to `consensus::verify_transaction()` for DRY validation

### Fixed
- **Double balance update bug**: Removed premature balance updates from `add_transaction()`. Balances now only update when blocks are mined in `mine_block()`, preventing balance drift during chain adoption/sync.

### Changed
- Transaction signature verification unified in consensus module (was duplicated inline in `add_transaction`)

### Tests
- All 24 integration tests pass including:
  - `three_instances_receive_transfer` — multi-node sync with signature verification
  - `no_rollback_on_shorter_chain` — chain adoption with balance reconciliation
  - `real_network_three_nodes` / `real_network_fast_registration_race` — concurrent P2P operations
  - `hundred_transactions_five_wallets` — stress test with many concurrent transfers

## [0.8.6] - 2026-09-10

### Added
- Stage 0 foundation: module skeleton, tracing migration, secp256k1 wallet migration
- Canonical binary serialization with blake3 (serialize.rs)
- Chain ID, nonce, address derivation (P05)
- Golden vector tests for serialization determinism

### Changed
- Migration from Ed25519 to secp256k1 (ECDSA) for EOA signatures
- Structured logging via tracing crate (replaced println!)