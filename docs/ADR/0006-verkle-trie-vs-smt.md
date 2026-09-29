# ADR-0006: Verkle Trie for State Root

**Status:** Accepted  
**Date:** 2026-09-29  
**Deciders:** Core team  
**Stage:** 1 (S1-P06)  

## Context

Invariant #19 requires `state.root_after(block) == block.state_root` (ARCHITECT3 §5). The block header must carry a deterministic commitment to the post-state, enabling:

- Stateless validation (light clients verify blocks without full state)
- Efficient state sync (compare roots instead of full state)
- Future proof systems (S1-P07 StateWitness)

We need a deterministic hash-based accumulator over the key-value state (address → {balance, nonce}) that produces a single `[u8; 32]` root.

## Decision

Implement a **minimal Verkle Trie** (256-ary sparse Merkle tree with blake3 hashing) inside `crates/strangecoin-core/src/state/verkle.rs`. No external crate.

## Alternatives Considered

### 1. Sparse Merkle Tree (SMT)
- **Pros:** Well-understood, simpler implementation, battle-tested in production systems.
- **Cons:** O(256·log(n)) proof size for n leaves; less efficient than Verkle for large state.
- **Verdict:** Our minimal implementation IS effectively an SMT. The distinction at this stage is naming — we adopt the Verkle-compatible interface (`compute_root`, future `prove`/`verify`) so upgrading to a true Verkle Trie (e.g., via a crate like `verkle-trie` when mature) is a drop-in replacement.

### 2. Merkle Patricia Trie (MPT) — Ethereum style
- **Pros:** Proven at scale (Ethereum mainnet).
- **Cons:** Complex implementation (extension nodes, hex-prefix encoding); no existing Rust crate that's production-ready and I/O-free.
- **Verdict:** Over-engineered for Stage 1. The 256-ary sparse tree covers the same use case with less complexity.

### 3. Ready-made crate (verkle-trie, ark-*-verkle)
- **Pros:** No maintenance burden; cryptographic correctness delegated.
- **Cons:** As of 2026-09, no mature Rust Verkle Trie crate exists that: (a) is I/O-free, (b) works with our account model, (c) has stable API. The `verkle-trie` crate on crates.io is a stub. Ethereum's `verkle` crate depends on `ark-*` heavy crypto libraries.
- **Verdict:** Not viable for Stage 1. Revisit on Stage 3 when the ecosystem matures.

### 4. Hash-only accumulator (no trie, just blake3 of sorted state)
- **Pros:** Simplest possible (sort accounts, concatenate, hash).
- **Cons:** No efficient proofs (must send entire state); not compatible with future witness generation.
- **Verdict:** Rejected — blocks future light client work without a rewrite.

## Consequences

### Positive
- Invariant #19 enforced at block validation time
- `root_after(state, block)` is a pure function — 0 I/O, testable with proptest
- Deterministic: same state → same root, regardless of insertion order (sorted keys)
- Foundation for S1-P07 (StateWitness) — the `prove`/`verify` API can be added later

### Negative
- Minimal implementation does NOT provide cryptographic proofs (witness generation is S1-P07)
- The 256-ary tree has higher memory overhead than a compact trie for very large states — acceptable at Stage 1 scale
- FORMAT_VERSION bump (2→3) breaks serialized format — acceptable because mainnet is not launched

### Risks
- If a production-grade Verkle Trie crate appears before Stage 3, migration is needed. Mitigation: the `verkle` module interface is designed as a drop-in replacement boundary.
- The hash function (blake3) is not a Verkle commitment. Upgrading to a true Verkle polynomial commitment is a future task (Stage 3+).

## Implementation Notes

- Key addressing: `blake3(address)` → 32-byte key, inserted into the trie
- Internal node hashing: `blake3(children_hashes_concatenated)`
- Empty node hash: `blake3([])` (constant)
- Root computation: `compute_root(accounts: &HashMap<String, AccountState>) -> [u8; 32]`
- Accounts are sorted by key hash before insertion (determinism)
