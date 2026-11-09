# Strangecoin Documentation

Welcome to the official documentation for Strangecoin, a production-grade Layer-1 blockchain written from scratch in Rust for AI agents.

## Quick Links

- [Introduction](README.md)
- [Architecture](analytics/ARCHITECT3.md)
- [ADR Index](docs/ADR/)
- [SCIP Index](docs/SCIP/)
- [Security Policy](SECURITY.md)

## What is Strangecoin?

Strangecoin is a full cryptocurrency designed specifically for AI agents, featuring:

- **Production-grade Layer-1 blockchain** with PoW consensus
- **MCP-native** architecture with built-in Model Context Protocol server
- **Noise-encrypted P2P networking** with Erlay reconciliation
- **WASM smart contracts** with wasmi runtime
- **Built-in ACP support** for agent-to-agent communication
- **Comprehensive tooling** and developer experience

## Getting Started

### Prerequisites

- Rust 1.74+ (edition 2021)
- Cargo package manager
- OpenSSL development headers

### Installation

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

# Run with GUI
cargo run

# Run headless server
cargo run -- --headless
```

## Architecture

Strangecoin follows a layered architecture with a pure core and impure shell:

- **strangecoin-core**: Zero-I/O consensus logic
- **strangecoin-net**: P2P networking layer
- **strangecoin-vm-wasm**: WASM smart contract execution
- **strangecoin-storage**: Storage engine (RocksDB)
- **Main binary**: Orchestration and user interface

## Documentation Structure

### Architecture Decision Records (ADRs)

All significant design decisions are documented in ADRs:

- [ADR-0001: Project Goals and Scope](docs/ADR/0001-project-goals-and-scope.md)
- [ADR-0002: Cryptographic Primitives](docs/ADR/0002-cryptographic-primitives.md)
- [ADR-0028: Strangecoin as Cryptocurrency for AI Agents](docs/ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md)
- [ADR-0029: MCP Server Implementation](docs/ADR/0029-mcp-server.md)
- [ADR-0030: ACP Agent Communication Protocol](docs/ADR/0030-acp-agent-communication-protocol.md)

### Standard Improvement Proposals (SCIPs)

Protocol improvements follow the SCIP format:

- [SCIP-0001: Genesis Key Replacement](docs/SCIP/scip-0001-genesis-key-replacement.md)
- [SCIP-0002: Consensus Parameters](docs/SCIP/scip-0002-consensus-parameters.md)

## Development Roadmap

### Stage 3: Storage and Scalability (Current)
- [x] Storage schema and engine
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
- [ ] Mainnet launch

## Contributing

We welcome contributions! Please see our [Contributing Guidelines](CONTRIBUTING.md) for details.

### Getting Started

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a Pull Request

### Code Style

- Follow Rust best practices
- Run `cargo fmt` before committing
- Run `cargo clippy` to catch issues
- Update documentation for new features

## Support

- **GitHub Issues**: Report bugs and request features
- **GitHub Discussions**: Join community discussions
- **Security**: Report vulnerabilities privately
- **Documentation**: This documentation site

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

---

*Built with Rust, blake3, secp256k1, wasmi, Noise, MCP, ACP, and a refusal to ship "almost."*