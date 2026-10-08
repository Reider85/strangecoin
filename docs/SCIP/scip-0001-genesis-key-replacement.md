---
scip: 1
title: Genesis key replacement
status: Draft
consensus_version: TBD (mainnet genesis)
activation_height: TBD — mainnet freeze gate (before mainnet block 1)
author: strangecoin team
discussions: https://github.com/anomalyco/strangecoin/issues
created: 2026-10-08
---

# SCIP-0001: Genesis key replacement

## Abstract

The testnet genesis allocation (1,000,000,000 SC to the initial holder) is
currently controlled by a private key derived from the **public** seed string
`"strangecoin-genesis-seed-2026"` (SHA-256 → secp256k1). Anyone who reads the
repository history can recover that key. This SCIP declares the key **burned**
for any public network and defines the mandatory offline-key replacement gate
before mainnet freeze (BUG-S0-015 / S1.5-P01).

## Motivation

- **Critical severity (BUG-S0-015):** a publicly derivable key controls the
  genesis allocation. Mainnet launch with this key would be instantly drained.
- Stage 0/1 docs recorded the compromise as a residual risk but without a
  concrete replacement gate or SCIP.
- Code must never ship a genesis secret derivation path again.

## Current state (post S1.5-P01)

| Artifact | State |
|----------|--------|
| `src/consensus/mod.rs` | `genesis_keypair()` **removed**; no seed string in code |
| `genesis.json` | `initial_holder_pubkey` only (33-byte compressed pubkey hex) |
| Testnet chain | Continues with the burned key's pubkey — testnet/regtest value only |
| `EXPECTED_GENESIS_HASH` | Still pins the testnet genesis built from the burned pubkey |
| Regression guard | `tests/genesis_key.rs` fails if the seed string reappears in consensus source |

## Specification

### 1. Key handling rules (all networks)

1. Genesis private keys are **generated offline** (air-gapped machine or HSM).
2. Only the **compressed public key** (33 bytes, hex) is written to
   `genesis.json` as `initial_holder_pubkey`.
3. The private key is stored offline (paper/HSM). It is **never** committed to
   the repository, never embedded in binaries, never logged.
4. The node code path for genesis validation uses the pubkey only
   (`load_genesis` → `encode_address` → coinbase tx). No code path may derive
   or import a genesis secret.

### 2. Burned key

The keypair derived from `"strangecoin-genesis-seed-2026"` is **BURNED**:

- It must not receive real value on any public network.
- It may continue to exist on historical testnet data; that chain is
  disposable.
- At mainnet freeze this seed is treated as fully public forever.

### 3. Mainnet freeze gate (activation)

**Before mainnet block 1:**

1. Generate a fresh secp256k1 key offline.
2. Compute the compressed pubkey hex; write **only** the pubkey to
   `genesis.json` (`initial_holder_pubkey`, `network_id = 1`, `chain_id = 1`).
3. Rebuild the genesis block; update `EXPECTED_GENESIS_HASH` in
   `crates/strangecoin-core/src/consensus.rs` to the new hash.
4. Update any pinned golden vectors / DoD evidence that reference the old hash.
5. Mark this SCIP `Finalized` once the mainnet genesis is signed off.
6. The current testnet `genesis.json` (network_id=1 with the burned pubkey)
   **must not** be used as mainnet genesis until step 2–3 are done.

`activation_height` is therefore not a mid-chain hard fork: genesis key
replacement happens at **genesis creation** for the new public network. For
any future testnet reset the same offline procedure applies.

### 4. Offline generation procedure (operators)

```text
# On an air-gapped machine (example; any secp256k1 tool is fine):
# 1. Generate key
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:secp256k1 -out genesis.key
# 2. Export compressed pubkey only
openssl ec -in genesis.key -pubout -conv_form compressed -outform DER | tail -c 33 | xxd -p -c 33
# 3. Store genesis.key offline (HSM/paper). Never copy it to the repo host.
# 4. On the build host: put the 33-byte hex pubkey into genesis.json
#    as initial_holder_pubkey (0x-prefixed).
```

Alternative: a one-off Rust example or hardware wallet export may be used; the
rule is the same — **pubkey in git, secret offline**.

## Rationale

- A burned public seed cannot be un-burned; documenting the gate converts an
  undefined residual into an enforceable release checklist item.
- Genesis-key change is not an activation-height fork on an existing chain; it
  is a genesis parameter change for a network that does not yet exist.
- Removing the derivation path from code guarantees the node binary never
  contains a recoverable genesis secret, regardless of operator discipline.

## Backward Compatibility

- **Testnet (current):** genesis hash unchanged; only the JSON field was
  renamed (`initial_holder` → `initial_holder_pubkey`). Nodes that load the
  updated `genesis.json` produce the identical block and hash.
- **Mainnet:** will not launch until SCIP-0001 steps 3.1–3.4 are complete.
  Direct consensus edits before mainnet freeze are allowed per
  `docs/SCIP/README.md`; this change is logged in `Changelog.md`.

## Reference Implementation

- `src/consensus/mod.rs` — `GenesisConfig`, `load_genesis`, `validate_genesis`
  (no private key API)
- `genesis.json` — `initial_holder_pubkey`
- `tests/genesis_key.rs` — node validates `EXPECTED_GENESIS_HASH` without any
  secret in code; seed-string regression guard
- `docs/security/THREAT_MODEL.md` — V-31 residual risk
- `analytics/bugfixes-stage0.md` — BUG-S0-015
