# AGENTS.md — Strangecoin Developer Guide

## Project Overview
Strangecoin is a Rust cryptocurrency (**v1.5.0**, edition 2021) — PoW blockchain with secp256k1 signatures, blake3 hashing, LevelDB storage. **Stage 1.5 complete** (tag `v1.5.0-stage1.5`, see `docs/stage1.5/STAGE1_5_SUMMARY.md`): WASM smart contracts via `crates/strangecoin-vm-wasm` (wasmi `=2.0.0`, 13 host functions, 7 precompiles, JSON ABI, gas metering, reentrancy guard, limits, receipts/events, contract storage в SMT). **Stage 1 complete** earlier (tag `v1.1.0-stage1`, `docs/stage1/STAGE1_SUMMARY.md`): pure core, Sparse Merkle state root (32-byte keys, depth-256 binary SMT; true Verkle/KZG deferred to Stage 3+ — ADR-0006 amended), headers-first sync, EventBus, bech32, tokio, 5-component blockchain split. Stage 0 closed via debt prompts D01–D03 (`docs/stage0/STAGE0_SUMMARY.md`). **Residual obligations remain** — `docs/stage1.5/STAGE1_5_SUMMARY.md` §4 (Stage 2+ deferred list + 2 ops-gates for mainnet freeze) and `docs/stage1/STAGE1_SUMMARY.md` §6.

**Key docs**: `analytics/prompt-stage0.md`, `analytics/prompt-stage1.md`, `analytics/prompt-stage1.5.md`, `analytics/ARCHITECT3.md`, `analytics/ROADMAP3.md`, `docs/ADR/`, `docs/stage1.5/STAGE1_5_SUMMARY.md`, `docs/stage1/STAGE1_SUMMARY.md`, `docs/security/THREAT_MODEL.md`.

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
- VM tests: `crates/strangecoin-vm-wasm/tests/**` (wasmi e2e; `wasmtime` is a dev-only differential oracle, never linked into the production binary)
- Tests create temporary LevelDB instances in system temp dir — not `target/debug/`
- Network tests use `NETWORK_TEST_LOCK` (write `network.json` next to test exe)
- Real TCP nodes, random ports; no mocks
- `proptest` for property tests (core consensus, chain_selector, state round-trip, merkle, witness, abi types)
- Run tests via `cargo test --workspace` (single-package `-p` runs may rebuild dev-deps differently on this Windows host)

## Current Architecture (Stage 1.5)
```
crates/strangecoin-core/src/     # pure core, 0 I/O
├── lib.rs            # modules + re-exports (incl. VmExecutor)
├── types.rs          # Block, BlockHeader, Transaction (Transfer/Create/Call), AccountState (EOA/Contract)
├── serialize.rs      # canonical binary (blake3), FORMAT_VERSION, merkle_root, receipts
├── consensus.rs      # chain_id, U256, retarget, MTP, CURRENT_CONSENSUS_VERSION, MAX_CODE_SIZE
├── state/            # inner (apply/unapply + apply_block_with_vm), sparse_merkle.rs, contract_storage.rs, witness.rs
├── economics/        # emission.rs, fee_market.rs (Stage 5 stub)
├── governance/       # scip.rs (SCIP + activation height)
├── abi/              # JSON ABI (S1.5-P12): AbiDocument, head/tail encode/decode, blake3 method_selector
├── chain_selector.rs # fork choice: work → timestamp → hash
├── address.rs        # bech32 (sc1/tsc1/rsc1)
├── vm/               # VmExecutor trait, ExecutionContext/ExecutionResult/Receipt, gas (BLOCK_GAS_LIMIT), limits
└── error.rs          # CoreError

crates/strangecoin-vm-wasm/src/  # wasmi runtime (feature vm-wasm, default-on)
├── lib.rs            # WasmiExecutor (register_code, insert_abi, execute; precompile routing)
├── gas.rs            # instruction counting + host-call weights (ADR-0014)
├── host/             # 13 host functions: crypto/ storage/ calls/ context/ logs/
└── precompiles/      # 0x01..0x07 (bls_aggregate NotImplemented until Stage 7)

src/                  # node monolith (orchestrator)
├── lib.rs            # Node, mining, P2P orchestration
├── blockchain/       # facade, block_executor (gas-limit enforcement), state_cache, chain_selector, consensus_manager
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
- **Canonical binary serialization** (blake3) — serde_json only for config/api/tooling (ABI JSON documents are tooling-side; calldata bytes go through the canonical ABI codec)
- **Grant blocks**: only with `allow_grant_blocks=true` (regtest / legacy migration)
- **wasmi pinned** `=2.0.0`; gas constants hardcoded (ADR-0014/0015) — post-freeze changes via SCIP only
- **Anti-goals Stage 2+**: Noise/Erlay Stage 2, Block-STM Stage 3+, RocksDB Stage 3, WIT ABI Stage 4+, EIP-1559/AA Stage 5, K-framework/ZK-VM Stage 6+, BLS12-381 Stage 7, PoS Stage 7
- **22 invariants**: `docs/stage1.5/INVARIANTS_ENFORCED.md` (21 enforced + #20 fee-split deferred Stage 5)

## Lock Ordering (critical — see `src/error.rs`)
1. **blockchain** (via `BlockchainFacade`, internally `RwLock`) — outer, first
2. **wallet** (file-based keystore lock) — inner, second
Never acquire wallet lock while holding blockchain write lock from a different call site.

## Development Workflow
1. Stage 0 prompts: `analytics/prompt-stage0.md` (done, D01–D03)
2. Stage 1 prompts: `analytics/prompt-stage1.md` (done, S1-P01–S1-P22)
3. Stage 1.5 prompts: `analytics/prompt-stage1.5.md` (done, S1.5-P01–S1.5-P19; P12 gap closed 2026-10-18)
4. Each prompt: implement → `cargo check` → `cargo test --workspace` → verify checklist
5. Do not commit unless explicitly asked
6. ADR before code that changes architecture; Changelog only after artifacts exist
7. Stage 2 (network crate) / Stage 3 (RocksDB) / Stage 4 (dev-experience) open after tag `v1.5.0-stage1.5`

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
| `analytics/prompt-stage1.5.md` | Stage 1.5 task breakdown (S1.5-P01..P19) |
| `docs/stage1.5/STAGE1_5_SUMMARY.md` | Stage 1.5 DoD evidence + residual obligations §4 |
| `docs/stage1.5/INVARIANTS_ENFORCED.md` | 22 invariants, Stage 1.5 locations (source of truth) |
| `docs/stage1/STAGE1_SUMMARY.md` | Stage 1 DoD evidence + open obligations §6 |
| `docs/security/THREAT_MODEL.md` | STRIDE model v3.1 (Stage 0/1/1.5 vectors) |
| `docs/ADR/0012..0015` | Stage 1.5 architecture decisions (wasmi, JSON ABI, gas, precompiles) |
| `crates/strangecoin-vm-wasm/src/lib.rs` | WasmiExecutor (runtime entry point) |
| `crates/strangecoin-core/src/abi/` | JSON ABI codec (encode/decode/selector) |
| `genesis.json` | Genesis block definition |

## Common Gotchas
- **Windows paths**: Use `C:\projects\strangecoin` not `/c/projects/strangecoin`
- **PowerShell**: Use `;` not `&&` for command chaining
- **GUI**: feature `gui` is defined; default build is headless
- **cargo-fuzz**: not runnable on this Windows host — use `cargo run --example canonical_decode_soak`; coverage-guided runs live in CI jobs `fuzz-canonical-decode` + Stage 1.5 VM targets (push/PR)
- **tracing** only (no println!)
- **Network test isolation**: `NETWORK_TEST_LOCK` + `network.json` next to test exe
- **vm-wasm feature**: default-on; light nodes build with `--no-default-features` (no VM, Create/Call txs rejected as `UnsupportedTxType`)
- **ABI golden vectors**: `tests/abi_roundtrip.rs` pins selector bytes and wire offsets — changing them is a wire-format break

## PR / Commit Conventions
- No commits without explicit user request
- Commits/push are made as **Reider85 <krot1113@yandex.ru>** (remote: `github.com/Reider85/strangecoin`); verify `git config user.name`/`user.email` before committing
- ADRs written **before** code changes
- Each prompt = one logical change set; prompt ID in commit message
