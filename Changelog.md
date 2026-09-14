# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.6] - 2026-09-14

### Added
- P2P rate limiting per peer (`src/network/rate_limiter.rs`)
  - 100 messages per 10 seconds window per peer
  - 5-minute ban on rate limit exceeded
  - Automatic ban expiry
  - Independent counters per peer
  - `tracing::warn!` logging on ban events
- `PeerBanned` error variant in `StrangecoinError`
- Rate limiting applied to:
  - Incoming P2P connections (`Node::start_server`)
  - Outgoing blockchain sync requests (`Node::sync_blockchain`)
- Mempool implementation (`src/mempool/mod.rs`) — P14
  - `Mempool` struct with `HashMap<TxId, Transaction>` and sender nonce index
  - `MAX_PENDING_TXS = 10_000` capacity limit
  - `insert()` with full validation: signature, duplicate, chain_id, nonce, balance
  - `remove()` for mined transaction cleanup
  - `get_pending()` for block construction
- New error variants in `StrangecoinError`:
  - `MempoolFull(usize)` — capacity exceeded
  - `DuplicateTx` — duplicate transaction rejection
  - `InsufficientBalance { available, required }` — balance check failure

### Changed
- `Node` struct now includes `rate_limiter: Arc<RateLimiter>`
- `MiningTask` struct includes rate limiter for sync after mining
- `src/network/mod.rs` exports `RateLimiter`
- `Blockchain` struct: replaced `pending_transactions: Vec<Transaction>` with `mempool: Mempool`
- `add_transaction()` now delegates to `Mempool::insert()` with canonical validation
- `mine_block()` uses `mempool.get_pending()` and removes mined txs via `mempool.remove()`
- `save_state()` persists mempool transactions to LevelDB
- `validate_chain()` validates mempool transactions against reconstructed balances
- Network protocol serializes `mempool_txs` for P2P sync
- `Blockchain::serialize` / `Deserialize` custom impl for mempool wire format

### Tests
- `network::rate_limiter::tests::test_rate_limit_exceeded`
- `network::rate_limiter::tests::test_banned_peer_rejected`
- `network::rate_limiter::tests::test_ban_expires`
- `network::rate_limiter::tests::test_independent_peers`
- All 36 core integration tests pass (mempool validation, mining, sync, serialization)

### Implemented Prompts
- P13: P2P rate limiting per peer (from `analytics/prompt-stage0.md`)
- P14: Mempool: validation on insert + MAX_PENDING_TXS (from `analytics/prompt-stage0.md`)