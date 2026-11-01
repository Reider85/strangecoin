# Strangecoin

**A from-scratch, Rust-native cryptocurrency — blockchain, P2P network, WASM VM and an `egui` desktop wallet, all in one Cargo workspace.**

[![CI](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml/badge.svg)](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Rust Edition](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Release](https://img.shields.io/badge/release-v2.0.0--stage2-yellow.svg)](https://github.com/Reider85/strangecoin/releases)
[![Lines of Rust](https://img.shields.io/badge/rust-99.3%25-lightgrey.svg)](https://github.com/Reider85/strangecoin/search?l=rust)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-green.svg)](#prerequisites)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](#contributing)
[![Stars](https://img.shields.io/github/stars/Reider85/strangecoin?style=social)](https://github.com/Reider85/strangecoin/stargazers)

Navigation: [Introduction](#introduction) · [Quick Start](#quick-start) · [Testing](#testing) · [Architecture](#architecture) · [Roadmap](#roadmap) · [Contributing](#contributing)

---

> Warning: **TESTNET-ONLY — BURNED GENESIS KEY** (`BUG-S0-015` / `SCIP-0001`)
>
> Strangecoin is currently **testnet-only**. The genesis allocation (1e9 SC) was historically controlled by a key derived from a **public** seed string — that key is **burned**. Anyone knowing the seed (or reading old git history) controls the genesis allocation.
>
> **Do not send real value.** Mainnet is impossible until [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md) replaces the genesis key with an offline-generated one.

---

## Introduction

### What is Strangecoin?

Strangecoin is a **full cryptocurrency written from scratch in Rust**. It is not a fork of any existing coin — the address format (`rsc1...` bech32), chain IDs, genesis block, consensus parameters, network protocol and improvement process are all native to this project.

What makes it interesting is **scope**: most educational blockchains stop at a toy in-memory chain with a CLI. Strangecoin goes further and ships, in a single Cargo workspace:

- a **PoW blockchain** with persistent LevelDB storage
- a **tokio-driven P2P layer** — Noise handshake, gossip, compact blocks, Erlay-style set reconciliation
- an **in-protocol WASM virtual machine** for smart contracts
- an **`eframe`/`egui` desktop wallet** — click-to-send, no CLI required
- an **ADR + SCIP governance trail** — every non-trivial decision is recorded

It is small enough to read in a weekend, and serious enough to study how production cryptocurrencies are actually structured.

### Why does this exist?

Most "learn blockchain" tutorials end at a 200-line `Block` struct and a `mine()` function. The moment you try to build something real, you hit a wall: *where does the network code live? how do peers authenticate? what about peer scoring? rate limiting? forks? reorgs? the mempool?* The toy tutorial has nothing to say.

Strangecoin is the answer to one question:

> *What does a clean, auditable, end-to-end cryptocurrency look like when every layer — from storage to consensus to the GUI — is written in modern Rust, tested against property-based + integration tests, and documented through Architecture Decision Records?*

### Goals

| Goal | How it shows up |
| --- | --- |
| **Correctness first** | Every consensus rule has a unit / `proptest` / integration test. `validate_chain()` is called after every mutation. |
| **Layered & replaceable** | `strangecoin-core`, `-net`, `-vm-wasm`, `-storage` are separate crates behind Cargo features. |
| **Testnet safety** | Genesis key is intentionally burned — nobody, including the author, can run a real-value mainnet. |
| **Docs as a first-class artifact** | ADRs, SCIPs, stage-by-stage definition-of-done, and a fuzz corpus live next to the code. |

**Non-goals:** competing with Bitcoin/Ethereum on hash rate, hosting a public mainnet with real value, or replacing any existing chain.

---

## Features

### Blockchain & Consensus
- **Proof-of-Work** — hash must start with the required zero bits (regtest difficulty = 2)
- **Persistent storage** — LevelDB (`rusty-leveldb`), survives restarts
- **Balance-checked txs** — insufficient funds are rejected before block inclusion
- **Bech32 addresses** — native `rsc1...` on regtest (`network_id = 3`)
- **secp256k1 + ECDSA** signatures
- **Chain validation** — `validate_chain()` runs after every block

### P2P Networking
- **Noise-encrypted handshakes** — authenticated peer-to-peer
- **Gossip protocol** — transaction & block propagation
- **Compact blocks** — BIP-152-style relay
- **Erlay set reconciliation** — `minisketch`-based
- **Peer scoring** — latency / invalid / violation weights -> temp & persistent bans
- **Token-bucket rate limiting** — per-message-type sub-buckets

### Smart Contracts
- **In-protocol WASM VM** — deploy & call on-chain
- **Gas metering** — `vm_gas_exhaustion` test
- **Reentrancy protection** — `vm_reentrancy_attack` test
- **Precompiles** — `vm_precompiles_integration` test
- **Contract-to-contract calls** — `vm_transfer_to_contract`

### Desktop Wallet (egui)
- **`eframe`/`egui` UI** — native, immediate-mode GUI
- **Wallet view** — address, balance, recipient, amount, Send button
- **Password gate** — regtest default `password`
- **Two-node demo** — runs on `8081` and `8082` simultaneously
- **Graceful shutdown** — `Drop` impls + `SIGTERM`/`SIGINT`

### Developer Experience
- **Event bus** — crossbeam -> tokio bridge fans `NodeEvent`s (block applied, peer connected, ...) to sync + GUI consumers
- **Fuzzing harness** — `cargo-fuzz` targets for `noise`, `gossip`, `compact_blocks`, `erlay`
- **ADR-governed** — every protocol change is an Architecture Decision Record under `docs/ADR/`
- **SCIP process** — Strangecoin Improvement Proposals under `docs/SCIP/`
- **Stage-based roadmap** — numbered milestones with definition-of-done notes

---

## Architecture

```
??????????????????????????????????????????????????????????????????????
?                    strangecoin  (binary / GUI)                      ?
?       src/main.rs  ->  eframe wallet  +  tokio runtime  +  bus      ?
??????????????????????????????????????????????????????????????????????
               ?                                    ?
   ????????????????????????             ????????????????????????
   ?  strangecoin-core    ?             ?  strangecoin-net     ?
   ?  blockchain · PoW    ????? sync ????  Noise · gossip      ?
   ?  mempool · wallet    ?             ?  compact blocks      ?
   ????????????????????????             ?  erlay · peer score  ?
           ?                            ????????????????????????
   ????????????????????             ????????????????????????????
   ? strangecoin-     ?             ?  strangecoin-storage     ?
   ? vm-wasm          ?             ?  StorageEngine trait    ?
   ? (smart contracts)?             ?  (LevelDB backend)      ?
   ????????????????????             ????????????????????????????
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
| `events`      | `NodeEvent` bus (crossbeam <-> tokio)                                          |
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

| Feature   | Enables                                       | Default |
| --------- | --------------------------------------------- | ------- |
| `net`     | `strangecoin-net` — P2P networking            | yes |
| `vm-wasm` | `strangecoin-vm-wasm` — on-chain smart contracts | yes |
| `gui`     | `eframe` + `winapi` — the egui desktop wallet | yes |

### Key dependencies

`tokio` (multi-thread runtime) · `secp256k1` · `blake3` · `aes-gcm` · `pbkdf2` · `eframe`/`egui 0.22` · `rusty-leveldb` · `tracing` + `tracing-subscriber` · `crossbeam-channel` · `serde`/`serde_json` · `toml` · `hex`

> **Windows build note.** `eframe 0.22` depends on `winapi` without declaring the features it needs (`winuser`, `commctrl`, `shellapi`, ...). This repo declares them explicitly. Do not strip them.

---

## Quick Start

### Run the GUI wallet client

The default binary is the operator desktop client. It boots a tokio runtime, opens the P2P listener, and renders the wallet window.

```bash
cargo run --release --bin strangecoin -- --config config.toml
```

You should see:

1. An egui window titled **Strangecoin**.
2. A login screen — wallet address + password (regtest default: `password`).
3. The wallet view: your `rsc1...` address, current balance, recipient + amount fields, **Send** button.
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
listen_addr = "127.0.0.1:8081"   # <- change to :8082 for the second client
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
| `node_runs_on_tokio_and_shuts_down_cleanly` | A `BlockApplied` event crosses the crossbeam->tokio bridge; the sync task observes the shutdown flag and `Node::drop` is clean. |

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

The GUI is an `eframe`/`egui` desktop window. Drop PNG captures under `docs/screenshots/` and the images below will render automatically.

What each screen contains:
- **Login** — wallet address input + password field (regtest default `password`).
- **Wallet view** — `rsc1...` address (read-only), confirmed balance, recipient + amount inputs, **Send** button. Balance refreshes on every `BlockApplied` event.
- **Two clients** — follow the [Quick Start -> two clients](#run-two-clients-on-ports-8081--8082) recipe and capture both windows side by side.

### Login screen

![Login screen](docs/screenshots/login.png)

### Wallet view (address, balance, send form)

![Wallet view](docs/screenshots/wallet.png)

### Two clients running side by side (8081 / 8082)

![Two clients side by side](docs/screenshots/two_clients.png)

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
?   ??? address.rs            # bech32 addresses (rsc1...)
?   ??? config.rs             # config.toml parsing
?   ??? events.rs             # NodeEvent bus
?   ??? main.rs               # entry point
?   ??? wallet.rs             # keys, signing
??? crates/
?   ??? strangecoin-core/     # consensus / chain / mempool library
?   ??? strangecoin-net/     # Noise, gossip, compact blocks, Erlay
?   ??? strangecoin-vm-wasm/ # on-chain WASM VM
?   ??? strangecoin-storage/ # StorageEngine trait + LevelDB backend
??? tests/                    # ~35 integration & property tests
??? fuzz/                     # cargo-fuzz targets
??? docs/
?   ??? ADR/                  # Architecture Decision Records
?   ??? SCIP/                 # Strangecoin Improvement Proposals
?   ??? spec/                 # protocol spec
?   ??? security/             # threat models
?   ??? stage0..stage2/       # stage-by-stage definition-of-done
??? scripts/
?   ??? audit/                # audit helpers
?   ??? dev/                  # dev tooling
??? vendor/minisketch-rs/    # vendored copy (see Cargo.toml note)
??? .github/workflows/        # CI
??? .githooks/                # commit-msg hook ([S#-P##] format)
??? config.toml               # node config
??? genesis.json              # mainnet genesis template
??? genesis-testnet.json      # regtest genesis
??? ECONOMICS.md              # emission schedule
??? Changelog.md              # release history
??? Description.md            # design notes
??? AGENTS.md                 # contributor agent notes
??? LICENSE                   # Apache-2.0
```

---

## Roadmap

Strangecoin is developed in numbered stages. Current release line: **`2.0.0-stage2`**.

| Stage | Theme | Status |
| --- | --- | --- |
| **0** | Repo setup, LICENSE, ADR-0004/0005, genesis seed | done |
| **1** | Core chain, PoW, wallet, mempool, bech32 | done |
| **1.5** | Receipts root, fork-choice hardening, SCIP-0001 | done |
| **2** | P2P: Noise, gossip, compact blocks, Erlay, peer scoring, rate limit, fuzz | done — tagged `v2.0.0-stage2` |
| **3** | Storage crate, `StorageEngine` trait, schema (ADR-0024) | in progress |
| **4+** | Light-node `--no-default-features` builds, mainnet readiness (post-SCIP-0001) | planned |

**Next milestones:**

- **SCIP-0001** — replace the burned genesis key with an offline-generated one. **Blocks mainnet.**
- **Stage 3** — land `strangecoin-storage` and migrate the LevelDB fa?ade onto the `StorageEngine` trait.
- **Stage 4** — light-node / SPV build profiles and the first non-regtest network ID.

See [`docs/stage2/`](docs/stage2/) for the stage-2 definition-of-done, and [`Changelog.md`](Changelog.md) for the release history.

---

## Contributing

Contributions are welcome — but please read the conventions first, they are non-trivial.

1. **Commit format.** Every commit message must match `[S<stage>-P<prompt>] <area>: <summary>`, enforced by `.githooks/commit-msg`. Enable once: `git config core.hooksPath .githooks`.
2. **Style.** `cargo fmt` is run repo-wide; CI rejects drift. Before pushing: `cargo fmt --all`.
3. **Checks.** `cargo clippy --all-targets -- -D warnings` must pass, as must the full `cargo test --release` suite.
4. **ADRs.** Any non-trivial design decision gets an ADR under `docs/ADR/` (next free number) **before** the code lands. Protocol changes additionally get a SCIP under `docs/SCIP/`.
5. **Tests.** New consensus / network / VM behaviour must come with an integration test under `tests/`. Fuzz the network surface if you touch a parser.
6. **Pull requests.** Open a PR against `master`, reference the stage/prompt ID in the title, and link any ADR/SCIP in the description.

See [`AGENTS.md`](AGENTS.md) for the contributor agent notes that the maintainer uses during development.

---

## License

Licensed under the **Apache License, Version 2.0** — see [`LICENSE`](LICENSE) and <https://www.apache.org/licenses/LICENSE-2.0>.

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

If Strangecoin helped you learn or build, please [star the repo](https://github.com/Reider85/strangecoin/stargazers).
