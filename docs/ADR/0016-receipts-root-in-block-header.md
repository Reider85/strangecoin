# ADR-0016: Receipts Root in Block Header

## Status

Accepted (2026-10-19) — S1.5-FBP07

## Context

Stage 1.5 (S1.5-P14) added receipts as derived data with a proof-friendly
sub-trie: `receipts_subroot` / `prove_receipt` / `verify_receipt_proof`
(`crates/strangecoin-core/src/state/sparse_merkle.rs`) build a depth-256
binary SMT over `blake3(0x02 ‖ serialize_receipt(receipt))` leaves keyed by
txid. But the commitment lives nowhere in consensus: `BlockHeader` carries
`state_root` and `tx_root` only, so a light client that receives a receipt
plus a 256-sibling proof has **no on-chain root to verify against** — it would
have to re-execute the chain (full state) to recompute the receipts root.
Changelog 1.1.7 listed this as an MVP-obligation («receipts_root fold в
header»); ADR-0006 covered the account-state SMT only.

Forces at play:

- **Light-client receipts** — verifying `emit_log`/value-transfer trails
  without full state requires a header-committed root (Ethereum precedent:
  separate `receiptsRoot` field, not folded into `stateRoot`).
- **Commitment separation** — mixing receipts into the account `state_root`
  couples two independent concerns and breaks the ADR-0006 SMT key space
  (account keys are `blake3(address)`; receipts are txid-keyed).
- **Wire stability** — the header encoding is the PoW preimage
  (`header_hash = blake3(serialize_header)`); any field addition is a
  consensus-breaking change and must land before mainnet freeze, with the
  version byte and golden vectors regenerated in the same commit
  (procedure BUG-S1-007; `serialize.rs` receipt doc-comment already deferred
  the receipts-domain version byte to this lock-in).
- **Genesis build path** — `load_genesis` constructs the genesis block from
  `genesis.json` without executing it (only `tx_root` is computed); the
  genesis header cannot compute a receipts subroot at build time.

## Decision

**Add `receipts_root: [u8; 32]` to `BlockHeader` as a separate
Ethereum-style commitment**, committed in the canonical header encoding as
the last field (after `state_root`).

| Aspect | Decision |
|---|---|
| Field | `BlockHeader.receipts_root: [u8; 32]` (+ mirrored `Block.receipts_root`, `#[serde(default)]`) |
| Root function | `receipts_root(receipts) := receipts_subroot(receipts)` — the txid-keyed SMT (proof-friendly), **not** the flat `compute_receipts_root` merkle |
| Wire position | End of `serialize_header` (offset 189..221); `HEADER_WIRE_LEN` 189 → 221 |
| Version byte | `FORMAT_VERSION` 6 → 7 — locks the header layout change and the receipts-domain encoding together; genesis hashes + golden vectors regenerated (BUG-S1-007) |
| Validation | `block_executor::validate_and_apply`: after the `state_root` check, recompute `receipts_root(new_state.receipts)` and reject on mismatch (`StrangecoinError::InvalidBlock`) |
| Empty/genesis policy | Genesis blocks (and only `index == 0`) may carry `receipts_root == [0u8; 32]`, mirroring the zero-`state_root` tolerance; all other blocks must match the computed subroot exactly |
| Block production | `mine_block_inner` and `create_grant_block` set the real subroot from the applied post-state receipts before sealing; `load_genesis` / `regtest_genesis_block` / `create_genesis_block` keep `[0u8; 32]` |
| Core surface | Pure helper `state::receipts_root(&[Receipt]) -> [u8; 32]` (alias of `receipts_subroot`); `root_after` unchanged (account-state root stays separate) |

### Genesis zero policy (rationale)

`receipts_subroot(&[])` returns the SMT empty-subtree sentinel (non-zero),
while every existing root convention in the header (`state_root`,
`tx_root`, empty merkle) uses `[0u8; 32]` as the "unset" value. The genesis
block is built without execution, so its header stores `[0u8; 32]` exactly
like `state_root` does; validation tolerates the zero value for `index == 0`
only. Consequence: light-client receipt proofs against the *genesis* header
root are not meaningful — genesis is trusted by pinned hash
(`EXPECTED_GENESIS_HASH`), same as its zero `state_root`.

### Light-client flow

```
receipt = tip_receipts()[i]                 // or any full-node answer
proof   = prove_receipt(&receipts, &txid)   // 256 sibling hashes
verify_receipt_proof(&block.receipts_root, &receipt, &proof)  // header-committed
```

No full state, no re-execution: the header (already verified by PoW +
`state_root`) now anchors the receipt.

## Alternatives Considered

1. **Fold `receipts_subroot` into the global `state_root`** — a composite
   root over account state + receipts. Rejected: mixes concerns, breaks the
   ADR-0006 key space, and forces every state proof to carry receipts
   context. (Ethereum also keeps `receiptsRoot` separate.)
2. **Receipts as account data** (per-tx pseudo-accounts holding receipts) —
   overkill: bloats the account trie, complicates `apply/unapply`, and still
   needs a separate proof story.
3. **Keep receipts uncommitted (status quo)** — light clients cannot verify
   logs/value-transfer trails; the Stage 1.5 DoD («receipts proofs») stays
   partially unmet.
4. **`compute_receipts_root` (flat sorted merkle) as the header root** —
   order-independent but proof-unfriendly (no sparse proofs; O(n) witness)
   and inconsistent with the already-shipped SMT receipt proofs.

## Consequences

- **Consensus-breaking**: header wire format, `header_hash` (PoW preimage),
  and therefore `EXPECTED_GENESIS_HASH` / `EXPECTED_TESTNET_GENESIS_HASH`
  and all golden vectors change. Acceptable pre-mainnet (regtest/devnet
  only); no SCIP/activation height required until mainnet freeze. Old and
  new nodes diverge at the version byte + length check.
- **Receipts are now consensus-validated**, not merely derived: a block
  whose header commits to a wrong receipts root is rejected exactly like a
  wrong `state_root`. `State.receipts` remains recomputed (never persisted);
  the invariant is `block.receipts_root == receipts_subroot(applied receipts)`.
- **`FORMAT_VERSION` 7** moves the shared version byte for transactions,
  receipts, and headers — Create/Call wire bytes keep the same layout but
  the leading byte differs from v6.
- **Genesis receipt proofs are out of scope** (zero header root; trusted by
  pinned hash) — documented above.
- **TLA+**: the bounded model gains a `receipts_root` field and a
  `ReceiptsRootMatch` invariant (deterministic fold over the block's txs),
  keeping the spec aligned with the wire.
- Incremental SMT updates (recomputing the receipts subroot per touched
  leaf) remain Stage 3+; per-block O(receipts) fold is acceptable at the
  current block gas limit.

## Implementation Notes

- `crates/strangecoin-core/src/types.rs` — field on `Block` + `BlockHeader`,
  threaded through `header()` / `from_header()`.
- `crates/strangecoin-core/src/serialize.rs` — `FORMAT_VERSION = 7`,
  `serialize_header` / `deserialize_header`, `HEADER_WIRE_LEN = 221`.
- `crates/strangecoin-core/src/state/mod.rs` — `receipts_root` alias,
  re-exported from `lib.rs`.
- `src/blockchain/block_executor.rs` — set at `mine_block_inner` /
  `create_grant_block`; checked in `validate_and_apply`.
- Genesis hashes regenerated via `cargo run --example print_genesis_hashes`;
  golden vectors via `cargo run -p strangecoin-core --example
  print_serialized_block` (BUG-S1-007).
