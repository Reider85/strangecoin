# Changelog

## 1.0.0 — Stage 0 (Sanitized Prototype)

### P25: Bug bounty + ADR-0003 (hybrid PoW→PoS) (2026-09-17)

- Created `docs/ADR/0003-hybrid-pow-pos.md` — hybrid PoW (Stage 0–6) → PoS (Stage 7+) migration decision
- Created `docs/security/BOUNTY.md` — bug bounty program: scope, 3 reward tiers ($1k/$10k/$100k), 90-day disclosure, Immunefi setup
- Created `docs/security/SECURITY.md` — security contacts, PGP key placeholder, 48h SLA, safe harbor policy

### P24: Reproducible builds (2026-09-17)

- Added `[profile.release]` with LTO, single codegen unit, symbol stripping for deterministic builds
- Created `.github/workflows/release.yml` — release pipeline triggered on `v*` tags
  - Builds 6 targets: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64/aarch64
  - `RUSTFLAGS="--remap-path-prefix"` strips build paths for reproducibility
  - SHA256 checksums for each artifact
  - SLSA Level 3 provenance via `slsa-github-generator`
  - Keyless cosign signatures (Sigstore/OIDC)
- Created `docs/security/REPRODUCIBLE_BUILDS.md` — verification instructions

### P23: TLA+ consensus spec skeleton

- Created `docs/spec/consensus.tla` with safety properties (NoDoubleSpend, NoInflation, AllTxSigned, NonceMonotonic, ChainContinuity) and liveness
- Created `docs/spec/consensus.cfg` for TLC model checker
- Created `docs/spec/README.md` with verification instructions

### ~~P22: Threat Model (STRIDE)~~ — NOT COMPLETED IN STAGE 0

- **Entry corrected:** P22 was NOT completed during Stage 0. This changelog entry was added prematurely (commit "docs: add CHANGELOG.md" predates any THREAT_MODEL.md file).
- P22 is executed in **D02** (debt prompt, see `analytics/prompt-stage1.md`).
- Files `docs/security/THREAT_MODEL.md` and `docs/security/INCIDENT_RESPONSE.md` are created in D02 commit.

### D03: DoD-verification + version sync + tag (2026-09-28)

- Fixed `.github/workflows/release.yml`: added `aggregate-hashes` job for SLSA provenance, added 6th target (`aarch64-pc-windows-msvc`)
- Cleaned repository: removed `test.md`, `.idea/`, `.codebuddy/`, `.opencodeignore` from tracking; `Cargo.lock` now tracked
- Synchronized version to `1.0.0` across `Cargo.toml`, `Changelog.md`, `AGENTS.md`
- Created `docs/stage0/CRITICAL_ISSUES_CLOSED.md` — 13 issues from ARCHITECT2 §1.1 with honest statuses
- Created `docs/stage0/INVARIANTS_ENFORCED.md` — 22 invariants from ARCHITECT3 §5 with enforcement locations
- Created `docs/stage0/STAGE0_SUMMARY.md` — Stage 0 completion summary with Stage 1 obligations

### S1-P05: Governance skeleton — SCIP + consensus_version + activation height

- Created `crates/strangecoin-core/src/governance/` module: `scip.rs` with `ScipDocument`, `ScipStatus`, `ConsensusRules`, `current_consensus_version()` for activation-by-height logic
- Added `consensus_version: u32` field to `Block` struct (`#[serde(default)]` for JSON backward compat)
- Added `CURRENT_CONSENSUS_VERSION: u32 = 1` constant to `crates/strangecoin-core/src/consensus.rs`
- Bumped `FORMAT_VERSION` from 1 to 2 in `crates/strangecoin-core/src/serialize.rs` — `serialize_block_header` now includes `consensus_version` in canonical binary encoding
- Updated `deserialize_transaction` to accept both format versions 1 and 2
- Added consensus_version validation to `validate_chain()` — rejects blocks with mismatched version
- Updated all 20 Block struct literal sites across the codebase
- Created `docs/SCIP/README.md` and `docs/SCIP/scip-0000-process.md` — SCIP process skeleton
- Created `tests/consensus_version.rs` — rejection tests for stale/future consensus_version
- All golden vector tests updated for new serialization format

### S1-P08: Merkle tx root in block header

- Added public `merkle_root(txids: &[[u8; 32]]) -> [u8; 32]` function to `crates/strangecoin-core/src/serialize.rs` — blake3-merkle tree with odd-node duplication
- Added `compute_tx_root(transactions: &[Transaction]) -> [u8; 32]` helper that computes txids and calls `merkle_root`
- Added `tx_root: [u8; 32]` field to `Block` struct (`#[serde(default)]` for JSON backward compat)
- Bumped `FORMAT_VERSION` from 3 to 4 — `serialize_block_header` now includes stored `tx_root` instead of recomputing it
- Added `TxRootMismatch { expected, got }` variant to `CoreError`
- Added `validate_tx_root(block: &Block) -> Result<(), CoreError>` to consensus module
- Integrated tx_root computation in block creation paths: regtest genesis, `create_grant_block`, `mine_block_inner`, `load_genesis_block`
- Added tx_root validation loop in `validate_chain()`
- Updated all 29 Block literal sites across codebase
- Created `crates/strangecoin-core/tests/merkle.rs` — 10 unit tests + 2 proptests: empty, single, pair, odd, 7-tx, deterministic, different inputs, compute_tx_root round-trip, proptest determinism, proptest non-zero
- All golden vector tests updated for new serialization format

### S1-P12: block_executor + state_cache

- Created `src/blockchain/block_executor.rs` — `validate_and_apply(parent_state, block, view)`: the single "validate + apply" path for one block (position/header hash, `consensus_version`, `tx_root`, timestamp MTP + future bound, PoW + retarget schedule for `block.target`, transaction signatures with the grant-block exemption, `core::state::apply_block`, `state_root`); it never selects a tip and never writes to storage
- Added `BlockView { chain, now, allow_grant_blocks }` (`new`/`next`) and `now_secs()` — everything a block is validated against besides its parent state
- Created `src/blockchain/state_cache.rs` — `StateCache` is the only reader of balances/nonces: read API (`balance`, `nonce`, `keys`, `iter`, `nonzero_balances`, …), write API (`commit`, `replace`, `invalidate`, `unapply_block`, `credit`, `ensure_account`) and `rebuild_from_chain` (reconstruction from the chain, mechanism lifted out of `validate_chain`)
- Added `Blockchain::rebuild_state_cache()`; `validate_chain` now delegates validation to the executor and compares against the reconstructed balances for invariant #1 (the chain wins over the cache)
- Switched the monolith apply paths to the executor: `create_genesis_block`, `create_grant_block`, `mine_block` all run through `validate_and_apply` + `StateCache::commit`
- `Blockchain.balances` is now a `StateCache`; `BlockchainDeserialize` and the chain-adoption/sync paths build it with `StateCache::from_accounts`. Direct `balances` mutations outside `state_cache` removed (rg control: only a commented-out legacy block in `Blockchain::new` remains)
- Added `StrangecoinError::InvalidBlock(String)` carrying `block N: <reason>`
- Created `tests/block_executor.rs` — 20 component tests: valid block applied to parent state + rejection matrix (index, previous_hash, header hash, `consensus_version`, `tx_root`, MTP/future timestamp, PoW above target, target change outside a retarget height, unsigned transfer, spending without funds, coinbase inflation, missing coinbase, wrong `state_root`, matching `state_root`, grant-block opt-in flag, genesis on a non-empty chain)
- Created `tests/state_cache.rs` — 5 component tests: rebuild reproduces the chain state, rebuild repairs a tampered cache (invariant #1), `unapply_block` rolls the cache back on reorg, `invalidate`, tampered chain rejected (header hash and `tx_root` paths)
- Fixed two clippy deny-level lints that stopped `cargo clippy` from compiling (`consensus_proptest` reward bound, `first_wallet` loop); the workspace now clippies without errors — 89 pre-existing warnings in untouched files remain as debt
- `cargo test --workspace` green (strangecoin 15 + 24 integration targets, strangecoin-core 38 + 7 test targets)

### S1-P15: bech32 addresses with network HRP

- Added `bech32 = "0.11"` to `crates/strangecoin-core/Cargo.toml`; `crates/strangecoin-core/src/address.rs` rewritten: `hrp_for_network` (sc / tsc / rsc for mainnet / testnet / regtest), `encode_address(pk, network_id)`, `decode_address(s) -> (PublicKey, network_id)`, `address_from_public_key` now returns `Result<String, CoreError>` (bech32m of the 33-byte compressed pubkey) — base64 addresses eliminated from the core crate
- Added `CoreError` variants: `InvalidAddressChecksum`, `InvalidAddressFormat`, `UnknownAddressHrp`, `UnknownNetworkId`, `Bech32EncodeError`; mapped to `StrangecoinError` in `src/error.rs`
- `src/consensus/mod.rs::load_genesis` now derives the genesis receiver via `encode_address` — genesis block hash changed (breaking, mainnet not launched): recomputed `EXPECTED_GENESIS_HASH` = `0xcf3440b9…a356375` via `--print-genesis-hash`, updated `genesis.json`
- `src/blockchain/blockchain_facade.rs`: legacy base64-address migration on DB open (existing account keys and wire fields re-encoded to bech32, unknown strings left as-is)
- `crates/strangecoin-core/tests/consensus_proptest.rs`: address strategies now handle the `Result` return
- Added `tests/two_clients.rs::bech32_address_transfer` — HRP assertions (`rsc1…`) + transfer between bech32 addresses
- Fixed the build: `eframe`/`winapi` made optional with a declared `gui` feature (`gui = ["dep:eframe", "dep:winapi"]`), eframe usage in `src/lib.rs` gated with `#[cfg(feature = "gui")]`, headless path drains the sync channel until shutdown
- Closed issue #6 ("Addresses without checksum") in `docs/stage0/CRITICAL_ISSUES_CLOSED.md` — 10 closed, 0 deferred
- Audit: zero base64 in `strangecoin-core/src`; remaining base64 only in `wallet.rs` (keystore key material) and the intentional legacy-migration path in `blockchain_facade.rs`
- `cargo test --workspace` green (all targets, including the network suite)

### S1-P16: headers-first sync (binary wire protocol)

- Added `BlockHeader` struct (index, timestamp, previous_hash, hash, nonce, target, consensus_version, state_root, tx_root) to `crates/strangecoin-core/src/types.rs` with `Block::header()` / `Block::from_header()` round-trip
- Added canonical header codec to `crates/strangecoin-core/src/serialize.rs`: `serialize_header` (hash excluded, 157-byte `HEADER_WIRE_LEN`), `deserialize_header` (recomputes the hash, never trusts the wire), `header_hash`; `serialize_block` refactored onto the same prefix; added `deserialize_block` and `deserialize_transaction_signed` — counts validated before allocation, trailing bytes rejected (invariants #7/#16)
- Added `validate_header_pow` (declared-hash + PoW) and `cumulative_work_headers` to `crates/strangecoin-core/src/consensus.rs` — fork choice can run on headers alone; body rules still run in `validate_chain` via `adopt_candidate`
- Added `CoreError::HeaderHashMismatch` mapped to `StrangecoinError::InvalidBlock` in `src/error.rs`
- Rewrote `src/network/protocol.rs` with binary count-prefixed messages: `GET_HEADERS [0x01][from_height:u64]`, `HEADERS [0x02][count][len][bytes]…`, `GET_BLOCKS [0x03][count][hash×32]`, `BLOCKS [0x04][count][len][bytes]…`; `MAX_HEADERS_BATCH=2000` / `MAX_BLOCKS_BATCH=128` enforced before any allocation; text messages start with ASCII letters so they can never collide with tags 0x01..=0x04 (pre-P16 peers silently ignore binary requests → EOF/timeout → fallback); unit tests for roundtrips, oversized counts, trailing garbage, tag/text collision
- Created `src/network/sync.rs`: `HeaderCache` (PoW + parent linkage + index continuity, stops at the first invalid header so a bad header never reaches tip selection), `plan_best_branch` (tips → ancestry to shared genesis → `ChainSelector::is_better` against the local chain), `sync_headers_first` (HELLO → batched GET_HEADERS → plan → GET_BLOCKS for exactly the missing hashes → block-hash verification → `adopt_candidate`, which still runs the full `validate_chain` over the bodies); any error means the peer cannot serve the binary protocol and the caller falls back to legacy `GET_BLOCKCHAIN`
- Added `BlockchainFacade::headers_from_height` / `blocks_by_hashes` — server-side lookups for the new requests
- `Node::start_server` now serves multiple requests per connection (10s read timeout bounds idle handlers), dispatching on the first payload byte before the UTF-8 text path; `Node::sync_blockchain` now fetches even on a fresh chain (previously skipped entirely), tries headers-first first, falls back to `GET_BLOCKCHAIN` when the peer is pre-P16 or had nothing better (mempool gossip preserved), and skips the fallback after a headers-first adoption
- Created `tests/sync_headers.rs` — 3 real-TCP tests: fresh node syncs 20 blocks via headers-first (balances + `validate_chain`), equal chain reports nothing better (0 blocks downloaded), longer fork resolved via headers-first (reorg + balance equality)
- `cargo test --workspace` green (all targets)

### S1-P18: SyncEngine inbox — network↔blockchain cycle broken (ADR-0010)

- Created `docs/ADR/0010-sync-engine.md` — decision record written before the code; audit rule: network may use facade-API reads and the inbox, never `adopt_candidate` / `adopt_wire` / `apply_tx` / `save_state` (GUI `sync_rx → adopt_wire` remains a documented residual, mining is out of scope)
- Created `src/network/sync_engine.rs`: `Incoming` enum (`NewBlock`, `NewHeaders`, `NewTx`, `CandidateChain` — each with `from: Option<SocketAddr>` attribution); `Inbox` = two bounded tokio-mpsc lanes (headers cap 64, blocks/candidate cap 256) with `try_send`, a bounded seen-set (cap 4096, cleared on overflow) deduplicating by tip hash / block hash / txid, `push` for pull paths (drop on full, no ban) and `push_inbound` for the `UPDATE_BLOCKCHAIN` handler (drop on full + ban the attributed peer through the existing `RateLimiter`, ADR-0010 backpressure); `SyncEngine::run` drains strictly sequentially with `select!` biased toward the headers lane, observes the shared shutdown flag on a 100ms tick and exits when both lanes close; `spawn` attaches to the ambient tokio runtime, or runs on a dedicated thread with a current-thread runtime when there is none (plain `#[test]` threads)
- Engine pipeline per message: `CandidateChain` → `facade.adopt_candidate`; success → `announce` (tip-changed check → `BlockReorged` only when the old tip did not survive → `StatePersisted` → `BlockApplied` → GUI snapshot over `sync_tx`); rejection → legacy mempool salvage (`mempool_txs` ∪ `pending_transactions` → `apply_tx` → `save_state`, no events); `NewBlock` → `add_block` + announce; `NewTx` → `apply_tx` + `TxAccepted`/`TxRejected`; `NewHeaders` acknowledged only (sync is pull-driven in Stage 1)
- `src/lib.rs`: `Node` and `MiningTask` gained an `inbox: Option<Inbox>` field; `start_server` spawns the engine once and clones the inbox into every connection handler; the inbound `UPDATE_BLOCKCHAIN` handler now parses and `push_inbound`s only; `sync_blockchain()` lost its `sync_tx` parameter and enqueues candidates (headers-first result and legacy `GET_BLOCKCHAIN` fallback) instead of adopting/announcing inline; `Node::new` no longer takes `sync_tx`; the sync task, GUI node and post-mining sync round inherit `node.inbox`; read-serving handlers (`GET_HEADERS` / `GET_BLOCKS` / `GET_BLOCKCHAIN`) unchanged
- `src/network/sync.rs`: `sync_headers_first(addr, network_id, local_chain, shutdown)` is now pure download + plan — `SyncOutcome { candidate: Option<Vec<Block>>, headers_ingested, blocks_downloaded, peer_tip_height }` replaces `adopted`; `BlockchainFacade` import removed from the download loop; adoption moved to the engine
- Created `tests/sync_engine.rs` — deterministic race: two peers barrier-synchronized push the same branch (exactly 1 `BlockApplied`, 0 reorgs), a worse fork twice (0 events, chain unchanged), a better fork twice (1 `BlockApplied` + 1 `BlockReorged`), final tip/`validate_chain` asserted; 4 engine unit tests (inbox dedupe, full pull lane drops without ban, full inbound lane bans the producer, lane separation)
- Updated `tests/sync_headers.rs` (takes `outcome.candidate` and adopts through the facade API in the test) and `tests/network.rs` (sync nodes inherit the server node's inbox; `sync_blockchain()` without arguments)
- Audit: `rg 'adopt_candidate|adopt_wire|apply_tx|save_state' src/network/` → only `sync_engine.rs` (plus one doc reference); handler and `sync_blockchain` write-calls eliminated
- `cargo test` green (all targets), `cargo clippy --all-targets` adds no new warnings

### S1-P19: Stage 1 integration test matrix + first fuzz target

- Created `tests/state_root.rs` — e2e invariant #19 through the node adoption path: a chain whose headers commit to real state roots (crafted child over the canonical genesis + grant prefix) passes `adopt_candidate` → `rebuild_from_chain` → `block_executor` on a second facade (balances applied, `validate_chain` green); a byte-flipped `state_root` (re-sealed, valid PoW) is rejected and leaves the second node untouched; zero `state_root` (opt-in "no commitment") still adopts
- Created `tests/events.rs` — DoD «3 subscribers» on a live node: real `start_server` (SyncEngine spawned per ADR-0010), three bus subscribers, engine-driven `TxAccepted` + `StatePersisted`/`BlockApplied` fan-out asserted per subscriber in publish order (tip hash + heights pinned)
- Created `tests/network_id.rs` — HELLO isolation (S1-P14): foreign `network_id` → connection closed with EOF, peer banned at the rate limiter (`check(local_addr)` → `PeerBanned`); control connection with the native `CHAIN_ID_REGTEST` is served `GET_BLOCKCHAIN`
- Verified `tests/two_clients.rs` / `tests/reorg.rs`: already on the facade API + bech32 (`rsc1`) — no update needed; DoD test-side items (headers-first, 3 subscribers, tie-breaking proptest, bech32 round-trip) all have artifacts (table row match for S1-P22)
- Created `fuzz/Cargo.toml` + `fuzz/fuzz_targets/canonical_decode.rs` (cargo-fuzz target: raw bytes → the four canonical decoders, panic = crash) + `fuzz/README.md` (run instructions + host constraints + run log)
- **cargo-fuzz/libFuzzer cannot run on this dev machine** (recorded in `fuzz/README.md`): ASan unsupported on the linkable `x86_64-pc-windows-gnu` target (rustc: «address sanitizer is not supported for this target»), no Visual Studio/MSVC (`link.exe` absent, so the ASan-capable msvc target cannot build), libFuzzer's Windows C++ runtime needs clang/MSVC (`__pragma(comment(linker, "/alternatename:..."))` — g++ unsupported by design; MSYS2 `clang64` empty), and `cargo install cargo-fuzz` fails in MinGW `ld` on the non-ASCII user profile path
- Fallback runner `examples/canonical_decode_soak.rs` (deterministic xorshift: garbage frames + size mix + mutated real serialized blocks; panic = non-zero exit) **found a real bug within seconds**: OOB panic in `deserialize_block` on a mutated real block — `read_u32_be`/`read_u64_be` indexed past truncated frames (36-byte tx slice at `sig_len`), block loop also used a stale `remaining` check. Fixed in `crates/strangecoin-core/src/serialize.rs`: bounds-checked `Result` integer readers, all decoder call sites propagate `?`, live per-transaction length check; clippy cleanups in the same file (`is_coinbase` via `bytes.get`, `map(txid)`)
- Fallback soak result (2026-10-04): 15 s smoke clean; **10-minute run: 28,744,131 inputs, 0 panics, exit 0**; `cargo test --workspace` green after the fix (each new test also runs isolated), full suite ≈ 46–71 s < 180 s; `cargo clippy --all-targets` exit 0

### S1-P21: Threat model update for Stage 1 surfaces

- `docs/security/THREAT_MODEL.md` → **v3.0**: added section 5 «Additional Stage 1-Specific Vectors» with **V-34..V-42** — headers-first poisoning (S1-P16), state_root manipulation (S1-P06), witness spoofing (S1-P07), tx_root manipulation (S1-P08), consensus_version downgrade (S1-P05), network downgrade/confusion (S1-P14), RBF fee-war DoS (S1-P17), SyncEngine inbox flooding (S1-P18), HRP confusion (S1-P15); each vector: Mitigation → S1-PXX → код → тест + Residual Risk + Monitoring
- Updated existing vectors honestly: **V-32** (grant blocks) — obligation closed by S1-P01 (`tests/grant_flag.rs`), residual shrinks to «flag must stay off on mainnet»; **V-33** (release pipeline) — workflow fixed in D03, GitHub Actions run still pending first v* tag
- Mitigations Map: new §6.2 Stage 1 prompt→vector table; Coverage Matrix extended; Test Coverage Map rows for V-34..V-42 (all green) + Stage 0 gap reassessment (V-11 closed via `tests/network_id.rs`, V-15 partial)
- Residual Risks (§7): cargo-fuzz not runnable on Windows dev host (no MSVC/ASan) — fallback soak found and fixed OOB panic in `deserialize_block`; zero state_root opt-in; witness not on the wire until Stage 2; TLA+ spec does not model Verkle/reorg yet
- Monitoring (§8): Stage 1 metrics (root/version rejects, HELLO bans, header rejects, RBF rate, inbox drops); §8.4 fuzzing status folded from S1-P19 (target `canonical_decode`, 28,744,131 inputs / 0 panics post-fix)
- Review checklist (§9): self-review per ARCHITECT3 §17 security items completed for this revision
- `docs/security/INCIDENT_RESPONSE.md` → **v3.0**: Stage 1 scope + alert sources (root/tx_root rejects, consensus_version rejects, HELLO mismatches, header rejects, RBF rate, inbox drops, soak-runner crash); «Stage 1+» placeholders activated
- Docs-only change for the threat model; no consensus/code behavior touched
- Also fixed pre-existing compile error in untracked `crates/strangecoin-core/src/chain_selector.rs` (required by `lib.rs` but never committed; test type error `String` vs `&str` in `chain_info_from_blocks`) — file added to the tree so `cargo test --workspace` builds from clean checkout
- `cargo test --workspace` + `cargo clippy --all-targets` green after the fix
