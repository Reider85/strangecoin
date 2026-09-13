# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-09-14

### Added
- **P10: Deterministic genesis** (Stage 0 consensus hardening)
  - `genesis.json` — canonical genesis configuration (format_version, network_id, chain_id, timestamp, initial_holder, initial_amount, block_reward, tail_emission_rate, max_supply_pre_tail, target_block_time, retarget_interval)
  - Deterministic genesis keypair derived from fixed seed `strangecoin-genesis-seed-2026` (Stage 0; real offline key before mainnet freeze)
  - `EXPECTED_GENESIS_HASH` constant — blake3 hash of genesis block header for mainnet anchor
  - `load_genesis()` — loads and constructs genesis block from genesis.json
  - `validate_genesis()` — enforces genesis hash match on startup (skipped for regtest)
  - `is_regtest(network_id)` — detects regtest mode (network_id == 3) for test genesis generation
  - `--print-genesis-hash` CLI command — outputs genesis hash for verification
  - GenesisMismatch error — clear panic on genesis hash mismatch with expected/got values
  - Units: satoshi-based (1 SC = 10^8 satoshi); `initial_amount=1_000_000_000` = 10 SC premine

### Changed
- **Blockchain startup**: Replaced inline `create_genesis_block()` with `genesis.json`-driven genesis loading
- **Existing DB**: Validates first block hash == `EXPECTED_GENESIS_HASH` on startup (unless regtest)
- **Regtest mode**: Generates own genesis with chain_id=3, bypasses EXPECTED_GENESIS_HASH check

### Security
- Closes critical gap: non-deterministic genesis allowed chain splits between nodes
- Deterministic genesis ensures all mainnet nodes start with identical genesis block
- Genesis hash anchored in code prevents silent chain substitution attacks