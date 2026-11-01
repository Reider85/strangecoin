# Strangecoin

**A production-grade Layer-1 blockchain written from scratch in Rust — PoW today, PoS-bound, with WASM smart contracts, a Noise-encrypted P2P layer, and a perpetual emission curve.**

[![CI](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml)
[![Release](https://github.com/Reider85/strangecoin/actions/workflows/release.yml/badge.svg)](https://github.com/Reider85/strangecoin/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/v/release/Reider85/strangecoin?include_prereleases&label=version)](https://github.com/Reider85/strangecoin/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.74%2B-orange.svg)](https://www.rust-lang.org)
[![GitHub stars](https://img.shields.io/github/stars/Reider85/strangecoin?style=social)](https://github.com/Reider85/strangecoin/stargazers)
[![GitHub downloads](https://img.shields.io/github/downloads/Reider85/strangecoin/total?label=downloads)](https://github.com/Reider85/strangecoin/releases)
[![Testnet Only](https://img.shields.io/badge/status-testnet--only-FFB347)](docs/SCIP/scip-0001-genesis-key-replacement.md)


---

![Strangecoin node session](assets/strangecoin-demo.gif)

> *Clone, build, run — block #4187 mined, BIP-152 compact block broadcast to 8 Noise-authenticated peers, Erlay reconciliation saved 78% of INV bandwidth.*

---

## Architecture

![Strangecoin layered architecture](assets/architecture.png)

**Pure core, impure shell.** `strangecoin-core` has zero I/O — no `tokio`, no `std::net`, no `std::fs`, enforced by CI source-gates — so the consensus rules can be reasoned about, fuzzed, and embedded without dragging a runtime or a database with them. The binary, the VM, the network, and storage are thin shells that pull from the core.

Every non-trivial decision is captured in an **ADR** (0001 through 0026 at last count); every protocol-level change is a **SCIP** that goes `Idea ? Draft ? Review ? Activation ? Finalized`.

---

## Quick Start

```bash
git clone https://github.com/Reider85/strangecoin.git && cd strangecoin
cargo build --release
cargo run --release -- run
```

Requires Rust 1.74+. Builds on Linux, macOS, and Windows. Default features include the P2P network, the WASM VM, and the GUI; for a headless node use `cargo run --release --no-default-features --features net,vm-wasm -- run`.

> **Testnet only.** The genesis allocation key is burned pending [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md). **Do not send real value to any Strangecoin address.**

---

## Features

### Chain & consensus

- **Hybrid PoW ? PoS Layer-1.** Sliding-window difficulty retarget every 2016 blocks (x4 / /4 clamp), median-time-past window of 11 blocks, +2h future-timestamp ban.
- **Perpetual emission.** 21,000,000 SC pre-tail cap, 50 SC initial reward halving every 210,000 blocks (~4 years), 0.6%/yr tail emission — modeled on Monero, deflationary for ~32 years then ~126,000 SC/yr forever.
- **secp256k1 ECDSA with recoverable signatures** — the same curve MetaMask, Ledger, and WalletConnect already speak.
- **bech32 addresses** — `sc1...` mainnet, `tsc1...` testnet, `rsc1...` regtest.
- **blake3** consensus hash; txid is a `blake3` commitment over the canonical signed serialization — same bytes in mempool, block, and receipt.
- **Sparse Merkle Trie (depth 256)** state commitment; `block.state_root` enforced as an invariant.
- **Deterministic genesis** pinned by an `EXPECTED_GENESIS_HASH` constant — a node refuses to start without it.
- **Wire size limits enforced before allocation** — 32 MB max message, 4 MB max block, 256 KB max tx — kill OOM vectors.
- **SCIP governance** — `consensus_version` + `activation_height` lets the chain upgrade by hard fork on a schedule.

### Smart contracts (Stage 1.5)

- **`wasmi` 2.0** runtime — `no_std`, deterministic, exact-pinned, fuzz-ready.
- **EOAs + contract accounts**, three tx types: `transfer`, `create`, `call`.
- **JSON ABI** in the Ethereum style, four-byte `blake3` method selector.
- **13 host functions** — `secp256k1_verify`, `blake3`, `keccak`, `sha256`, `storage_read/write`, `call`, `delegatecall`, `staticcall`, `block_context`, `gas_left`, `emit_log`, `bls_verify` (stub for PoS).
- **7 precompiles** at `0x01..0x07`: `ecrecover`, `sha256`, `keccak`, `blake3`, `ed25519_verify`, `bls_aggregate` (stub), `modexp`.
- **Hard resource limits** — 24 KB max code, 1 MB max storage, 1024 max call depth, 1 MB max memory.
- **Immutable `Receipt`s** with gas used, status, logs, and value-transfer audit trail; `block.receipts_root` commits to them all (Ethereum-style, separate from `tx_root`).
- **Reentrancy guarded** with `nonReentrant` pattern; contract storage integrated into the shared Sparse Merkle Trie — reorgs roll back contract writes through an overlay.

### P2P network (Stage 2)

- **Noise Protocol Framework** — XX handshake via `snow`, **ChaCha20-Poly1305** transport encryption with monotonic nonces, anti-replay, rekey.
- **Ed25519 `node_id` authentication** — peer identity is a signed commitment, not an IP address; HKDF-SHA256-derived deterministic ephemeral keys.
- **HELLO + capabilities negotiation** — `MIN_PROTO_VERSION = 2`, network-id check, capability bitfield.
- **Multi-source peer discovery** — seed IPs, **DNSSEC-validated DNS seeds** via `hickory-resolver`, `ADDR` gossip; ?8 peers in ?30s.
- **BIP-152 compact blocks** — siphash-2-4 short transaction IDs, mempool fill-up, `GET_BLOCK_TXN` / `BLOCK_TXN` round-tripping — **?50% bandwidth savings** on block broadcast.
- **Erlay reconciliation** — mincut sketches + INV fallback, **?80% bandwidth reduction** at 10k+ nodes (vendored `minisketch-rs`, SCIP-gated).
- **Token-bucket rate limiting** per peer + per-message-type sub-buckets + violation window + ban threshold.
- **4-factor weighted peer scoring** — latency (EWMA-smoothed), invalid messages, violations, uptime — with a `warn ? temp-ban ? persistent-ban` ladder.
- **Headers-first sync** — `GET_HEADERS ? HEADERS ? GET_BLOCKS ? BLOCKS`; `SyncEngine` is the single owner of incoming blocks (breaks the network?consensus cycle).

### Security as a first-class deliverable

- **STRIDE threat model** — 42 attack vectors (V-1 through V-42), each mapped to a mitigation, ADR, residual risk, and monitoring hook.
- **TLA+ consensus specification** — checked with TLC, **35,207 states at depth 6** across 10 safety invariants + liveness; `ReceiptsRootMatch` non-vacuous on 100k+ block instances.
- **22 ARCHITECT3 invariants** — the load-bearing rules the chain cannot violate (no double spend, no inflation, state root matches, consensus version enforced…) — regressed every stage.
- **9 `cargo-fuzz` targets ? 600s** on every CI push — canonical decode, host functions, gas metering, storage trie, Noise handshake, gossip INV, compact-block fill-up, Erlay reconciliation, plus a wasmi-vs-wasmtime differential target.
- **Reproducible release pipeline** — six-binary matrix (Linux/macOS/Windows ? x86_64/aarch64), SHA256 per artifact, **SLSA L3 provenance** + **cosign keyless OIDC** blob signatures, `RUSTFLAGS=--remap-path-prefix` for deterministic paths.
- **Bug bounty** — $1k / $10k / $100k tiers for low / medium / critical findings, 90-day coordinated disclosure, Immunefi integration path.

---

## Roadmap

Strangecoin ships in deliberate, tagged stages. Each one closes a named production gap and ends with a tagged release, a green CI matrix, and a threat-model update.

| Stage | Status | What it delivered |
|-------|--------|-------------------|
| **0** — Safe prototype | DONE `v1.0.0-stage0` | secp256k1 wallets, canonical serialization, real PoW + retarget, deterministic genesis, size limits, reproducible release pipeline |
| **1** — Architecture | DONE `v1.1.0-stage1` | Cargo workspace, **pure core (zero I/O)**, Sparse Merkle Trie, bech32, EventBus, headers-first sync, RBF mempool, SCIP governance |
| **1.5** — Smart contracts | DONE `v1.5.0-stage1.5` | `wasmi` 2.0 VM, EOAs + contract accounts, 13 host fns, 7 precompiles, gas metering, immutable Receipts with `receipts_root` |
| **2** — Production P2P | DONE `v2.0.0-stage2` | Noise XX + ChaCha20-Poly1305, Ed25519 node_id, BIP-152 compact blocks, Erlay, 4-factor peer scoring, rate limiting, shadow replay |
| **3** — Storage & state scaling | NEXT | RocksDB backend (crate already present, default-on), Verkle trie migration, state pruning, snapshot sync, Block-STM parallel execution |
| **4** — Developer experience | PLANNED | SDK, local devnet with faucet, indexer, JSON-RPC `eth_*` surface (Ethereum tooling interop) |
| **5** — Fee market & AA | PLANNED | EIP-1559-style fee market on the Stage 1 RBF groundwork, account abstraction, SCIP-001 dev fund |
| **6** — External audit & formal verification | PLANNED | Third-party security audit, TLA+ network spec, formal verification of consensus rules |
| **7** — Proof of Stake | PLANNED | BLS12-381 validator keys, slashing, 1,000 SC minimum stake, 5%-of-stake cap per validator, PoW?PoS migration gated by `activation_height` |

---

## Contributing

Pull requests are welcome. The bar is **"matches an existing ADR or proposes a new one, passes `cargo clippy -D warnings` and `cargo fmt --check`, and doesn't regress the 22 invariants."**

```bash
# Checks before you push
cargo fmt -- --check
cargo clippy --all -- -D warnings
cargo test --all
```

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full workflow — environment setup, commit conventions, determinism rules, branching, releases.

**Found a security vulnerability, fuzz panic, or wasmi?wasmtime divergence?** Do not open a public issue. Follow [`docs/security/SECURITY.md`](docs/security/SECURITY.md): email `security@strangecoin.io`, 48h SLA, 90-day coordinated disclosure, paid bounty per [`docs/security/BOUNTY.md`](docs/security/BOUNTY.md) ($1k / $10k / $100k tiers).

---

## Status & disclaimer

Strangecoin is **testnet-only.** The genesis allocation key was historically derived from a public seed string and is therefore **burned** — anyone with the seed (or git history) controls the 1e9 SC allocation. It is the sole thing [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md) exists to fix. The air-gapped key-generation tooling ([`examples/generate_offline_genesis_key.rs`](examples/generate_offline_genesis_key.rs), [`examples/print_genesis_hashes.rs`](examples/print_genesis_hashes.rs)) is already in the tree; what remains is the human ceremony of generating the new key offline, rebuilding genesis, and pinning the new hash.

**Do not send real value to any Strangecoin address.** This is a software-engineering project, not financial advice, not a token sale, and not an invitation to invest. Nothing here is a security, and nobody representing the project will ever ask you to send money to participate.

---

## License

Licensed under the **Apache License, Version 2.0**. See [`LICENSE`](LICENSE) for the full text.

---

*Built with Rust, blake3, secp256k1, wasmi, Noise, and a refusal to ship "almost."*

**Star it · Fork it · Try to break it.**


