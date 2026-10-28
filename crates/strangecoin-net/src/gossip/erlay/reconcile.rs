//! S2-P15 — Erlay reconciliation state machine (ADR-0021).
//!
//! Each Eray-capable peer pair keeps a [`PeerReconciliation`] record; the
//! [`ReconciliationState`] owns the local known-tx set and the round trigger.
//! A round ([`ReconciliationState::reconcile`]) is:
//!
//! 1. build a sketch of the local known set with the pair's adaptive
//!    capacity ([`super::mincut::create_sketch`]);
//! 2. XOR it with the peer's sketch and decode the difference
//!    ([`super::mincut::decode_difference`]);
//! 3. **success** → resolve the half of the difference *this side holds*
//!    back to TxIds (sorted lexicographically, ADR-0021 determinism #3) and
//!    return it — the caller announces those TxIds to the peer as one INV;
//!    the peer's symmetric call returns the complementary half, so together
//!    the two INV directions carry exactly the symmetric difference and the
//!    existing GET_DATA path (S2-P13) fetches only the missing tx bodies;
//! 4. **failure** → count the mismatch, grow the capacity after
//!    [`CAPACITY_GROW_FAILURES`] consecutive failures (bounded by
//!    [`MAX_SKETCH_CAPACITY`]) and surface the error — the caller runs
//!    [`super::fallback::fallback_full_relay`] (ADR-0021 mandatory fallback,
//!    no tx loss).
//!
//! **Deviation from the prompt's literal struct** (recorded per FBP rule
//! «все отклонения — явно»): `last_reconciled` is `Option<Instant>` (a state
//! that never reconciled yet is due immediately) and rounds take an injected
//! clock through `reconcile_at` (the `PeerManager::tick_at` precedent);
//! `PeerReconciliation` gains `current_capacity`, `consecutive_failures` and
//! `mismatch_count` — the adaptive-capacity bookkeeping the prompt's tasks 6/7
//! require and the mismatch-rate signal ADR-0021 surfaces to peer scoring
//! (S2-P17).
//!
//! **Trigger defaults**: ADR-0021 (accepted 2026-10-24, *after* the S2-P15
//! prompt draft) fixes `interval = 60s` and `M = 1000` new txs — the prompt
//! text said 5s/5000. The accepted ADR wins; both values are operator-tunable
//! through [`ReconciliationConfig`] (deviation from the prompt's literal
//! defaults recorded here and in the S2-P15 commit message).
//!
//! **Determinism** (Stage 2 checklist, threat V-21 extension): sketches and
//! decoded deltas are pure functions of (known set, capacity); `Instant` is
//! read only to stamp the *trigger* (`reconcile_at`), never inside sketch
//! construction or decoding; no RNG anywhere; the resolved TxId list is
//! sorted before it leaves this module.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::mincut::{create_sketch, decode_difference, short_id};
use crate::error::NetError;
use crate::noise::NodeId;

/// Reconcile-trigger interval (ADR-0021 Decision table).
pub const DEFAULT_RECONCILE_INTERVAL: Duration = Duration::from_secs(60);
/// New-local-tx trigger `M` (ADR-0021 Decision table).
pub const DEFAULT_NEW_TX_TRIGGER: u64 = 1000;
/// Starting sketch capacity before any round has failed (prompt task 6).
pub const INITIAL_SKETCH_CAPACITY: usize = 100;
/// Upper bound of adaptive capacity (ADR-0021: ~10k txs per round).
pub const MAX_SKETCH_CAPACITY: usize = 10_000;
/// Consecutive decode failures that double the capacity (prompt task 7).
pub const CAPACITY_GROW_FAILURES: u32 = 2;

/// Operator-tunable reconciliation parameters (ADR-0021: config.toml
/// surface; the live wiring lands with the connection loop in S2-P19).
#[derive(Debug, Clone)]
pub struct ReconciliationConfig {
    /// Time-based trigger (`DEFAULT_RECONCILE_INTERVAL`).
    pub interval: Duration,
    /// New-tx count trigger `DEFAULT_NEW_TX_TRIGGER`.
    pub new_tx_trigger: u64,
    /// Initial per-peer sketch capacity (`INITIAL_SKETCH_CAPACITY`).
    pub initial_capacity: usize,
}

impl Default for ReconciliationConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_RECONCILE_INTERVAL,
            new_tx_trigger: DEFAULT_NEW_TX_TRIGGER,
            initial_capacity: INITIAL_SKETCH_CAPACITY,
        }
    }
}

/// Per-peer-pair reconciliation bookkeeping (prompt task 3).
#[derive(Debug, Clone)]
pub struct PeerReconciliation {
    /// Known-set size at the last sketch build (ADR-0021: basis for the
    /// next round's capacity estimate once set sizes travel on the wire).
    pub last_set_size: usize,
    /// Our last built sketch envelope for this peer.
    pub last_sketch: Vec<u8>,
    /// Reconcile interval for this pair (defaults to the config value).
    pub reconciliation_interval: Duration,
    /// Current adaptive capacity (grows on repeated decode failure).
    pub current_capacity: usize,
    /// Consecutive decode failures since the last success; reaching
    /// [`CAPACITY_GROW_FAILURES`] doubles `current_capacity` and resets.
    pub consecutive_failures: u32,
    /// Lifetime decode-failure counter — the mismatch-rate signal ADR-0021
    /// surfaces to peer scoring (ADR-0022, S2-P17).
    pub mismatch_count: u64,
}

impl PeerReconciliation {
    fn new(config: &ReconciliationConfig) -> Self {
        Self {
            last_set_size: 0,
            last_sketch: Vec::new(),
            reconciliation_interval: config.interval,
            current_capacity: config.initial_capacity,
            consecutive_failures: 0,
            mismatch_count: 0,
        }
    }
}

/// Local reconciliation state: the known-tx set, the round trigger and the
/// per-peer records (prompt task 3).
#[derive(Debug)]
pub struct ReconciliationState {
    known_set: HashSet<[u8; 32]>,
    new_txs_since_reconcile: u64,
    last_reconciled: Option<Instant>,
    peer_state: HashMap<NodeId, PeerReconciliation>,
    config: ReconciliationConfig,
}

impl Default for ReconciliationState {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl ReconciliationState {
    pub fn new(config: ReconciliationConfig) -> Self {
        Self {
            known_set: HashSet::new(),
            new_txs_since_reconcile: 0,
            last_reconciled: None,
            peer_state: HashMap::new(),
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(ReconciliationConfig::default())
    }

    pub fn config(&self) -> &ReconciliationConfig {
        &self.config
    }

    /// Mirror a new local tx into the known set. Returns `true` when the
    /// txid was new (the new-tx trigger counts *new* txs only).
    pub fn note_new_tx(&mut self, txid: [u8; 32]) -> bool {
        let inserted = self.known_set.insert(txid);
        if inserted {
            self.new_txs_since_reconcile = self.new_txs_since_reconcile.saturating_add(1);
        }
        inserted
    }

    /// Bulk variant of [`note_new_tx`]; returns how many txids were new.
    pub fn note_txs<I: IntoIterator<Item = [u8; 32]>>(&mut self, txids: I) -> usize {
        txids.into_iter().filter(|t| self.note_new_tx(*t)).count()
    }

    pub fn known_set(&self) -> &HashSet<[u8; 32]> {
        &self.known_set
    }

    pub fn known_len(&self) -> usize {
        self.known_set.len()
    }

    /// Stamp a completed round and reset the new-tx counter (rounds do this
    /// themselves via `reconcile_at`; public for trigger tests and for
    /// callers that reconcile through another channel).
    pub fn mark_reconciled_at(&mut self, now: Instant) {
        self.last_reconciled = Some(now);
        self.new_txs_since_reconcile = 0;
    }

    /// Round trigger (prompt task 6): due when the interval elapsed **or**
    /// `new_tx_trigger` new local txs arrived, whichever comes first. A
    /// state that never reconciled is due immediately. The clock is injected
    /// (determinism checklist: no wall-clock reads inside this module).
    pub fn due_at(&self, now: Instant) -> bool {
        if self.new_txs_since_reconcile >= self.config.new_tx_trigger {
            return true;
        }
        match self.last_reconciled {
            None => true,
            Some(prev) => now.saturating_duration_since(prev) >= self.config.interval,
        }
    }

    /// Per-peer record (creates it with config defaults on first use).
    pub fn peer(&self, peer: &NodeId) -> Option<&PeerReconciliation> {
        self.peer_state.get(peer)
    }

    /// Build (and store) this node's sketch envelope for `peer` without
    /// decoding anything — the *initiating* half of a reconciliation round
    /// (S2-P19 wire carriage): the caller sends it as an [`ErlayReconcile`]
    /// with `respond = true` and the peer's symmetric call answers with its
    /// own sketch.
    ///
    /// [`ErlayReconcile`]: crate::gossip::erlay::ErlayReconcile
    pub fn local_sketch(&mut self, peer: &NodeId) -> Vec<u8> {
        let Self {
            known_set,
            peer_state,
            config,
            ..
        } = self;
        let record = peer_state
            .entry(*peer)
            .or_insert_with(|| PeerReconciliation::new(config));
        let sketch = create_sketch(known_set, record.current_capacity);
        record.last_sketch = sketch.clone();
        record.last_set_size = known_set.len();
        sketch
    }

    /// Drop a disconnected peer's reconciliation record (bounded memory).
    pub fn forget_peer(&mut self, peer: &NodeId) {
        self.peer_state.remove(peer);
    }

    /// Reconcile with `peer` (prompt task 3 signature): build the local
    /// sketch, decode the difference against `peer_sketch`, and return the
    /// TxIds **this side holds that the peer lacks** (lexicographically
    /// sorted) — the delta to announce as one INV. On failure the peer
    /// record is updated (mismatch counter, adaptive capacity) and the
    /// decode error is surfaced for the fallback path.
    ///
    /// `peer_sketch` is the peer's [`super::mincut::create_sketch`] envelope
    /// (its wire carriage lands with the S2-P19 connection loop — the prompt
    /// scopes S2-P15 to the reconciliation library + tests).
    pub fn reconcile(
        &mut self,
        peer: &NodeId,
        peer_sketch: &[u8],
    ) -> Result<Vec<[u8; 32]>, NetError> {
        self.reconcile_at(peer, peer_sketch, Instant::now())
    }

    /// Clock-injected variant of [`reconcile`] (the `tick_at` pattern):
    /// `now` stamps the trigger only — sketches and decoding stay pure.
    pub fn reconcile_at(
        &mut self,
        peer: &NodeId,
        peer_sketch: &[u8],
        now: Instant,
    ) -> Result<Vec<[u8; 32]>, NetError> {
        let Self {
            known_set,
            peer_state,
            new_txs_since_reconcile,
            last_reconciled,
            config,
        } = self;

        let record = peer_state
            .entry(*peer)
            .or_insert_with(|| PeerReconciliation::new(config));

        let local_sketch = create_sketch(known_set, record.current_capacity);
        record.last_sketch = local_sketch.clone();
        record.last_set_size = known_set.len();

        match decode_difference(&local_sketch, peer_sketch) {
            Ok(difference) => {
                record.consecutive_failures = 0;
                // Shrink toward the observed difference with 2x headroom,
                // floored at the initial capacity (ADR-0021 protocol sketch
                // step 5; never grows on success).
                let target = (difference.len() * 2).max(INITIAL_SKETCH_CAPACITY);
                record.current_capacity = record.current_capacity.min(target);

                let difference: HashSet<u64> = difference.into_iter().collect();
                let mut exclusive: Vec<[u8; 32]> = known_set
                    .iter()
                    .filter(|txid| short_id(txid).is_some_and(|id| difference.contains(&id)))
                    .copied()
                    .collect();
                exclusive.sort(); // lexicographic wire order (ADR-0021 #3)

                *last_reconciled = Some(now);
                *new_txs_since_reconcile = 0;
                Ok(exclusive)
            }
            Err(err) => {
                record.consecutive_failures = record.consecutive_failures.saturating_add(1);
                record.mismatch_count = record.mismatch_count.saturating_add(1);
                if record.consecutive_failures >= CAPACITY_GROW_FAILURES {
                    record.current_capacity = record
                        .current_capacity
                        .saturating_mul(2)
                        .min(MAX_SKETCH_CAPACITY);
                    record.consecutive_failures = 0;
                }
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gossip::erlay::mincut::create_sketch;

    fn txid(tag: u8, n: u32) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = tag;
        id[1..5].copy_from_slice(&n.to_be_bytes());
        id
    }

    /// Sets with a controlled symmetric difference: `shared` txids common to
    /// both sides, plus `only_a` / `only_b` exclusive txids. short IDs come
    /// from the first byte (tag), so distinct tags never collide.
    fn split_sets(shared: u32, only_a: u32, only_b: u32) -> (HashSet<[u8; 32]>, HashSet<[u8; 32]>) {
        let mut a = HashSet::new();
        let mut b = HashSet::new();
        for n in 0..shared {
            a.insert(txid(0xAA, n));
            b.insert(txid(0xAA, n));
        }
        for n in 0..only_a {
            a.insert(txid(0xA1, n));
        }
        for n in 0..only_b {
            b.insert(txid(0xB1, n));
        }
        (a, b)
    }

    #[test]
    fn defaults_follow_adr_0021() {
        // ADR-0021 Decision table (60s / 1000 txs) supersedes the prompt's
        // literal 5s/5000 — deviation recorded in the module docs + commit.
        assert_eq!(DEFAULT_RECONCILE_INTERVAL, Duration::from_secs(60));
        assert_eq!(DEFAULT_NEW_TX_TRIGGER, 1000);
        assert_eq!(INITIAL_SKETCH_CAPACITY, 100);
        let config = ReconciliationConfig::default();
        assert_eq!(config.interval, DEFAULT_RECONCILE_INTERVAL);
        assert_eq!(config.new_tx_trigger, DEFAULT_NEW_TX_TRIGGER);
    }

    #[test]
    fn trigger_fires_on_interval_or_new_tx_count() {
        let peer = NodeId::from_public_bytes([9u8; 32]);
        let mut state = ReconciliationState::with_defaults();
        let t0 = Instant::now();

        // Never reconciled → due immediately.
        assert!(state.due_at(t0));

        state.mark_reconciled_at(t0);
        assert!(!state.due_at(t0), "fresh stamp, nothing due");
        assert!(!state.due_at(t0 + Duration::from_secs(59)), "interval 60s");
        assert!(
            state.due_at(t0 + Duration::from_secs(61)),
            "interval elapsed"
        );

        // New-tx trigger fires without waiting for the interval.
        state.mark_reconciled_at(t0);
        for n in 0..DEFAULT_NEW_TX_TRIGGER - 1 {
            state.note_new_tx(txid(0xB7, n as u32));
        }
        assert!(!state.due_at(t0), "one tx short of M does not trigger");
        state.note_new_tx(txid(0xB7, 99_999));
        assert!(state.due_at(t0), "M new txs trigger regardless of time");

        // A completed round resets the counter (reconcile_at does both).
        let (a, b) = split_sets(4, 0, 0);
        let mut fresh = ReconciliationState::new(ReconciliationConfig {
            new_tx_trigger: 4,
            ..Default::default()
        });
        fresh.mark_reconciled_at(t0);
        fresh.note_txs(a);
        assert!(fresh.due_at(t0), "4 new txs hit the lowered trigger");
        let sketch_b = create_sketch(&b, 64);
        fresh.reconcile_at(&peer, &sketch_b, t0).expect("round");
        assert!(!fresh.due_at(t0), "counter reset by the round");
    }

    #[test]
    fn reconcile_returns_own_exclusive_txids_sorted() {
        let peer = NodeId::from_public_bytes([1u8; 32]);
        let (set_a, set_b) = split_sets(10, 3, 4);
        let sketch_b = create_sketch(&set_b, 64);

        let mut state = ReconciliationState::new(ReconciliationConfig {
            initial_capacity: 64,
            ..Default::default()
        });
        state.note_txs(set_a.iter().copied());

        let exclusive = state.reconcile(&peer, &sketch_b).expect("decodes");
        // Only A's own3 txs — B's 4 exclusive txids are not resolvable here
        // (they arrive as B's INV); the union of both directions is the
        // full symmetric difference.
        assert_eq!(exclusive.len(), 3);
        let mut expected: Vec<[u8; 32]> = set_a
            .iter()
            .copied()
            .filter(|t| !set_b.contains(t))
            .collect();
        expected.sort();
        assert_eq!(exclusive, expected, "lexicographic order");
        // Sorted = deterministic across runs.
        assert_eq!(
            state.reconcile(&peer, &sketch_b).expect("decodes"),
            exclusive
        );
    }

    #[test]
    fn adaptive_capacity_grows_after_two_consecutive_failures() {
        let peer = NodeId::from_public_bytes([2u8; 32]);
        // Symmetric difference of 150 > initial capacity 100.
        let (set_a, set_b) = split_sets(200, 75, 75);
        let mut state = ReconciliationState::new(ReconciliationConfig {
            initial_capacity: 100,
            ..Default::default()
        });
        state.note_txs(set_a.iter().copied());

        let peer_sketch_100 = create_sketch(&set_b, 100);
        assert!(state.reconcile(&peer, &peer_sketch_100).is_err());
        assert_eq!(
            state.peer(&peer).expect("record").consecutive_failures,
            1,
            "first failure only counts"
        );
        assert_eq!(state.peer(&peer).expect("record").current_capacity, 100);

        assert!(state.reconcile(&peer, &peer_sketch_100).is_err());
        let record = state.peer(&peer).expect("record");
        assert_eq!(record.consecutive_failures, 0, "counter resets on growth");
        assert_eq!(record.current_capacity, 200, "doubled after 2 failures");
        assert_eq!(record.mismatch_count, 2, "mismatch rate keeps counting");

        // The peer grew symmetrically (decode failure is symmetric — both
        // sides see the same two envelopes): it re-sends a capacity-200
        // sketch and the round succeeds.
        let peer_sketch_200 = create_sketch(&set_b, 200);
        let exclusive = state.reconcile(&peer, &peer_sketch_200).expect("decodes");
        assert_eq!(exclusive.len(), 75);
        let record = state.peer(&peer).expect("record");
        assert_eq!(record.consecutive_failures, 0);
        // Shrink-on-success: min(200, max(150*2, 100)) = 200 stays put.
        assert_eq!(record.current_capacity, 200);
    }

    #[test]
    fn capacity_growth_is_bounded_by_max() {
        let peer = NodeId::from_public_bytes([3u8; 32]);
        // Difference of 12000 exceeds MAX_SKETCH_CAPACITY: even at the cap
        // every round fails, and growth must saturate instead of overflowing.
        let (set_a, set_b) = split_sets(0, 6000, 6000);
        let mut state = ReconciliationState::new(ReconciliationConfig {
            initial_capacity: MAX_SKETCH_CAPACITY,
            ..Default::default()
        });
        state.note_txs(set_a.iter().copied());
        let remote = create_sketch(&set_b, MAX_SKETCH_CAPACITY);
        assert!(state.reconcile(&peer, &remote).is_err());
        assert!(state.reconcile(&peer, &remote).is_err());
        assert_eq!(
            state.peer(&peer).expect("record").current_capacity,
            MAX_SKETCH_CAPACITY,
            "saturates at MAX_SKETCH_CAPACITY"
        );
    }

    #[test]
    fn forget_peer_drops_the_record() {
        let peer = NodeId::from_public_bytes([4u8; 32]);
        let mut state = ReconciliationState::with_defaults();
        let sketch = create_sketch(&HashSet::new(), 64);
        let _ = state.reconcile(&peer, &sketch);
        assert!(state.peer(&peer).is_some());
        state.forget_peer(&peer);
        assert!(state.peer(&peer).is_none());
    }
}
