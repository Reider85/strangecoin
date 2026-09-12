# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **P06: Canonical Binary Serialization** — `serialize.rs` module
  - `serialize_transaction()`: canonical unsigned tx encoding (format_version, length-prefixed strings, BE integers)
  - `serialize_transaction_signed()`: unsigned encoding + length-prefixed signature
  - `txid()`: blake3 hash of signed transaction bytes (32-byte commitment)
  - `serialize_block_header()`: format_version, index, timestamp, previous_hash, merkle_root, nonce
  - `serialize_block()`: header + length-prefixed transaction count + signed transactions
  - `block_hash()`: blake3 hash of block header
  - Golden vector tests (12 test cases) for transactions, blocks, and hashes to prevent encoding drift
  - Uses `blake3` for consensus hashing (faster than SHA-256, standardized)
- **P05: Replay Protection** — chain_id, nonce, address_from_public_key
  - Extended `Transaction` struct with `nonce`, `chain_id`, `signature`, `is_coinbase` fields
  - Removed legacy `id` field (will be replaced by `txid` in P07)
  - Created `AccountState` struct with `balance` and `nonce` for account tracking
  - Added chain ID constants in `consensus/mod.rs`: MAINNET=1, TESTNET=2, REGTEST=3
  - Implemented `address_from_public_key()` in `address.rs` using base64 encoding
  - Created canonical binary serialization in `serialize.rs` with blake3 hashing
  - Updated `add_transaction` validation:
    - Rejects transactions with mismatched `chain_id`
    - Rejects transactions with invalid `nonce` (must be `account.nonce + 1`)
    - Verifies ECDSA signatures via secp256k1 public key recovery
    - Updates sender/receiver balances and nonces atomically
  - Updated `wallet::sign_transaction()` to sign canonical binary bytes
  - Updated genesis block and test transaction creation with new fields
  - Added `hex` and `blake3` dependencies to Cargo.toml
  - Added `recovery` feature to secp256k1 for signature recovery

### Changed
- `balances` HashMap now maps `String -> AccountState` instead of `String -> u64`
- LevelDB transaction keys changed from UUID to `sender:nonce` format
- Signature format changed from 64-byte compact to 65-byte recoverable (64 bytes + recovery ID)

### Fixed
- Backward compatibility: new Transaction fields use `#[serde(default)]` for existing LevelDB data