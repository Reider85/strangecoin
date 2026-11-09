# Strangecoin README

Welcome to Strangecoin! A production-grade Layer-1 blockchain written from scratch in Rust for AI agents.

## Quick Start

### Prerequisites

- Rust 1.74+ (edition 2021)
- Cargo (comes with Rust)
- OpenSSL development headers

### Build and Run

```bash
# Clone the repository
git clone https://github.com/Reider85/strangecoin.git
cd strangecoin

# Build the project
cargo build --release

# Run the node
cargo run --release -- --headless
```

### Development

```bash
# Run tests
cargo test --workspace

# Run with GUI (default)
cargo run

# Run headless (server mode)
cargo run -- --headless
```

## What is Strangecoin?

Strangecoin is a **full cryptocurrency written from scratch in Rust** — and **a cryptocurrency for AI agents**. It is not a fork of any existing coin — the address format (`rsc1` bech32), chain IDs, genesis block, consensus parameters, network protocol and improvement process are all native to this project.

### Built for AI Agents

Strangecoin is engineered so that autonomous economic agents are first-class participants:

- **MCP (Model Context Protocol) server** — the node exposes itself as an [MCP](https://modelcontextprotocol.io) server over stdio + HTTP+SSE, serving chain state as tools (`get_block`, `get_balance`, `get_nonce`, `submit_tx`, `call_contract`, `estimate_fee`, `subscribe_events`) and resources.
- **ACP (Agent Communication Protocol)** — a wire message family (`ACP_NEGOTIATE`, `ACP_TASK`, `ACP_SETTLE`, `ACP_REVOKE`) rides on top of the existing Noise-authenticated P2P layer so agents can negotiate tasks, quote and pay for services, and settle micropayments peer-to-peer.
- **Agent identity = wallet identity** — secp256k1 EOA (same curve as MetaMask/Ledger), HD derivation, key isolation, batched and sponsored txs via WASM contracts.

### Key Features

| Feature | Implementation |
|---------|----------------|
| **Consensus** | PoW with retargeting, MTP validation |
| **Network** | Noise XX handshake, ChaCha20-Poly1305, peer scoring |
| **Storage** | RocksDB with column families, versioning |
| **VM** | wasmi 2.0 with 13 host functions |
| **Crypto** | secp256k1, blake3, AES-GCM, PBKDF2 |

## Architecture

Strangecoin follows a layered architecture with a pure core and impure shell:

- **strangecoin-core**: Zero-I/O consensus logic
- **strangecoin-net**: P2P networking layer
- **strangecoin-vm-wasm**: WASM smart contract execution
- **strangecoin-storage**: Storage engine (RocksDB)
- **Main binary**: Orchestration and user interface

## Documentation

For comprehensive documentation, visit our [GitHub Pages site](https://reider85.github.io/strangecoin/).

## Contributing

We welcome contributions! Please see our [Contributing Guidelines](CONTRIBUTING.md) for details.

### Getting Started

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Add tests for your changes
5. Ensure all tests pass (`cargo test`)
6. Commit your changes (`git commit -m 'feat: add amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

## Roadmap

### Stage 3 (Current): Storage and Scalability
- [x] Storage schema and engine (RocksDB)
- [x] State pruning strategy
- [x] Snapshot sync format
- [ ] Indices and performance optimization
- [ ] Stateless validation witnesses

### Stage 4: Developer Experience
- [ ] Professional UI replacement
- [ ] SDK and devnet
- [ ] JSON-RPC surface
- [ ] Enhanced documentation

### Stage 5: Advanced Features
- [ ] Account abstraction
- [ ] Dynamic fee market
- [ ] Threshold encryption mempool

### Stage 6+: Research and Production
- [ ] TLA+ formal verification
- [ ] ZK-VM integration
- [ ] Mainnet launch (after SCIP-0001)

## Status & Disclaimer

**Strangecoin is testnet-only.** The genesis allocation key was historically derived from a public seed string and is therefore **burned** — anyone with the seed (or git history) controls the 1e9 SC allocation. It is the sole thing [SCIP-0001](docs/SCIP/scip-0001-genesis-key-replacement.md) exists to fix.

**Do not send real value to any Strangecoin address.** This is a software-engineering project, not financial advice, not a token sale, and not an invitation to invest. Nothing here is a security, and nobody representing the project will ever ask you to send money to participate.

## License

Licensed under the **Apache License, Version 2.0**. See [LICENSE](LICENSE) for the full text.

```
SPDX-License-Identifier: Apache-2.0
```

---

*Built with Rust, blake3, secp256k1, wasmi, Noise, MCP, ACP, and a refusal to ship "almost."*

**Star it · Fork it · Try to break it · Point your agents at it.**