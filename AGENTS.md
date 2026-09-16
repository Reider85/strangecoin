# AGENTS.md — Strangecoin Developer Guide

## Project Overview
Strangecoin is a Rust cryptocurrency (v1.0.0, edition 2021) — PoW blockchain with secp256k1 signatures, blake3 hashing, LevelDB storage. Monolithic `src/main.rs` (~2950 lines) + `src/wallet.rs`, modularized per Stage 0 roadmap (now complete).

**Key docs**: `analytics/prompt-stage0.md` (26 prompts for Stage 0), `analytics/ARCHITECT3.md` (architecture), `analytics/ROADMAP3.md` (phases), `docs/ADR/` (architecture decisions), `docs/CONTRIBUTING.md` (workflow, style, ADR process).

## Build & Test Commands
```bash
cargo check          # fast typecheck
cargo test           # runs all tests (~6s+)
cargo build          # debug build
cargo build --release
cargo clippy         # lints (no clippy.toml — default config)
```

## Running the Node
```bash
# GUI client (default)
cargo run

# Headless (if implemented)
STRANGECOIN_WALLET_PASSWORD=xxx cargo run -- --headless
```

## Test Notes
- Integration tests are in `src/main.rs` bottom (`#[cfg(test)]` module)
- Unit tests in `src/serialize.rs`, `src/consensus/`, `src/economics/emission.rs`, `src/network/rate_limiter.rs`
- Tests create temporary LevelDB instances in system temp dir (`std::env::temp_dir()`) — not `target/debug/`
- Network tests use `NETWORK_TEST_LOCK` static mutex to serialize execution (they write `network.json` next to test exe)
- Tests spin up real nodes with real TCP ports — no mocks or fixtures
- `proptest` (dev-dependency) used for property-based tests in `src/consensus/proptest.rs`

## Current Architecture (Stage 0)
```
src/
├── main.rs           # Blockchain, Node, WalletApp (GUI), mining, P2P sync
├── wallet.rs         # secp256k1 keystore (PBKDF2+AES-GCM)
├── error.rs          # StrangecoinError (thiserror) — includes lock ordering docs
├── config.rs         # Config struct, TOML loading, validation
├── address.rs        # address_from_public_key (secp256k1 → base64)
├── serialize.rs      # canonical binary serialization (blake3), txid, block_hash
├── consensus/        # real impl: chain_id, U256 math, retarget, verify_transaction
├── network/          # real impl: Node, RateLimiter, protocol, P2P TCP
├── mempool/          # real impl: Mempool with insert/eviction/size tracking
├── storage/          # real impl: LevelDB wrapper (Arc<Mutex<DB>>)
├── economics/        # real impl: emission schedule
├── cli/              # print_genesis_hash
├── blockchain/       # stub (// TODO: P03+)
├── api/              # stub (// TODO: P15+)
├── gui/              # stub, feature-gated (#[cfg(feature = "gui")]) — feature not in Cargo.toml
└── governance/       # stub (// TODO: P11+)
```

## Key Constraints (from ARCHITECT3.md)
- **No tokio** until Stage 1 (current: std threads + mpsc channels)
- **No state rent, PoS, EIP-1559, AA** — deferred to later stages
- **Consensus changes only via SCIP + activation height** (post Stage 0 freeze)
- **Canonical binary serialization** (blake3) — serde_json only for config/api
- **22 invariants** (ARCHITECT3 §5) must be enforced by end of Stage 0
- **25 STRIDE attack vectors** (ARCHITECT3 §6) must be mitigated by end of Stage 0

## Lock Ordering (critical — see `src/error.rs`)
1. **blockchain** (via `RwLock<BlockchainInner>`) — outer, first
2. **wallet** (file-based keystore lock) — inner, second
Never acquire wallet lock while holding blockchain write lock from a different call site.

## Development Workflow
1. Work through `analytics/prompt-stage0.md` prompts sequentially (P01→P26) — Stage 0 is complete as of v1.0.0
2. Each prompt: implement → `cargo check` → `cargo test` → verify checklist
3. Do not commit unless explicitly asked
4. New modules declared in `main.rs` with `mod xyz;` — code stays in main.rs until later prompts move it

## Config & Secrets
- `config.toml` — non-secret config only (network_id, node_mode, listen_addr, seeds, data_dir, log_level)
- **Never** put passwords/keys in config files
- Wallet password: env var `STRANGECOIN_WALLET_PASSWORD` or interactive prompt
- Keystore: `keystore/*.json` (encrypted PBKDF2+AES-GCM, filenames sanitize public key with `_` replacing `/`, `+`, `=`)
- `config.json` is legacy — migrated to `config.toml` on first run

## Database
- LevelDB at `./data/leveldb/` (configurable via `config.toml` `[storage]` path)
- Keys: `chain`, `balances`, `difficulty`, `<txid>` for pending txs
- `LOCK` file contention handled with retry logic

## Important Files to Know
| File | Purpose |
|------|---------|
| `analytics/prompt-stage0.md` | Stage 0 task breakdown (26 prompts) |
| `analytics/ARCHITECT3.md` | Full architecture spec (invariants, STRIDE, subsystems) |
| `analytics/ROADMAP3.md` | Phase timeline |
| `docs/ADR/0001-secp256k1-vs-ed25519.md` | Migration decision record |
| `src/error.rs` | All typed errors + lock ordering docs |
| `genesis.json` | Genesis block definition |

## Common Gotchas
- **Windows paths**: Use `C:\projects\strangecoin` not `/c/projects/strangecoin`
- **PowerShell**: Use `;` not `&&` for command chaining
- **GUI feature gate**: `#[cfg(feature = "gui")]` is in main.rs but `gui` feature is not defined in Cargo.toml — building with gui feature requires adding it to Cargo.toml first
- **Module stubs** have `// TODO: P0X наполнит` comments — real implementation comes in later prompts
- **tracing** already in use (not println!) — `tracing::info/warn/error/debug` throughout codebase
- **secp256k1 migration complete**: ed25519-dalek removed from Cargo.toml, wallet.rs uses secp256k1 exclusively
- **Network test isolation**: `real_network_three_nodes` test writes/reads `network.json` next to test exe — protected by `NETWORK_TEST_LOCK`

## PR / Commit Conventions
- No commits without explicit user request
- ADRs written **before** code changes (per ROADMAP3 §8)
- Each prompt = one logical change set with verifiable artifacts