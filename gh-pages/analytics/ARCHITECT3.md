# Strangecoin Architecture

Welcome to the comprehensive architecture documentation for Strangecoin. This document provides a detailed overview of the project's structure, design decisions, and implementation details.

## Table of Contents

1. [Overview](#overview)
2. [Layered Architecture](#layered-architecture)
3. [Core Components](#core-components)
4. [Network Layer](#network-layer)
5. [Storage Layer](#storage-layer)
6. [Virtual Machine](#virtual-machine)
7. [AI Agent Integration](#ai-agent-integration)
8. [Security Considerations](#security-considerations)
9. [Development Workflow](#development-workflow)
10. [Future Roadmap](#future-roadmap)

## Overview

Strangecoin is a production-grade Layer-1 blockchain written from scratch in Rust, designed specifically for AI agents. The project follows a clean architecture with strict separation of concerns, ensuring that consensus logic remains pure and free from I/O dependencies.

### Key Design Principles

- **Correctness First**: Every consensus rule has comprehensive test coverage
- **Layered & Replaceable**: Components are modular and can be swapped independently
- **Testnet Safety**: Genesis key is intentionally burned to prevent real-value mainnet
- **Documentation as Artifact**: ADRs, SCIPs, and detailed documentation are first-class artifacts

## Layered Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    User Interface                        │
│              (eframe/egui wallet, CLI)                 │
└─────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────┐
│                  MCP Server / JSON-RPC                  │
│          (Agent-native API surface)                     │
└─────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────┐
│                   Application Layer                      │
│           (Blockchain, Mempool, Wallet)                 │
└─────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────┐
│                   Virtual Machine                       │
│               (WASM Smart Contracts)                   │
└─────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────┐
│                   Network Layer                        │
│         (Noise, Gossip, Compact Blocks, Erlay)         │
└─────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────┐
│                   Storage Layer                        │
│               (RocksDB with Column Families)            │
└─────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────┐
│                   Consensus Core                        │
│          (Pure logic, 0 I/O, fuzzable)                 │
└─────────────────────────────────────────────────────────┘
```

## Core Components

### strangecoin-core

The core crate contains the pure consensus logic with zero I/O dependencies:

```rust
// Zero-I/O consensus enforcement
pub struct Consensus {
    // Consensus parameters
    pub chain_id: ChainId,
    pub max_block_size: u64,
    pub target_block_time: u64,
    
    // State management
    pub state: SparseMerkleTree,
    pub mempool: Mempool,
    
    // Validation logic
    pub validate_block: fn(Block) -> Result<(), ConsensusError>,
}
```

**Key Features:**
- Sparse Merkle tree for state representation
- Provable testing with `proptest`
- Unit test coverage for all consensus rules
- No dependencies on `tokio`, `std::net`, or `std::fs`

### Key Architectural Decisions

#### ADR-0008: Storage Architecture
- **Decision**: Move from LevelDB to RocksDB
- **Rationale**: Better performance, concurrency, and crash recovery
- **Implementation**: Column families for different data types
- **Migration**: Automatic LevelDB → RocksDB migration

#### ADR-0024: Storage Schema and Engine
- **Decision**: Versioned storage schema with 7 column families
- **Rationale**: Clear separation of concerns and future extensibility
- **Implementation**: Schema versioning, atomic writes, prefix iteration
- **Column Families**:
  - `blocks`: Block data by hash
  - `headers`: Block headers by height
  - `transactions`: Transactions by hash
  - `accounts`: Account states by address
  - `contracts`: Contract storage by address
  - `metadata`: Chain metadata and configuration
  - `mempool`: Pending transactions

#### ADR-0025: State Pruning Strategy
- **Decision**: Height-based pruning with archival mode
- **Rationale**: Balance between storage efficiency and data availability
- **Implementation**: Configurable pruning window, SCIP-gated expiry
- **Features**:
  - Automatic pruning of old state
  - Archival mode for full history
  - State root verification

## Network Layer

The network layer implements a production-grade P2P stack:

### Protocol Stack

1. **Transport Layer**: Noise XX handshake with ChaCha20-Poly1305 encryption
2. **Gossip Layer**: Transaction and block propagation
3. **Optimization Layer**: Compact blocks (BIP 152) and Erlay reconciliation
4. **Discovery Layer**: DNSSEC-validated peer discovery

### Key Components

#### Noise Protocol Framework
```rust
pub struct NoiseHandshake {
    static_key: StaticKey,
    ephemeral_key: EphemeralKey,
    handshake_state: HandshakeState,
    node_id: NodeId,
}
```

#### Peer Scoring
```rust
pub struct PeerScoring {
    // Performance metrics
    uptime_fraction: f32,
    latency_ms: u64,
    response_rate: f32,
    
    // Violation tracking
    invalid_blocks: u32,
    protocol_violations: u32,
    
    // Ban state
    ban_state: BanState,
}
```

#### Erlay Reconciliation
```rust
pub struct ErlayReconciler {
    sketch: PinSketch,
    short_ids: HashMap<TxId, u64>,
    trigger_interval: Duration,
}
```

## Storage Layer

### RocksDB Implementation

```rust
pub struct RocksDbEngine {
    db: rocksdb::DB,
    column_families: HashMap<String, ColumnFamily>,
    schema_version: u32,
}
```

**Features:**
- 7 column families for different data types
- Atomic WriteBatch operations
- Prefix iteration for efficient range queries
- Schema versioning with fail-closed upgrades
- Automatic crash recovery

### Storage Schema

```rust
// Key encoding with BigEndian for numeric ordering
pub struct KeyPrefix {
    pub index: u8,        // Column family identifier
    pub namespace: u8,    // Data type namespace
    pub key_data: Vec<u8>, // Actual key data
}

// Key constructors
pub fn block_hash_key(hash: &BlockHash) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(&KeyPrefix::BLOCK_HASH.to_be_bytes());
    key.extend_from_slice(hash);
    key
}

pub fn account_state_key(address: &Address) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(&KeyPrefix::ACCOUNT_STATE.to_be_bytes());
    key.extend_from_slice(&address.to_bytes());
    key
}
```

## Virtual Machine

### WASM Smart Contracts

```rust
pub struct WasmiExecutor {
    vm: wasmi::Module,
    gas_limit: u64,
    host_functions: HostFunctions,
}
```

**Host Functions:**
- Crypto: secp256k1, blake3, AES-GCM
- Storage: Get/Set contract state
- Context: Block info, transaction data
- Logging: Event emission
- Math: Safe arithmetic operations

### Smart Contract Features

- **Gas Metering**: Instruction counting with configurable limits
- **Reentrancy Guard**: Protection against recursive calls
- **Storage Management**: Persistent contract state
- **Event System**: Loggable events for off-chain processing
- **Precompiles**: Optimized implementations for common operations

## AI Agent Integration

### MCP (Model Context Protocol) Server

```rust
pub struct McpServer {
    blockchain: Arc<Blockchain>,
    rpc_server: RpcServer,
    tools: HashMap<String, Tool>,
    resources: HashMap<String, Resource>,
}
```

**Available Tools:**
- `get_block`: Retrieve block by height or hash
- `get_balance`: Get account balance
- `submit_tx`: Submit transaction
- `call_contract`: Execute contract method
- `estimate_fee`: Estimate transaction cost
- `subscribe_events`: Stream chain events

### ACP (Agent Communication Protocol)

```rust
pub enum AcpMessage {
    Negotiate(AcpNegotiate),
    Task(AcpTask),
    Settle(AcpSettle),
    Revoke(AcpRevoke),
}
```

**Features:**
- Peer-to-peer agent communication
- Task negotiation and settlement
- Payment micropayments
- Replay protection with nonces
- Escrow integration

### Agent Identity Management

```rust
pub struct AgentIdentity {
    // secp256k1 EOA with HD derivation
    public_key: PublicKey,
    address: Address,
    
    // Agent-specific features
    key_derivation: HdDerivation,
    batch_capability: bool,
    sponsor_capability: bool,
}
```

## Security Considerations

### Cryptographic Security

- **secp256k1**: Elliptic curve cryptography for signatures
- **blake3**: Cryptographic hash function
- **AES-GCM**: Authenticated encryption for key management
- **PBKDF2**: Key derivation for password hashing

### Network Security

- **Noise Protocol**: Mutual authentication and encrypted transport
- **Peer Scoring**: Malicious peer detection and banning
- **Rate Limiting**: Protection against DoS attacks
- **DNSSEC**: Validated peer discovery

### Smart Contract Security

- **WASM Sandboxing**: Code execution isolation
- **Gas Limits**: Protection against infinite loops
- **Reentrancy Guard**: Prevention of recursive attacks
- **Safe Math**: Overflow/underflow protection

## Development Workflow

### Build System

```bash
# Development build
cargo build

# Release build with optimizations
cargo build --release

# Cross-platform builds
cargo build --target x86_64-unknown-linux-gnu
cargo build --target x86_64-apple-darwin
cargo build --target x86_64-pc-windows-msvc
```

### Testing

```bash
# Run all tests
cargo test --workspace

# Property-based testing
cargo test --release

# Fuzz testing
cargo fuzz run canonical_decode
```

### Code Quality

```bash
# Format code
cargo fmt --all

# Lint code
cargo clippy --all-targets -- -D warnings

# Check documentation
cargo test --doc
```

## Future Roadmap

### Stage 3: Storage and Scalability
- [x] Storage schema and engine
- [x] State pruning strategy
- [x] Snapshot sync format
- [ ] Indices and performance optimization
- [ ] Stateless validation witnesses

### Stage 4: Developer Experience
- [ ] Professional UI replacement
- [ ] SDK and devnet
- [ ] Enhanced JSON-RPC surface
- [ ] Documentation website

### Stage 5: Advanced Features
- [ ] Account abstraction (ERC-4337)
- [ ] Dynamic fee market (EIP-1559)
- [ ] Threshold encryption mempool

### Stage 6+: Research and Production
- [ ] TLA+ formal verification
- [ ] ZK-VM integration
- [ ] Mainnet launch (after SCIP-0001)

## Conclusion

Strangecoin represents a comprehensive approach to building a production-grade blockchain for AI agents. The architecture emphasizes correctness, modularity, and extensibility while maintaining strict separation of concerns. By following established patterns and incorporating modern best practices, the project aims to provide a solid foundation for decentralized AI agent interactions.

---

*This architecture document is continuously updated as the project evolves. For the most current information, please refer to the latest version in the repository.*