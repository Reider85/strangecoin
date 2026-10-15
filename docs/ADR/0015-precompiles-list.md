# ADR-0015: Precompiles List — 7 Built-in Contracts

## Status

Accepted (2026-10-15) — S1.5-P04

> **Numbering note:** reserved as ADR-0014 in `analytics/prompt-stage1.5.md`;
> renumbered to **ADR-0015** (0011 = async migration, 0012 = WASM VM,
> 0013 = JSON ABI, 0014 = gas metering). See the numbering note in ADR-0012.

## Context

Contracts need cryptographic and math primitives that are too expensive or too
error-prone to implement in portable WASM (signature verification, hash
functions, modular exponentiation). The standard pattern — borrowed from
Ethereum — is **precompiles**: fixed built-in contracts at reserved addresses,
invoked via the normal `call(address, data)` path, with no WASM bytecode behind
them. `ARCHITECT3.md` §3.9 lists candidates
(`ecrecover, sha256, keccak, blake3, ed25519_verify, bls_aggregate, modexp`)
without addresses, costs, or I/O formats. This ADR fixes the list before the
implementation prompt (S1.5-P11).

Forces at play:

- **Replay protection on-chain** — contracts verifying meta-transactions or
  login-with-ethereum style flows need `ecrecover` (secp256k1), matching the
  chain's EOA signature scheme (ADR-0001).
- **Hash agility** — the chain hashes with blake3 (canonical serialization),
  but contract ecosystems (and Stage 4 `eth_*` interop) speak sha256/keccak;
  contracts need all three.
- **Future PoS** — Stage 7 hybrid PoS will need BLS12-381 aggregate
  verification; reserving the address now avoids a later address reshuffle.
- **Audit surface** — every precompile is consensus-critical code; the list
  should stay minimal and each entry must justify its cost.
- **Address-space discipline** — precompile addresses live in the same address
  space as EOA/contract accounts; the mapping must be unambiguous and stable.

## Decision

We will ship **7 precompiles at addresses `0x01..0x07`**, callable via
`call(precompile_address, data)`, charged through the ADR-0014 gas model.
`bls_aggregate` returns `NotImplemented` until Stage 7.

| Addr | Name | Gas (sketch) | Status |
|---|---|---|---|
| `0x01` | `ecrecover` | 3000 | MVP |
| `0x02` | `sha256` | 50 + 12 per 32-byte word | MVP |
| `0x03` | `keccak` | 30 + 6 per 32-byte word (Keccak-256, *not* NIST SHA3) | MVP |
| `0x04` | `blake3` | 50 + 6 per 64-byte chunk | MVP |
| `0x05` | `ed25519_verify` | 2000 | MVP |
| `0x06` | `bls_aggregate` | 12000 | **NotImplemented until Stage 7** (no BLS12-381 dependency yet) |
| `0x07` | `modexp` | 50 + 50 per byte of result | MVP |

Key points:

- **Addresses `0x01..0x07`** — order follows Ethereum convention for
  familiarity, but the *list differs* (no ripemd160/identity/altbn128; we add
  keccak, blake3, ed25519, bls). Callers must never assume Ethereum address
  semantics beyond "small integer = precompile".
- **Reserved-range rule** — accounts at addresses `0x00..0xff` with the low
  byte non-zero and upper bytes zero are reserved for future precompiles;
  `create` (S1.5-P08) rejects deploying contracts into the reserved range.
  Address `0x00` is the null address (burn), never a precompile.
- **Determinism** — every precompile is pure (input → output), no state access,
  no ambient context; identical results on all validators (threat V-21).
- **Failure semantics** — invalid input (bad length, bad point, non-canonical
  encoding) consumes the full gas of the call frame and returns failure;
  precompiles never panic.
- **Costs are consensus rules** — sketch values above are estimates; normative
  values land with S1.5-P11 and are SCIP-gated after mainnet freeze
  (same policy as ADR-0014 constants).
- **Implementation home** — precompiles live in `crates/strangecoin-vm-wasm/`
  (host-call path) and are also reachable from the `VmExecutor` call machinery;
  core stays runtime-free (ADR-0012).

### ABI sketches (non-normative)

- **`0x01 ecrecover`** — input: `hash (32) ‖ v (1) ‖ r (32) ‖ s (32)` = 97 bytes
  (or Ethereum-style `hash ‖ v ‖ r ‖ s` with 64-byte compact r‖s + recovery id —
  exact layout fixed in S1.5-P11); output: 32-byte address = `blake3(pubkey)[..32]`
  of the recovered secp256k1 pubkey, or 32 zero bytes on failure. Uses the same
  secp256k1 curve as EOA signatures (ADR-0001); address derivation matches
  `address_from_public_key`.
- **`0x02 sha256`** — input: arbitrary bytes; output: 32-byte SHA-256 digest.
- **`0x03 keccak`** — input: arbitrary bytes; output: 32-byte Keccak-256 digest
  (the Ethereum variant, padding `0x01`, not NIST SHA3-256).
- **`0x04 blake3`** — input: arbitrary bytes; output: 32-byte blake3 hash
  (same function as chain serialization — `serialize.rs`).
- **`0x05 ed25519_verify`** — input: `pubkey (32) ‖ signature (64) ‖ msg (…)`;
  output: 32-byte `1` on success, `0` on failure. Legacy interop only; EOAs use
  secp256k1 (ADR-0001).
- **`0x06 bls_aggregate`** — input/output: reserved (BLS12-381 aggregate verify);
  MVP returns failure with gas consumed; format fixed in a Stage 7 SCIP.
- **`0x07 modexp`** — input: Ethereum-style `base_len ‖ exp_len ‖ mod_len ‖ base ‖ exp ‖ mod`
  (three 32-byte big-endian length headers); output: `base^exp mod mod`,
  right-padded/truncated to `mod_len` bytes; `mod = 0` → empty result.

## Consequences

### Positive

- Covers the full MVP need: replay/meta-tx verification (`ecrecover`), three
  hash functions for ecosystem interop, legacy ed25519, modular arithmetic.
- Reserving `0x06` for BLS avoids a breaking address reshuffle at Stage 7 PoS.
- Precompiles keep heavy crypto out of WASM loops → predictable gas, better
  throughput, smaller attack surface for quadratic blowups inside WASM.
- Reserved-range rule keeps the address space clean for future precompiles.

### Negative

- 7 precompiles is a larger consensus surface than a minimal set; each one is
  mandatory test/fuzz territory (S1.5-P16).
- `bls_aggregate` ships as a stub — callers that ignore the NotImplemented
  failure will see call reverts until Stage 7; documented, gas still charged.
- Keccak + sha256 + blake3 (three hashes) is redundant on paper; justified by
  interop (sha256/keccak) vs native (blake3), but triple maintenance/testing.
- Ethereum-style address range is familiar but *not* semantically compatible —
  foot-gun for ported Ethereum code assuming ripemd160 at `0x02` etc.

### Neutral

- Precompile gas costs share the ADR-0014 unit system and `GasCounter`; no
  separate meter.
- Ed25519 is legacy-only (EOAs are secp256k1 per ADR-0001); the precompile
  exists for cross-chain verification use-cases, not for wallet signatures.
- Exact input/output byte layouts are deferred to S1.5-P11 (implementation);
  this ADR fixes addresses, costs, and membership.

## Alternatives

### Alternative 1: Full Ethereum precompile list

- Pros: maximal portability of Ethereum contracts; known-good set.
- Cons: includes ripemd160 (weak, unused on this chain), identity (trivial
  padding oracle surface), altbn128 pairing (huge audit cost, unused until
  zk work at Stage 6+); excludes blake3 and ed25519 we actually need.
- Why not chosen: dead weight and audit cost for features Strangecoin does not
  use; portability is served by keeping *overlapping* addresses for the shared
  primitives only where semantics match.

### Alternative 2: Minimal set (ecrecover + sha256 only)

- Pros: smallest audit surface.
- Cons: no blake3 (chain-native hash) in contracts; no keccak → Stage 4 `eth_*`
  interop painful; no ed25519; no modexp → many crypto protocols awkward.
- Why not chosen: under-serves interop and native-hash needs; adding
  precompiles later is additive, but address reservation is cheaper to do now.

### Alternative 3: Extended set (+ RSA verify, VDF verify, ZK verify)

- Pros: future-proofs exotic protocols.
- Cons: enormous audit surface; no current use-case; dependencies (RSA/VDF/ZK
  proving systems) are heavy and consensus-risky; ROADMAP defers ZK to Stage 6+.
- Why not chosen: speculative; revisit per-SCIP when a concrete protocol needs one.

## Related

- `docs/ADR/0012-wasm-wasmi-vs-risc-v.md` — VM decision (call path)
- `docs/ADR/0014-gas-metering-strategy.md` — gas unit system (same constants style)
- `docs/ADR/0001-secp256k1-vs-ed25519.md` — EOA curve (ecrecover alignment)
- `ARCHITECT3.md` §3.9 (precompiles list), §6 threat V-21
- `ROADMAP3.md` Stage 1.5 (precompiles implemented + tested)
- `analytics/prompt-stage1.5.md` S1.5-P04 (this ADR), S1.5-P11 (precompiles impl)
- SCIP process (`docs/SCIP/scip-0000-process.md`) — post-freeze cost/list changes
