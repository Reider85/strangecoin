# Strangecoin

> A production-grade Layer-1 blockchain written from scratch in Rust -- PoW today, PoS-bound, with WASM smart contracts, a Noise-encrypted P2P layer, and an emission curve that doesn't end.

[![CI](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml/badge.svg)](https://github.com/Reider85/strangecoin/actions/workflows/ci.yml)
[![Release](https://github.com/Reider85/strangecoin/actions/workflows/release.yml/badge.svg)](https://github.com/Reider85/strangecoin/actions/workflows/release.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.74%2B-orange.svg)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-v2.0.0--stage2-success)](Changelog.md)

**Strangecoin** is a full-stack L1: a hand-rolled consensus engine, a deterministic state machine backed by a Sparse Merkle Trie, a `wasmi`-powered smart-contract runtime, and a real Noise-Protocol P2P network with BIP-152 compact blocks and Erlay reconciliation. It is not a demo, not a fork of another chain, and not a wrapper around an existing framework. Every consensus rule, every signature, every handshake was written, fuzzed, and formally specified in-house.

The chain is **testnet-only today, one gate away from mainnet.** That gate is [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md): replacing the burned genesis key with an offline, air-gapped one. The tooling for that swap already exists and is documented; the human/ops step is what remains. Everything else -- consensus, state, VM, network, release pipeline -- is shipped and green.

If you're tired of "blockchain in 500 lines" tutorials and want to read a codebase that takes itself seriously, you're in the right place. **Star it, fork it, try to break it.**

---

## Why Strangecoin

Most Rust chain projects stop at "blocks and balances." Strangecoin kept going. The four development stages shipped so far -- **Stage 0, Stage 1, Stage 1.5, Stage 2** -- each closed a specific, named production gap, and each one ended with a tagged release, a green CI matrix, and a threat-model update. The result is a chain that already behaves like a mainnet coin should: it has replay protection, deterministic genesis, a real difficulty retarget, a real mempool, real peer scoring, real cryptographic receipts, and a release pipeline that signs binaries with SLSA L3 provenance and cosign keyless OIDC.

Strangecoin is also honest about what it isn't. It isn't mainnet. It isn't asking you to send real value. It is a rigorously engineered testnet whose job is to be ready the moment the genesis key is rotated -- and to be interesting enough along the way that you want to build on it, audit it, or just learn from it.

---

## The chain in one breath

Strangecoin (**SC**, 8 decimals) is a hybrid-PoW to PoS Layer-1 with a **21,000,000 SC pre-tail cap** and a **0.6% per year perpetual tail emission** modeled on Monero. The initial block reward is **50 SC**, halving every **210,000 blocks** (~4 years) Bitcoin-style via 64-bit right-shift, with the tail kicking in once halving would otherwise starve the network. The result is a supply curve that stays deflationary for the first ~32 years and then settles into a predictable, perpetual ~126,000 SC/year -- enough to keep miners (and later validators) paid forever, without runaway inflation. Block time is 10 minutes. Block gas limit is 1,000,000.

Accounts are **secp256k1 ECDSA with recoverable signatures** -- the same curve MetaMask, Ledger, and WalletConnect already speak -- paired with **bech32 addresses** (`sc1...` on mainnet, `tsc1...` on testnet, `rsc1...` on regtest). Keys live in a keystore encrypted with **PBKDF2 + AES-256-GCM**. The consensus hash is **blake3**; transaction IDs are a `blake3` commitment over the canonical signed serialization, so a txid is the same bytes in the mempool, in a block, and in a receipt.

---

## What landed across the four shipped stages

Strangecoin was built in four deliberate stages. Each one is a tagged release (`v1.0.0-stage0`, `v1.1.0-stage1`, `v1.5.0-stage1.5`, `v2.0.0-stage2`) with a full changelog and a definition-of-done checklist. The prose below is what those stages actually delivered -- not a backlog, the code on `main`.

**Stage 0** was about turning an unsafe prototype into a chain that obeys its own rules. It moved wallets to secp256k1, introduced canonical binary serialization so every consensus object has exactly one encoding, and made `txid` a real commitment over the signed bytes -- which in turn made replay protection, mempool dedup, and rollback all share one identity. It closed the obvious critical bugs: there is now a real **Proof-of-Work target with sliding-window difficulty retargeting every 2016 blocks** (with the standard x4 / /4 clamp), a **median-time-past window of 11 blocks** plus a +2h future-timestamp ban to shut down time-warp attacks, and a **deterministic genesis pinned by an `EXPECTED_GENESIS_HASH` constant** that a node refuses to start without. Stage 0 also introduced the **tail-emission reward curve**, the size limits that make the wire safe (32 MB max message, 4 MB max block, 256 KB max tx, all enforced *before* allocation to kill OOM vectors), per-peer rate limiting, structured `tracing` logs, scoped `RwLock` concurrency, graceful shutdown, and the **STRIDE threat model** and **TLA+ consensus specification** that every later stage extends. It also shipped the **reproducible release pipeline** -- six-binary matrix across Linux/macOS/Windows x x86_64/aarch64, SHA256 per artifact, SLSA L3 provenance, cosign keyless OIDC blob signing -- that has been running green on every tag since.

**Stage 1** was about architecture. The codebase was decomposed into a real **Cargo workspace** with a pure `crates/strangecoin-core` that has zero I/O -- no tokio, no `std::net`, no `std::fs`, enforced by CI audits -- so the consensus rules can be reasoned about, fuzzed, and embedded without dragging a runtime with them. The monolithic `Blockchain` was split into five focused components: a `chain_selector` that does deterministic tip selection by cumulative work with a clean tie-break (more work, then earlier timestamp, then smaller hash, no races), a `block_executor` that separates validation from application, a `state_cache` that is the single read path for balances (closing the direct-mutation bug class with a CI source-gate), a `blockchain_facade` as the single entry point, and a `consensus_manager` that owns which rules apply at which height. A **Sparse Merkle Tree of depth 256** became the state commitment, with `block.state_root` enforced as an invariant -- a block that doesn't reproduce the state root doesn't validate. A **Merkle root of transactions (`tx_root`)** landed in the header for SPV readiness, and a **`StateWitness` API** opened the door to stateless validation and light clients. Stage 1 also introduced **bech32 addresses**, an **`EventBus`** that lets the GUI, metrics, and tests react to consensus events deterministically instead of polling, **headers-first sync** (`GET_HEADERS -> HEADERS -> GET_BLOCKS -> BLOCKS`), **Replace-By-Fee in the mempool** as the first fee-market seed, the **`SyncEngine`** as the single owner of incoming blocks (breaking the network to consensus cycle), and the **SCIP governance container** -- `consensus_version` plus `activation_height` -- that lets the chain upgrade by hard fork on a schedule rather than by surprise.

**Stage 1.5** brought smart contracts online. A dedicated `crates/strangecoin-vm-wasm` wraps **`wasmi` 2.0** (exact-pinned, `no_std`, deterministic, fuzz-ready) and exposes a real account model with **EOAs and contract accounts**, three transaction types (**`transfer`, `create`, `call`**), a **JSON ABI in the Ethereum style** with a four-byte `blake3` method selector, and a **gas metering layer** driven by wasmi's `fuel` feature plus weighted host-call costs. Thirteen host functions cover the realistic surface a contract needs -- `secp256k1_verify`, `blake3`, `keccak`, `sha256`, `storage_read`, `storage_write`, `call`, `delegatecall`, `staticcall`, `block_context`, `gas_left`, `emit_log`, and a stubbed `bls_verify` reserved for the later PoS stage -- and every one of them is deterministic, gas-metered, and input-validated. Seven precompiles live at addresses `0x01..0x07`: `ecrecover`, `sha256`, `keccak` (Ethereum variant), `blake3`, `ed25519_verify`, `bls_aggregate` (stub), and `modexp`. Reentrancy is guarded, with a `nonReentrant` pattern and hard resource limits -- **24 KB max code size, 1 MB max storage per contract, 1024 max call depth, 1 MB max memory** -- so a contract cannot eat the chain. Contract storage is integrated into the **shared Sparse Merkle Trie**, which means the global `state_root` already commits to contract state, and reorgs roll back contract writes through an overlay. Every transaction produces an **immutable `Receipt`** with gas used, status, logs, and a value-transfer audit trail, and `block.receipts_root` commits to them all -- Ethereum-style, separately from `tx_root`, so a light client can prove a transaction's outcome without trusting anyone.

**Stage 2** is the production network layer. A new `crates/strangecoin-net` carries every byte between peers over the **Noise Protocol Framework** -- the **XX handshake pattern** via `snow`, **ChaCha20-Poly1305** transport encryption with monotonic nonces, anti-replay, and rekey, **Ed25519 `node_id` authentication** so a peer's identity is a signed commitment rather than an IP address, and **HKDF-SHA256-derived deterministic ephemeral keys** so handshakes are reproducible and fuzzable. On top of that encrypted transport sits a real **HELLO + capabilities negotiation** (`MIN_PROTO_VERSION = 2`, network-id check, capability bitfield), multi-source **peer discovery** (hard-coded seed IPs, **DNSSEC-validated DNS seeds** via `hickory-resolver`, and `ADDR` gossip), a **peer store / manager / pool split** with a ban state machine and a per-IP-range Sybil cap, Bitcoin-style **`INV` / `GET_DATA` / `TX` / `BLOCK` / `NOTFOUND` gossip**, **BIP-152 compact blocks** using siphash-2-4 short transaction IDs with mempool fill-up and `GET_BLOCK_TXN` / `BLOCK_TXN` round-tripping for **>=50% bandwidth savings** on block broadcast, and **Erlay reconciliation** with mincut sketches and INV fallback targeting **>=80% bandwidth reduction** at 10k+ nodes (the full `minisketch-rs` is vendored and SCIP-gated for a future flip-on). The network defends itself with **token-bucket rate limiting per peer plus per-message-type sub-buckets**, a **four-factor weighted peer-scoring algorithm** (latency, invalid messages, violations, uptime, with EWMA-smoothed latency and a warn to temp-ban to persistent-ban ladder), and a clean ban-expiry policy. Stage 2 also retired the legacy v0.8.6 text wire protocol entirely, completed the async migration to `tokio` (no thread fallbacks left in the production path), and shipped **shadow network tooling** (`sc shadow replay`) that replays mainnet blocks onto staging to catch consensus divergence before it hits production. The release run for `v2.0.0-stage2` was 13/13 jobs green, with 26 signed release assets attached to the tag.

---

## Security as a first-class deliverable

Strangecoin treats security as something the repository produces, not something it promises. The **STRIDE threat model** lives in `docs/security/THREAT_MODEL.md` and tracks 42 attack vectors (V-1 through V-42), each one mapped to a mitigation prompt, an ADR, a residual risk note, and a monitoring hook -- Eclipse, Partition, network MITM, spam, invalid-block flooding, sybil, reentrancy, gas exhaustion, call-depth attacks, OOM, time-warp, and the rest. The **TLA+ consensus specification** in `docs/spec/consensus.tla` is checked with TLC and currently passes 35,207 states at depth 6 across ten safety invariants plus liveness, with `ReceiptsRootMatch` non-vacuous on over 100,000 block instances. Twenty-two **ARCHITECT3 invariants** -- the load-bearing rules the chain cannot violate, from "no double spend" and "no inflation" to "state root matches" and "consensus version enforced" -- are regressed every stage and documented in `docs/stage2/INVARIANTS_ENFORCED.md`.

The build is verifiable. Every release tag ships **SLSA L3 provenance** via the `slsa-framework/slsa-github-generator` and **cosign keyless OIDC blob signatures** on every binary, with an aggregated `SHA256SUMS.txt` and `RUSTFLAGS=--remap-path-prefix` for deterministic paths, so anyone can confirm a binary they downloaded was actually built from the tagged source. The **bug-bounty program** (`docs/security/BOUNTY.md`) is three-tiered -- $1k / $10k / $100k for low / medium / critical findings -- with 90-day coordinated disclosure and an Immunefi integration path. Nine **`cargo-fuzz` targets** run for 600 seconds each on every CI push, covering canonical decoding, host functions, gas metering, the storage trie, the Noise handshake, gossip INV, compact-block fill-up, and Erlay reconciliation; a **differential-testing target** pits the wasmi consensus runtime against wasmtime as a reference oracle. The wire-path, handshake-path, and DNS-trust-path crates (`snow`, `ed25519-dalek`, `hkdf`, `sha2`, `siphasher`, `hickory-resolver`, `minisketch-rs`, `wasmi`) are all **exact-pinned** and only move via SCIP after mainnet freeze.

The one outstanding production gate is disclosed openly: the genesis allocation key was historically derived from a **public seed string and is therefore burned** -- anyone with the seed (or git history) controls the 1e9 SC allocation. That is why the chain is testnet-only today, and it is the sole thing [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md) exists to fix. The air-gapped key-generation tooling (`examples/generate_offline_genesis_key.rs`, `examples/print_genesis_hashes.rs`) is already in the tree; what remains is the human ceremony of generating the new key offline, rebuilding genesis, and pinning the new hash. **Do not send real value to testnet addresses.**

---

## Repository layout

```
strangecoin/
+- src/                       # node binary: wallet, gui, mempool, api, cli, main
+- crates/
|   +- strangecoin-core/      # pure consensus, state, economics, governance (zero I/O)
|   +- strangecoin-vm-wasm/   # wasmi runtime, host functions, precompiles, ABI
|   +- strangecoin-net/       # Noise P2P, gossip, compact blocks, Erlay, peer mgmt
|   +- strangecoin-storage/   # Stage 3 storage (RocksDB-backed, default-on)
+- tests/                     # 40+ integration tests (reorg, double-spend, VM, net, ...)
+- fuzz/                      # 9 cargo-fuzz targets
+- docs/                      # ADRs, SCIPs, threat model, TLA+ spec, reproducible builds
+- examples/                  # offline genesis key tooling
+- vendor/minisketch-rs/      # Erlay PinSketch (SCIP-gated)
+- .github/workflows/         # ci.yml, release.yml
+- Cargo.toml                 # workspace root
```

The design philosophy is **pure core, impure shell.** `strangecoin-core` has no I/O, so the consensus rules can be embedded, fuzzed, and formally verified without dragging a runtime or a database with them. The binary, the VM, the network, and storage are thin shells that pull from the core. Every non-trivial decision is captured in an **ADR** (0001 through 0026 at last count); every protocol-level change is a **SCIP** that goes Idea -> Draft -> Review -> Activation -> Finalized.

---

## Build and run

Strangecoin builds on stable Rust (1.74+) across Linux, macOS, and Windows.

```bash
git clone https://github.com/Reider85/strangecoin.git
cd strangecoin
cargo build --release

# Run a node
cargo run --release -- run

# Generate an offline genesis key (air-gapped machine recommended)
cargo run --release --example generate_offline_genesis_key

# Replay mainnet blocks onto staging for consensus-divergence testing
cargo run --release -- shadow replay ./mainnet-blocks.bin
```

Regtest and devnet modes are first-class (`network_id` 3 and a devnet id respectively), with a regtest reward of zero and a self-generated genesis, so you can spin up a private chain for testing without touching the public testnet.

---

## What's next

Strangecoin's roadmap is staged, tagged, and documented in `docs/`. The stages already shipped unlock the next ones without rework:

- **Stage 3 -- Storage & state scaling.** RocksDB backend (crate already present, default-on), Verkle trie migration, state pruning, snapshot sync, and Block-STM parallel execution.
- **Stage 4 -- Developer experience.** SDK, local devnet with faucet, an indexer, and a JSON-RPC `eth_*` surface so existing Ethereum tooling can talk to Strangecoin.
- **Stage 5 -- Fee market & account abstraction.** EIP-1559-style fee market on top of the Stage 1 RBF groundwork, account abstraction, and the SCIP-001 dev fund.
- **Stage 6 -- External audit & formal verification.** Third-party security audit, TLA+ network spec, and formal verification of the consensus rules.
- **Stage 7 -- Proof of Stake.** BLS12-381 validator keys, slashing, a 1,000 SC minimum stake, a 5%-of-stake cap per validator, and a PoW to PoS migration gated by `activation_height`.

Each stage is a tagged release with a full changelog entry, a definition-of-done checklist, and an updated threat model -- the same cadence that got Stages 0 through 2 over the line.

---

## Contribute

Strangecoin is open source under the MIT license. Pull requests are welcome; the bar is "matches an existing ADR or proposes a new one, passes `cargo clippy -D warnings` and `cargo fmt --check`, and doesn't regress the 22 invariants." Read [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md) for the full workflow, and `docs/security/SECURITY.md` for how to report a vulnerability (PGP key, 48h SLA, 90-day coordinated disclosure, paid bug bounty).

If you find a bug, a fuzz panic, or a divergence between wasmi and wasmtime, the bounty table wants to hear from you.

---

## Status & disclaimer

Strangecoin is **testnet-only.** The genesis allocation key is burned pending [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md). **Do not send real value to any Strangecoin address.** This is a software-engineering project, not financial advice, not a token sale, and not an invitation to invest. Nothing here is a security, and nobody representing the project will ever ask you to send money to participate.

What Strangecoin *is*, is a serious, end-to-end, auditable Layer-1 written in safe Rust -- and it's one key ceremony away from mainnet. If that's the kind of project you want to watch, build on, or tear apart: **star the repo, clone it, and try to break it.**

---

*Built with Rust, blake3, secp256k1, wasmi, Noise, and a refusal to ship "almost."*
