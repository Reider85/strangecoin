//! S2-P15 — Erlay reconciliation-based relay (ADR-0021).
//!
//! Full-relay gossip costs O(N) bandwidth per tx; Erlay replaces the *bulk
//! set sync* with code-based set reconciliation so a round transmits only
//! the set difference (ROADMAP3 target: ≥80% reduction at 10k+ nodes). The
//! low-latency announce path (S2-P13 INV) is untouched — ADR-0021: Erlay
//! optimizes bulk divergence, never replaces INV.
//!
//! ## Round protocol (library level; live wiring lands with S2-P19)
//!
//! ```text
//! Peer A                                                Peer B
//! known_set_A ── create_sketch(set, capacity) ──▶
//!                                                     known_set_B
//!    ◀──────────── sketch_B ────────────────────  create_sketch(set, capacity)
//!
//!    decode_difference(sketch_A, sketch_B)  ← both sides, symmetric
//!    │
//!    ├─ Ok(diff) ─▶ each side resolves ITS OWN exclusive TxIds
//!    │             INV(txids it holds, peer lacks) ──────────▶
//!    │             ◀────────── GET_DATA / TX (only the delta)
//!    │
//!    └─ Err ──────▶ fallback::fallback_full_relay (chunked full INV)
//!                   → handle_inv → GET_DATA → TX — no tx loss
//! ```
//!
//! * [`mincut`] — TxId→GF(2^64) short IDs, sketch envelope, difference
//!   decode (capacity-bounded, deterministic);
//! * [`reconcile`] — per-peer state machine: known set, round trigger
//!   (interval / new-tx count), adaptive capacity, exclusive-TxId
//!   resolution;
//! * [`fallback`] — mandatory full-relay recovery on any decode failure.
//!
//! ## Deviations from the prompt's literal spec (FBP «все отклонения — явно»)
//!
//! 1. **`minisketch-rs` is pinned optional + default-off; the active
//!    implementation is a documented stub.** minisketch-rs 0.1.9 builds
//!    libminisketch via cc-rs + bindgen 0.49, and bindgen needs libclang —
//!    absent on this Windows host (verified 2026-10-27). ADR-0021
//!    «Fallback dependency» and AGENTS.md Stage 2 Pitfalls explicitly allow
//!    a stub here; the envelope is contract-compatible with PinSketch
//!    (capacity-bounded decode, `impl_id` reserved for the SCIP-gated real
//!    payload behind the `minisketch` cargo feature).
//! 2. **Trigger defaults 60s / 1000 new txs, not the prompt's 5s / 5000** —
//!    ADR-0021 (accepted 2026-10-24, after this prompt's draft) is the
//!    accepted decision record and supersedes; both are tunable through
//!    `ReconciliationConfig`.
//! 3. **`decode_difference` returns short IDs (`Vec<u64>`), not
//!    `Vec<TxId>`** — a 32-byte TxId is not invertible from a 64-bit field
//!    element. Each side resolves only the half of the difference it holds
//!    (`reconcile` returns `Vec<TxId>` per the prompt's signature); the
//!    complementary half arrives as the peer's symmetric INV.
//! 4. **`fallback_full_relay` signature** — `mempool: &Mempool` →
//!    `&dyn BlockchainView` (cycle break, `broadcast.rs` precedent), plus
//!    `peer` + `GossipState` parameters; empty `txids` = announce the whole
//!    mempool inventory.
//! 5. **`last_reconciled: Option<Instant>` + clock-injected `reconcile_at`**
//!    (the `tick_at` pattern) instead of a plain `Instant`; `Instant` is
//!    read only to stamp the trigger, never in sketch/decode (determinism
//!    checklist).
//! 6. **Wire carriage of sketch envelopes is deferred to S2-P19** — the
//!    prompt scopes S2-P15 to the library + tests; the connection loop does
//!    not dispatch messages yet. The envelope itself is wire-ready
//!    (self-describing, bounded parse).
//!
//! ## Determinism (Stage 2 checklist, threat V-21 extension)
//!
//! * `mincut::{create_sketch, decode_difference}` and `reconcile`'s TxId
//!   resolution are pure functions of (known set, capacity) — sorting and
//!   byte encoding only; no `SystemTime`/`Instant`, no RNG, no I/O;
//! * `Instant` appears only at the round-trigger edge (`reconcile_at`,
//!   `due_at(now)` with injected clock);
//! * decoded TxId lists are sorted lexicographically before transmission
//!   (ADR-0021 determinism #3); fallback inventories are sorted by txid
//!   (checklist #4);
//! * `cargo test --features deterministic_replay` — the feature does not
//!   exist in this workspace (doc-only reference); reproducibility is
//!   covered by the pure-function tests above (byte-identical sketches,
//!   identical decoded differences across runs).
//!
//! ## Capability gating
//!
//! Reconciliation runs only with peers advertising `Capability::Erlay =
//! 0x04` (S2-P10 HELLO negotiation; already included in
//! `STAGE2_CAPABILITIES`). Non-Erlay peers keep the S2-P13 full-relay path.
//! The gate is enforced by the caller when the S2-P19 connection loop starts
//! scheduling rounds — this prompt ships the library the loop will call.

pub mod fallback;
pub mod mincut;
pub mod reconcile;

pub use fallback::fallback_full_relay;
pub use mincut::{create_sketch, decode_difference, short_id};
pub use reconcile::{
    PeerReconciliation, ReconciliationConfig, ReconciliationState, CAPACITY_GROW_FAILURES,
    DEFAULT_NEW_TX_TRIGGER, DEFAULT_RECONCILE_INTERVAL, INITIAL_SKETCH_CAPACITY,
    MAX_SKETCH_CAPACITY,
};
