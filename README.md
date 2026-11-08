# Strangecoin

**A production-grade Layer-1 blockchain written from scratch in Rust — a cryptocurrency for AI agents — PoW today, PoS-bound, with WASM smart contracts, a Noise-encrypted P2P layer, and a perpetual emission curve. Built MCP-first (chain state served as tools), ACP-native (agent-to-agent payments and task coordination), with an agent-native JSON-RPC surface.**

> **AI-agent orientation.** Strangecoin is engineered for autonomous economic agents: wallets are first-class agent identities (programmable key derivation, batched/sponsored txs, gasless UX via paymasters in Stage 5), the node exposes itself as an **MCP server** (tools: `get_block`, `get_balance`, `submit_tx`, `call_contract`, `subscribe_events`), and the protocol supports **ACP** (Agent Communication Protocol) messages so agents can negotiate, settle, and route payments peer-to-peer without a centralized coordinator. See [ADR-0028](docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md), [ADR-0029](docs/ADR/0029-mcp-server.md), [ADR-0030](docs/ADR/0030-acp-agent-communication-protocol.md), and the extension prompts in [`analytics/prompt-stage3.md`](analytics/prompt-stage3.md) §8.

[![CI](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml)
[![Release](https://github.com/Reider85/strangecoin/actions/workflows/release.yml/badge.svg)](https://github.com/Reider85/strangecoin/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/v/release/Reider85/strangecoin?include_prereleases&label=version)](https://github.com/Reider85/strangecoin/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.74%2B-orange.svg)](https://www.rust-lang.org)
[![Rust Edition](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Release](https://img.shields.io/badge/release-v2.0.0--stage2-yellow.svg)](https://github.com/Reider85/strangecoin/releases)
[![Lines of Rust](https://img.shields.io/badge/rust-99.3%25-lightgrey.svg)](https://github.com/Reider85/strangecoin/search?l=rust)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-green.svg)](#prerequisites)
[![GitHub stars](https://img.shields.io/github/stars/Reider85/strangecoin?style=social)](https://github.com/Reider85/strangecoin/stargazers)
[![GitHub downloads](https://img.shields.io/github/downloads/Reider85/strangecoin/total?label=downloads)](https://github.com/Reider85/strangecoin/releases)
[![Testnet Only](https://img.shields.io/badge/status-testnet--only-FFB347)](docs/SCIP/scip-0001-genesis-key-replacement.md)
[![AI-Agent Ready](https://img.shields.io/badge/AI--agent-MCP%20%7C%20ACP%20%7C%20JSON--RPC-blueviolet)](docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](#contributing)

---

![Strangecoin node session](assets/strangecoin-demo.gif)

> *Clone, build, run — block #4187 mined, BIP-152 compact block broadcast to 8 Noise-authenticated peers, Erlay reconciliation saved 78% of INV bandwidth.*

---

[Introduction](#introduction) · [Quick Start](#quick-start) · [Testing](#testing) · [Architecture](#architecture) · [Roadmap](#roadmap) · [Contributing](#contributing)

---

> **TESTNET-ONLY — BURNED GENESIS KEY** (`BUG-S0-015` / `SCIP-0001`)
>
> Strangecoin is currently **testnet-only**. The genesis allocation (1e9 SC) was historically controlled by a key derived from a **public** seed string — that key is **burned**. Anyone knowing the seed (or reading old git history) controls the genesis allocation.
>
> **Do not send real value.** Mainnet is impossible until [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md) replaces the genesis key with an offline-generated one.

---

## Introduction

### What is Strangecoin?

Strangecoin is a **full cryptocurrency written from scratch in Rust** — and, starting with the Stage 4 design track, **a cryptocurrency for AI agents**. It is not a fork of any existing coin — the address format (`rsc1` bech32), chain IDs, genesis block, consensus parameters, network protocol and improvement process are all native to this project.

**Built for AI agents.** Strangecoin is engineered so that autonomous economic agents are first-class participants, not bolted-on consumers of a human wallet:

- **MCP (Model Context Protocol) server** — the node exposes itself as an [MCP](https://modelcontextprotocol.io) server over stdio + HTTP+SSE, serving chain state as tools (`get_block`, `get_balance`, `get_nonce`, `submit_tx`, `call_contract`, `estimate_fee`, `subscribe_events`) and resources (`chain://tip`, `chain://block/{h}`, `chain://tx/{txid}`, `chain://contract/{addr}`). Any MCP-capable LLM (Claude, GPT, Gemini, open-weight via `mcp-cli`) reads on-chain state and submits transactions without a custom plugin. See [ADR-0029](docs/ADR/0029-mcp-server.md) and [`docs/spec/mcp_server.md`](docs/spec/mcp_server.md).
- **ACP (Agent Communication Protocol)** — a wire message family (`ACP_NEGOTIATE`, `ACP_TASK`, `ACP_SETTLE`, `ACP_REVOKE`) rides on top of the existing Noise-authenticated P2P layer so agents can negotiate tasks, quote and pay for services, and settle micropayments peer-to-peer — without a centralized marketplace. Escrow references on-chain txid; replay-protected by monotonic per-counterparty nonces. See [ADR-0030](docs/ADR/0030-acp-agent-communication-protocol.md) and [`docs/spec/acp_messages.md`](docs/spec/acp_messages.md).
- **Agent identity = wallet identity.** secp256k1 EOA (same curve as MetaMask/Ledger), HD derivation `m/44'/1234'/<agent_id>'`, key isolation (`OperatorKeystore` vs `AgentKeystore`), batched and sponsored txs via WASM contracts — no consensus change. See [ADR-0028](docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md) and [`docs/spec/agent_integration.md`](docs/spec/agent_integration.md).
- **Agent-native JSON-RPC** — `agent_*` namespace (`agent_getSession`, `agent_signAndSubmitBatch`, `agent_sponsorTx` Stage 5, `agent_estimateTaskCost`, `agent_getActivity`) alongside `eth_*` for Ethereum tooling interop.
- **Agent SDK + devnet** — Rust + TypeScript + Python SDK, one-command `cargo strangecoin agent devnet` with N pre-funded agent wallets, multi-agent ACP task-chain e2e test. See [`analytics/BACKLOG.md`](analytics/BACKLOG.md).
- **Professional UI** — the `eframe`/`egui` demo wallet is replaced by a production-grade UI (design system, 5 screens: wallet / node dashboard / agent activity / tx detail / settings, WCAG AA accessibility, dark/light themes, i18n en/ru/zh). See [ADR-0031](docs/ADR/0031-professional-ui-replacement.md) and [`docs/spec/ui_professional.md`](docs/spec/ui_professional.md).

The AI-agent / MCP / ACP / professional-UI work is **gated behind Stage 4** in the canonical roadmap, but the design prompts, ADRs and spec files already land alongside Stage 3 — see [`analytics/prompt-stage3.md`](analytics/prompt-stage3.md) §8 (extension prompts `AI-P01`…`AI-P05`, `UI-P01`…`UI-P05`).

What makes it interesting is **scope**: most educational blockchains stop at a toy in-memory chain with a CLI. Strangecoin goes further and ships, in a single Cargo workspace:

- a **PoW blockchain** with persistent LevelDB storage
- a **tokio-driven P2P layer** — Noise handshake, gossip, compact blocks, Erlay-style set reconciliation
- an **in-protocol WASM virtual machine** for smart contracts
- an **`eframe`/`egui` desktop wallet** (Stage 4: replaced by a [professional UI](docs/ADR/0031-professional-ui-replacement.md)) — click-to-send, no CLI required
- an **ADR + SCIP governance trail** — every non-trivial decision is recorded
- an **MCP server + ACP transport + `agent_*` JSON-RPC + agent SDK** (Stage 4) — the AI-agent-native surface

It is small enough to read in a weekend, and serious enough to study how production cryptocurrencies are actually structured.

### Why does this exist?

Most "learn blockchain" tutorials end at a 200-line `Block` struct and a `mine()` function. The moment you try to build something real, you hit a wall: *where does the network code live? how do peers authenticate? what about peer scoring? rate limiting? forks? reorgs? the mempool?* The toy tutorial has nothing to say.

Strangecoin is the answer to two questions:

> *What does a clean, auditable, end-to-end cryptocurrency look like when every layer — from storage to consensus to the GUI — is written in modern Rust, tested against property-based + integration tests, and documented through Architecture Decision Records?*

> *And what does it look like when that cryptocurrency is designed for AI agents — not as an afterthought, but as a first-class participant with an MCP-served state surface, ACP-routed peer-to-peer payments, and an agent-native JSON-RPC + SDK stack?*

### Goals

| Goal | How it shows up |
| --- | --- |
| **Correctness first** | Every consensus rule has a unit / `proptest` / integration test. `validate_chain()` is called after every mutation. |
| **Layered & replaceable** | `strangecoin-core`, `-net`, `-vm-wasm`, `-storage` are separate crates behind Cargo features. |
| **Testnet safety** | Genesis key is intentionally burned — nobody, including the author, can run a real-value mainnet. |
| **Docs as a first-class artifact** | ADRs, SCIPs, stage-by-stage definition-of-done, and a fuzz corpus live next to the code. |

**Non-goals:** competing with Bitcoin/Ethereum on hash rate, hosting a public mainnet with real value, or replacing any existing chain.

---

## Architecture

![Strangecoin layered architecture](assets/architecture.png)

**Pure core, impure shell.** `strangecoin-core` has zero I/O — no `tokio`, no `std::net`, no `std::fs`, enforced by CI source-gates — so the consensus rules can be reasoned about, fuzzed, and embedded without dragging a runtime or a database with them. The binary, the VM, the network, and storage are thin shells that pull from the core.

Every non-trivial decision is captured in an **ADR** (0001 through 0026 at last count); every protocol-level change is a **SCIP** that goes `Idea ? Draft ? Review ? Activation ? Finalized`.

```
???????????????????????????????????????????????????????????????????????
?                    strangecoin  (binary / GUI)                      ?
?       src/main.rs  ·  eframe wallet  +  tokio runtime  +  bus       ?
???????????????????????????????????????????????????????????????????????
               ?                                    ?
   ?????????????????????????             ?????????????????????????
   ?  strangecoin-core     ?             ?  strangecoin-net      ?
   ?  blockchain · PoW     ???? sync ???? ?  Noise · gossip       ?
   ?  mempool · wallet     ?             ?  compact blocks       ?
   ?????????????????????????             ?  erlay · peer score   ?
           ?                             ?????????????????????????
   ?????????????????????             ?????????????????????????????
   ? strangecoin-      ?             ?  strangecoin-storage      ?
   ? vm-wasm           ?             ?  StorageEngine trait     ?
   ? (smart contracts) ?             ?  (LevelDB backend)        ?
   ?????????????????????             ?????????????????????????????
```

### Module layout

| Module        | Responsibility                                                              |
| ------------- | --------------------------------------------------------------------------- |
| `blockchain`  | Block / header types, chain fork selection, `validate_chain()`              |
| `consensus`   | PoW difficulty, chain IDs, consensus versioning                              |
| `mempool`     | Pending transaction pool, RBF, double-spend rejection                       |
| `network`     | Tokio P2P runtime, sync loop, peer manager                                   |
| `storage`     | LevelDB persistence fa?ade                                                   |
| `gui`         | `eframe`/`egui` wallet screens                                               |
| `cli`         | Headless subcommands                                                         |
| `wallet`      | Key management, signing, bech32 addresses                                    |
| `economics`   | Emission schedule, grants                                                    |
| `governance`  | On-chain flags                                                               |
| `api`         | JSON-RPC surface for the GUI                                                 |
| `events`      | `NodeEvent` bus (crossbeam ? tokio)                                          |
| `config`      | `config.toml` parsing                                                        |

---

## Prerequisites

| Tool | Version | Notes |
| --- | --- | --- |
| **Rust toolchain** | stable, ? 1.74 | Install via [rustup](https://rustup.rs/). `edition = "2021"`. |
| **Cargo** | ships with Rust | Workspace uses `resolver = "2"`. |
| **C toolchain** (`cc`/`clang`) | any modern | Required to build `secp256k1`, `blake3` and (optional) `minisketch`. |
| **Git** | ? 2.30 | Required for the `.githooks/commit-msg` hook. |
| **cargo-fuzz** *(optional)* | latest | Only needed to run `fuzz/` targets. |

**First-class platforms:** Linux · macOS · Windows 10+
(On Windows the `winapi` features are declared explicitly in `Cargo.toml` — do not strip them or the GUI will not link.)

No external services, databases, or runtime daemons are required — everything is embedded.

---

## Installation

```bash
# 1. Clone the repo
git clone https://github.com/Reider85/strangecoin.git
cd strangecoin

# 2. (optional) enable the commit-msg hook — enforces [S#-P##] format
git config core.hooksPath .githooks

# 3. Build everything (default features: net + vm-wasm + gui)
cargo build --release

# 4. Or build a headless / light node (no GUI, no VM)
cargo build --release --no-default-features --features net
```

### Cargo features

| Feature   | Enables                                          | Default |
| --------- | ------------------------------------------------ | ------- |
| `net`     | `strangecoin-net` — P2P networking               | yes     |
| `vm-wasm` | `strangecoin-vm-wasm` — on-chain smart contracts | yes     |
| `gui`     | `eframe` + `winapi` — the egui desktop wallet   | yes     |

### Key dependencies

`tokio` (multi-thread runtime) · `secp256k1` · `blake3` · `aes-gcm` · `pbkdf2` · `eframe`/`egui 0.22` · `rusty-leveldb` · `tracing` + `tracing-subscriber` · `crossbeam-channel` · `serde`/`serde_json` · `toml` · `hex`

> **Windows build note.** `eframe 0.22` depends on `winapi` without declaring the features it needs (`winuser`, `commctrl`, `shellapi`, …). This repo declares them explicitly. Do not strip them.

---

## Quick Start

```bash
git clone https://github.com/Reider85/strangecoin.git && cd strangecoin
cargo build --release
cargo run --release -- run
```

Requires Rust 1.74+. Builds on Linux, macOS, and Windows. Default features include the P2P network, the WASM VM, and the GUI; for a headless node use `cargo run --release --no-default-features --features net,vm-wasm -- run`.

### Run the GUI wallet client

The default binary is the operator desktop client. It boots a tokio runtime, opens the P2P listener, and renders the wallet window.

```bash
cargo run --release --bin strangecoin -- --config config.toml
```

You should see:

1. An egui window titled **Strangecoin**.
2. A login screen — wallet address + password (regtest default: `password`).
3. The wallet view: your `rsc1…` address, current balance, recipient + amount fields, **Send** button.
4. The node simultaneously listens on `127.0.0.1:8081` and runs the sync loop in the background.

To quit: close the window or send `SIGINT`/`SIGTERM` (`Ctrl+C`) — the `Drop` impl cancels the sync task cleanly.

### Run two clients on ports 8081 / 8082

The classic local two-node demo. You need **two terminals** and **two config files**.

```bash
# Terminal 1 — node A on 8081 (uses the shipped config.toml)
cargo run --release -- --config config.toml

# Terminal 2 — node B on 8082
cp config.toml config.8082.toml
#  edit config.8082.toml:
#    [network]
#    listen_addr = "127.0.0.1:8082"
#    seeds       = ["127.0.0.1:8081"]
cargo run --release -- --config config.8082.toml
```

Node B bootstraps from node A, exchanges `version`/`verack` over the Noise handshake, then syncs headers and blocks. Watch the logs (`log_level = "info"`) for `peer connected`, `headers synced`, `block applied`.

### Run headless / light node

```bash
cargo run --release --no-default-features --features net -- \
    --config config.toml --no-gui
```

---

## Configuration

All runtime knobs live in [`config.toml`](config.toml). Highlights:

```toml
network_id = 3                # 3 = regtest
node_mode   = "Full"
log_level   = "info"
data_dir    = "./data"

[network]
listen_addr = "127.0.0.1:8081"   # change to :8082 for the second client
seeds       = []                  # add ["127.0.0.1:8081"] to bootstrap
max_peers   = 50

# S2-P17 (ADR-0022): peer-scoring weights & ban bands
peer_ban_threshold                      = 0.2    # temp ban (1 h) below this
peer_scoring_persistent_threshold       = 0.05   # perm ban (30 d) below this
peer_scoring_temp_ban_duration_sec      = 3600
peer_scoring_persistent_ban_duration_sec = 2592000

# S2-P16 (ADR-0023): token-bucket rate limiting
rate_limit_main_capacity = 1000
rate_limit_main_refill   = 100     # tokens / sec

[storage]
path = "./data/leveldb"
```

Genesis templates: [`genesis.json`](genesis.json) (mainnet template) and [`genesis-testnet.json`](genesis-testnet.json) (regtest). Both reference the **burned** genesis key.

---

## Features

### Chain & consensus

- **Hybrid PoW ? PoS Layer-1.** Sliding-window difficulty retarget every 2016 blocks (?4 / ?4 clamp), median-time-past window of 11 blocks, +2h future-timestamp ban.
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

### Desktop wallet (egui)

- **`eframe`/`egui` UI** — native, immediate-mode GUI.
- **Wallet view** — address, balance, recipient, amount, Send button.
- **Password gate** — regtest default `password`.
- **Two-node demo** — runs on `8081` and `8082` simultaneously.
- **Graceful shutdown** — `Drop` impls + `SIGTERM`/`SIGINT`.

### AI agents (Stage 3+ extensions / Stage 4 target)

Strangecoin is positioned as a **cryptocurrency for AI agents** — an L1 where autonomous agents are first-class economic actors, not bolted-on consumers of a human wallet.

- **Agent identity = wallet identity.** Each agent owns a `secp256k1` keypair (same curve as MetaMask/Ledger), with optional HD derivation (`m/44'/1234'/<agent_id>'`) for sub-agents and rotating session keys. See [ADR-0028](docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md).
- **MCP (Model Context Protocol) server.** The node exposes itself as an [MCP](https://modelcontextprotocol.io) server over stdio + HTTP+SSE transports, serving chain state as **tools** (`get_block`, `get_balance`, `get_nonce`, `submit_tx`, `call_contract`, `estimate_fee`, `subscribe_events`) and **resources** (`chain://tip`, `chain://block/{h}`, `chain://tx/{txid}`, `chain://contract/{addr}`). Any MCP-capable LLM (Claude, GPT, Gemini, open-weight via `mcp-cli`) can read on-chain state and submit transactions without a custom plugin. See [ADR-0029](docs/ADR/0029-mcp-server.md) and [`docs/spec/mcp_server.md`](docs/spec/mcp_server.md).
- **ACP (Agent Communication Protocol) messages.** A dedicated wire message family (`ACP_NEGOTIATE`, `ACP_TASK`, `ACP_SETTLE`, `ACP_REVOKE`) rides on top of the existing Noise-authenticated P2P layer so agents can negotiate tasks, quote and pay for services, and settle micropayments peer-to-peer — without a centralized marketplace. ACP messages reference on-chain txid for escrow. See [ADR-0030](docs/ADR/0030-acp-agent-communication-protocol.md) and [`docs/spec/acp_messages.md`](docs/spec/acp_messages.md).
- **Agent-native JSON-RPC.** The Stage 4 JSON-RPC surface ships `agent_*` namespaced methods: `agent_getSession`, `agent_signAndSubmitBatch`, `agent_sponsorTx` (paymaster hook, Stage 5), `agent_estimateTaskCost`, `agent_getActivity`. Compatible with `eth_*` methods (Ethereum tooling interop unchanged).
- **Agent-tx features.** Batched transactions (one signature, N calls), sponsored gas (paymaster), programmable policy wallets (multisig / session-key validators) — all delivered through Stage 1.5 WASM contracts, no consensus changes for the base layer.
- **Agent devnet + faucet.** Stage 4 ships `cargo-strangecoin agent devnet` — a one-command local agent network with a faucet that funds N agent wallets from a single seed, ready for multi-agent orchestration tests.
- **Professional UI.** The current `eframe`/`egui` demo wallet is replaced by a production-grade UI (component library, dark/light themes, accessibility, dashboard for node operators, agent activity feed, transaction detail with receipts and witness proofs). Migration governed by [ADR-0031](docs/ADR/0031-professional-ui-replacement.md). See [`docs/spec/ui_professional.md`](docs/spec/ui_professional.md).

> The AI-agent / MCP / ACP / professional-UI work is **gated behind Stage 4** in the canonical roadmap, but the design prompts, ADRs and spec files already land alongside Stage 3 (see [`analytics/prompt-stage3.md`](analytics/prompt-stage3.md) §8 — extension prompts `AI-P01`…`AI-P05`, `UI-P01`…`UI-P05`) so the design is settled before the implementation track opens.

### Developer experience

- **Event bus** — crossbeam ? tokio bridge fans `NodeEvent`s (block applied, peer connected, …) to sync + GUI consumers.
- **Fuzzing harness** — `cargo-fuzz` targets for `noise`, `gossip`, `compact_blocks`, `erlay`.
- **ADR-governed** — every protocol change is an Architecture Decision Record under `docs/ADR/`.
- **SCIP process** — Strangecoin Improvement Proposals under `docs/SCIP/`.
- **Stage-based roadmap** — numbered milestones with definition-of-done notes.

### Security as a first-class deliverable

- **STRIDE threat model** — 42 attack vectors (V-1 through V-42), each mapped to a mitigation, ADR, residual risk, and monitoring hook.
- **TLA+ consensus specification** — checked with TLC, **35,207 states at depth 6** across 10 safety invariants + liveness; `ReceiptsRootMatch` non-vacuous on 100k+ block instances.
- **22 ARCHITECT3 invariants** — the load-bearing rules the chain cannot violate (no double spend, no inflation, state root matches, consensus version enforced…) — regressed every stage.
- **9 `cargo-fuzz` targets ? 600s** on every CI push — canonical decode, host functions, gas metering, storage trie, Noise handshake, gossip INV, compact-block fill-up, Erlay reconciliation, plus a wasmi-vs-wasmtime differential target.
- **Reproducible release pipeline** — six-binary matrix (Linux/macOS/Windows ? x86_64/aarch64), SHA256 per artifact, **SLSA L3 provenance** + **cosign keyless OIDC** blob signatures, `RUSTFLAGS=--remap-path-prefix` for deterministic paths.
- **Bug bounty** — $1k / $10k / $100k tiers for low / medium / critical findings, 90-day coordinated disclosure, Immunefi integration path.

---

## Testing

Strangecoin ships with **~35 integration tests** in [`tests/`](tests/), plus unit tests inside each crate. They cover consensus, networking, the VM, reorgs, RBF, rate limiting, peer scoring, and the event bus.

### Run the `two_clients` test

The headline integration test is [`tests/two_clients.rs`](tests/two_clients.rs). It does **not** open two GUI windows — it spins up multiple `BlockchainFacade` instances in one process, grants an initial balance, then drives 100 signed transfers across 5 wallets while continuously syncing to the longest chain.

```bash
# Run only the two_clients test file
cargo test --release --test two_clients

# Run a single test function inside it
cargo test --release --test two_clients -- hundred_transactions_five_wallets --nocapture
```

**Expected output:**

```
running 3 tests
test two_clients::hundred_transactions_five_wallets           ... ok
test two_clients::bech32_address_transfer                     ... ok
test two_clients::node_runs_on_tokio_and_shuts_down_cleanly   ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

What each subtest verifies:

| Test | Asserts |
| --- | --- |
| `hundred_transactions_five_wallets` | 100 sequential transfers across 5 wallets settle to `[0,0,0,0,10000]`; all chains reach length 102 and pass `validate_chain()`. |
| `bech32_address_transfer` | Addresses use `rsc1` regtest HRP; a 1000-SC transfer lands and both chains are valid (length 3). |
| `node_runs_on_tokio_and_shuts_down_cleanly` | A `BlockApplied` event crosses the crossbeam?tokio bridge; the sync task observes the shutdown flag and `Node::drop` is clean. |

### Other test suites

```bash
# Everything
cargo test --release

# Consensus rules
cargo test --release --test pow --test double_spend --test reorg --test retarget --test rbf

# Network layer (Noise, gossip, compact blocks, Erlay, peer scoring, rate limit)
cargo test --release --test net_noise_handshake --test net_gossip \
                     --test net_compact_blocks --test net_erlay \
                     --test net_peer_scoring --test net_rate_limit_integration \
                     --test sync_engine --test sync_headers

# VM
cargo test --release --test vm_deploy_call --test vm_gas_exhaustion \
                     --test vm_reentrancy_attack --test vm_transfer_to_contract \
                     --test vm_precompiles_integration

# Property-based (proptest)
cargo test --release --test chain_selector_proptest
```

### Fuzzing

The `fuzz/` directory contains `cargo-fuzz` targets for the network surface:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run noise_handshake -- -max_total_time=60
cargo +nightly fuzz run gossip
cargo +nightly fuzz run compact_blocks
cargo +nightly fuzz run erlay
```

---

## Screenshots

> The GUI is an `eframe`/`egui` desktop window. To enable the screenshots below, capture them with your platform's screenshot tool, save as PNG under `docs/screenshots/`, and add image links as shown.
>
> What each screen contains:
> - **Login** — wallet address input + password field (regtest default `password`).
> - **Wallet view** — `rsc1…` address (read-only), confirmed balance, recipient + amount inputs, **Send** button. Balance refreshes on every `BlockApplied` event.
> - **Two clients** — follow the [Quick Start ? two clients](#run-two-clients-on-ports-8081--8082) recipe and capture both windows side by side.

Screenshot links to add once images exist:

```
![Login](docs/screenshots/login.png)
![Wallet](docs/screenshots/wallet.png)
![Two clients](docs/screenshots/two_clients.png)
```

---

## Project Structure

```
strangecoin/
??? src/                      # top-level crate (binary + GUI)
?   ??? api/                  # JSON-RPC surface for the GUI
?   ??? blockchain/           # blocks, headers, fork selection
?   ??? cli/                  # headless subcommands
?   ??? consensus/            # PoW, chain IDs, versioning
?   ??? economics/            # emission & grants
?   ??? governance/           # on-chain flags
?   ??? gui/                  # eframe / egui screens
?   ??? mempool/              # tx pool, RBF, double-spend
?   ??? network/              # tokio P2P runtime
?   ??? storage/              # LevelDB fa?ade
?   ??? address.rs            # bech32 addresses (rsc1…)
?   ??? config.rs             # config.toml parsing
?   ??? events.rs             # NodeEvent bus
?   ??? main.rs               # entry point
?   ??? wallet.rs             # keys, signing
??? crates/
?   ??? strangecoin-core/     # consensus / chain / mempool library
?   ??? strangecoin-net/      # Noise, gossip, compact blocks, Erlay
?   ??? strangecoin-vm-wasm/  # on-chain WASM VM
?   ??? strangecoin-storage/  # StorageEngine trait + LevelDB backend
??? tests/                    # ~35 integration & property tests
??? fuzz/                     # cargo-fuzz targets
??? docs/
?   ??? ADR/                  # Architecture Decision Records (0001..0027 + 0028 AI agents / 0029 MCP / 0030 ACP / 0031 Professional UI)
?   ??? SCIP/                 # Strangecoin Improvement Proposals
?   ??? spec/                 # protocol spec + agent_integration / mcp_server / acp_messages / ui_professional
?   ??? security/             # threat models
?   ??? stage0..stage2/       # stage-by-stage definition-of-done
??? scripts/
?   ??? audit/                # audit helpers
?   ??? dev/                  # dev tooling
??? vendor/minisketch-rs/     # vendored copy (see Cargo.toml note)
??? .github/workflows/        # CI
??? .githooks/                # commit-msg hook ([S#-P##] format)
??? config.toml               # node config
??? genesis.json              # mainnet genesis template
??? genesis-testnet.json      # regtest genesis
??? ECONOMICS.md              # emission schedule
??? Changelog.md              # release history
??? Description.md            # design notes
??? AGENTS.md                 # contributor agent notes
??? analytics/BACKLOG.md      # cross-stage backlog (incl. AI agent / MCP / ACP / professional UI work items)
??? LICENSE                   # Apache-2.0
```

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
| **4** — Developer experience + AI agents | PLANNED | SDK, local devnet with faucet, indexer, JSON-RPC `eth_*` + `agent_*` surface, **MCP server** ([ADR-0029](docs/ADR/0029-mcp-server.md)), **ACP messages** ([ADR-0030](docs/ADR/0030-acp-agent-communication-protocol.md)), **professional UI** replacing `egui` demo ([ADR-0031](docs/ADR/0031-professional-ui-replacement.md)) |
| **5** — Fee market & AA | PLANNED | EIP-1559-style fee market on the Stage 1 RBF groundwork, account abstraction, SCIP-001 dev fund |
| **6** — External audit & formal verification | PLANNED | Third-party security audit, TLA+ network spec, formal verification of consensus rules |
| **7** — Proof of Stake | PLANNED | BLS12-381 validator keys, slashing, 1,000 SC minimum stake, 5%-of-stake cap per validator, PoW?PoS migration gated by `activation_height` |

**Next milestones:**

- **SCIP-0001** — replace the burned genesis key with an offline-generated one. **Blocks mainnet.**
- **Stage 3** — land `strangecoin-storage` and migrate the LevelDB fa?ade onto the `StorageEngine` trait.
- **Stage 4** — light-node / SPV build profiles and the first non-regtest network ID.

See [`docs/stage2/`](docs/stage2/) for the stage-2 definition-of-done, and [`Changelog.md`](Changelog.md) for the release history.

---

## Contributing

Pull requests are welcome. The bar is **"matches an existing ADR or proposes a new one, passes `cargo clippy -D warnings` and `cargo fmt --check`, and doesn't regress the 22 invariants."**

Contributions are welcome — but please read the conventions first, they are non-trivial.

1. **Commit format.** Every commit message must match `[S<stage>-P<prompt>] <area>: <summary>`, enforced by `.githooks/commit-msg`. Enable once: `git config core.hooksPath .githooks`.
2. **Style.** `cargo fmt` is run repo-wide; CI rejects drift. Before pushing: `cargo fmt --all`.
3. **Checks.** `cargo clippy --all-targets -- -D warnings` must pass, as must the full `cargo test --release` suite.
4. **ADRs.** Any non-trivial design decision gets an ADR under `docs/ADR/` (next free number) **before** the code lands. Protocol changes additionally get a SCIP under `docs/SCIP/`.
5. **Tests.** New consensus / network / VM behaviour must come with an integration test under `tests/`. Fuzz the network surface if you touch a parser.
6. **Pull requests.** Open a PR against `master`, reference the stage/prompt ID in the title, and link any ADR/SCIP in the description.

```bash
# Checks before you push
cargo fmt -- --check
cargo clippy --all -- -D warnings
cargo test --all
```

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full workflow — environment setup, commit conventions, determinism rules, branching, releases.

See [`AGENTS.md`](AGENTS.md) for the contributor agent notes that the maintainer uses during development.

**Found a security vulnerability, fuzz panic, or wasmi-vs-wasmtime divergence?** Do not open a public issue. Follow [`docs/security/SECURITY.md`](docs/security/SECURITY.md): email `security@strangecoin.io`, 48h SLA, 90-day coordinated disclosure, paid bounty per [`docs/security/BOUNTY.md`](docs/security/BOUNTY.md) ($1k / $10k / $100k tiers).

---

## Status & disclaimer

Strangecoin is **testnet-only.** The genesis allocation key was historically derived from a public seed string and is therefore **burned** — anyone with the seed (or git history) controls the 1e9 SC allocation. It is the sole thing [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md) exists to fix. The air-gapped key-generation tooling ([`examples/generate_offline_genesis_key.rs`](examples/generate_offline_genesis_key.rs), [`examples/print_genesis_hashes.rs`](examples/print_genesis_hashes.rs)) is already in the tree; what remains is the human ceremony of generating the new key offline, rebuilding genesis, and pinning the new hash.

**Do not send real value to any Strangecoin address.** This is a software-engineering project, not financial advice, not a token sale, and not an invitation to invest. Nothing here is a security, and nobody representing the project will ever ask you to send money to participate.

---

## License

Licensed under the **Apache License, Version 2.0**. See [`LICENSE`](LICENSE) for the full text.

```
SPDX-License-Identifier: Apache-2.0
```

The `license` field in [`Cargo.toml`](Cargo.toml) declares `Apache-2.0` for crates.io compatibility.

> The Apache-2.0 license covers the **source code**. It does **not** grant any right to operate a mainnet — the genesis key is burned (see the warning at the top), and mainnet is gated behind [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md).

---

## Acknowledgements

Strangecoin stands on the shoulders of many open-source projects and protocol designs:

- **[Rust](https://www.rust-lang.org/)** & the **[tokio](https://tokio.rs)** async runtime
- **[egui / eframe](https://github.com/emilk/egui)** — the immediate-mode GUI driving the wallet client
- **[rusty-leveldb](https://crates.io/crates/rusty-leveldb)** — pure-Rust LevelDB
- **secp256k1**, **blake3**, **aes-gcm**, **pbkdf2** — the cryptographic primitives
- **[minisketch](https://github.com/sipa/minisketch)** (Erlay set reconciliation) — vendored copy in `vendor/minisketch-rs/`, see the provenance note in `Cargo.toml`
- Bitcoin's P2P protocol design and BIP-style improvement proposals (the SCIP format is inspired by BIPs)
- The **Erlay** paper (Maxwell, Rielland, et al.) for compact-block & set-reconciliation ideas

Protocol design decisions are recorded as ADRs under [`docs/ADR/`](docs/ADR) and as SCIPs under [`docs/SCIP/`](docs/SCIP) — read them for the rationale behind every non-obvious choice.

---

*Built with Rust, blake3, secp256k1, wasmi, Noise, MCP, ACP, and a refusal to ship "almost."*

**Star it · Fork it · Try to break it · Point your agents at it.**

If Strangecoin helped you learn or build, please [star the repo](https://github.com/Reider85/strangecoin/stargazers) — it really helps.

Made with care in Rust.
