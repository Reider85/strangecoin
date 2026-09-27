# ADR-0008: RocksDB as Persistent Storage Engine

## Status

Accepted

## Context

Strangecoin currently uses `rusty-leveldb` (v4.0.0) for persistent storage — block indices, balances, pending transactions, and chain state. This is a thin wrapper around Google's LevelDB with limited Rust ecosystem support.

Problems with the current approach:
- `rusty-leveldb` is a minimal binding; upstream maintenance is sporadic
- No compaction tuning exposed; performance degrades under sustained write load
- No native encryption-at-rest (required for future light-client wallet on-disk keys)
- Limited operational tooling (no `ldb` CLI equivalent for inspection/repair)

The storage layer is isolated behind `src/storage/mod.rs` (`Arc<Mutex<DB>>`), so the migration surface is small. However, the migration must preserve data integrity for any existing local testnets.

## Decision

We will migrate from `rusty-leveldb` to **RocksDB** (via the `rust-rocksdb` crate) in **Stage 3**. The migration path will use a `migrations.rs` module that reads the old LevelDB format and writes to RocksDB, activated by a config flag (`storage.engine = "rocksdb"`).

Key design choices:
- **Write path**: all writes go through a thin `StorageEngine` trait (defined in Stage 3), with `RocksDbEngine` as the default implementation
- **Read path**: same trait; the monolith calls `storage.get(key)` / `storage.put(key, value)` / `storage.delete(key)` — no direct DB handle exposure
- **Compaction**: `Options::set_level_compaction_dynamic_level_bytes(true)` for LSM-tree efficiency
- **Encryption**: RocksDB's `EncryptionAtRest` module (or application-level AES-GCM wrapping of the DB directory) — TBD in ADR at migration time
- **Migration**: one-shot `migrate_from_leveldb()` utility, run automatically on first startup with `storage.engine = "rocksdb"`, with rollback capability

## Consequences

### Positive
- Battle-tested storage engine used by Bitcoin Core, reth, and other production blockchain nodes
- Superior write throughput and compaction control compared to LevelDB
- Active upstream (Facebook/Meta); well-documented tuning parameters
- Native support for encryption-at-rest and column families (useful for separating chain state from mempool)

### Negative
- Larger binary size (~2-4 MB added); optional via Cargo feature flag `rocksdb`
- C++ dependency requires a C compiler at build time (cross-compilation complexity)
- Initial migration adds startup latency for existing users (one-time, < 1s for typical chain sizes)

### Neutral
- `rusty-leveldb` dependency removed from root `Cargo.toml`; `storage` module interface unchanged
- No consensus rule changes; storage is an implementation detail

## Alternatives

### Alternative 1: redb (pure Rust embedded database)
- Pros: zero C dependencies, compiles everywhere, ACID transactions
- Cons: less battle-tested for high-throughput write workloads; no compaction control; limited operational tooling; not used in production blockchain systems
- Why not chosen: RocksDB's maturity and proven performance in Bitcoin Core/reth outweigh the pure-Rust convenience. redb may be revisited for embedded light-client use cases in Stage 5+.

### Alternative 2: sled (pure Rust, tree-based)
- Pros: pure Rust, lock-free reads
- Cons: project maintenance has stalled (last significant release 2022); known memory-mapping issues on Windows; lacks encryption support
- Why not chosen: project health concerns; insufficient feature set for production blockchain storage.

### Alternative 3: Keep rusty-leveldb
- Pros: zero migration work; familiar API
- Cons: no path to encryption-at-rest; limited compaction tuning; upstream maintenance concerns; operational tooling gap
- Why not chosen: the storage layer is a critical infrastructure component; deferring the migration increases technical debt and blocks future features (encrypted wallet storage, compaction tuning for large chains).

## Related

- `src/storage/mod.rs` — current storage wrapper (to be replaced by `StorageEngine` trait)
- ADR-0001 (secp256k1) — crypto primitives (unchanged)
- Stage 3 roadmap item: "RocksDB migration + column families"
- ARCHITECT3 §3.4: state_cache as the sole read path for balances
