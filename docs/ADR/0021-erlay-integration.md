# ADR-0021: Erlay Integration for Transaction Relay

## Status

Accepted (2026-10-24) — S2-P02

## Context

Strangecoin's Stage 1 gossip relays every new transaction to every peer
(full relay, Bitcoin pre-Erlay style). Each tx therefore costs O(N)
bandwidth per hop — on a 10k-node network a single transaction
announcement fanout becomes operationally untenable. ARCHITECT3 §3.6
mandates «Erlay (Stage 2)» in `gossip.rs`; ROADMAP3 Этап 2 DoD requires
«Erlay работает (test: peer reconciliation успешна)» with ≥80% bandwidth
reduction vs naive broadcast.

Forces at play:

- **Bandwidth scaling (10k+ узлов)** — full relay is O(N) per tx; the
  network target is O(log N)-class scaling via reconciliation-based
  relay. Without Erlay, Stage 2 production-grade P2P at 10k+ nodes is
  not viable.
- **No tx loss** — ROADMAP3 Этап 2 risk: «Erlay reconciliation может
  приводить к потере txs». Any reconciliation failure path must degrade
  to a protocol that provably delivers the missing set (full relay).
- **Complementary to compact blocks** — BIP 152 (S2-P14) reduces
  *block* broadcast bandwidth; it does nothing for *tx* relay, since
  txs are still fully relayed until inclusion in a block. Erlay and
  compact blocks solve different halves of the relay problem and must
  coexist.
- **Dependency on mempool-aware gossip** — reconciliation operates on
  known-tx sets fed by the INV/GET_DATA gossip layer (S2-P13) and the
  compact-block-aware mempool (S2-P14). Erlay is gated behind both
  (Stage 2 Dependency Map, gate 6).
- **Determinism (V-21 extension)** — sketch construction and difference
  decoding must be deterministic: fixed element mapping, fixed
  capacity-selection rule, no `SystemTime`/`Instant`/RNG in the
  reconciliation hot path (Stage 2 Determinism Checklist; prompt-stage2
  §6.7: «Erlay sketches — deterministic PinSketch encoding»).
- **Battle-tested primitives** — code-based set reconciliation (PinSketch
  / libminisketch, Bitcoin Core's Erlay paper «Erlay: Efficient
  Transaction Relay for Bitcoin», 2019; BIP-330 draft) is the reference
  design. ARCHITECT3 §15 anti-goal rules out inventing our own
  reconciliation codes.

## Decision

**We will implement Erlay reconciliation-based relay — mincut protocol
with libminisketch-style PinSketch sketches — with a mandatory fallback
to full relay whenever sketch reconciliation fails or is unavailable.**

| Aspect | Decision |
|---|---|
| Protocol | Erlay mincut reconciliation (Erlay paper 2019 / BIP-330 draft), PinSketch code-based set reconciliation |
| Sketch field | GF(2^64) — 64-bit field elements; capacity up to ~10k txs per reconciliation round |
| TxId → element mapping | 32-byte blake3 TxId mapped deterministically into field elements (fixed, documented encoding; no hashing with per-run salt) |
| Reconcile trigger | Every `reconcile_interval = 60s` **or** after `M = 1000` new local txs, whichever comes first; both tunable in `config.toml` |
| Sketch size | Adaptive capacity: initial estimate from known-set size delta vs peer's last advertised set size; capacity grows on repeated decode failure (bounded by `MAX_SKETCH_CAPACITY`) |
| Difference decode | Local sketch XOR remote sketch → minisketch decode → set of missing TxIds → fetch only delta via GET_DATA (S2-P13) |
| Fallback policy | Sketch decode failure → request full tx list via INV/GET_DATA → broadcast missed txs; log mismatch rate per peer (see Fallback policy below) |
| Capability gate | Reconciliation runs only with peers advertising `Capability::Erlay = 0x04` (S2-P10 HELLO negotiation); non-Erlay peers keep full INV relay |
| Announce path | New txs are still announced via INV fanout (S2-P13); Erlay replaces the *bulk set sync*, not the low-latency announce path |
| Dependency | `minisketch-rs` (Rust bindings over C++ libminisketch via cc-rs), **pinned exact** per Stage 2 Pin Policy; exact version is a placeholder here — fixed in S2-P15 |
| Fallback dependency | If `minisketch-rs` cannot build on a platform (Windows MSBuild requirement), S2-P15 may ship a documented stub with SCIP-gated full impl (explicitly allowed by S2-P15) |

### Sketch reconciliation protocol sketch

```
Peer A                                                 Peer B
──────                                                 ──────
known_tx_set_A ── create_sketch(set, capacity) ──▶
                                                       known_tx_set_B
   ◀── sketch_B (or sketch_B sent proactively) ──  create_sketch(set, capacity)

sketch_A XOR sketch_B ──▶ minisketch decode
   │
   ├─ success ─▶ delta = decoded set difference
   │             GET_DATA(missing TxIds) ───────────▶
   │             ◀────────────────────── TX (only delta)
   │             apply delta to local known_tx_set / mempool
   │
   └─ failure (capacity exceeded / undecodable) ──▶ FALLBACK
```

1. Each peer pair maintains `PeerReconciliation` state: last known set
   size, last sketch, configured interval, mismatch counter.
2. On trigger (interval elapsed or M new txs): build sketch of local
   known-tx set with adaptive capacity; exchange sketches over the
   noise-encrypted channel (ADR-0020, S2-P08).
3. Both sides XOR their sketches and decode the difference. Decoding is
   symmetric and deterministic — same two sets + same capacity → same
   decoded delta on both sides.
4. The side that decoded missing TxIds requests them via GET_DATA
   (S2-P13) and integrates the responses into its mempool. Only the
   delta crosses the wire — this is where the ≥80% bandwidth reduction
   comes from (e.g., two peers with 1000 txs each, 800 shared → only
   ~200 txs transmitted).
5. Sketch capacity for the next round adapts: successful round → shrink
   toward observed difference; failed round → grow (bounded).

### Fallback policy (mandatory)

Per ROADMAP3 risk mitigation, reconciliation failure must never silently
drop txs:

- **Decode failure** (capacity exceeded, malformed sketch, timeout):
  fall back to full relay for this round — request the peer's complete
  recent-tx inventory via INV/GET_DATA (S2-P13) and broadcast any
  missed txs. Temporary bandwidth cost is accepted; correctness wins.
- **Mismatch-rate monitoring**: each peer pair keeps a mismatch counter
  and rate. Sustained high mismatch rate is logged via `tracing`
  (target `net::erlay`) and surfaced to peer scoring (ADR-0022) as a
  quality signal — a peer that consistently forces full-relay fallback
  is a bandwidth liability.
- **Non-Erlay peers**: no reconciliation state at all; they remain on
  the S2-P13 full-relay path. Capability negotiation (S2-P10) decides
  per-peer which path applies.
- **No cross-round state carryover**: a failed round does not poison
  the next round's known-set; sketches are rebuilt from current state.

### Determinism requirements

Per Stage 2 Determinism Checklist (threat V-21 extension):

1. No `SystemTime::now()` / `Instant::now()` in sketch construction or
   decode. The reconcile *trigger* may use a timer, but the produced
   sketches and decoded deltas must be pure functions of (known-tx set,
   capacity).
2. No `OsRng` / `thread_rng` anywhere in the reconciliation path —
   minisketch encoding is deterministic by construction (fixed
   generator polynomial, fixed field).
3. TxId → field-element mapping is fixed and documented; sorting of
   decoded TxId sets is by lexicographic order before transmission so
   both peers observe identical wire order.
4. `cargo test --features deterministic_replay` round-trips
   identically across runs (fixed-seed test vectors for set
   construction).

## Consequences

### Positive

- **Bandwidth O(log N)-class scaling** — reconciliation transmits only
  the set difference; ROADMAP3 target ≥80% reduction vs naive broadcast
  on 10k+ nodes (integration test: 2 peers × 1000 txs, 800 shared →
  ~200 transmitted, S2-P22 `tests/net_erlay.rs`).
- **Battle-tested protocol** — Erlay/PinSketch is Bitcoin Core's
  reference design (Erlay paper 2019, BIP-330 draft); libminisketch is
  the reference implementation. No hand-rolled reconciliation codes
  (ARCHITECT3 §15 anti-goal).
- **No tx loss by construction** — mandatory full-relay fallback on any
  decode failure means reconciliation can reduce bandwidth but can
  never reduce correctness; directly mitigates the ROADMAP3 risk.
- **Composes with compact blocks** — Erlay optimizes tx-set sync;
  BIP 152 (S2-P14) optimizes block broadcast; together they cover both
  relay channels.
- **Capability-gated rollout** — only Eray-capable peers (HELLO
  capability bit, S2-P10) participate; the network degrades gracefully
  to full relay during mixed-version operation.

### Negative

- **Per-peer reconciliation state** — each Eray-capable peer pair
  carries sketch + set-size + mismatch bookkeeping (memory overhead
  proportional to peer count; bounded by peer-connection limits,
  S2-P12).
- **Mismatch fallback can temporarily double bandwidth** — a failed
  round costs both a reconciliation attempt *and* a full-relay pass.
  Mitigation: adaptive sketch capacity (grow on failure) + per-peer
  mismatch-rate monitoring surfaced to scoring (ADR-0022).
- **Pinned native dependency** — `minisketch-rs` builds C++ libminisketch
  via cc-rs; exact version pinned (Stage 2 Pin Policy), Windows builds
  require MSBuild (Stage 2 Pitfalls). Documented stub-impl allowance in
  S2-P15 is the escape hatch if the toolchain blocks a platform.
- **Reconciliation latency** — bulk set sync happens on the
  interval/M-trigger cadence (60s / 1000 txs default), not instantly.
  Low-latency propagation of individual txs remains the INV announce
  path (S2-P13); Erlay only handles bulk divergence.

### Neutral

- **`minisketch-rs` vs FFI choice resolved** — Rust bindings crate over
  the C++ libminisketch (cc-rs build), not a pure-Rust reimplementation
  and not a manual FFI shim. Exact version number deliberately deferred
  to S2-P15 (the implementing prompt resolves and pins it; the pin
  rationale lives in AGENTS.md Stage 2 Pin Policy).
- **Erlay does not replace INV gossip** — S2-P13 announce/GET_DATA stays
  the latency path for new txs and the fallback path for reconciliation
  failures; S2-P15 adds reconciliation as a bandwidth optimization on
  top.
- **Config surface** — `reconcile_interval`, new-tx trigger threshold
  `M`, and capacity bounds are operator-tunable in `config.toml`
  (defaults: 60s, 1000 txs); consensus is unaffected (relay policy, not
  consensus rule — no SCIP/activation height needed pre-mainnet-freeze).
- **Fuzz + integration coverage deferred** — fuzz target
  `erlay_reconciliation` (S2-P21) and integration test `net_erlay.rs`
  (S2-P22) verify termination and bandwidth reduction; this ADR fixes
  the design they test against.

## Alternatives Considered

### Alternative 1: Full relay only (status quo, Bitcoin pre-Erlay)

- Pros: simplest possible code, no new dependency, no reconciliation
  state, no fallback complexity.
- Cons: O(N) bandwidth per tx — at 10k+ nodes every transaction costs a
  network-wide fanout; the Stage 2 production-grade P2P goal is
  unreachable. Bitcoin Core adopted Erlay precisely because full-relay
  fanout stopped scaling.
- Why not chosen: ARCHITECT3 §3.6 and ROADMAP3 Этап 2 explicitly require
  Erlay; full relay remains only as the fallback path.

### Alternative 2: BIP 152 compact blocks only

- Pros: solves the dominant block-broadcast bandwidth cost; already
  planned as S2-P14 with siphash short tx IDs.
- Cons: compact blocks only help once txs are *in a block*. Tx relay
  itself stays O(N) — the very channel Erlay optimizes. Adopting compact
  blocks without Erlay leaves the tx-relay scaling problem untouched.
- Why not chosen: complementary, not substitutive — S2-P14 (compact
  blocks) and S2-P15 (Erlay) are both required; this ADR covers only the
  latter.

### Alternative 3: FIBRE (Fast Internet Bitcoin Relay Engine)

- Pros: very low block-relay latency over a curated relay network.
- Cons: operates as a closed network of trusted relay nodes —
  centralization-friendly, not suitable for a permissionless P2P layer;
  addresses block latency, not tx-set reconciliation bandwidth.
- Why not chosen: decentralization anti-requirement; Erlay achieves the
  bandwidth goal on the open peer graph.

### Alternative 4: Threshold relay (Dandelion++)

- Pros: strong tx-origin privacy (stem/fluffle phases), well-analyzed
  latency properties.
- Cons: privacy-first, not bandwidth-first — does not deliver the O(log
  N)-class bandwidth reduction Stage 2 targets; stem phase adds latency
  to propagation.
- Why not chosen: wrong optimization axis for Stage 2 (bandwidth
  scaling); could be revisited post-Stage 2 as a privacy layer on top
  of Erlay, but that is out of scope here.

### Alternative 5: Ad-hoc inventory diffing (send inv lists, diff hashes)

- Pros: no native dependency; trivial to implement over existing INV
  messages.
- Cons: exchanging full inventory hash lists to compute a diff is itself
  O(N) bandwidth — it defeats the purpose; no error-correcting property,
  so any lost/dropped frame forces a full re-exchange. PinSketch sketches
  are fixed-size regardless of set difference and tolerate bounded
  errors.
- Why not chosen: reinvents set reconciliation poorly; libminisketch's
  information-theoretically optimal minisketch is the battle-tested
  primitive (ARCHITECT3 §15 anti-goal: no hand-rolled primitives).

## Related

- ARCHITECT3 §3.6 (network/p2p — «gossip.rs: … Erlay (Stage 2)»), §15
  (anti-goals), §10.4 (strangler Stage 2)
- ROADMAP3 Этап 2 — DoD «Erlay работает» (≥80% bandwidth reduction);
  risk «Erlay reconciliation может приводить к потере txs» + mitigation
  «fallback на full relay»
- ADR-0020 (Noise Protocol Framework — reconciliation runs over the
  encrypted transport)
- ADR-0022 (peer scoring — sustained mismatch rate surfaces as a peer
  quality signal)
- Stage 2 prompts: S2-P05 (crate scaffolding — comment reference),
  S2-P10 (HELLO capability `Erlay = 0x04`), S2-P13 (INV/GET_DATA —
  announce + fallback path), S2-P14 (compact blocks — gate for S2-P15),
  S2-P15 (Erlay implementation, pins `minisketch-rs` exact version)
- Stage 2 Pin Policy (AGENTS.md) — `minisketch-rs` pinned exact
- Stage 2 Determinism Checklist (AGENTS.md) — deterministic PinSketch
  encoding
- Erlay paper: «Erlay: Efficient Transaction Relay for Bitcoin» (2019);
  BIP-330 draft (efficient transaction reconciliation)

## Renumbering note

ROADMAP3 originally planned this ADR as ADR-0017 (second of the planned
ADR-0016..0019 block for Stage 2). Stage 1.5 FBP07 occupied ADR-0016 for
«Receipts Root in Block Header», so all Stage 2 ADRs were renumbered to
ADR-0020..0023 (noise / erlay / peer-scoring / rate-limiting). See the
full renumbering note in ADR-0020; the decision is recorded in the
S2-P01 commit message.
