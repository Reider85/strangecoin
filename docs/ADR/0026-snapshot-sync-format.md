# ADR-0026: Snapshot Sync Format (Commitment-Verified, File-Based)

## Status

Accepted (2026-10-30) — S3-P03

## Context

ROADMAP3 Этап 3 requires "быстрая синхронизация новых узлов без скачивания
всей цепочки", a snapshot file format, and snapshot generation every N
blocks. Today a new node bootstraps via headers-first sync (Stage 1) and
re-executes **every** block — O(chain length) in time and bandwidth. That
cost grows linearly forever; at some chain length it becomes the dominant
barrier to network growth.

Snapshot sync is the standard answer — but it trades *verification* for
*speed*, and Stage 2 already priced that trade:
`docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1 records "Snapshot sync —
узел доверяет снимку → новые векторы (**forged snapshot, snapshot
withholding, split-brain** между snapshot- и replay-нодами); требует
отдельного threat-моделинга + верификации snapshot-**commitment** (state
root + height) перед применением". Any format designed here must close
the forged-snapshot vector by construction.

Baseline facts the format builds on:

- **State root** is computed from the account HashMap via
  `SparseMerkleTrie::compute_root` (`crates/strangecoin-core/src/state/sparse_merkle.rs`);
  the same function is the consensus oracle for every block header's
  `state_root` field. A snapshot can therefore *prove* its contents
  against the header chain without any new cryptography.
- **Canonical binary serialization** (`serialize.rs`, blake3,
  `FORMAT_VERSION = 7`) is the wire/disk encoding for consensus types;
  serde_json is tooling-only (AGENTS.md Key Constraints).
- **Block headers** carry `state_root` and `receipts_root`
  (`types.rs`; receipts root per ADR-0016) — all four commitment fields
  a snapshot header needs already exist in the chain.
- **Network id** is a `u32` (`consensus.rs`); mainnet/regtest/devnet
  separation is a config-level fact the snapshot must respect.
- **Naming disambiguation.** The existing `ChainSnapshot` struct
  (`types.rs`) is an *in-memory* sync/GUI channel type (chain +
  balances + difficulty), not a file format. The on-disk format defined
  here is unrelated; reusing the name for the file format is rejected.
- **Archival nodes** (ADR-0025) are the natural snapshot producers:
  they are the only node mode guaranteed to have deep history and an
  operator interest in serving bootstrap material.

Forces in tension: bootstrap speed vs. trust; single-file simplicity vs.
evolution room; scope discipline (a P2P message would ripple through
`strangecoin-net`) vs. operator ergonomics.

## Decision

**We will define a single-file, canonical-binary snapshot format with a
commitment header, lexicographically sorted body, and a whole-file blake3
trailer; enforce a four-step verification (checksum, chain id, state-root
recomputation, post-catch-up block-hash match) before an imported node
treats the snapshot as trusted; generate snapshots by height cadence on
archival nodes or by explicit operator command; and ship Stage 3
transport as CLI file-based only, deferring any P2P wire transport to a
Stage 4+ residual.**

| Aspect | Decision |
|---|---|
| **File name** | `snapshot-{height}-{state_root_hex}.scsnap` (height decimal, state root lowercase hex); self-describing for ops tooling |
| **Header** | magic `SCSNAP01` (8 bytes ASCII), `format_version` (u8, starts at 1), `network_id` (u32 BE), `height` (u64 BE), `block_hash` ([u8; 32]), `state_root` ([u8; 32]), `receipts_root` ([u8; 32]) |
| **Body — accounts** | one record per account: address (canonical bytes) ‖ canonical `AccountState` encoding; **sorted lexicographically by address** |
| **Body — contract storage** | one record per slot: address ‖ slot key ‖ value bytes; **sorted by (address, slot)** |
| **Body encoding** | **canonical binary serialization from `strangecoin-core`** (blake3 ecosystem, `FORMAT_VERSION`-aligned); serde_json is allowed **only** as an optional sidecar manifest for tooling, never as the body |
| **Trailer** | blake3 checksum ([u8; 32]) over header ‖ body (whole-file, position-independent) |
| **Verification before apply** | **(a)** recompute whole-file blake3 == trailer; **(b)** `network_id` matches local node config; **(c)** recompute state root from body accounts via `SparseMerkleTrie::compute_root` == header `state_root` — mismatch → **reject** (forged-snapshot mitigation); **(d)** after catch-up, `block_hash` at `height` in the local header chain must equal header `block_hash`. Node is **trusted only after (c) + (d)** |
| **Generation** | every `snapshot.every_blocks` (default **1000**) blocks of tip progress, **height-triggered** (Stage 3 determinism checklist — no wall clock); default producer = **archival** nodes (ADR-0025); also on explicit operator command (`sc snapshot export`) on any node mode |
| **Generator determinism** | sorted body order; no RNG, no `SystemTime` in file content; two exports of the same state at the same height produce **byte-identical** header+body (trailer included) |
| **Transport (Stage 3)** | **file-based only**: `sc snapshot export` / `sc snapshot import` (S3-P13), moved by scp/https/removable media — ops responsibility |
| **Transport (residual)** | P2P `GET_SNAPSHOT` message — Stage 4+ residual (would require new wire message, rate-limit class, capability gating, Erlay/compact interaction in `strangecoin-net`) |
| **Application** | import → run verification (a)–(c) → write `state` CF + `contract_storage` CF + `meta` tip = `height` → resume headers-first sync (Stage 1) to current tip → trusted only after (d). No block bodies below `height` are needed to *apply*; they remain needed only if the operator later wants deep replay |
| **Interaction with pruning** | snapshots capture **current state only**, not `state_history` (ADR-0025); a snapshot-bootstrapped node's history window starts at the import height |
| **Format evolution** | `format_version` byte gates parsers: unknown version → reject (fail-closed, same rail as ADR-0024 schema versioning); additive fields require a version bump |

### Threat mitigations (Stage 2 residual §1)

| Threat | Mitigation in this format |
|---|---|
| **Forged snapshot** (attacker supplies state that never existed) | Step **(c)**: state root is *recomputed* from body contents and must equal the header commitment, which is later pinned to the canonical chain by step **(d)**. A forger would need a preimage for a state root already fixed in the public header chain — computationally infeasible under blake3/SMT assumptions |
| **Snapshot withholding** (no honest snapshot available) | *Not fully solvable at the format layer*: a node without a file falls back to full replay (correctness preserved, speed lost). Documented as an availability residual; addressed operationally by multiple independent snapshot mirrors (Stage 4 ops guidance), not by protocol in Stage 3 |
| **Split-brain** (snapshot disagrees with replay nodes) | Step **(d)**: the imported node re-joins the header chain and validates `block_hash` at `height`; divergence is detected at catch-up, same as any bad-tip scenario. Nodes that applied different valid snapshots converge on the same tip via fork choice |
| **Wrong-network snapshot** (regtest file into mainnet node) | Step **(b)**: `network_id` is checked before any write |
| **Truncated / corrupted file** | Step **(a)**: whole-file blake3 trailer; plus explicit length framing in the body records |
| **Format downgrade / parser confusion** | `format_version` fail-closed gate; magic bytes checked first |

## Consequences

### Positive

- **Bootstrap cost drops from O(chain) to O(state)** — a new node
  downloads one file proportional to live state and catches up only the
  blocks since the snapshot; ROADMAP3 "быстрая синхронизация" DoD becomes
  testable (S3-P13 `tests/snapshot_sync.rs`).
- **Forged-snapshot vector is closed by construction** — commitment
  verification (steps a–d) directly implements the Stage 2 residual §1
  requirement; no node trusts an unverified file.
- **Reuses existing consensus oracles** — `SparseMerkleTrie::compute_root`,
  header `state_root`/`receipts_root`, canonical serialization: no new
  crypto, no new trust anchors, no SCIP.
- **Deterministic exports** — sorted body + clock-free content satisfy
  the Stage 3 determinism checklist; reproducible snapshots simplify
  distribution verification (mirror operators can compare blake3
  trailers out-of-band).
- **Clean division of labor with ADR-0025** — archival nodes produce
  snapshots; full nodes consume them; pruning (ADR-0025) never deletes
  the data a snapshot is made of (current `state` CF is never pruned).
- **Fail-closed evolution** — `format_version` mirrors the ADR-0024
  schema-versioning rail; parsers reject what they do not understand.

### Negative

- **Snapshot withholding remains an availability risk** — a network
  with no reachable snapshot forces new nodes into full replay.
  Accepted: correctness is unaffected; mitigation is operational
  (mirrors), residual until a wire transport exists.
- **File-based transport is ops-heavy** — operators must move files
  securely and verify out-of-band if they want belt-and-suspenders.
  Accepted for Stage 3 scope; CLI ergonomics (S3-P13) is the mitigation.
- **Split-brain windows until step (d)** — between import and catch-up
  the node acts on snapshot-derived state that is *probabilistically*
  correct (committed but not yet re-pinned by its own header chain).
  Accepted: the node is not "trusted" until (d), and fork choice
  resolves any divergence at catch-up.
- **Receipts/state-history are not in the snapshot** — a
  snapshot-bootstrapped node cannot answer historical queries below the
  import height (no `state_history`) and has no receipts for
  pre-snapshot blocks (receipts are derived, invariant #1). Accepted:
  historical queries are archival-node territory (ADR-0025/Stage 4
  indexer); receipts re-derive from blocks on demand.

### Neutral

- **No consensus change** — storage/sync is an implementation detail;
  a snapshot node and a replay node validate identical headers by the
  same rules (invariant #1 preserved: current state remains
  rebuild-capable from `blocks` for nodes that have them). No SCIP, no
  activation height.
- **Wire GET_SNAPSHOT is explicitly out of Stage 3** — recorded as a
  Stage 4+ residual so the format does not preclude it: the same file
  bytes can later be chunked over a new P2P message without format
  change.
- **`ChainSnapshot` (types.rs) keeps its meaning** — in-memory sync/GUI
  channel; no rename required, no coupling introduced.

## Alternatives Considered

### Alternative 1: Wire snapshot sync via P2P (`GET_SNAPSHOT` message)

- Pros: first-class protocol feature; no ops file-moving; could reuse
  Noise transport, peer scoring and rate limiting from Stage 2.
- Cons: scope explosion for Stage 3 — new wire message in
  `strangecoin-net`, new rate-limit class (ADR-0023), capability
  negotiation (HELLO), interaction with Erlay/compact-block paths, and
  a serving-side DoS surface (any peer can demand a multi-GB dump). None
  of it is needed to prove the *format* and *verification* correct.
- Why not chosen: Stage 3 delivers format + CLI + verification (S3-P13);
  wire transport is a Stage 4+ residual. The file format was designed
  to be transport-agnostic so this alternative remains open.

### Alternative 2: State-only dump without commitment verification

- Pros: smallest implementation — serialize `state` CF to a file,
  import it, done.
- Cons: exactly the forged-snapshot vulnerability flagged in Stage 2
  residual §1: any file, honestly labeled or maliciously crafted, would
  be accepted; a single hostile mirror could poison every new node.
- Why not chosen: недопустимо — verification is the point of the ADR;
  speed without integrity is a net loss for a consensus network.

### Alternative 3: Full dump (blocks + state in one snapshot)

- Pros: a node importing a full dump is instantly at the tip with full
  replay-verifiability and complete history; no catch-up phase.
- Cons: not "быстрая синхронизация" — the file is O(chain), the
  dominant cost Stage 3 is trying to eliminate; duplicates data the
  P2P network already distributes efficiently (gossip/compact blocks,
  headers-first sync).
- Why not chosen: orthogonal to the goal; snapshot is a *state*
  bootstrap, chain history rides the normal sync path.

### Alternative 4: Directory-based multi-file layout as the canonical format

- Pros: parallel-friendly (accounts shard files), partial retry without
  re-downloading everything, inspectable with standard tools.
- Cons: whole-file integrity (step a) becomes a manifest-of-checksums
  problem instead of one blake3 trailer; transport, versioning and
  atomic import all get harder (partial directory = ambiguous state);
  single `.scsnap` is trivially atomic (file appears or does not).
- Why not chosen: single file is the canonical artifact; an operator may
  *extract* a snapshot into a directory for inspection, but extraction
  is not the format. Revisit with the wire transport (Alternative 1),
  where chunking may justify an internal shard layout.

### Alternative 5: Embed state rent / expiry data in the snapshot

- Pros: would future-proof bootstrap for a post-expiry consensus regime.
- Cons: state rent and expiry are SCIP-gated consensus features
  (`prompt-stage3.md` §6 anti-goals; ADR-0025); embedding them now
  would force the snapshot format to freeze semantics that consensus has
  not decided, and would couple a storage detail to a pending SCIP.
- Why not chosen: the format carries current state only; a future SCIP
  that changes account semantics will bump `format_version` through the
  normal fail-closed rail.

## Related

- `docs/ADR/0024-storage-schema-and-engine.md` — `state` /
  `contract_storage` / `meta` CFs the import writes; multi-CF backup
  gap that snapshots complement ("portable state export")
- `docs/ADR/0025-state-pruning-strategy.md` — archival nodes are the
  default snapshot producers; snapshot-bootstrapped nodes start their
  `state_history` window at the import height; current `state` CF is
  never pruned, so snapshot source data is safe on any node mode
- `docs/ADR/0016-receipts-root-in-block-header.md` — `receipts_root` in
  the snapshot header rides the same commitment field
- `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1 — forged snapshot /
  withholding / split-brain (mitigations table above)
- Invariant #1 (rebuild-from-chain) — `docs/stage2/INVARIANTS_ENFORCED.md`
- `crates/strangecoin-core/src/serialize.rs` — canonical binary
  serialization, `FORMAT_VERSION` (body encoding source)
- `crates/strangecoin-core/src/state/sparse_merkle.rs` —
  `compute_root` (step c recomputation oracle)
- ARCHITECT3 §3.7 (storage/pruning/snapshot), §3.4 (state_cache)
- ROADMAP3 «Этап 3» — snapshot task + DoD 4
- Stage 3 prompts: S3-P13 (implementation: `sc snapshot export`/
  `import`, commitment verification, `tests/snapshot_sync.rs`);
  S3-P12 (pruning — interaction above); S3-P14 (threat model carries
  this table into `docs/stage3/`); S3-P02/S3-P01 (ADR lineage)
- Stage 3 determinism checklist (AGENTS.md) — generation cadence by
  height; sorted, clock-free, RNG-free file content
