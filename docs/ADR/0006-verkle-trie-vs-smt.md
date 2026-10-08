# ADR-0006: Sparse Merkle Tree for State Root

**Status:** Amended  
**Date:** 2026-09-29 (original) / 2026-10-08 (amended — BUG-S0-011)  
**Deciders:** Core team  
**Stage:** 1 (S1-P06) → amended at Stage 1.5 debt track (BUG-S0-011 / S1.5-P02)

## Context

Invariant #19 requires `state.root_after(block) == block.state_root` (ARCHITECT3 §5). The block header must carry a deterministic commitment to the post-state, enabling:

- Stateless validation (light clients verify blocks without full state)
- Efficient state sync (compare roots instead of full state)
- Future proof systems (S1-P07 StateWitness)

We need a deterministic hash-based accumulator over the key-value state (address → {balance, nonce}) that produces a single `[u8; 32]` root.

### Original decision (S1-P06) and why it was wrong

The original decision text said «minimal Verkle Trie (256-ary sparse Merkle tree with blake3)». The implementation that shipped as `state/verkle.rs` was **neither**: a flat `[[u8; 32]; 256]` array indexed by `key[0]` only.

| Claim | Fact (BUG-S0-011) |
|-------|-------------------|
| Trie with branching | `insert_at_depth` writes `nodes[key[0]] = value`; recursion exits at depth 0 |
| Depth 32 | Effective depth 1 |
| KZG / BLS12-381 / Verkle multi-opening | Absent from `Cargo.toml` and code |
| Collision resistance | Two accounts sharing `blake3(address)[0]` overwrite each other once state > 256 accounts (BUG-S0-014) |
| Proofs | `prove()` returned 255 sibling hashes of a depth-1 tree; ignored the `account` parameter (BUG-S0-016) |

Naming the module `verkle` and marking DoD «Verkle Trie ✅» was a false claim. This ADR replaces it with an honest decision.

## Decision

Implement a **binary Sparse Merkle Tree (SMT)** over the full 256-bit account key inside `crates/strangecoin-core/src/state/sparse_merkle.rs`. No external crate.

### Construction

| Parameter | Value |
|-----------|-------|
| Key | `blake3(address)` → 32 bytes (256 bits) |
| Branching | Binary (width 2) |
| Depth | **256** (one level per key bit, MSB of byte 0 first) |
| Leaf hash | `blake3(key ‖ balance_le ‖ nonce_le)` |
| Empty leaf slot | `EMPTY_HASH = blake3("")` |
| Internal node | `blake3(left ‖ right)` |
| Empty subtree hash | Precomputed per depth (0..=256), `empty[256] = EMPTY_HASH`, `empty[d] = blake3(empty[d+1] ‖ empty[d+1])` |
| Root | Hash of the depth-0 subtree |
| Proof | 256 sibling hashes (8 KiB), one per level |
| Public API | `new`, `insert`, `root`, `compute_root`, `empty_root`, `prove`, `verify_proof` |

### Why depth 256, not depth 32

The bug report (BUG-S0-011 recommendation C) said «32 уровня … proof O(32) = 1 KB». A 32-level **binary** tree can address only 2^32 leaves; mapping `blake3(address)` onto 32 bits would reintroduce the same birthday-collision class as BUG-S0-014 (expected collision at ~2^16 accounts). A 32-level **256-ary** tree requires 32 × 255 = 8160 sibling hashes per proof (256 KiB).

Depth-256 binary branching uses every bit of the key:

- No collisions for any n ≤ 2^256 (BUG-S0-014 closed).
- Proof size 256 × 32 B = 8 KiB — same order as the old 255-hash depth-1 proofs, not a regression.
- `prove(address, account)` consumes `account` (BUG-S0-016 closed): the claimed account is validated against the trie; a pruned account (`balance == 0 && nonce == 0`) is proven by `EMPTY_HASH`; mismatch returns `CoreError::ProofAccountMismatch`.

True Verkle (KZG polynomial commitments over BLS12-381, multi-opening) remains **deferred to Stage 3+**: no mature I/O-free Rust crate exists (see Alternatives), and the heavy `ark-*` stack is out of scope for the 0-I/O core at Stage 1.5.

## Alternatives Considered

### 1. True Verkle Trie (KZG + BLS12-381)
- **Pros:** O(1) multi-openings for light clients; the «real» Verkle the original ADR promised.
- **Cons:** No mature Rust crate that is (a) I/O-free, (b) works with our account model, (c) has a stable API (`verkle-trie` on crates.io is a stub). `ark-bls12-381` / ethereum `verkle` pull heavy crypto into `strangecoin-core`, which must stay 0-I/O and dependency-light. Effort estimate 1–2 weeks plus crypto review.
- **Verdict:** Deferred to Stage 3+ (ecosystem maturity gate in the original ADR still holds). The SMT API (`compute_root` / `prove` / `verify_proof`) is the drop-in replacement boundary for a future Verkle.

### 2. 256-ary SMT, 32 levels (literal reading of the bug report)
- **Pros:** Matches «32 уровня» wording; node hash = blake3 of 256 children.
- **Cons:** Proof = 32 × 255 hashes = 256 KiB per account — impractical for light clients and for `StateWitness`.
- **Verdict:** Rejected on proof size. Depth-256 binary gives collision-free addressing at 8 KiB proofs.

### 3. 32-level binary SMT over truncated key (first 32 bits)
- **Pros:** Proof = 32 hashes = 1 KiB (the O(32) figure in the bug report).
- **Cons:** Collides at birthday bound ~2^16 accounts — the same defect class as BUG-S0-014.
- **Verdict:** Rejected. Correctness beats proof-size aesthetics before mainnet freeze.

### 4. Merkle Patricia Trie (MPT) — Ethereum style
- **Pros:** Proven at scale (Ethereum mainnet).
- **Cons:** Complex (extension nodes, hex-prefix encoding); no production-ready I/O-free Rust crate.
- **Verdict:** Over-engineered for Stage 1.5; revisit with the Verkle gate.

### 5. Hash-only accumulator (blake3 of sorted state)
- **Pros:** Simplest possible.
- **Cons:** No efficient proofs; incompatible with StateWitness.
- **Verdict:** Rejected — blocks light-client work without a rewrite.

## Consequences

### Positive
- Invariant #19 enforced at block validation time (`root_after`)
- `root_after(state, block)` stays a pure function — 0 I/O, proptest-friendly
- Deterministic: same state → same root regardless of insertion order (sorted leaves)
- Collision-free for any realistic account count (full 256-bit key)
- `prove`/`verify_proof` actually verify the account (BUG-S0-016): `prove` validates the claimed account against the trie and returns `Result`
- Foundation for S1-P07 StateWitness; interface unchanged (`compute_state_root`)

### Negative
- **Breaking change to `state_root` values:** every historical root computed by the flat structure is invalid under the SMT. Mainnet is not launched; operators must reset LevelDB or resync. Wire format (`FORMAT_VERSION = 4`) is unchanged — only the semantic meaning of committed roots.
- Proof size 8 KiB per account (vs 8 KiB for the old depth-1 proofs — not worse, but not O(32))
- True Verkle still absent; light clients do not get KZG multi-openings until Stage 3+

### Risks
- If a production-grade Verkle Trie crate appears before Stage 3, migration is needed. Mitigation: `sparse_merkle` module interface is the drop-in replacement boundary (`compute_root`, `prove`, `verify_proof` signatures stay).
- blake3 is a hash, not a polynomial commitment. Upgrading to true Verkle remains a future task (Stage 3+).
- Existing LevelDB data with old roots will fail `root_after` on resync — expected; documented in Changelog.

## Implementation Notes (S1.5-P02)

- Module: `crates/strangecoin-core/src/state/sparse_merkle.rs`
- Type: `SparseMerkleTrie` (replaces `VerkleTrie`; `verkle.rs` removed)
- Key addressing: `blake3(address)` → 32 bytes; bit `i` (MSB-first) selects left/right at level `i`
- Leaf hash: `blake3(key_hash ‖ balance.to_le_bytes() ‖ nonce.to_le_bytes())`
- Empty subtree hashes: precomputed table `empty_subtree[depth]` for depth 0..=256
- Root computation: sort leaves by key, recursive partition on each bit, O(n · 256) with precomputed empty hashes
- `compute_root(accounts: &HashMap<String, AccountState>) -> [u8; 32]`
- Accounts with `balance == 0 && nonce == 0` are pruned (not inserted); `prove` for such an account emits `EMPTY_HASH` as the leaf and validates absence; a claimed account that does not match the trie returns `CoreError::ProofAccountMismatch`
- Tests: proptest 1000+ accounts → unique deterministic roots; 256→512 account sweep → no collision; prove/verify round-trip; tampered proof rejected
