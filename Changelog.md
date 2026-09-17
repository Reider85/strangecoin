# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-09-17 - P21: Warning & Dead Code Cleanup

### Fixed
- Removed 11 unused imports in `src/main.rs` (blake3, rand::Rng, rusty_leveldb::*, secp256k1::{RecoverableSignature, RecoveryId, Message, PublicKey, Secp256k1}, sha2::{Digest, Sha256}, uuid::Uuid) — these were only used in `#[cfg(test)]` code
- Removed 3 unused imports in `src/wallet.rs` (RecoveryId, uuid::Uuid, tracing::warn)
- Removed unused import `tracing::warn` in `src/network/mod.rs`
- Removed unused import `std::io::Result as IoResult` in `src/network/protocol.rs`
- Removed dead fields `mining_thread: Option<JoinHandle<()>>` and `last_sync: f64` from `WalletApp` struct
- Marked `create_genesis_block()` as `#[cfg(test)]` (only called from tests)
- Removed dead functions `bits_to_target()` and `target_to_bits()` from `src/consensus/mod.rs`
- Removed dead function `arbitrary_block()` and unused `Block` import from `src/consensus/proptest.rs`
- Fixed 2 deprecated `base64::encode()` calls → `BASE64.encode()` in `src/main.rs`

### Removed
- Unused dependency `winapi` from `Cargo.toml`
- `config.json` from git tracking (contains plaintext passwords; migrated to `config.toml`)

## [1.0.0] - 2026-09-16 - Sanitized Prototype (Stage 0 Complete)

### Added
- **Graceful shutdown (P17)**: Implemented coordinated shutdown sequence per ARCHITECT3 §4.7
  - Added `Storage` struct with `Drop` impl for LevelDB flush/close (removes manual LOCK file handling)
  - Added `Drop` impl for `Wallet` (keystore lock)
  - Enhanced `Node` with `Drop` impl for TCP listener/connection cleanup
  - Added shared `shutdown: Arc<AtomicBool>` signal across all components
  - Mining thread now respects shutdown signal in `mine_block_inner` loop
  - Sync thread now respects shutdown signal in sync loop
  - Network server now respects shutdown signal in accept loop
  - 30-second shutdown timeout with diagnostic panic on timeout
  - Structured logging for each shutdown phase
- **Storage module (P02/P16/P17)**: New `src/storage/mod.rs` with `Storage` wrapper for LevelDB
- **Module skeleton (P02)**: Created 10 subsystem stubs (blockchain, consensus, network, mempool, storage, api, cli, gui, economics, governance)
- **Typed errors (P02)**: `src/error.rs` with `StrangecoinError` enum (22 variants)
- **Tracing migration (P03)**: Replaced all `println!` with structured `tracing` crate logging
- **secp256k1 wallet (P04)**: Full migration from Ed25519 to secp256k1 (ECDSA) with PBKDF2+AES-GCM keystore
- **Replay protection (P05)**: Added `chain_id`, `nonce`, `address_from_public_key` to transactions
- **Canonical serialization (P06)**: Binary serialization with length-prefixing, big-endian encoding, blake3 hashing
- **txid + signature verification (P07)**: `txid` from canonical bytes, full `verify_transaction` in consensus
- **Difficulty validation + retargeting (P08)**: Sliding window retarget, target in block header, u256 comparison
- **Median-time-past + timestamp validation (P09)**: MTP with 11-block window, 2-hour future limit
- **Deterministic genesis (P10)**: `genesis.json` + `EXPECTED_GENESIS_HASH` constant
- **Tail emission (P11)**: Monero-style 0.6%/year tail emission after 21M cap, removed artificial mining limits
- **Size limits + length-prefixed framing (P12)**: MAX_MESSAGE_SIZE, MAX_BLOCK_SIZE, MAX_TX_SIZE with pre-allocation checks
- **P2P rate limiting (P13)**: Per-peer rate limiter (100 msg/10s, 5-min ban)
- **Mempool with validation (P14)**: Insert validates signature, nonce, chain_id, balance; MAX_PENDING_TXS=10000
- **Unified Config (P15)**: Single `config.toml` (no secrets), env var for wallet password, keystore-only secrets
- **RwLock for Blockchain (P16)**: Replaced `Mutex<Blockchain>` with `Arc<RwLock<Blockchain>>` for read concurrency
- **Property-based tests (P18)**: proptest coverage for consensus rules (7 properties)
- **Integration tests (P19)**: 7 E2E scenarios (two_clients, reorg, double_spend, pow, emission, time, network)
- **CI pipeline (P20)**: GitHub Actions matrix (Linux/macOS/Windows × stable/beta), clippy -D warnings
- **Warning cleanup (P21)**: Zero warnings, zero dead code, cargo fmt clean
- **Threat model (P22)**: STRIDE document with 25 attack vectors and mitigations
- **TLA+ spec skeleton (P23)**: Consensus safety/liveness properties in `docs/spec/consensus.tla`
- **Reproducible builds (P24)**: SLSA provenance + cosign signatures in release workflow
- **Bug bounty + ADR-0003 (P25)**: Immunefi program docs, hybrid PoW→PoS ADR
- **LICENSE + ADRs (P01)**: MIT/Apache-2.0 dual license, ADR-0004 (license), ADR-0005 (tracing)
- **DoD verification (P26)**: Stage 0 closure artifacts

### Changed
- `Blockchain` struct now uses `Storage` instead of direct `Arc<Mutex<DB>>`
- Manual LOCK file removal code removed from `Blockchain::new()`
- `ctrlc` handler replaced with graceful shutdown sequence

### Fixed
- Critical OOM vulnerability in network framing (P12)
- Missing difficulty validation in consensus (P08)
- Time-warp attack vector (P09)
- Non-deterministic genesis (P10)
- Inflation via coinbase overpayment (P11)
- Missing Drop impls for resources (P17)

### Removed
- Manual `fs::remove_file` for LevelDB LOCK file
- Artificial mining iteration limits (1000 iterations / 5 second sleep)
- `println!` statements (~30 locations)
- Ed25519 dependency (`ed25519-dalek`)
- Legacy `config.json` (migrated to `config.toml`)
- `[wallet]` section from `Cargo.toml`

### Security
- All 22 invariants from ARCHITECT3 §5 enforced
- 25 STRIDE attack vectors from ARCHITECT3 §6 mitigated
- Secrets never written to config files (P15)
- Replay protection via chain_id + nonce (P05)
- Rate limiting per peer (P13)
- Size limits checked before allocation (P12)