# AGENTS.md — Strangecoin Developer Guide

## Project Overview
Strangecoin is a Rust cryptocurrency (**v1.1.0**, edition 2021) — PoW blockchain with secp256k1 signatures, blake3 hashing, LevelDB storage. **Stage 1 complete** (tag `v1.1.0-stage1`, see `docs/stage1/STAGE1_SUMMARY.md`): pure core in `crates/strangecoin-core`, Sparse Merkle state root (32-byte keys, depth-256 binary SMT; true Verkle/KZG deferred to Stage 3+ — ADR-0006 amended, BUG-S0-011 fixed), headers-first sync, EventBus, bech32, tokio, 5-component blockchain split. Stage 0 closed with D03 (`docs/stage0/STAGE0_SUMMARY.md`); Stage 0 was completed via **debt prompts D01–D03** (P19/P22/P26 were not executed inline). **Residual obligations remain** — see `docs/stage1/STAGE1_SUMMARY.md §6` (offline genesis key → ops gate via SCIP-0001 after S1.5-P01 code fix, cargo-fuzz on Windows, TLA+ coverage, release-pipeline CI run, zero `state_root` opt-in, repo hygiene).

**Key docs**: `analytics/prompt-stage0.md`, `analytics/prompt-stage1.md`, `analytics/ARCHITECT3.md`, `analytics/ROADMAP3.md`, `docs/ADR/`, `docs/stage1/STAGE1_SUMMARY.md`, `docs/security/THREAT_MODEL.md`.

## Build & Test Commands
```bash
cargo check                      # fast typecheck
cargo test --workspace           # all tests (workspace)
cargo clippy --all-targets -- -D warnings
cargo build
cargo build --release
cargo run --example canonical_decode_soak   # fuzz smoke (fallback; cargo-fuzz via CI job fuzz-canonical-decode)
```

## Running the Node
```bash
# Headless (gui feature off by default)
STRANGECOIN_WALLET_PASSWORD=xxx cargo run -- --headless

# GUI client (requires --features gui)
cargo run --features gui
```

## Test Notes
- Integration tests live in top-level `tests/` (not in `src/main.rs`)
- Core unit tests: `crates/strangecoin-core/src/**` and `crates/strangecoin-core/tests/`
- Tests create temporary LevelDB instances in system temp dir — not `target/debug/`
- Network tests use `NETWORK_TEST_LOCK` (write `network.json` next to test exe)
- Real TCP nodes, random ports; no mocks
- `proptest` for property tests (core consensus, chain_selector, state round-trip, merkle, witness)

## Current Architecture (Stage 1)
```
crates/strangecoin-core/src/     # pure core, 0 I/O
├── lib.rs            # modules + re-exports (incl. VmExecutor)
├── types.rs          # Block, BlockHeader, Transaction, AccountState
├── serialize.rs      # canonical binary (blake3), FORMAT_VERSION, merkle_root
├── consensus.rs      # chain_id, U256, retarget, MTP, CURRENT_CONSENSUS_VERSION
├── state/            # inner (apply/unapply), sparse_merkle.rs, witness.rs
├── economics/        # emission.rs, fee_market.rs (Stage 5 stub)
├── governance/       # scip.rs (SCIP + activation height)
├── chain_selector.rs # fork choice: work → timestamp → hash
├── address.rs        # bech32 (sc1/tsc1/rsc1)
├── vm/traits.rs      # VmExecutor trait only (Stage 1.5 wasmi lives outside)
└── error.rs          # CoreError

src/                  # node monolith (orchestrator)
├── lib.rs            # Node, mining, P2P orchestration
├── blockchain/       # facade, block_executor, state_cache, chain_selector, consensus_manager
├── network/          # protocol (headers-first), rate_limiter, sync, sync_engine
├── events.rs         # EventBus (crossbeam)
├── mempool/          # RBF-capable mempool
├── wallet.rs         # secp256k1 keystore (PBKDF2+AES-GCM)
├── config.rs         # allow_grant_blocks flag, TOML config
├── storage/          # LevelDB wrapper
└── error.rs          # StrangecoinError + lock ordering docs
```

## Key Constraints (from ARCHITECT3.md / ROADMAP3)
- **Strangler**: core grows in `strangecoin-core`; monolith remains working orchestrator
- **0 I/O in core**: no fs/net/tokio/leveldb in `crates/strangecoin-core/src`
- **Consensus changes** via SCIP + activation height (post mainnet freeze)
- **Canonical binary serialization** (blake3) — serde_json only for config/api
- **Grant blocks**: only with `allow_grant_blocks=true` (regtest / legacy migration)
- **Anti-goals Stage 1.5+**: WASM until 1.5, Noise/Erlay Stage 2, RocksDB Stage 3, EIP-1559/AA Stage 5, PoS Stage 7
- **22 invariants** enforced (`docs/stage1/INVARIANTS_ENFORCED.md`)

## Lock Ordering (critical — see `src/error.rs`)
1. **blockchain** (via `BlockchainFacade`, internally `RwLock`) — outer, first
2. **wallet** (file-based keystore lock) — inner, second
Never acquire wallet lock while holding blockchain write lock from a different call site.

## Development Workflow
1. Stage 0 prompts: `analytics/prompt-stage0.md` (done, D01–D03)
2. Stage 1 prompts: `analytics/prompt-stage1.md` (done, S1-P01–S1-P22)
3. Each prompt: implement → `cargo check` → `cargo test --workspace` → verify checklist
4. Do not commit unless explicitly asked
5. ADR before code that changes architecture; Changelog only after artifacts exist

## Config & Secrets
- `config.toml` — non-secret config only
- **Never** put passwords/keys in config files
- Wallet password: env var `STRANGECOIN_WALLET_PASSWORD` or interactive prompt
- Keystore: `keystore/*.json` (encrypted PBKDF2+AES-GCM)
- Genesis key residual: seed string is public until offline key replacement (see STAGE1_SUMMARY obligations)

## Database
- LevelDB at `./data/leveldb/` (configurable via `config.toml` `[storage]` path)
- State cache rebuilt from chain on adoption (invariant #1)

## Important Files to Know
| File | Purpose |
|------|---------|
| `analytics/prompt-stage1.md` | Stage 1 task breakdown (D01–D03 + S1-P01..S1-P22) |
| `docs/stage1/STAGE1_SUMMARY.md` | Stage 1 DoD evidence + open obligations |
| `docs/stage1/INVARIANTS_ENFORCED.md` | 22 invariants, Stage 1 locations |
| `docs/security/THREAT_MODEL.md` | STRIDE model v3.0 (Stage 0 + Stage 1 vectors) |
| `docs/ADR/0006..0010` | Stage 1 architecture decisions |
| `crates/strangecoin-core/src/vm/traits.rs` | VmExecutor (Stage 1.5 hook) |
| `genesis.json` | Genesis block definition |

## Common Gotchas
- **Windows paths**: Use `C:\projects\strangecoin` not `/c/projects/strangecoin`
- **PowerShell**: Use `;` not `&&` for command chaining
- **GUI**: feature `gui` is defined; default build is headless
- **cargo-fuzz**: not runnable on this Windows host — use `cargo run --example canonical_decode_soak`; coverage-guided 10-min runs live in CI job `fuzz-canonical-decode` (push/PR)
- **tracing** only (no println!)
- **Network test isolation**: `NETWORK_TEST_LOCK` + `network.json` next to test exe

## PR / Commit Conventions
- No commits without explicit user request
- Commits/push are made as **Reider85 <krot1113@yandex.ru>** (remote: `github.com/Reider85/strangecoin`); verify `git config user.name`/`user.email` before committing
- ADRs written **before** code changes
- Each prompt = one logical change set; prompt ID in commit message
