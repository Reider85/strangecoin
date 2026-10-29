//! S2-P21 — Erlay reconciliation fuzzing (ADR-0021, S2-P15).
//!
//! Contract: two random tx sets (varying overlap) sketch-encoded and
//! cross-decoded never panic; a successful decode is the exact symmetric
//! difference of the short-ID sets (sorted, strictly ascending), and the
//! computed reconciliation delta through [`ReconciliationState`] is exactly
//! the local-exclusive TxIds. Bandwidth math runs on every input: when the
//! sets share at least a quarter of their combined size, transmitting only
//! the difference is a ≥50% reduction against a naive full relay — the
//! Stage 2 DoD bound, checked as arithmetic on every qualifying input.
//! Decode failures (capacity overflow, malformed envelopes) are typed
//! errors routing to the mandatory full-relay fallback — never a panic,
//! never a silently wrong difference.

#![no_main]

use std::collections::HashSet;
use std::time::{Duration, Instant};

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use strangecoin_net::{
    create_sketch, decode_difference, NodeId, ReconciliationConfig, ReconciliationState,
};

/// Per-group set bound: keeps sketches (and the linear stub decode) cheap.
const MAX_SET: usize = 256;
const MAX_CORRUPTIONS: usize = 8;

#[derive(Arbitrary, Debug)]
struct ErlayInput {
    /// Seeds of txids both peers hold (tagged `1`, distinct from the
    /// exclusive groups so short IDs never collide across groups).
    shared: Vec<[u8; 16]>,
    /// Seeds of txids only the local peer holds (tagged `2`).
    local_only: Vec<[u8; 16]>,
    /// Seeds of txids only the remote peer holds (tagged `3`).
    remote_only: Vec<[u8; 16]>,
    /// Sketch capacity (clamped to the ADR-0021 bounds).
    capacity: u16,
    /// Values mixed into byte flips on the remote sketch.
    corrupt: Vec<u8>,
}

/// Documented ADR-0021 TxId → GF(2^64) mapping: first 8 bytes big-endian,
/// zero excluded. Mirrors `mincut::short_id` — the decode contract is that
/// reconciliation is exact under *this* encoding.
fn short(txid: &[u8; 32]) -> Option<u64> {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&txid[..8]);
    let id = u64::from_be_bytes(buf);
    (id != 0).then_some(id)
}

fn txid_from(tag: u8, seed: &[u8; 16]) -> [u8; 32] {
    let mut txid = [0u8; 32];
    txid[0] = tag;
    txid[1..17].copy_from_slice(seed);
    txid
}

/// Build a txid set whose short IDs are unique (set semantics: a short-ID
/// collision inside one group collapses, as it would in a real sketch).
fn set_of(tag: u8, seeds: &[[u8; 16]]) -> HashSet<[u8; 32]> {
    let mut by_short: HashSet<u64> = HashSet::new();
    seeds
        .iter()
        .take(MAX_SET)
        .map(|seed| txid_from(tag, seed))
        .filter(|txid| short(txid).is_some_and(|id| by_short.insert(id)))
        .collect()
}

fn short_set(set: &HashSet<[u8; 32]>) -> HashSet<u64> {
    set.iter().filter_map(short).collect()
}

fuzz_target!(|input: ErlayInput| {
    let shared = set_of(1, &input.shared);
    let local: HashSet<[u8; 32]> = shared
        .union(&set_of(2, &input.local_only))
        .copied()
        .collect();
    let remote: HashSet<[u8; 32]> = shared
        .union(&set_of(3, &input.remote_only))
        .copied()
        .collect();

    let capacity = (input.capacity as usize).clamp(1, 10_000);
    let local_sketch = create_sketch(&local, capacity);
    let remote_sketch = create_sketch(&remote, capacity);

    // True symmetric difference of the short-ID sets.
    let mut expected_diff: Vec<u64> = short_set(&local)
        .symmetric_difference(&short_set(&remote))
        .copied()
        .collect();
    expected_diff.sort_unstable();

    match decode_difference(&local_sketch, &remote_sketch) {
        Ok(diff) => {
            assert_eq!(
                diff, expected_diff,
                "decoded difference must equal the true set difference"
            );
            for pair in diff.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "decoded difference must be strictly ascending"
                );
            }

            // Bandwidth math: naive full relay re-sends every tx of both
            // sets; Erlay transmits only the symmetric difference. ≥50%
            // reduction whenever shared ≥ (|local| + |remote|) / 4.
            let naive_items = local.len() + remote.len();
            let shared_items = short_set(&local).intersection(&short_set(&remote)).count();
            if naive_items > 0 && shared_items * 4 >= naive_items {
                assert!(
                    diff.len() * 2 <= naive_items,
                    "difference {} exceeds half of naive {} under sufficient overlap",
                    diff.len(),
                    naive_items
                );
            }
        }
        // Capacity overflow (or a malformed envelope) → typed error, the
        // mandatory full-relay fallback signal.
        Err(_) => {}
    }

    // --- state-level round (S2-P15 API surface) --------------------------
    let peer = NodeId::from_public_bytes([9u8; 32]);
    let config = ReconciliationConfig {
        interval: Duration::from_secs(60),
        new_tx_trigger: 1000,
        initial_capacity: capacity,
    };
    let mut state = ReconciliationState::new(config);
    state.note_txs(local.iter().copied());
    let _ = state.local_sketch(&peer);
    match state.reconcile_at(&peer, &remote_sketch, Instant::now()) {
        Ok(delta) => {
            // delta = local TxIds the peer lacks (resolved from this side's
            // half of the difference), lexicographically sorted.
            let diff_set: HashSet<u64> = expected_diff.iter().copied().collect();
            let mut expected_delta: Vec<[u8; 32]> = local
                .iter()
                .filter(|txid| short(txid).is_some_and(|id| diff_set.contains(&id)))
                .copied()
                .collect();
            expected_delta.sort();
            assert_eq!(
                delta, expected_delta,
                "state-level delta must be the local-exclusive set"
            );
        }
        Err(_) => {}
    }

    // --- corruption path: flip remote-sketch bytes, decode ---------------
    let mut corrupted = remote_sketch;
    for (i, value) in input.corrupt.iter().take(MAX_CORRUPTIONS).enumerate() {
        let pos = (value.wrapping_add(i as u8) as usize) % corrupted.len().max(1);
        if let Some(byte) = corrupted.get_mut(pos) {
            *byte ^= 0xFF;
        }
    }
    if let Ok(diff) = decode_difference(&local_sketch, &corrupted) {
        // Even a "lucky" decode of a corrupted sketch must be well-formed.
        for pair in diff.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }
});
