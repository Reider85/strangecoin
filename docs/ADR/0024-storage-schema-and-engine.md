# ADR-0024: Storage Schema and StorageEngine Trait

## Status

Accepted (2026-10-30) — S3-P01

## Context

ADR-0008 (Accepted) decided *that* Strangecoin migrates from `rusty-leveldb`
to RocksDB via a `StorageEngine` trait with a `migrations.rs` one-shot and a
`storage.engine` config flag — but it deliberately left the **concrete
schema** open ("the trait is defined in Stage 3"). Stage 3 is that stage.
Before any storage code lands, the on-disk layout, versioning semantics and
config surface must be fixed, or the migration will be re-done twice.

The *actual* LevelDB schema (verified in HEAD `db116f9`, `src/storage/mod.rs`
+ `src/blockchain/state_cache.rs`) is a thin `Arc<Mutex<rusty_leveldb::DB>>`
wrapper over five ad-hoc entries:

| Key (UTF-8 bytes) | Value | Problem |
|---|---|---|
| `b"chain"` | serde_json `Vec<Block>` — **the entire chain as one blob** | every `save_state()` rewrites the whole chain: O(chain length) write amplification per block |
| `b"balances"` | serde_json `HashMap<String, AccountState>` | full-state rewrite per save |
| `b"contract_storage"` | serde_json sorted `(addr, slots)` vector | full rewrite per save |
| `b"difficulty"` | serde_json `u32` | fine, but rides the same rewrite |
| `"{sender}:{nonce}"` | serde_json `Transaction` (mempool) | loaded by **full-DB scan**; never deleted — mined/stale txs accumulate forever |

Additional forces at play:

- **No schema version, no column families.** Legacy compatibility is
  handled by ad-hoc runtime hacks at open time
  (`migrate_initial_wallet_balance`, `migrate_addresses_to_bech32`,
  dual-format balance parsing). ROADMAP3 Этап 3 explicitly requires
  `schema.rs` (`KeyPrefix` enum, versioning) and
  `current_schema_version` in `m/schema_version`.
- **Dead config key.** `config.storage.path` (`./data/leveldb`) is
  validated but never used; the real path is `<exe_dir>/blockchain_db_{port}`
  (`db_path_for_port`). Operators cannot relocate the DB. The migration must
  close this bug, not carry it over.
- **ROADMAP3 CF list** specifies `chain`, `state`, `transactions`,
  `nonces`, `meta`, `peers`, `mempool_cache` — two of those names do not
  match the implemented architecture (see deviations below) and must be
  reconciled explicitly rather than silently.
- **Lock ordering.** Stage 1 nests `Arc<Mutex<DB>>` as the innermost lock
  level (AGENTS.md Lock Ordering). `rocksdb::DB` is thread-safe: the
  migration *removes* a lock level from the hierarchy — a documented
  improvement, not a side effect.
- **Invariant #1** (state cache rebuild from chain wins over any persisted
  cache) and the pure-core boundary (storage values that are consensus-
  derived stay derived) constrain what is allowed to become persistent.
- **Determinism (V-21 extension).** Key encoding must be pure functions;
  any height/time-like component in keys uses explicit big-endian `u64`
  parameters, never wall clock.

## Decision

**We will introduce `crates/strangecoin-storage` (S3-P04) exposing a
`StorageEngine` trait over seven RocksDB column families with a versioned
key schema (`SCHEMA_VERSION = 1`), per-block/per-account/per-transaction
value keys (eliminating the chain blob), and a fail-closed schema-version
guard; upgrades happen only through `migrations.rs`, never implicitly on
open.**

| Aspect | Decision |
|---|---|
| Engine | RocksDB via the `rocksdb` crate (per ADR-0008; redb/sled not revisited) |
| Column families | **7**: `meta`, `blocks`, `state`, `transactions`, `contract_storage`, `state_history`, `mempool` (table below) |
| Key encoding | pure functions in `schema.rs`: big-endian `u64` for heights/nonces, raw address/slot/txid bytes; lexicographic key order == numeric order for height-prefixed keys |
| Values | **opaque bytes** at the engine boundary; encode/decode of core types (`Block`, `AccountState`, `Transaction`) happens at the monolith edge. `strangecoin-storage` may depend on `strangecoin-core` (pure, 0 I/O) for the snapshot/witness modules (S3-P11/S3-P13); the reverse dependency is forbidden |
| Schema versioning | `meta.schema_version: u32`; legacy LevelDB blob layout = **0**, this schema = **1**; fresh DB → initialize to 1; existing DB with different value → `StorageError::SchemaVersion { found, expected }` fail-closed; upgrades only via `migrations.rs` (S3-P08) |
| Trait surface | `get` / `put` / `delete` / `write_batch` (atomic) / `iterate_prefix` (sorted, deterministic) / `flush`; `Cf` enum mirrors the CF list; **raw DB handle never leaves the crate** (ADR-0008 Decision, now binding) |
| Concurrency | `rocksdb::DB` is thread-safe ⇒ no `Arc<Mutex<DB>>` inside the engine; the DB-lock level disappears from the monolith lock ordering (S3-P07 documents the new order) |
| Write path | adopt-block writes **O(changed records)**: changed accounts → `state`, block → `blocks`, txs → `transactions` + `mempool` deletes, touched pre-state → `state_history`; no full-chain or full-state rewrite |
| Config surface | `storage.engine` (`"rocksdb"` default; `"leveldb"` transitional until S3-P08, then removed), `storage.path` **wired for real** (default `./data/rocksdb`); legacy `<exe_dir>/blockchain_db_{port}` detected for auto-migration (S3-P08) |
| Derived data | receipts, VM codes and `total_work` remain **derived** (rebuild-from-chain, invariant #1); not persisted in Stage 3 |
| Pin policy | `rocksdb` pinned via `=` (version resolved in S3-P06 against the current stable `rust-rocksdb`, then recorded here and in AGENTS.md); update path = SCIP after mainnet freeze |

### Column families

| CF | Key | Value | Notes |
|---|---|---|---|
| `meta` | `schema_version`, `tip_height`, `tip_hash`, `difficulty`, `chain_id` (name keys) | version `u32` BE / height `u64` BE / hash bytes / difficulty bytes / chain id bytes | replaces top-level `difficulty` blob; owns `m/schema_version` (ROADMAP3) |
| `blocks` | `height: u64 BE` → canonical block bytes; `hash: [u8;32]` → height `u64 BE` (index sub-keyspace) | block / height | replaces the `chain` blob; height key order == insertion order |
| `state` | address (UTF-8) | canonical `AccountState` bytes | replaces `balances` blob; O(1) `address → balance` (ROADMAP3 index — already the natural access path) |
| `transactions` | `txid: [u8;32]` (raw + index sub-keys) | `(height, tx_index)` + raw tx bytes | enables O(1) `txid → block` (replaces linear scan in `chain_has_tx`) |
| `contract_storage` | `address ‖ slot_key` | value bytes | replaces the `contract_storage` blob |
| `state_history` | `height: u64 BE ‖ address` | pre-block `AccountState` snapshot (canonical bytes) | written on adopt; **pruned** by height window (ADR-0025); powers archival queries and future expiry research |
| `mempool` | `sender ‖ nonce: u64 BE` | raw tx bytes | prefix-iterate per sender (no full-DB scan); **deleted** on mine/confirm — closes the Stage 1 leak |

### Deviations from the ROADMAP3 CF list (explicit, FBP precedent)

1. **No `nonces` CF** — nonce lives inside `AccountState` in the `state`
   CF; a separate CF would duplicate consensus state and risk divergence.
2. **No `peers` CF** — peer/ban state is the Stage 2 `network.json`
   subsystem (`strangecoin-net::peer::store`); moving it into RocksDB is
   not a Stage 3 task and provides no DoD coverage.
3. **Added `state_history`** — required to make pruning (ADR-0025)
   meaningful at all: the legacy schema stores current-state only, so
   EIP-4444-analog pruning would be a no-op without a historical
   sub-space.
4. **`chain` renamed to `blocks`** with per-height keys + hash index —
   ROADMAP3's name kept in spirit; the blob layout is explicitly
   rejected (write amplification).

## Consequences

### Positive

- **Write amplification drops from O(chain) to O(changed)** per block —
  the single largest pathology of the legacy schema; required before any
  meaningful chain length on disk.
- **O(1) lookups** for `txid → block`, `block hash → height`,
  `address → balance` (state CF), `contract address → slots` — linear
  scans (`chain_has_tx`, `block_by_hash`) are deleted.
- **Mempool lifecycle closes**: prefix-iterate per sender, delete on
  mine — the Stage 1 "stale txs accumulate forever" leak dies here.
- **Versioned upgrades**: `schema_version` + fail-closed guard replaces
  open-time hacks; future layout changes have a real migration rail.
- **One fewer lock level**: thread-safe `rocksdb::DB` removes the
  innermost `Mutex` from the monolith hierarchy.
- **Operational tooling**: RocksDB ships `ldb`/`sst_dump` inspection,
  block-based compaction tuning, snapshots — none of which exist for
  `rusty-leveldb` (ADR-0008 operational-tooling gap, now concretized).
- **Column-family isolation**: mempool churn compacts separately from
  chain data; `state_history` compaction/pruning (ADR-0025) does not
  touch consensus-critical CFs.

### Negative

- **C++ build dependency** on every platform (cc-rs); Windows requires
  the msys64 host preamble (AGENTS.md Common Gotchas); cold builds take
  minutes. Accepted — ADR-0008 already weighed this against redb.
- **Multi-CF backup/restore is not a single file copy** — operational
  procedure must use RocksDB checkpoint APIs or `ldb` bulk dump;
  snapshot files (ADR-0026) become the *portable* state export, which
  is the intended division of labor.
- **`state_history` grows with chain length** on archival nodes —
  bounded on full nodes by the ADR-0025 height window; archival growth
  is an accepted operator choice.
- **Migration complexity**: legacy blob layout → per-block layout is a
  *format conversion*, not a byte copy (S3-P08); rollback keeps the
  legacy directory as `.bak` (ADR-0008 rollback capability).
- **Binary size** grows with the RocksDB static link (~2–4 MB, ADR-0008
  estimate carried over).

### Neutral

- **Storage is an implementation detail, not consensus**: no SCIP is
  required for this schema itself. However, *future* consensus-relevant
  behaviors that ride on it (state expiry, receipts prune) are declared
  SCIP-gated now (ADR-0025) so the rail exists before mainnet freeze.
- **`strangecoin-storage` → `strangecoin-core` dependency is allowed**
  (core is pure, 0 I/O; snapshot/witness modules need `AccountState`
  serialization and SMT root recomputation). The reverse direction
  stays forbidden; the monolith depends on both.
- **`storage.engine` is transitional**: after S3-P08 deletes
  `rusty-leveldb`, the flag collapses to a single valid value and is
  removed in the same cleanup window (Changelog 3.0.0 breaking list).
- **Determinism**: key encoding is pure; nothing in the schema reads
  clocks or RNG. Height-window triggers (pruning, snapshot cadence)
  take the tip height as a parameter (Stage 2 `tick_at` pattern).

## Alternatives Considered

### Alternative 1: Single column family + `KeyPrefix` byte tags

- Pros: one CF to open/backup/checkpoint; simpler `open` path; prefix
  scans can span logical spaces in one iterator.
- Cons: one shared LSM compacts mempool churn together with chain data
  (write-amplification returns under mixed load); prefix-scan cost is
  paid over the whole keyspace index; per-space tuning (compression,
  bloom filters) is impossible. RocksDB CFs exist precisely for this
  workload split (Bitcoin Core, reth, FoundationDB-style separation).
- Why not chosen: compaction isolation and per-space tuning are core
  reasons to move to RocksDB at all.

### Alternative 2: Keep the JSON blob schema on top of RocksDB

- Pros: smallest possible diff; `migrations.rs` would be a byte copy.
- Cons: retains O(chain) write amplification — the migration would fix
  the engine but not the disease; no indices, no mempool delete, no
  versioning; ROADMAP3 Этап 3 tasks (schema.rs, indices, batch
  validation) all become no-ops. The DoD item "RocksDB migration
  завершена" would be technically green while every Stage 3 performance
  goal stays unreachable.
- Why not chosen: the point of Stage 3 is the schema, not the logo on
  the storage engine.

### Alternative 3: Persist the Sparse Merkle trie nodes in DB

- Pros: incremental `state_root` (no full HashMap recompute per block);
  natural substrate for Verkle migration and wire witnesses.
- Cons: large consensus-adjacent change: invariant #1 (rebuild-from-
  chain wins) semantics must be re-derived; bug surface moves into
  trie-node persistence; not required by any ROADMAP3 Этап 3 DoD item
  (light-client DoD is satisfied by witness verification, S3-P11);
  premature before the Verkle SCIP (ADR-0006 amendment) decides the
  trie's future at all.
- Why not chosen: scope and risk; recorded as a residual (persistent
  SMT) in `docs/stage3/RESIDUAL_RISKS_STAGE4_PLUS.md` (S3-P14).

### Alternative 4: redb / sled (revisit ADR-0008)

- Pros/Cons: identical to ADR-0008 §Alternatives (redb: less
  battle-tested at high write throughput, no CF model of this shape;
  sled: stalled maintenance).
- Why not chosen: ADR-0008 remains Accepted; this ADR does not reopen
  the engine choice, only its schema. Revisit trigger unchanged: a
  pure-Rust embedded engine may return for *embedded light-client* use
  cases in Stage 5+.

### Alternative 5: Expose the raw DB handle to the monolith

- Pros: no trait indirection; fastest possible bring-up.
- Cons: ADR-0008 explicitly rejected it ("no direct DB handle
  exposure"); the monolith would hard-code `rocksdb` types everywhere,
  making the engine untestable in-memory and re-creating the Stage 1
  `Arc<Mutex<DB>>` coupling.
- Why not chosen: the trait is the migration and testability rail;
  in-memory mock engines (S3-P04) depend on it.

## Related

- `docs/ADR/0008-rocksdb-plan.md` — Accepted engine decision (RocksDB vs
  redb/sled/leveldb); this ADR **amends** its §Decision with the
  concrete schema and binds the deferred "trait defined in Stage 3" item
- `docs/ADR/0025-state-pruning-strategy.md` (S3-P02) — consumes
  `state_history`; defines what pruning may delete
- `docs/ADR/0026-snapshot-sync-format.md` (S3-P03) — portable state
  export complements multi-CF backup
- ROADMAP3 «Этап 3» — CF list, `schema.rs`, `migrations.rs`,
  `current_schema_version`, indices, crash-recovery (deviations above)
- ARCHITECT3 §3.4 (state_cache as sole read path), §3.7 (storage/
  pruning/snapshot), §10.5 (strangler Stage 3)
- Invariant #1 (rebuild-from-chain) — `docs/stage2/INVARIANTS_ENFORCED.md`;
  preserved by keeping receipts/codes/total_work derived
- Stage 2 residuals — `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1
  (RocksDB crash-recovery semantics risk → S3-P10 crash suite)
- Stage 3 prompts: S3-P04 (crate + trait), S3-P05 (`schema.rs`),
  S3-P06 (`RocksDbEngine` + pin resolution), S3-P07 (monolith
  migration + config wiring), S3-P08 (`migrations.rs` + leveldb
  removal), S3-P09 (indices), S3-P10 (batch/crash/LRU)
- Windows host preamble (AGENTS.md) — rocksdb C++ build follows the
  blake3/secp256k1/wasmtime cc-rs precedent

## Renumbering note

ROADMAP3 originally planned the Stage 3 ADR block as ADR-0020..0022
(RocksDB vs redb / state pruning / snapshot sync). Stage 2 occupied
ADR-0020..0023, and the "RocksDB vs redb" question was already answered
by the earlier ADR-0008 (Accepted). Stage 3 therefore renumbers to
ADR-0024..0026: this ADR (schema, amending ADR-0008), ADR-0025
(pruning), ADR-0026 (snapshot format). The decision is recorded in
`analytics/prompt-stage3.md` §0.1/§2.1 and the S3-P01 commit message.
