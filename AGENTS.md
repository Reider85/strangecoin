# AGENTS.md — Strangecoin Developer Guide

## Project Overview
Strangecoin is a Rust cryptocurrency (v0.8.6, edition 2021) implementing a PoW blockchain with Ed25519→secp256k1 migration in progress. Current state: monolithic `src/main.rs` (~2400 lines) + `src/wallet.rs` being incrementally modularized per Stage 0 roadmap.

**Key docs**: `analytics/prompt-stage0.md` (26 prompts for Stage 0), `ARCHITECT3.md` (architecture), `ROADMAP3.md` (phases), `docs/ADR/` (architecture decisions).

## Build & Test Commands
```bash
cargo check          # fast typecheck
cargo test           # runs 5 integration tests (~6s)
cargo build          # debug build
cargo build --release
```

## Running the Node
```bash
# GUI client (default)
cargo run

# Headless (if implemented)
STRANGECOIN_WALLET_PASSWORD=xxx cargo run -- --headless
```

## Test Notes
- Tests are in `src/main.rs` bottom (`#[cfg(test)]` module)
- 5 integration tests cover: chain validation, multi-node sync, concurrent transfers
- Tests create temporary LevelDB instances in `target/debug/blockchain_db_<port>`
- No test fixtures or mocks — tests spin up real nodes

## Current Architecture (Stage 0)
```
src/
├── main.rs           # Blockchain, Node, WalletApp (GUI), mining, P2P sync
├── wallet.rs         # Ed25519 keystore (PBKDF2+AES-GCM), being migrated to secp256k1
├── error.rs          # StrangecoinError (thiserror)
├── blockchain/       # stub
├── consensus/        # stub
├── network/          # stub
├── mempool/          # stub
├── storage/          # stub
├── api/              # stub
├── cli/              # stub
├── gui/              # stub (feature-gated)
├── economics/        # stub
└── governance/       # stub
```

## Key Constraints (from ARCHITECT3.md)
- **No tokio** until Stage 1 (current: std threads + mpsc channels)
- **No state rent, PoS, EIP-1559, AA** — all deferred to later stages
- **Consensus changes only via SCIP + activation height** (post Stage 0 freeze)
- **Canonical binary serialization** (blake3) — serde_json only for config/api
- **22 invariants** (ARCHITECT3 §5) must be enforced by end of Stage 0
- **25 STRIDE attack vectors** (ARCHITECT3 §6) must be mitigated by end of Stage 0

## Development Workflow
1. Work through `analytics/prompt-stage0.md` prompts sequentially (P01→P26)
2. Each prompt: implement → `cargo check` → `cargo test` → verify checklist
3. Do not commit unless explicitly asked
4. New modules declared in `main.rs` with `mod xyz;` — code stays in main.rs until later prompts move it

## Config & Secrets
- `config.toml` — non-secret config only (port, network_id, data_dir, log_level)
- **Never** put passwords/keys in config files
- Wallet password: env var `STRANGECOIN_WALLET_PASSWORD` or interactive prompt
- Keystore: `keystore/*.json` (encrypted PBKDF2+AES-GCM)
- `config.json` is legacy — migrated to `config.toml` on first run

## Database
- LevelDB at `./blockchain_db_<port>/` (port from config or env `PORT`)
- Keys: `chain`, `balances`, `difficulty`, `<txid>` for pending txs
- `LOCK` file contention handled with retry logic

## Important Files to Know
| File | Purpose |
|------|---------|
| `analytics/prompt-stage0.md` | Stage 0 task breakdown (26 prompts) |
| `ARCHITECT3.md` | Full architecture spec (invariants, STRIDE, subsystems) |
| `ROADMAP3.md` | Phase timeline |
| `docs/ADR/0001-template.md` | ADR template |
| `src/error.rs` | All typed errors (extend per prompt) |

## Common Gotchas
- **Windows paths**: Use `C:\projects\strangecoin` not `/c/projects/strangecoin`
- **PowerShell**: Use `;` not `&&` for command chaining
- **cargo test** runs integration tests that bind ports — run sequentially
- **println!** still present (~30 locations) — being migrated to `tracing` (P03)
- **ed25519-dalek** still in Cargo.toml — being replaced by `secp256k1` (P04)
- **Module stubs** have `// TODO: P0X наполнит` comments — real impl comes in later prompts

## PR / Commit Conventions
- No commits without explicit user request
- ADRs written **before** code changes (per ROADMAP3 §8)
- Each prompt = one logical change set with verifiable artifacts