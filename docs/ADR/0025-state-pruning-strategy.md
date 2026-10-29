# ADR-0025: State Pruning Strategy (Height-Window, Archival Mode)

## Status

Accepted (2026-10-30) — S3-P02

## Context

ROADMAP3 Этап 3 requires "удаление исторического state старше N блоков
(EIP-4444 analog)" plus a *future* state-revival path via Verkle witnesses,
a `state_expiry_check` hook in `state.apply_block` and, optionally, state
rent. Two forces pull in opposite directions:

- **Disk growth.** A full node that keeps every historical account snapshot
  grows without bound. ROADMAP3 DoD item 3 ("state pruning работает — disk
  usage уменьшается после N блоков") is unreachable without a retention
  policy.
- **Replay-verifiability.** `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1
  records the flip side explicitly: "State pruning / expiry — удаление
  старых state/receipts ослабляет replay-verifiability у полных нод
  (EIP-4444 analog) — Stage 3+ через SCIP (consensus-changing)". §3
  (carry-over from Stage 1.5) adds receipts prune and contract-storage
  prune to the same SCIP-gated bucket.

A third force shapes the design space: the *baseline schema stores
current-state only* (`balances` blob → `state` CF per ADR-0024). Ethereum's
EIP-4444 prunes historical **trie nodes**; Strangecoin has no persistent
historical tries at all (invariant #1: rebuild-from-chain wins; state root
is recomputed from the HashMap via `SparseMerkleTrie::compute_root`).
A literal 1:1 EIP-4444 port is therefore **inapplicable** — there are
historical account snapshots only from the moment ADR-0024's `state_history`
CF exists, and that CF is introduced *by Stage 3* precisely so that pruning
has something meaningful to delete.

Additional forces:

- **ROADMAP3 risk: "State pruning breaks light clients."** A node that
  cannot answer historical queries is useless as an index backend; the
  escape hatch must be a first-class node mode, not an operator hack.
- **Consensus boundary.** Storage is an implementation detail (ADR-0024
  Neutral): pruning must not change validation semantics. Anything that
  *does* change consensus (removing dormant accounts from **current**
  state, so that revival needs wire witnesses) is SCIP-gated before
  mainnet freeze — the rail must exist now (`docs/stage1.5/` residuals,
  Stage 2 residual risks above).
- **Determinism (V-21 extension, Stage 3 checklist).** Retention and sweep
  triggers must be pure height arithmetic — an injected tip height, never
  wall clock. Two nodes at the same height with the same config must prune
  identically; a clock-based TTL would make pruning order-dependent on
  system time (threat V-21: non-determinism → divergent node behavior).

## Decision

**We will prune implementation-level historical data — the `state_history`
column family — by a deterministic height window on full nodes, keep
archival nodes fully history-preserving as a config-selectable node mode,
never touch consensus-necessary current state, blocks, or the chain tip,
and gate everything that would alter consensus (state expiry, receipts
prune, contract-storage prune) behind SCIP, limiting Stage 3 to
marker-preparation for expiry research.**

| Aspect | Decision |
|---|---|
| **Full node (default)** | Prunes `state_history` records older than `pruning.retention_blocks` (**default 5000**, `config.toml`); blocks / state / transactions / contract_storage / mempool / meta are **never** pruned |
| **Sweep trigger** | By **height**: every `pruning.sweep_every_blocks` (**default 100**) blocks of chain progress; **never** wall clock (V-21 determinism) |
| **Archival node** | `node.mode = "archival"` in config: `state_history` is kept in full; the node answers deep historical queries (future Stage 4 indexer); disk growth is an accepted operator choice |
| **Never pruned** | Current `state` CF (consensus state), `blocks` CF (replay chain, invariant #1), `meta` tip / schema version; also: `transactions`, `contract_storage`, `mempool` |
| **State expiry** | Dormant-account removal from **current** state is **consensus-changing** (revival requires witness semantics on the wire → Verkle SCIP, ADR-0006 amendment). Stage 3 ships only a **touch-tracking marker** in `state_history` (data for a future SCIP); it **does not** delete current-state records |
| **Receipts prune / contract-storage prune** | Remain SCIP-gated residuals (Stage 1.5 / Stage 2 carry-over, `RESIDUAL_RISKS_STAGE3_PLUS.md` §3); **not** Stage 3 |
| **Config surface (sketch)** | `[pruning] enabled = true, retention_blocks = 5000, sweep_every_blocks = 100` + `[node] mode = "full" \| "archival"` |
| **Mechanics (S3-P12)** | Sweep = pure height arithmetic over the `state_history` key prefix (`height: u64 BE ‖ address`): delete every key with `height ≤ tip_height − retention_blocks`, via `StorageEngine::write_batch`; `retention_blocks = 0` or `enabled = false` = no-op; archival mode bypasses the sweep entirely |
| **What history even exists** | Only what ADR-0024 writes: pre-block `AccountState` snapshots on adopt. Nodes syncing via snapshot import (ADR-0026) start their history at the snapshot height — retention applies relative to their tip, not to genesis |

### Prune / never-prune exhaustive list

| Data | CF / location | Pruned on full node? | Rationale |
|---|---|---|---|
| Historical account snapshots | `state_history` | **Yes** — older than `retention_blocks` | Implementation-level history; restorable by re-execution from `blocks` |
| Touch-tracking expiry markers | `state_history` (marker sub-space, S3-P12) | **Yes** — same window | Research data for the future expiry SCIP, not consensus |
| Current account state | `state` | **No** | Consensus state; pruning it = state expiry = SCIP |
| Blocks | `blocks` | **No** | Replay chain; invariant #1 depends on it |
| Transactions index | `transactions` | **No** | O(1) txid→block lookups; small relative to history |
| Contract storage slots | `contract_storage` | **No** | Current consensus state of contracts |
| Mempool | `mempool` | **No** (already deleted on mine) | Lifecycle closed in S3-P07/ADR-0024 |
| Tip, schema version, chain id, difficulty | `meta` | **No** | Node identity / bootstrap |
| Receipts | *derived, not persisted* | N/A in Stage 3 | Receipts prune = SCIP residual when/if receipts become persisted |

## Consequences

### Positive

- **Disk usage is bounded on full nodes** — `state_history` is the only
  unbounded implementation-level growth vector in the ADR-0024 schema;
  the height window caps it at `retention_blocks × accounts-touched`.
  ROADMAP3 DoD 3 becomes testable (S3-P12 `tests/pruning.rs`).
- **Archival mode preserves auditability** — the ROADMAP3 light-client /
  indexer risk ("pruning breaks light clients") is answered by a
  first-class `node.mode`, not an ops workaround; Stage 4 indexer has a
  designated backend.
- **No consensus change.** Pruning deletes only data the engine can
  re-derive from `blocks` + re-execution; validation results are
  identical on pruned and archival nodes (invariant #1 preserved). No
  SCIP needed for the mechanism itself.
- **Deterministic by construction** — height-window arithmetic and
  height-based triggers satisfy the Stage 3 determinism checklist
  (V-21 extension): same tip + same config → same sweep result on every
  node; no clock, no RNG.
- **CF isolation does the heavy lifting** — RocksDB per-CF compaction
  (ADR-0024) means the mass-delete of a sweep reclaims disk through
  ordinary compaction without write-amplifying consensus CFs.

### Negative

- **A pruned full node cannot answer deep historical queries** —
  `state_history` lookups beyond the window return "absent". Accepted:
  such queries are served by archival nodes (Stage 4 indexer design).
- **Replay-verifiability is weakened for *history*, not for *verification***
  — the chain is still fully verifiable from `blocks` (re-execution), but
  a pruned node cannot cheaply *prove* past balances without re-deriving
  them. This is the EIP-4444 trade-off, recorded in
  `RESIDUAL_RISKS_STAGE3_PLUS.md` §1 and accepted here.
- **`retention_blocks` has no universal optimum** — 5000 is a reasonable
  default for the current chain pace; it is a tuning knob. Wrong values
  either waste disk (too large) or starve tooling (too small). Mitigation:
  config surface, documented in S3-P12.
- **Partial history on snapshot-bootstrapped nodes** — a node that
  imported a snapshot at height H has no `state_history` below H; its
  "window" is effectively min(retention, tip − H). Accepted: history
  below a snapshot is available from archival peers / re-execution.

### Neutral

- **Pruning is an implementation detail**, not consensus: no activation
  height, no SCIP for the sweep itself. Consensus-relevant follow-ups
  (state expiry, receipts prune) are declared SCIP-gated *now* so the
  rail exists before mainnet freeze (process rule 9, Stage 3).
- **The expiry marker is data, not behavior** — Stage 3 records
  touch-tracking information in `state_history` so a future SCIP has
  real telemetry to design against; `state.apply_block` semantics are
  unchanged (no `state_expiry_check` enforcement in Stage 3).
- **Interaction with snapshots (ADR-0026)** — snapshot export on an
  archival node captures current state only (not `state_history`); the
  two features are orthogonal: snapshots accelerate bootstrap, pruning
  bounds steady-state disk.

## Alternatives Considered

### Alternative 1: State expiry now (dormant-account removal from current state)

- Pros: attacks the *consensus* state growth, not just history; the true
  long-term fix; ROADMAP3 lists `state_expiry_check` in `state.apply_block`.
- Cons: consensus-breaking — revival of an expired account requires
  witness/proof semantics on the wire (exactly the Verkle/stateless
  machinery of ADR-0006, SCIP-gated); without that, expiry is an
  irreversible fund-destruction bug. Mainnet is not frozen yet, but the
  change is declared SCIP-relevant and cannot ship as a storage detail.
- Why not chosen: out of Stage 3 scope per ROADMAP3 risks and residual
  §1; Stage 3 prepares (markers only), SCIP decides later.

### Alternative 2: Receipts prune (and/or contract-storage prune) instead

- Pros: receipts are pure derived data with a smaller footprint; pruning
  them would relieve some disk pressure.
- Cons: does not address `state_history` growth at all (the actual
  unbounded vector in ADR-0024); receipts are not even persisted in
  Stage 3 (derived, invariant #1) — there is nothing to prune; contract-
  storage prune is consensus-adjacent (contract's storage is live state).
  Both are carry-over SCIP residuals (`RESIDUAL_RISKS_STAGE3_PLUS.md` §3).
- Why not chosen: wrong target for the Stage 3 DoD, and consensus-gated
  where it would matter.

### Alternative 3: No pruning (archival-only network)

- Pros: simplest possible implementation (zero code); maximum auditability;
  no replay-verifiability debate.
- Cons: ROADMAP3 DoD 3 ("disk usage уменьшается после N блоков") is
  unreachable — Stage 3 fails its own gate; full-node ops cost grows
  linearly with chain length, which is the problem Stage 3 exists to fix.
- Why not chosen: DoD failure; unbounded operator cost.

### Alternative 4: Wall-clock TTL (delete history older than T days)

- Pros: intuitive operator knob ("keep 30 days"); self-tuning under
  varying chain pace.
- Cons: **anti-deterministic** — nodes with different clocks or different
  uptime prune different key sets at the same height (V-21: same input →
  same output is violated); wall-clock in the storage hot path is
  forbidden by the Stage 3 determinism checklist (no `SystemTime::now()`
  in pruning paths). It also couples a consensus-adjacent operation to
  NTP quality.
- Why not chosen: determinism is a hard requirement; height is the only
  admissible time-like parameter (Stage 2 `tick_at` precedent).

### Alternative 5: Literal EIP-4444 (prune historical trie nodes)

- Pros: matches the Ethereum roadmap vocabulary; would be the right
  primitive *if* Strangecoin persisted historical tries.
- Cons: inapplicable 1:1 — the baseline schema stores current-state only;
  historical tries do not exist (invariant #1: state root is recomputed
  from the HashMap; persistent SMT is a recorded residual of ADR-0024).
  Pruning "trie nodes" would be a no-op or require first shipping the
  persistent-trie Alternative rejected in ADR-0024 §Alternative 3.
- Why not chosen: the codebase has no artifact this alternative could
  delete; the ADR-0024 `state_history` CF is the Strangecoin-shaped
  substrate that makes a height-window analog meaningful at all.

## Related

- `docs/ADR/0024-storage-schema-and-engine.md` — introduces the
  `state_history` CF this ADR prunes; its §Consequences Negative already
  defers "state_history grows with chain length" to this decision
- `docs/ADR/0006-verkle-trie-vs-smt.md` — the SCIP-gated substrate any
  future state-expiry revival semantics would build on
- `docs/ADR/0008-rocksdb-plan.md` — engine decision; pruning rides
  RocksDB CF isolation and compaction
- `docs/stage2/RESIDUAL_RISKS_STAGE3_PLUS.md` §1 (state pruning/expiry
  → SCIP, replay-verifiability trade-off) and §3 (receipts prune,
  contract-storage prune, state rent — SCIP carry-overs)
- `docs/stage1.5/RESIDUAL_RISKS_STAGE2_PLUS.md` — original receipts/
  contract-storage prune residuals
- ROADMAP3 «Этап 3» — pruning task, DoD 3, light-client risk
- ARCHITECT3 §3.7 (storage/pruning/snapshot), §3.4 (state_cache)
- Invariant #1 (rebuild-from-chain) — `docs/stage2/INVARIANTS_ENFORCED.md`;
  preserved because only re-derivable history is deleted
- Stage 3 prompts: S3-P12 (height-window sweep + archival mode + expiry
  markers — the implementation of this ADR); S3-P13 (snapshots,
  orthogonal); S3-P14 (invariants/threat-model Stage 3 — carries this
  trade-off into the Stage 3 residual list)
- Stage 3 determinism checklist (AGENTS.md) — height-based triggers,
  no wall clock / no RNG in the sweep path
